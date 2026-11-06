//! Converting through Microsoft Word itself, when it is installed (Windows): Word opens the file
//! read-only with macros off and saves it as PDF, so the result is exactly Word's own "Save as
//! PDF". Word is driven by a short PowerShell script (passed encoded on the command line, so the
//! execution policy doesn't apply); the file paths and the password reach it as environment
//! variables, never inside the script's text.
//!
//! The cases it is built for (each checked against real Word by `word_fidelity --checks`):
//! - Word not installed, or installed but unable to start (blocked, broken, not licensed): told
//!   apart, so Automatic falls back and Microsoft Word says which.
//! - Word already open with the user's documents: a separate, hidden Word is started and only it
//!   is closed; the user's Word is never touched.
//! - Several conversions at once (Combine): one Word at a time.
//! - Word stuck (a dialog nobody sees, a huge document): stopped after a time that grows with the
//!   file size, and the hidden Word it started is ended too, never left running.
//! - Password-protected files, files with macros (`.docm`), templates, Word 97-2003 `.doc`.
//! - User and temp folders with Arabic names.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Why Word did not convert the file.
#[derive(Debug, PartialEq, Eq)]
pub enum OfficeError {
    /// No Microsoft Word on this computer (or not Windows).
    Unavailable,
    /// Word is installed but could not be started: its message.
    CannotStart(String),
    /// Word took longer than its time limit and was stopped.
    TimedOut,
    /// Word could not open the file with the password given (or without one).
    Password,
    /// Word failed on this file: its message.
    Failed(String),
}

/// The time Word gets for a file of `len` bytes: three minutes, plus half a minute per MiB, at most
/// ten minutes.
pub fn timeout_for(len: usize) -> Duration {
    let mib = u64::try_from(len / (1024 * 1024)).unwrap_or(u64::MAX);
    Duration::from_secs(180u64.saturating_add(mib.saturating_mul(30)).min(600))
}

/// Exit codes the script uses.
const EXIT_UNAVAILABLE: i32 = 10;
const EXIT_CANNOT_START: i32 = 11;
const EXIT_PASSWORD: i32 = 3;

/// Given to Word when there is no password: a protected file then fails at once.
const NO_PASSWORD: &str = "\u{1}shurapdf-no-password\u{1}";

