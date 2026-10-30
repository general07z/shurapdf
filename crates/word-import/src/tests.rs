use super::testkit::{encrypted_docx, synthetic_doc, synthetic_docx};
use super::{WordImportError, WordImportOptions, word_to_pdf};

#[test]
fn a_docx_becomes_a_pdf() {
    let pdf = word_to_pdf(&synthetic_docx(), &WordImportOptions::default()).unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
}

#[test]
fn garbage_is_an_error_not_a_panic() {
    let err = word_to_pdf(b"not a zip", &WordImportOptions::default()).unwrap_err();
    assert!(matches!(err, WordImportError::Read(_)));
}

/// A Word 97-2003 `.doc` converts too (Arabic text from the piece table).
#[test]
fn a_doc_becomes_a_pdf() {
    let pdf = word_to_pdf(&synthetic_doc(&["مجلس الشورى", "Hello DOC"]), &WordImportOptions::default()).unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
}

/// A protected `.docx` asks for its password, refuses a wrong one and opens with the right one.
#[test]
fn a_protected_docx_needs_its_password() {
    let locked = encrypted_docx("سر-2026");
    let with = |pw: Option<&str>| word_to_pdf(&locked, &WordImportOptions { password: pw.map(str::to_owned), ..Default::default() });
    assert!(matches!(with(None), Err(WordImportError::Encrypted)), "{:?}", with(None).err());
    assert!(matches!(with(Some("")), Err(WordImportError::Encrypted)));
    assert!(matches!(with(Some("wrong")), Err(WordImportError::WrongPassword)));
    assert!(with(Some("سر-2026")).unwrap().starts_with(b"%PDF-"));
}

/// A compound file that is neither is an error, not a panic.
#[test]
fn a_broken_compound_file_is_an_error() {
    let mut junk = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    junk.extend_from_slice(&[0; 600]);
    assert!(matches!(word_to_pdf(&junk, &WordImportOptions::default()), Err(WordImportError::Read(_))));
}
