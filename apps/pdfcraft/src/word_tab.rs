//! Word's PDF tab for each user (`crates/word-import/src/word_addin.rs`).
//!
//! The installer puts the template beside `pdfcraft.exe`. Word loads every template in the user's
//! own startup folder (`%APPDATA%\Microsoft\Word\STARTUP`) and trusts that folder by default, so
//! its button's macro runs there. Office's own startup folder under Program Files is not trusted
//! by default (the owner's Word listed it nowhere), so a template there showed the tab but its
//! macro was disabled. The template is copied into each user's folder instead: by the installer for
//! the user who installs (`--register-user`), and at each start for every other user, and again
//! when a new version brings a new template.

use std::path::{Path, PathBuf};

/// The template's file name, the product's as the installer names it (`pdfcraft.wxs`).
// Used on Windows only, and by the tests everywhere.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn file_name() -> String {
    format!("{}.dotm", option_env!("PDFCRAFT_PRODUCT_NAME").filter(|n| !n.is_empty()).unwrap_or("PdfCraft"))
}

/// The user's own Word startup folder.
#[cfg(windows)]
fn startup_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Microsoft").join("Word").join("STARTUP"))
}

/// Copies the template in `from` (the install folder) into `to` (a Word startup folder) unless the
/// same bytes are there. `Ok(false)`: nothing to do (no template installed, or already current).
// Used on Windows only, and by the tests everywhere.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn install(from: &Path, to: &Path) -> Result<bool, String> {
    let source = from.join(file_name());
    let Ok(bytes) = std::fs::read(&source) else { return Ok(false) };
    let target: PathBuf = to.join(file_name());
    if std::fs::read(&target).is_ok_and(|have| have == bytes) {
        return Ok(false);
    }
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    use pdfcraft_platform::staging::{StagingName, write_atomically};
    // Word holds the file while it runs: the copy then fails and the next start tries again.
    write_atomically(&target, &bytes, StagingName::SuffixThenTag).map_err(|e| format!("{}: {e}", target.display()))?;
    Ok(true)
}

/// Removes the template from `to` (uninstall, for the user who uninstalls).
// Used on Windows only, and by the tests everywhere.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn uninstall(to: &Path) -> Result<(), String> {
    let target = to.join(file_name());
    match std::fs::remove_file(&target) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("{}: {e}", target.display())),
        _ => Ok(()),
    }
}

/// [`install`] or [`uninstall`] for the current user, beside the running `pdfcraft.exe`.
pub(crate) fn run(add: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        let Some(to) = startup_dir() else { return Ok(()) };
        if !add {
            return uninstall(&to);
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let Some(from) = exe.parent() else { return Ok(()) };
        install(from, &to).map(|_| ())
    }
    #[cfg(not(windows))]
    {
        let _ = add;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The template reaches the user's folder once, is replaced by a new version, and goes on
    /// uninstall; with none installed (the portable zip) nothing happens.
    #[test]
    fn the_template_is_copied_to_the_user_folder_and_kept_current() {
        let root = std::env::temp_dir().join(format!("pdfcraft-word-tab-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (from, to) = (root.join("install"), root.join("Word").join("STARTUP"));
        std::fs::create_dir_all(&from).unwrap();
        assert!(!install(&from, &to).unwrap(), "no template installed: nothing to copy");
        assert!(!to.exists());
        std::fs::write(from.join(file_name()), b"v1").unwrap();
        assert!(install(&from, &to).unwrap());
        assert_eq!(std::fs::read(to.join(file_name())).unwrap(), b"v1");
        assert!(!install(&from, &to).unwrap(), "already current");
        std::fs::write(from.join(file_name()), b"v2").unwrap();
        assert!(install(&from, &to).unwrap(), "a new version replaces it");
        assert_eq!(std::fs::read(to.join(file_name())).unwrap(), b"v2");
        uninstall(&to).unwrap();
        assert!(!to.join(file_name()).exists());
        uninstall(&to).unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }
}
