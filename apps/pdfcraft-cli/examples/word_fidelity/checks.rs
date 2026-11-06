//! Checks that ShuraPDF's connection to Microsoft Word holds in the situations users meet: each
//! one converts through real Word (Preferences ▸ Convert Word documents with ▸ Microsoft Word or
//! Automatic) and says what happened. Windows with Office only; elsewhere they are skipped.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use pdfcraft_word_import::testkit::{encrypted_docx, synthetic_doc, synthetic_docx, with_main_type};
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
    checks.push(check("Word: Word 97-2003 .doc", || {
        let r = with(WordEngine::Word, &synthetic_doc(&["نص عربي للتجربة", "Mixed English ١٢٣ text"]), None);
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
        let temp = work.join("مجلد مؤقت");
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
    set_engine(WordEngine::Auto);
    checks
}

/// The child half of the Arabic temp folder check: one conversion with Word, then exit.
pub fn convert_one() -> ! {
    let r = with(WordEngine::Word, &synthetic_docx(), None);
    println!("{} (temp: {})", outcome(&r), std::env::temp_dir().display());
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
