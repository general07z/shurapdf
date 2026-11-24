//! `combine` with a Word document: the .docx becomes PDF pages, in order, with its Arabic text.

use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_pdfcraft-cli");

#[test]
fn combine_takes_word_documents() {
    let dir = std::env::temp_dir().join(format!("pdfcraft-cli-word-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let first = dir.join("first.docx");
    let second = dir.join("agenda.docx");
    let out = dir.join("both.pdf");
    std::fs::write(&first, pdfcraft_word_import::testkit::synthetic_docx()).unwrap();
    std::fs::write(&second, pdfcraft_word_import::testkit::rich_arabic_docx()).unwrap();
    let run = Command::new(BIN).arg("combine").arg(&first).arg(&second).arg("--out").arg(&out).output().unwrap();
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let text = Command::new(BIN).arg("text").arg(&out).output().unwrap();
    assert!(text.status.success(), "{}", String::from_utf8_lossy(&text.stderr));
    let text = String::from_utf8(text.stdout).unwrap();
    let pages: Vec<&str> = text.split('\u{c}').filter(|p| !p.trim().is_empty()).collect();
    assert_eq!(pages.len(), 3, "{text}");
    assert!(pages[0].contains("مجلس الشورى"), "{}", pages[0]);
    assert!(pages[1].contains("جدول الأعمال"), "{}", pages[1]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The Windows 11 menu package's manifest, as package.ps1 asks for it: the certificate's subject as
/// publisher, and a clear error for an architecture File Explorer never runs as.
#[test]
fn shell_menu_manifest_names_the_publisher_and_refuses_x86() {
    let dir = std::env::temp_dir().join(format!("pdfcraft-cli-shell-menu-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("AppxManifest.xml");
    let subject = "CN=Shura Council, O=Shura Council, C=BH";
    let run = Command::new(BIN).args(["shell-menu-manifest", "--publisher", subject, "--arch", "x64", "--out"]).arg(&out).output().unwrap();
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let xml = std::fs::read_to_string(&out).unwrap();
    assert!(xml.contains(&format!(r#"Publisher="{subject}""#)) && xml.contains(r#"ProcessorArchitecture="x64""#), "{xml}");
    let x86 = Command::new(BIN).args(["shell-menu-manifest", "--publisher", subject, "--arch", "x86", "--out"]).arg(&out).output().unwrap();
    assert!(!x86.status.success());
    assert!(String::from_utf8_lossy(&x86.stderr).contains("use x64 or arm64"));
    let _ = std::fs::remove_dir_all(&dir);
}
