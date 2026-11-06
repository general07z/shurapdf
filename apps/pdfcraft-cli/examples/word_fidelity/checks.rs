//! Checks that ShuraPDF's connection to Microsoft Word holds in the situations users meet: each
//! one converts through real Word (Preferences ▸ Convert Word documents with ▸ Microsoft Word or
//! Automatic) and says what happened. Windows with Office only; elsewhere they are skipped.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use pdfcraft_word_import::testkit::{encrypted_docx, synthetic_docx, with_main_type};
use pdfcraft_word_import::{WordEngine, WordImportError, WordImportOptions, set_engine, word_to_pdf};

/// The argument that makes this program convert one document with Word and exit (0 on success):
/// the Arabic temp folder check runs it as a child with a different `TEMP`.
pub const CONVERT_ONE: &str = "--convert-one";

/// One check's outcome.
pub struct Check {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
    pub seconds: f64,
}

fn with(engine: WordEngine, bytes: &[u8], password: Option<&str>) -> Result<Vec<u8>, WordImportError> {
    set_engine(engine);
    word_to_pdf(bytes, &WordImportOptions { password: password.map(str::to_owned), ..Default::default() })
}

/// A PDF Word made (Word names itself as the producer); the built-in engine's PDFs don't.
fn made_by_word(pdf: &[u8]) -> bool {
    pdf.starts_with(b"%PDF-") && pdf.windows(9).any(|w| w == b"Microsoft")
}

fn outcome(r: &Result<Vec<u8>, WordImportError>) -> String {
    match r {
        Ok(pdf) if made_by_word(pdf) => format!("PDF from Word, {} KB", pdf.len() / 1024),
        Ok(pdf) => format!("PDF from the built-in engine, {} KB", pdf.len() / 1024),
        Err(e) => format!("error: {e}"),
    }
}

/// The running Word processes (`WINWORD.EXE`).
fn word_pids() -> Vec<u32> {
    let Ok(out) = Command::new("tasklist.exe").args(["/FI", "IMAGENAME eq WINWORD.EXE", "/FO", "CSV", "/NH"]).stderr(Stdio::null()).output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout).lines().filter_map(|l| l.split(',').nth(1)).filter_map(|pid| pid.trim_matches('"').parse().ok()).collect()
}

fn end(pids: &[u32]) {
    for pid in pids {
        let _ = Command::new("taskkill.exe").args(["/F", "/PID", &pid.to_string()]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

fn wait_for(mut done: impl FnMut() -> bool, limit: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    done()
}

fn check(name: &'static str, f: impl FnOnce() -> (bool, String)) -> Check {
    let start = Instant::now();
    let (passed, detail) = f();
    let c = Check { name, passed, detail, seconds: start.elapsed().as_secs_f64() };
    eprintln!("[{}] {} — {} ({:.1} s)", if c.passed { "PASS" } else { "FAIL" }, c.name, c.detail, c.seconds);
    c
}

/// Every check, in order. `work` is a folder this program may write in.
pub fn run(work: &Path) -> Vec<Check> {
    let docx = synthetic_docx();
    let before = word_pids();
    let mut checks = Vec::new();

    checks.push(check("Word: .docx", || {
        let r = with(WordEngine::Word, &docx, None);
        (r.as_ref().is_ok_and(|p| made_by_word(p)), outcome(&r))
    }));
    checks.push(check("Automatic uses Word when it is there", || {
        let r = with(WordEngine::Auto, &docx, None);
        (r.as_ref().is_ok_and(|p| made_by_word(p)), outcome(&r))
    }));
    checks.push(check("Word: Word 97-2003 .doc (saved by Word)", || {
        // Word refuses the minimal synthetic .doc as damaged; a .doc Word itself saved is what
        // users have.
        let Some(doc) = saved_as_doc(work, &docx) else { return (false, "Word couldn't save a .doc to test with".into()) };
        let r = with(WordEngine::Word, &doc, None);
        (r.as_ref().is_ok_and(|p| made_by_word(p)), outcome(&r))
    }));
    checks.push(check("Word: macro-enabled .docm (macros stay off)", || {
        let docm = with_main_type(&docx, "application/vnd.ms-word.document.macroEnabled.main+xml");
        let r = with(WordEngine::Word, &docm, None);
        (r.as_ref().is_ok_and(|p| made_by_word(p)), outcome(&r))
    }));
    checks.push(check("Word: template .dotx", || {
        let dotx = with_main_type(&docx, "application/vnd.openxmlformats-officedocument.wordprocessingml.template.main+xml");
        let r = with(WordEngine::Word, &dotx, None);
        (r.as_ref().is_ok_and(|p| made_by_word(p)), outcome(&r))
    }));
    let locked = encrypted_docx("كلمة-سر");
    checks.push(check("Word: protected file, no password → asks for it", || {
        let r = with(WordEngine::Word, &locked, None);
        (matches!(r, Err(WordImportError::Encrypted)), outcome(&r))
    }));
    checks.push(check("Word: protected file, wrong password → says so", || {
        let r = with(WordEngine::Word, &locked, Some("wrong"));
        (matches!(r, Err(WordImportError::WrongPassword)), outcome(&r))
    }));
    checks.push(check("Word: protected file, Arabic password → PDF", || {
        let r = with(WordEngine::Word, &locked, Some("كلمة-سر"));
        (r.as_ref().is_ok_and(|p| made_by_word(p)), outcome(&r))
    }));
    checks.push(check("Word: damaged file fails quickly, no hang", || {
        let start = Instant::now();
        let broken = docx.get(..docx.len() / 2).unwrap_or_default();
        let r = with(WordEngine::Word, broken, None);
        let quick = start.elapsed() < Duration::from_secs(90);
        (r.is_err() && quick, format!("{} after {:.0} s", outcome(&r), start.elapsed().as_secs_f64()))
    }));
    checks.push(check("Automatic: damaged file → clear error, no hang", || {
        let start = Instant::now();
        let broken = docx.get(..docx.len() / 2).unwrap_or_default();
        let r = with(WordEngine::Auto, broken, None);
        (start.elapsed() < Duration::from_secs(90), outcome(&r))
    }));
    checks.push(check("Three conversions at once (Combine)", || {
        set_engine(WordEngine::Word);
        let results: Vec<_> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..3).map(|_| s.spawn(|| word_to_pdf(&docx, &WordImportOptions::default()))).collect();
            handles.into_iter().map(|h| h.join().unwrap_or_else(|_| Err(WordImportError::Word("thread panicked".into())))).collect()
        });
        let ok = results.iter().filter(|r| r.as_ref().is_ok_and(|p| made_by_word(p))).count();
        (ok == 3, format!("{ok} of 3 made by Word"))
    }));
    checks.push(check("Temp folder with an Arabic name", || {
        // Outside the results folder, which is uploaded.
        let temp = std::env::temp_dir().join("ShuraPDF مجلد مؤقت");
        if let Err(e) = std::fs::create_dir_all(&temp) {
            return (false, format!("couldn't make {}: {e}", temp.display()));
        }
        let Ok(me) = std::env::current_exe() else { return (false, "no program path".into()) };
        let out = Command::new(me).arg(CONVERT_ONE).env("TEMP", &temp).env("TMP", &temp).output();
        match out {
            Ok(o) => (o.status.success(), String::from_utf8_lossy(&o.stdout).trim().to_owned()),
            Err(e) => (false, e.to_string()),
        }
    }));
    checks.push(check("No hidden Word left running afterwards", || {
        let left: Vec<u32> = Vec::new();
        let mut leftover = left;
        let clean = wait_for(
            || {
                leftover = word_pids().into_iter().filter(|p| !before.contains(p)).collect();
                leftover.is_empty()
            },
            Duration::from_secs(30),
        );
        end(&leftover);
        (clean, if clean { "none".into() } else { format!("{} Word process(es) left", leftover.len()) })
    }));
    checks.push(check("User's own Word stays open during a conversion", || {
        let started = Command::new("cmd.exe").args(["/C", "start", "", "/MIN", "winword.exe", "/q", "/n"]).status();
        if !started.is_ok_and(|s| s.success()) {
            return (false, "couldn't start Word as a user would".into());
        }
        let mut users = Vec::new();
        if !wait_for(
            || {
                users = word_pids().into_iter().filter(|p| !before.contains(p)).collect();
                !users.is_empty()
            },
            Duration::from_secs(60),
        ) {
            return (false, "Word didn't appear".into());
        }
        // Give Word time to settle, as a user's Word would be.
        std::thread::sleep(Duration::from_secs(5));
        let r = with(WordEngine::Word, &docx, None);
        let after = word_pids();
        let still_open = users.iter().all(|p| after.contains(p));
        end(&users);
        (
            r.as_ref().is_ok_and(|p| made_by_word(p)) && still_open,
            format!("{}; user's Word {}", outcome(&r), if still_open { "still open" } else { "was closed!" }),
        )
    }));
    set_engine(WordEngine::Auto);
    checks
}

