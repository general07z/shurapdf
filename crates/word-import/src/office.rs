//! Converting through Microsoft Word itself, when it is installed (Windows): Word opens the file
//! read-only with macros off and saves it as PDF, so the result is exactly Word's own "Save as
//! PDF". Word is driven by a short PowerShell script (passed encoded on the command line); the file paths and
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
/// document structure, as Word's Save as PDF does (if Word refuses those export options, a plain
/// Save As PDF). Macros are forced off and no dialog is shown. It succeeds only when the PDF is
/// there.
const SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
$word = $null
$doc = $null
$code = 0
try { $word = New-Object -ComObject Word.Application } catch { exit 10 }
try {
  $word.Visible = $false
  $word.DisplayAlerts = 0
  $word.AutomationSecurity = 3
  $password = $env:SHURAPDF_WORD_PASSWORD
  $out = $env:SHURAPDF_WORD_OUT
  $doc = $word.Documents.Open($env:SHURAPDF_WORD_IN, $false, $true, $false, $password, $password)
  try {
    $doc.ExportAsFixedFormat($out, 17, $false, 0, 0, 1, 1, 0, $true, $true, 1, $true, $true, $false)
  } catch {
    $doc.SaveAs2($out, 17)
  }
  if (-not (Test-Path -LiteralPath $out)) {
    [Console]::Error.WriteLine('Word finished without saving a PDF')
    $code = 2
  }
} catch {
  $message = $_.Exception.Message
  if ($message -match 'password' -or $_.Exception.HResult -eq -2146822880) {
    $code = 3
  } else {
    [Console]::Error.WriteLine($message)
    $code = 2
  }
} finally {
  if ($doc) { try { $doc.Close(0) } catch {} }
  if ($word) { try { $word.Quit(0) } catch {} }
}
exit $code
"#;

/// The script as PowerShell's `-EncodedCommand` takes it: base64 of its UTF-16LE text. Given on
/// standard input instead (`-Command -`), PowerShell reads it a line at a time as if typed, so
/// multi-line blocks never run and it exits having done nothing.
fn encoded(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64(&bytes)
}

/// Standard base64 (RFC 4648) with padding.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                // Six bits pick one of the 64 letters.
                out.push(char::from(ALPHABET.get(((n >> shift) & 63) as usize).copied().unwrap_or(b'A')));
            } else {
                out.push('=');
            }
        }
    }
    out
}

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
    let code = run(SCRIPT, &input, &output, &errors, password)?;
    match code {
        0 => std::fs::read(&output).map_err(|e| {
            let said = std::fs::read_to_string(&errors).unwrap_or_default();
            OfficeError::Failed(format!(
                "Word wrote no PDF ({e}){}",
                if said.trim().is_empty() { String::new() } else { format!(": {}", said.trim()) }
            ))
        }),
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
fn run(script: &str, input: &Path, output: &Path, errors: &Path, password: Option<&str>) -> Result<i32, OfficeError> {
    let error_file = std::fs::File::create(errors).map_err(|e| OfficeError::Failed(e.to_string()))?;
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand"])
        .arg(encoded(script))
        .env("SHURAPDF_WORD_IN", input)
        .env("SHURAPDF_WORD_OUT", output)
        // Without a password, one that can't match: an empty one could make Word ask for it in
        // a window nobody sees, until the timeout.
        .env("SHURAPDF_WORD_PASSWORD", password.filter(|p| !p.is_empty()).unwrap_or(NO_PASSWORD))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(error_file));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console flashes up.
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|_| OfficeError::Unavailable)?;
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

#[cfg(test)]
mod tests {
    use super::{SCRIPT, base64, encoded};

    #[test]
    fn base64_matches_rfc_4648() {
        for (plain, coded) in
            [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")]
        {
            assert_eq!(base64(plain.as_bytes()), coded, "{plain}");
        }
    }

    /// The script reaches PowerShell whole, as UTF-16LE in base64 (`-EncodedCommand`): its
    /// multi-line blocks run, which they don't when it is read line by line from standard input.
    #[test]
    fn the_script_is_passed_whole_as_utf16() {
        let coded = encoded(SCRIPT);
        let units: Vec<u8> = SCRIPT.encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(coded, base64(&units));
        assert_eq!(coded.len(), units.len().div_ceil(3) * 4);
        assert!(SCRIPT.contains("exit $code") && SCRIPT.contains("Test-Path"), "success needs the PDF to exist");
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::{OfficeError, convert, run};

    /// A multi-line script runs whole: the file its `try` block writes is there afterwards. (With
    /// Word installed, the conversion script's `try` block is where Word is driven; a build
    /// machine has no Word, so this probe stands in for it.)
    #[test]
    fn a_multi_line_script_runs_whole() {
        let dir = std::env::temp_dir().join(format!("shurapdf-probe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (out, errors) = (dir.join("out.txt"), dir.join("errors.txt"));
        let probe = "$ErrorActionPreference = 'Stop'\n$code = 0\ntry {\n  Set-Content -LiteralPath $env:SHURAPDF_WORD_OUT -Value 'ran'\n} catch {\n  $code = 2\n} finally {\n  $done = 1\n}\nexit $code\n";
        let code = run(probe, &dir.join("in.docx"), &out, &errors, None).unwrap();
        let said = std::fs::read_to_string(&errors).unwrap_or_default();
        assert_eq!(code, 0, "{said}");
        assert!(out.exists(), "the try block never ran: {said}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// On Windows, asking Word either converts (Office installed) or says Word isn't there (a
    /// build machine without Office); it never reports success without a PDF, which it did when
    /// the script reached PowerShell line by line and never ran.
    #[test]
    fn word_converts_or_says_it_is_missing() {
        match convert(&crate::testkit::synthetic_docx(), None) {
            Ok(pdf) => assert!(pdf.starts_with(b"%PDF-")),
            Err(e) => assert_eq!(e, OfficeError::Unavailable, "Word is either there or not: {e:?}"),
        }
    }
}
