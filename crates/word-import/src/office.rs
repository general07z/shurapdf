//! Converting through Microsoft Word itself, when it is installed (Windows): Word opens the file
//! read-only with macros off and saves it as PDF, so the result is exactly Word's own "Save as
//! PDF". Word is driven by a short PowerShell script given on standard input; the file paths and
//! the password reach it as environment variables, never inside the script's text.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Why Word did not convert the file.
#[derive(Debug, PartialEq, Eq)]
pub enum OfficeError {
    /// No Microsoft Word on this computer (or not Windows).
    Unavailable,
    /// Word could not open the file with the password given (or without one).
    Password,
    /// Word failed, or took longer than [`TIMEOUT`]: its message.
    Failed(String),
}

/// The longest a conversion may take before Word is stopped.
pub const TIMEOUT: Duration = Duration::from_secs(180);

/// Exit codes the script uses for [`OfficeError::Unavailable`] and [`OfficeError::Password`].
const EXIT_UNAVAILABLE: i32 = 10;
const EXIT_PASSWORD: i32 = 3;

/// Given to Word when there is no password: a protected file then fails at once.
const NO_PASSWORD: &str = "\u{1}shurapdf-no-password\u{1}";

/// Opens `SHURAPDF_WORD_IN` read-only (with the password in `SHURAPDF_WORD_PASSWORD`; a file
/// without one ignores it) and exports `SHURAPDF_WORD_OUT` as PDF with heading bookmarks and
/// document structure, as Word's Save as PDF does. Macros are forced off and no dialog is shown.
const SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
$word = $null
$doc = $null
try { $word = New-Object -ComObject Word.Application } catch { exit 10 }
try {
  $word.Visible = $false
  $word.DisplayAlerts = 0
  $word.AutomationSecurity = 3
  $password = $env:SHURAPDF_WORD_PASSWORD
  $doc = $word.Documents.Open($env:SHURAPDF_WORD_IN, $false, $true, $false, $password, $password)
  $doc.ExportAsFixedFormat($env:SHURAPDF_WORD_OUT, 17, $false, 0, 0, 1, 1, 0, $true, $true, 1, $true, $true, $false)
  exit 0
} catch {
  $message = $_.Exception.Message
  if ($message -match 'password' -or $_.Exception.HResult -eq -2146822880) { exit 3 }
  [Console]::Error.WriteLine($message)
  exit 2
} finally {
  if ($doc) { try { $doc.Close(0) } catch {} }
  if ($word) { try { $word.Quit(0) } catch {} }
}
"#;

/// Convert Word bytes to PDF bytes with Microsoft Word. `password` opens a protected file; a
/// protected file without one fails with [`OfficeError::Password`].
pub fn convert(bytes: &[u8], password: Option<&str>) -> Result<Vec<u8>, OfficeError> {
    if !cfg!(windows) {
        return Err(OfficeError::Unavailable);
    }
    let dir = TempDir::new().map_err(|e| OfficeError::Failed(format!("couldn't make a working folder ({e})")))?;
    // Word recognises the format from the content; the name only has to look like a Word file.
    let extension = if bytes.starts_with(&crate::COMPOUND_FILE) { "doc" } else { "docx" };
    let input = dir.path.join(format!("document.{extension}"));
    let output = dir.path.join("document.pdf");
    let errors = dir.path.join("errors.txt");
    std::fs::write(&input, bytes).map_err(|e| OfficeError::Failed(format!("couldn't write the Word file ({e})")))?;
    let code = run(&input, &output, &errors, password)?;
    match code {
        0 => std::fs::read(&output).map_err(|e| OfficeError::Failed(format!("Word wrote no PDF ({e})"))),
        EXIT_UNAVAILABLE => Err(OfficeError::Unavailable),
        EXIT_PASSWORD => Err(OfficeError::Password),
        _ => {
            let message = std::fs::read_to_string(&errors).unwrap_or_default();
            let message = message.trim();
            Err(OfficeError::Failed(if message.is_empty() { format!("Word stopped with code {code}") } else { message.to_owned() }))
        }
    }
}

/// Runs the script and waits for it, at most [`TIMEOUT`]. Its exit code; a PowerShell that can't
/// start means no Word.
fn run(input: &Path, output: &Path, errors: &Path, password: Option<&str>) -> Result<i32, OfficeError> {
    let error_file = std::fs::File::create(errors).map_err(|e| OfficeError::Failed(e.to_string()))?;
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", "-"])
        .env("SHURAPDF_WORD_IN", input)
        .env("SHURAPDF_WORD_OUT", output)
        // Without a password, one that can't match: an empty one could make Word ask for it in
        // a window nobody sees, until the timeout.
        .env("SHURAPDF_WORD_PASSWORD", password.filter(|p| !p.is_empty()).unwrap_or(NO_PASSWORD))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::from(error_file));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console flashes up.
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|_| OfficeError::Unavailable)?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        if let Err(e) = stdin.write_all(SCRIPT.as_bytes()) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(OfficeError::Failed(format!("couldn't start Word ({e})")));
        }
    }
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status.code().unwrap_or(-1)),
            Ok(None) if started.elapsed() < TIMEOUT => std::thread::sleep(Duration::from_millis(100)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(OfficeError::Failed(format!("Word took longer than {} seconds", TIMEOUT.as_secs())));
            }
            Err(e) => return Err(OfficeError::Failed(e.to_string())),
        }
    }
}

/// A folder of its own under the system temp folder, removed when dropped.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
        let path = std::env::temp_dir().join(format!("shurapdf-word-{}-{}-{nanos}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&path)?;
        Ok(Self { path })
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