/// One conversion at a time: each starts its own Word, and twenty at once would swamp the PC.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// Starts a hidden Word of its own (the user's open Word is never used), notes its process id in
/// `SHURAPDF_WORD_PID` so a stuck Word can be ended, opens `SHURAPDF_WORD_IN` read-only (with the
/// password in `SHURAPDF_WORD_PASSWORD`; a file without one ignores it) and exports
/// `SHURAPDF_WORD_OUT` as PDF with heading bookmarks and document structure, as Word's Save as PDF
/// does (if Word refuses those export options, a plain Save As PDF). Macros are forced off and no
/// dialog is shown. It succeeds only when the PDF is there.
const SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$word = $null
$doc = $null
$code = 0
if (-not (Test-Path -LiteralPath 'Registry::HKEY_CLASSES_ROOT\Word.Application\CLSID')) { exit 10 }
$before = @(Get-Process -Name WINWORD -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
try { $word = New-Object -ComObject Word.Application } catch {
  [Console]::Error.WriteLine($_.Exception.Message)
  exit 11
}
try {
  $mine = @(Get-Process -Name WINWORD -ErrorAction SilentlyContinue | Where-Object { $before -notcontains $_.Id } | ForEach-Object { $_.Id })
  if ($mine.Count -gt 0) { Set-Content -LiteralPath $env:SHURAPDF_WORD_PID -Value ($mine -join ' ') -Encoding ASCII }
  $word.Visible = $false
  $word.DisplayAlerts = 0
  $word.AutomationSecurity = 3
  $password = $env:SHURAPDF_WORD_PASSWORD
  $out = $env:SHURAPDF_WORD_OUT
  $missing = [Type]::Missing
  # FileName, ConfirmConversions, ReadOnly, AddToRecentFiles, PasswordDocument, PasswordTemplate,
  # Revert, WritePasswordDocument, WritePasswordTemplate, Format, Encoding, Visible, OpenAndRepair,
  # DocumentDirection, NoEncodingDialog
  try {
    $doc = $word.Documents.Open($env:SHURAPDF_WORD_IN, $false, $true, $false, $password, $password, $false, $password, $password, $missing, $missing, $false, $false, $missing, $true)
  } catch {
    # Some Word versions refuse the extra options ("Command failed"): the six every Word takes.
    if ($_.Exception.Message -match 'password' -or $_.Exception.HResult -eq -2146822880) { throw }
    $doc = $word.Documents.Open($env:SHURAPDF_WORD_IN, $false, $true, $false, $password, $password)
  }
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
    [Console]::Error.WriteLine("$message (script line $($_.InvocationInfo.ScriptLineNumber))")
    $code = 2
  }
} finally {
  # Close and Quit take their arguments by reference: PowerShell refuses a plain 0
  # ("should be a System.Management.Automation.PSReference").
  if ($doc) { try { $doc.Close([ref]0) } catch { try { $doc.Close() } catch {} } }
  if ($word) {
    # Only this script's own Word, and only once it holds nothing else.
    try { if ($word.Documents.Count -eq 0) { $word.Quit([ref]0) } } catch {}
    try { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($word) } catch {}
  }
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
    // A poisoned lock only means an earlier conversion panicked; the next one may still run.
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = TempDir::new().map_err(|e| OfficeError::Failed(format!("couldn't make a working folder ({e})")))?;
    let input = dir.path.join(format!("document.{}", extension(bytes)));
    let output = dir.path.join("document.pdf");
    let errors = dir.path.join("errors.txt");
    let pid = dir.path.join("word.pid");
    std::fs::write(&input, bytes).map_err(|e| OfficeError::Failed(format!("couldn't write the Word file ({e})")))?;
    let said = |path: &Path| std::fs::read_to_string(path).map(|s| plain_message(&s)).unwrap_or_default();
    let code = run(SCRIPT, &Paths { input: &input, output: &output, errors: &errors, pid: &pid }, password, timeout_for(bytes.len()))?;
    match code {
        0 => {
            let pdf = std::fs::read(&output).map_err(|e| {
                let said = said(&errors);
                OfficeError::Failed(format!("Word wrote no PDF ({e}){}", if said.is_empty() { String::new() } else { format!(": {said}") }))
            })?;
            if pdf.starts_with(b"%PDF-") { Ok(pdf) } else { Err(OfficeError::Failed("Word wrote a file that isn't a PDF".into())) }
        }
        EXIT_UNAVAILABLE => Err(OfficeError::Unavailable),
        EXIT_CANNOT_START => {
            let said = said(&errors);
            Err(OfficeError::CannotStart(if said.is_empty() { "Word didn't start".into() } else { said }))
        }
        EXIT_PASSWORD => Err(OfficeError::Password),
        _ => {
            let said = said(&errors);
            Err(OfficeError::Failed(if said.is_empty() { format!("Word stopped with code {code}") } else { said }))
        }
    }
}

/// The name Word needs to see: it refuses a macro-enabled document or a template whose name
/// says otherwise. Word 97-2003 files are `.doc`; a `.docx` package's content types tell the
/// rest. Unreadable packages are left to Word as `.docx`.
fn extension(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&crate::COMPOUND_FILE) {
        // A Word 97-2003 file, or a password-protected package: Word reads both as .doc.
        return "doc";
    }
    let Some(types) = content_types(bytes) else { return "docx" };
    let main = |kind: &str| types.contains(&format!("application/vnd.{kind}"));
    if main("ms-word.document.macroEnabled.main+xml") {
        "docm"
    } else if main("ms-word.template.macroEnabledTemplate.main+xml") {
        "dotm"
    } else if main("openxmlformats-officedocument.wordprocessingml.template.main+xml") {
        "dotx"
    } else {
        "docx"
    }
}

