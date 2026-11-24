//! Registering the sparse package that carries the menu entry, for the user who runs this.
//!
//! A sparse package is registered per user (`PackageManager`, with the install folder as its
//! external location). The installer registers it for the user who installs; every other user of
//! a per-machine install gets it the first time they start the app ([`ensure`]). All of it is
//! WinRT through the `windows` crate's safe bindings.

use std::path::{Path, PathBuf};

use windows::ApplicationModel::Package;
use windows::Foundation::Uri;
use windows::Management::Deployment::{AddPackageOptions, PackageManager, RemovalOptions};
use windows_core::HSTRING;

/// The package file the installer puts beside `pdfcraft.exe`.
pub const PACKAGE_FILE: &str = "shell-menu.msix";

/// Windows 11's first build. Windows 10 shows packaged entries in its only menu too, beside the
/// classic one the installer writes, so it would show the entry twice.
const WINDOWS_11: u64 = 22_000;

/// Whether this Windows has the first-level menu (Windows 11).
pub fn is_windows_11() -> bool {
    windows::System::Profile::AnalyticsInfo::VersionInfo()
        .and_then(|v| v.DeviceFamilyVersion())
        .ok()
        .and_then(|v| v.to_string().parse::<u64>().ok())
        // DeviceFamilyVersion packs major.minor.build.revision into 16-bit fields.
        .is_some_and(|v| (v >> 16) & 0xFFFF >= WINDOWS_11)
}

/// The current user's copies of this build's package.
fn installed(manager: &PackageManager) -> windows_core::Result<Vec<Package>> {
    let name = crate::package_name();
    let mut found = Vec::new();
    for package in manager.FindPackagesByUserSecurityId(&HSTRING::new())? {
        if package.Id().and_then(|id| id.Name()).is_ok_and(|n| n == name.as_str()) {
            found.push(package);
        }
    }
    Ok(found)
}

/// Whether `package` is this build's, registered for `folder`.
fn is_current(package: &Package, folder: &Path) -> bool {
    let version = package.Id().and_then(|id| id.Version()).map(|v| [v.Major, v.Minor, v.Build, v.Revision]);
    let location = package.EffectiveExternalPath().map(|p| PathBuf::from(p.to_string()));
    version.is_ok_and(|v| v == crate::package_version()) && location.is_ok_and(|l| same_folder(&l, folder))
}

fn same_folder(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| p.to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase();
    norm(a) == norm(b)
}

/// Registers the package beside `exe` for the current user, unless it is already registered for
/// that folder at this version. Does nothing (Ok) when the installer left no package there (the
/// portable zip, the menu turned off) or before Windows 11.
pub fn ensure(exe: &Path) -> Result<(), String> {
    let Some(folder) = exe.parent() else { return Ok(()) };
    let msix = folder.join(PACKAGE_FILE);
    if !msix.is_file() || !is_windows_11() {
        return Ok(());
    }
    let manager = PackageManager::new().map_err(|e| format!("package manager: {e}"))?;
    if installed(&manager).map_err(|e| format!("listing packages: {e}"))?.iter().any(|p| is_current(p, folder)) {
        return Ok(());
    }
    let uri = |p: &Path| Uri::CreateUri(&HSTRING::from(p.as_os_str())).map_err(|e| format!("{}: {e}", p.display()));
    let options = AddPackageOptions::new().map_err(|e| e.to_string())?;
    options.SetExternalLocationUri(&uri(folder)?).map_err(|e| e.to_string())?;
    options.SetForceUpdateFromAnyVersion(true).map_err(|e| e.to_string())?;
    let operation = manager.AddPackageByUriAsync(&uri(&msix)?, &options).map_err(|e| e.to_string())?;
    let result = operation.join().map_err(|e| format!("registering {}: {e}", msix.display()))?;
    let code = result.ExtendedErrorCode().map_err(|e| e.to_string())?;
    if code.is_err() {
        let text = result.ErrorText().map(|t| t.to_string()).unwrap_or_default();
        return Err(format!("registering {}: {code:?} {text}", msix.display()));
    }
    Ok(())
}

/// Removes the current user's registration (the installer, on uninstall).
pub fn remove() -> Result<(), String> {
    let manager = PackageManager::new().map_err(|e| format!("package manager: {e}"))?;
    for package in installed(&manager).map_err(|e| format!("listing packages: {e}"))? {
        let full = package.Id().and_then(|id| id.FullName()).map_err(|e| e.to_string())?;
        let operation = manager.RemovePackageWithOptionsAsync(&full, RemovalOptions::None).map_err(|e| e.to_string())?;
        operation.join().map_err(|e| format!("removing {full}: {e}"))?;
    }
    Ok(())
}