/// `docx` saved by Word as a Word 97-2003 `.doc` (in `work`), or `None` when Word can't.
fn saved_as_doc(work: &Path, docx: &[u8]) -> Option<Vec<u8>> {
    let dir = work.join("doc-fixture");
    std::fs::create_dir_all(&dir).ok()?;
    let (input, output) = (dir.join("fixture.docx"), dir.join("fixture.doc"));
    std::fs::write(&input, docx).ok()?;
    let script = "$ErrorActionPreference = 'Stop'\n$w = New-Object -ComObject Word.Application\ntry {\n  $w.Visible = $false\n  $w.DisplayAlerts = 0\n  $d = $w.Documents.Open($env:FIXTURE_IN, $false, $true)\n  $d.SaveAs2($env:FIXTURE_OUT, 0)\n  $d.Close([ref]0)\n} finally { $w.Quit([ref]0) }\n";
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let status = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &base64(&utf16)])
        .env("FIXTURE_IN", &input)
        .env("FIXTURE_OUT", &output)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?;
    let bytes = status.success().then(|| std::fs::read(&output).ok()).flatten();
    let _ = std::fs::remove_dir_all(&dir);
    bytes
}

/// Standard base64 with padding (PowerShell's `-EncodedCommand`).
fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            out.push(if i <= chunk.len() { char::from(A[((n >> shift) & 63) as usize]) } else { '=' });
        }
    }
    out
}

/// The child half of the Arabic temp folder check: one conversion with Word, then exit.
pub fn convert_one() -> ! {
    let r = with(WordEngine::Word, &synthetic_docx(), None);
    println!("{} (temp: {}; Word works in {})", outcome(&r), std::env::temp_dir().display(), pdfcraft_word_import::working_folder().display());
    std::process::exit(if r.as_ref().is_ok_and(|p| made_by_word(p)) { 0 } else { 1 });
}

/// The checks as a Markdown table.
pub fn table(checks: &[Check]) -> String {
    let mut md = String::from("\n### Connection to Microsoft Word\n\n| Check | Result | Details | Time |\n|---|---|---|---|\n");
    for c in checks {
        md.push_str(&format!(
            "| {} | {} | {} | {:.1} s |\n",
            c.name,
            if c.passed { "✅ pass" } else { "❌ FAIL" },
            c.detail.replace('|', "/").replace('\n', " "),
            c.seconds
        ));
    }
    md
}