/// `[Content_Types].xml` of a zip package (at most 1 MiB of it).
fn content_types(bytes: &[u8]) -> Option<String> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let part = zip.by_name("[Content_Types].xml").ok()?;
    let mut text = String::new();
    part.take(1024 * 1024).read_to_string(&mut text).ok()?;
    Some(text)
}

/// What PowerShell wrote to standard error, without the XML it wraps progress records in when
/// its output isn't a console (`#< CLIXML` and `<Objs …>` lines).
fn plain_message(stderr: &str) -> String {
    stderr.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("#< CLIXML") && !l.starts_with('<')).collect::<Vec<_>>().join(" ")
}

/// The files the script reads and writes.
struct Paths<'a> {
    input: &'a Path,
    output: &'a Path,
    errors: &'a Path,
    pid: &'a Path,
}

/// Runs the script and waits for it, at most `limit`. Its exit code; a PowerShell that can't
/// start means no Word. When the time is up, the script and the Word it started are both ended.
fn run(script: &str, paths: &Paths, password: Option<&str>, limit: Duration) -> Result<i32, OfficeError> {
    let error_file = std::fs::File::create(paths.errors).map_err(|e| OfficeError::Failed(e.to_string()))?;
    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand"])
        .arg(encoded(script))
        .env("SHURAPDF_WORD_IN", paths.input)
        .env("SHURAPDF_WORD_OUT", paths.output)
        .env("SHURAPDF_WORD_PID", paths.pid)
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
            Ok(Some(status)) => {
                // The script quits its Word; one that is still running a few seconds later
                // (a failed open can leave it holding a dialog nobody sees) is ended.
                linger_then_end(paths.pid);
                return Ok(status.code().unwrap_or(-1));
            }
            Ok(None) if started.elapsed() < limit => std::thread::sleep(Duration::from_millis(100)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                end_word(paths.pid);
                return Err(OfficeError::TimedOut);
            }
            Err(e) => return Err(OfficeError::Failed(e.to_string())),
        }
    }
}

/// Waits up to five seconds for the Word processes the script noted as its own to exit, then ends
/// those still running.
fn linger_then_end(pid_file: &Path) {
    let Ok(text) = std::fs::read_to_string(pid_file) else { return };
    let pids: Vec<u32> = text.split_whitespace().filter_map(|p| p.parse().ok()).collect();
    if pids.is_empty() {
        return;
    }
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        if !pids.iter().any(|pid| running(*pid)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    end_word(pid_file);
}

/// Whether a process with this id runs (`tasklist`, numbers only).
fn running(pid: u32) -> bool {
    let mut list = Command::new("tasklist.exe");
    list.args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"]).stdin(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        list.creation_flags(0x0800_0000);
    }
    list.output().is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&format!("\"{pid}\"")))
}

/// Ends the Word processes the script noted as its own (process ids, space-separated). Only
/// numbers are passed on, so the file can't make this run anything else.
fn end_word(pid_file: &Path) {
    let Ok(text) = std::fs::read_to_string(pid_file) else { return };
    for pid in text.split_whitespace().filter_map(|p| p.parse::<u32>().ok()) {
        let mut kill = Command::new("taskkill.exe");
        kill.args(["/F", "/PID", &pid.to_string()]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            kill.creation_flags(0x0800_0000);
        }
        let _ = kill.status();
    }
}

