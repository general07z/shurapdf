use super::testkit::synthetic_docx;
use super::{WordImportError, WordImportOptions, docx_to_pdf};

#[test]
fn a_docx_becomes_a_pdf() {
    let pdf = docx_to_pdf(&synthetic_docx(), &WordImportOptions::default()).unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
}

#[test]
fn garbage_is_an_error_not_a_panic() {
    let err = docx_to_pdf(b"not a zip", &WordImportOptions::default()).unwrap_err();
    assert!(matches!(err, WordImportError::Read(_)));
}
