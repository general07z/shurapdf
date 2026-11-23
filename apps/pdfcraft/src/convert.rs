//! File Explorer's "Convert to PDF with ShuraPDF" on a Word document (`--convert-to-pdf`).
//!
//! The launch converts before any window opens: each Word file becomes a PDF beside it (same
//! folder, same name), and that PDF is then opened exactly as a double-click opens one, in the
//! running app when there is one (`single_instance`). A file this can't convert (not a Word
//! document, protected by a password, unreadable, or one whose conversion fails) is opened as it
//! is instead, so the app shows what it always shows for it: the password prompt, or why it can't.
//! Nothing is overwritten: `Report.pdf` taken, the PDF is `Report (2).pdf`, and so on.
//!
//! The converter is the one Create uses (`Session::create_from_word`) with its default engine,
//! Automatic: Microsoft Word when it is installed (its own "Save as PDF"), else the built-in one.

use std::path::{Path, PathBuf};

/// Word files larger than this are left to the app, which reads them with its own limits.
const MAX_WORD_BYTES: u64 = 256 << 20;
/// Numbered names tried after `Report.pdf` (`Report (2).pdf` … `Report (99).pdf`).
const MAX_NUMBERED: u32 = 99;

/// What to open after converting `files`: the PDF written beside each Word file converted, and any
/// other file as it is.
pub(super) fn word_files_to_pdf(files: &[String]) -> Vec<String> {
    let session = pdfcraft_engine::Session::new();
    files
        .iter()
        .map(|file| match convert_one(&session, Path::new(file)) {
            Ok(pdf) => {
                log::info!("converted a Word document to {}", pdf.display());
                pdf.to_string_lossy().into_owned()
            }
            Err(why) => {
                log::warn!("not converted beside the Word file ({why}); opening it instead");
                file.clone()
            }
        })
        .collect()
}

fn convert_one(session: &pdfcraft_engine::Session, source: &Path) -> Result<PathBuf, String> {
    let size = std::fs::metadata(source).map_err(|e| e.to_string())?.len();
    if size > MAX_WORD_BYTES {
        return Err(format!("larger than {} MiB", MAX_WORD_BYTES >> 20));
    }
    let bytes = std::fs::read(source).map_err(|e| e.to_string())?;
    let name = source.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if pdfcraft_engine::source_kind(&name, &bytes) != Some(pdfcraft_engine::SourceKind::Word) {
        return Err("not a Word document".into());
    }
    let pdf = session.create_from_word(&bytes, None).map_err(|e| e.to_string())?;
    let target = beside(source, Path::exists).ok_or("every name for the PDF is taken")?;
    pdfcraft_platform::staging::write_atomically(&target, &pdf, pdfcraft_platform::staging::StagingName::TagThenSuffix).map_err(|e| e.to_string())?;
    Ok(target)
}

/// The PDF's path for the Word file `source`: `Report.pdf` beside it, else the first of
/// `Report (2).pdf` … that `exists` says is free. `None` when all are taken or `source` has no
/// file name.
fn beside(source: &Path, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let stem = source.file_stem()?.to_string_lossy().into_owned();
    let dir = source.parent().unwrap_or_else(|| Path::new(""));
    std::iter::once(format!("{stem}.pdf"))
        .chain((2..=MAX_NUMBERED).map(|n| format!("{stem} ({n}).pdf")))
        .map(|name| dir.join(name))
        .find(|candidate| !exists(candidate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pdf_goes_beside_the_word_file_and_never_over_another() {
        let source = Path::new("docs").join("مذكرة.docx");
        assert_eq!(beside(&source, |_| false), Some(Path::new("docs").join("مذكرة.pdf")));
        let taken = [Path::new("docs").join("مذكرة.pdf"), Path::new("docs").join("مذكرة (2).pdf")];
        assert_eq!(beside(&source, |p| taken.iter().any(|t| t == p)), Some(Path::new("docs").join("مذكرة (3).pdf")));
        assert_eq!(beside(&source, |_| true), None, "all taken: nothing is overwritten");
    }

    #[test]
    fn a_word_document_becomes_a_pdf_beside_it() {
        let dir = std::env::temp_dir().join(format!("shurapdf-convert-ok-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("مذكرة.docx");
        std::fs::write(&source, pdfcraft_word_import::testkit::rich_arabic_docx()).unwrap();
        std::fs::write(dir.join("مذكرة.pdf"), b"an older PDF, kept").unwrap();
        let opened = word_files_to_pdf(&[source.to_string_lossy().into_owned()]);
        let pdf = dir.join("مذكرة (2).pdf");
        assert_eq!(opened, [pdf.to_string_lossy().into_owned()], "the new PDF is what opens");
        assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"), "a PDF was written beside the Word file");
        assert_eq!(std::fs::read(dir.join("مذكرة.pdf")).unwrap(), b"an older PDF, kept", "nothing is overwritten");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_file_that_is_not_word_is_opened_as_it_is() {
        let dir = std::env::temp_dir().join(format!("shurapdf-convert-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let note = dir.join("note.docx");
        std::fs::write(&note, b"not a Word document").unwrap();
        let missing = dir.join("missing.docx");
        let files = [note.to_string_lossy().into_owned(), missing.to_string_lossy().into_owned()];
        assert_eq!(word_files_to_pdf(&files), files.to_vec(), "each is handed to the app unchanged");
        assert!(!dir.join("note.pdf").exists(), "no PDF is written for it");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
