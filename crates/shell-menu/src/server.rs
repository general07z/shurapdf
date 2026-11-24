//! The COM server: the `IExplorerCommand` class, its class factory and the DLL exports.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::os::windows::ffi::OsStringExt as _;
use std::path::PathBuf;

use windows::Win32::Foundation::{CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_NOTIMPL, E_POINTER, HMODULE, S_FALSE};
use windows::Win32::System::Com::{CoTaskMemFree, IBindCtx, IClassFactory, IClassFactory_Impl};
use windows::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleFileNameW, GetModuleHandleExW,
};
use windows::Win32::UI::Shell::{
    ECF_DEFAULT, ECS_ENABLED, ECS_HIDDEN, IEnumExplorerCommand, IExplorerCommand, IExplorerCommand_Impl, IShellItemArray, SHStrDupW,
    SIGDN_FILESYSPATH,
};
use windows_core::{BOOL, GUID, HRESULT, IUnknown, Interface as _, PCWSTR, PWSTR, Ref, implement};

#[implement(IExplorerCommand)]
struct ConvertCommand;

/// The file system paths of the selected items (none for an item with no path, such as a library
/// entry). At most 1000 are read.
fn selected(items: Ref<IShellItemArray>) -> Vec<PathBuf> {
    let Some(items) = items.as_ref() else { return Vec::new() };
    // SAFETY: a method of a live interface the shell handed us, with no pointer arguments.
    let count = unsafe { items.GetCount() }.unwrap_or(0).min(1000);
    (0..count)
        .filter_map(|i| {
            // SAFETY: as above; the index is below the count.
            let item = unsafe { items.GetItemAt(i) }.ok()?;
            // SAFETY: the callee allocates the name; it is read once, then freed with
            // CoTaskMemFree as the API requires.
            unsafe {
                let name = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
                let path = name.to_string().ok().map(PathBuf::from);
                CoTaskMemFree(Some(name.0 as *const c_void));
                path
            }
        })
        .collect()
}

/// `pdfcraft.exe` beside this DLL.
fn app() -> Option<PathBuf> {
    module_path().and_then(|dll| dll.parent().map(|dir| dir.join("pdfcraft.exe")))
}

/// This DLL's own path.
fn module_path() -> Option<PathBuf> {
    let mut module = HMODULE::default();
    let mut buffer = vec![0u16; 32_768];
    // SAFETY: the address is this function's own, inside this DLL; the handle is not reference
    // counted (UNCHANGED_REFCOUNT); the buffer is ours and its length goes with it.
    let len = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCWSTR(module_path as *const () as *const u16),
            &mut module,
        )
        .ok()?;
        GetModuleFileNameW(Some(module), &mut buffer)
    };
    buffer.truncate(usize::try_from(len).ok()?);
    (!buffer.is_empty()).then(|| PathBuf::from(std::ffi::OsString::from_wide(&buffer)))
}

/// `text` copied into memory the shell owns and frees.
fn dup(text: &str) -> windows_core::Result<PWSTR> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: a NUL-terminated buffer we own; the callee copies it.
    unsafe { SHStrDupW(PCWSTR(wide.as_ptr())) }
}

impl IExplorerCommand_Impl for ConvertCommand_Impl {
    fn GetTitle(&self, _items: Ref<IShellItemArray>) -> windows_core::Result<PWSTR> {
        dup(&crate::title())
    }

    fn GetIcon(&self, _items: Ref<IShellItemArray>) -> windows_core::Result<PWSTR> {
        let exe = app().ok_or_else(|| windows_core::Error::from(E_NOTIMPL))?;
        dup(&format!("{},0", exe.display()))
    }

    fn GetToolTip(&self, _items: Ref<IShellItemArray>) -> windows_core::Result<PWSTR> {
        Err(E_NOTIMPL.into())
    }

    fn GetCanonicalName(&self) -> windows_core::Result<GUID> {
        Ok(GUID::from_u128(crate::CLSID))
    }

    fn GetState(&self, items: Ref<IShellItemArray>, _slow: BOOL) -> windows_core::Result<u32> {
        let any = selected(items).iter().any(|p| crate::is_word(p));
        Ok(if any { ECS_ENABLED.0 as u32 } else { ECS_HIDDEN.0 as u32 })
    }

    fn Invoke(&self, items: Ref<IShellItemArray>, _bind: Ref<IBindCtx>) -> windows_core::Result<()> {
        let exe = app().ok_or_else(|| windows_core::Error::from(E_POINTER))?;
        for path in selected(items).into_iter().filter(|p| crate::is_word(p)) {
            // One launch per file, as the classic entry does; each hands its PDF to the window
            // already open.
            let _ = std::process::Command::new(&exe).arg("--convert-to-pdf").arg(path).spawn();
        }
        Ok(())
    }

    fn GetFlags(&self) -> windows_core::Result<u32> {
        Ok(ECF_DEFAULT.0 as u32)
    }

    fn EnumSubCommands(&self) -> windows_core::Result<IEnumExplorerCommand> {
        Err(E_NOTIMPL.into())
    }
}

#[implement(IClassFactory)]
struct Factory;

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(&self, outer: Ref<IUnknown>, iid: *const GUID, object: *mut *mut c_void) -> windows_core::Result<()> {
        if outer.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }
        if iid.is_null() || object.is_null() {
            return Err(E_POINTER.into());
        }
        let command: IExplorerCommand = ConvertCommand.into();
        // SAFETY: non-null pointers from COM's caller, which the COM contract makes valid.
        unsafe { command.query(iid, object).ok() }
    }

    fn LockServer(&self, _lock: BOOL) -> windows_core::Result<()> {
        Ok(())
    }
}

/// COM's entry point: the class factory for [`crate::CLSID`].
///
/// # Safety
/// COM calls it with pointers valid for reading (`clsid`, `iid`) and writing (`object`).
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(clsid: *const GUID, iid: *const GUID, object: *mut *mut c_void) -> HRESULT {
    if clsid.is_null() || iid.is_null() || object.is_null() {
        return E_POINTER;
    }
    // SAFETY: non-null pointers from COM, read and written as its contract describes.
    unsafe {
        *object = std::ptr::null_mut();
        if *clsid != GUID::from_u128(crate::CLSID) {
            return CLASS_E_CLASSNOTAVAILABLE;
        }
        let factory: IClassFactory = Factory.into();
        factory.query(iid, object)
    }
}

/// COM asks whether the DLL can be unloaded; the shell keeps it loaded as long as it likes.
#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    S_FALSE
}
