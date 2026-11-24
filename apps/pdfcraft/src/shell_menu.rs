//! "Convert to PDF with …" in Windows 11's first-level File Explorer menu (`crates/shell-menu`).
//!
//! The entry comes from a sparse package the installer puts beside `pdfcraft.exe`. Packages are
//! registered per user: the installer runs `pdfcraft.exe --register-user` as the installing
//! user (and `--unregister-user` on uninstall), and every start registers it for a user who
//! doesn't have it yet, such as another user of a per-machine install. Elsewhere, and where the
//! installer left no package (the portable zip, the menu turned off, Windows 10), nothing happens.

/// The installer's call (`--register-user`, `--unregister-user`), and every start's: add or remove
/// this user's Windows 11 menu entry and Word tab (`word_tab`). Failures are logged; the installer
/// ignores the exit code, and the classic menu entry stays under "Show more options".
pub(crate) fn run(add: bool) {
    if let Err(e) = crate::word_tab::run(add) {
        log::warn!("Word's PDF tab: {e}");
    }
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
        // Word's tab too (`word_tab`): a copy, quick, but on the same thread so the start doesn't wait.
        let spawned = std::thread::Builder::new().name("shell-menu".into()).spawn(|| run(true));
        if let Err(e) = spawned {
            log::warn!("Windows 11 menu entry: no thread: {e}");
        }
    }
}