/// Where the working folder goes: the temp folder, unless its path has characters outside ASCII
/// (a Windows account with an Arabic name: `C:\Users\<name>\AppData\Local\Temp`), which Word
/// fails to open files from (measured: "Exception from HRESULT: 0x800A13E9"). Then its short
/// (8.3) path when Windows keeps one, else `%ProgramData%\ShuraPDF`.
fn word_safe_base() -> PathBuf {
    let temp = std::env::temp_dir();
    if temp.to_string_lossy().is_ascii() || !cfg!(windows) {
        return temp;
    }
    // `cmd` writes a name it can't show in its code page as `?`: no short name, then.
    if let Some(short) = short_path(&temp).filter(|p| p.to_string_lossy().is_ascii() && !p.to_string_lossy().contains('?')) {
        return short;
    }
    std::env::var_os("ProgramData")
        .map(|p| PathBuf::from(p).join("ShuraPDF"))
        .filter(|p| p.to_string_lossy().is_ascii() && std::fs::create_dir_all(p).is_ok())
        .unwrap_or(temp)
}

/// The 8.3 short form of an existing folder, from `cmd`'s `%~s` expansion (no API call needed).
fn short_path(path: &Path) -> Option<PathBuf> {
    let mut command = Command::new("cmd.exe");
    command.current_dir(path).stdin(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Raw, so no quotes are added around the loop; `%~sI` is the short form of `.`.
        command.raw_arg("/D /C for %I in (.) do @echo %~sI").creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = &mut command;
    let out = command.output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let line = text.lines().next()?.trim();
    (out.status.success() && !line.is_empty()).then(|| PathBuf::from(line))
}

/// The folder conversions through Word work in (diagnostics: `word_fidelity --checks`).
pub fn working_folder() -> PathBuf {
    word_safe_base()
}

/// A folder of its own for one conversion ([`word_safe_base`]), removed when dropped.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> std::io::Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
        let path = word_safe_base().join(format!("shurapdf-word-{}-{}-{nanos}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
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
    use super::{SCRIPT, base64, encoded, extension, timeout_for};
    use crate::testkit::{encrypted_docx, synthetic_doc, synthetic_docx, with_main_type};

    /// Word refuses a macro-enabled document or a template saved under a `.docx` name, so the
    /// file Word is given is named after what it holds.
    #[test]
    fn word_gets_the_name_its_content_needs() {
        assert_eq!(extension(&synthetic_docx()), "docx");
        assert_eq!(extension(&synthetic_doc(&["نص"])), "doc");
        assert_eq!(extension(&encrypted_docx("كلمة")), "doc");
        assert_eq!(extension(&with_main_type(&synthetic_docx(), "application/vnd.ms-word.document.macroEnabled.main+xml")), "docm");
        assert_eq!(
            extension(&with_main_type(&synthetic_docx(), "application/vnd.openxmlformats-officedocument.wordprocessingml.template.main+xml")),
            "dotx"
        );
        assert_eq!(extension(&with_main_type(&synthetic_docx(), "application/vnd.ms-word.template.macroEnabledTemplate.main+xml")), "dotm");
        for junk in [&b""[..], b"PK\x03\x04 not a zip", b"plain text"] {
            assert_eq!(extension(junk), "docx");
        }
    }

    /// PowerShell's progress records (XML on standard error) never reach the user's message.
    #[test]
    fn progress_xml_is_dropped_from_messages() {
        let stderr = "#< CLIXML\r\nCommand failed (script line 30)\r\n<Objs Version=\"1.1.0.1\"><Obj S=\"progress\"></Obj></Objs>\r\n";
        assert_eq!(super::plain_message(stderr), "Command failed (script line 30)");
        assert_eq!(super::plain_message(""), "");
    }

    /// Three minutes, more for big files, never more than ten.
    #[test]
    fn bigger_files_get_more_time() {
        assert_eq!(timeout_for(0).as_secs(), 180);
        assert_eq!(timeout_for(4 * 1024 * 1024).as_secs(), 300);
        assert_eq!(timeout_for(usize::MAX).as_secs(), 600);
    }

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
        let (input, pid) = (dir.join("in.docx"), dir.join("word.pid"));
        let paths = super::Paths { input: &input, output: &out, errors: &errors, pid: &pid };
        let code = run(probe, &paths, None, super::timeout_for(0)).unwrap();
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
