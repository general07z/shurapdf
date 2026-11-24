//! "Convert to PDF with …" in Windows 11's first-level File Explorer menu (`crates/shell-menu`).
//!
//! The entry comes from a sparse package the installer puts beside `pdfcraft.exe`. Packages are
//! registered per user: the installer runs `pdfcraft.exe --register-shell-menu` as the installing
//! user (and `--unregister-shell-menu` on uninstall), and every start registers it for a user who
//! doesn't have it yet, such as another user of a per-machine install. Elsewhere, and where the
//! installer left no package (the portable zip, the menu turned off, Windows 10), nothing happens.

/// The installer's call: register (`add`) or remove the entry for this user. Failures are logged;
/// the installer ignores the exit code, and the classic entry stays under "Show more options".
pub(crate) fn run(add: bool) {
    #[cfg(windows)]
    {
        let result = if add {
            std::env::current_exe().map_err(|e| e.to_string()).and_then(|exe| pdfcraft_shell_menu::register::ensure(&exe))
        } else {
            pdfcraft_shell_menu::register::remove()
        };
        if let Err(e) = result {
            log::warn!("Windows 11 menu entry: {e}");
        }
    }
    #[cfg(not(windows))]
    let _ = add;
}

/// Registers the entry for this user if needed, off the main thread (a package registration takes
/// a moment, and the window shouldn't wait for it).
pub(crate) fn ensure_in_background() {
    #[cfg(windows)]
    {
        let spawned = std::thread::Builder::new().name("shell-menu".into()).spawn(|| run(true));
        if let Err(e) = spawned {
            log::warn!("Windows 11 menu entry: no thread: {e}");
        }
    }
}
