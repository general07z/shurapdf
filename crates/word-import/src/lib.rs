//! pdfcraft-word-import — Create PDF from a Word document.
//!
//! WordCraft reads the `.docx` (`wordcraft-docx`) or Word 97-2003 `.doc` (`wordcraft-docbin`),
//! decrypting a password-protected one with the password given, lays it out (`wordcraft-layout`, called
//! inside `wordcraft_pdf::export`) and writes PDF bytes with embedded, subsetted fonts, so text
//! stays selectable and searchable — including shaped Arabic in bidirectional order. The bytes
//! are an ordinary PDF: the engine opens them like any other file.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

/// Largest Word file accepted (bytes). The reader has its own part limits; this caps the input.
pub const MAX_WORD_FILE: usize = 512 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum WordImportError {
    #[error("the Word file is larger than {} MiB", MAX_WORD_FILE / (1024 * 1024))]
    TooLarge,
    /// The file is password-protected and no password was given.
    #[error("the Word file is password protected")]
    Encrypted,
    /// The password given does not open the file.
    #[error("the password is incorrect")]
    WrongPassword,
    #[error("could not read the Word file: {0}")]
    Read(String),
    #[error("could not write the PDF: {0}")]
    Write(#[from] wordcraft_pdf::PdfError),
}

impl From<wordcraft_docx::DocxError> for WordImportError {
    fn from(e: wordcraft_docx::DocxError) -> Self {
        match e {
            wordcraft_docx::DocxError::PasswordRequired => Self::Encrypted,
            wordcraft_docx::DocxError::WrongPassword => Self::WrongPassword,
            e => Self::Read(e.to_string()),
        }
    }
}

impl From<wordcraft_docbin::DocbinError> for WordImportError {
    fn from(e: wordcraft_docbin::DocbinError) -> Self {
        match e {
            wordcraft_docbin::DocbinError::PasswordRequired => Self::Encrypted,
            wordcraft_docbin::DocbinError::WrongPassword => Self::WrongPassword,
            e => Self::Read(e.to_string()),
        }
    }
}

/// Options for the conversion (Acrobat's Create PDF ▸ Word settings, minimal set).
#[derive(Clone, Debug)]
pub struct WordImportOptions {
    /// Write a structure tree (tagged PDF) for accessibility.
    pub tagged: bool,
    /// Password for a protected `.docx` or `.doc`.
    pub password: Option<String>,
}

impl Default for WordImportOptions {
    fn default() -> Self {
        Self { tagged: true, password: None }
    }
}

/// The signature of an OLE compound file: a Word 97-2003 `.doc`, or a password-protected `.docx`.
const COMPOUND_FILE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// Convert Word bytes to PDF bytes: a `.docx` (zip package), a password-protected `.docx`
/// (compound file holding an encrypted package) or a Word 97-2003 `.doc` (compound file with a
/// WordDocument stream), told apart by their bytes, never by a name.
pub fn word_to_pdf(bytes: &[u8], opts: &WordImportOptions) -> Result<Vec<u8>, WordImportError> {
    if bytes.len() > MAX_WORD_FILE {
        return Err(WordImportError::TooLarge);
    }
    let password = opts.password.as_deref().filter(|p| !p.is_empty());
    let doc = if wordcraft_docx::is_encrypted(bytes) {
        wordcraft_docx::read_with_password(bytes, password)?
    } else if bytes.starts_with(&COMPOUND_FILE) {
        wordcraft_docbin::read_with_password(bytes, password)?
    } else {
        wordcraft_docx::read(bytes)?
    };
    let pdf_opts = wordcraft_pdf::PdfOptions { tagged: opts.tagged, ..Default::default() };
    Ok(wordcraft_pdf::export(&doc, &pdf_opts)?)
}

#[cfg(any(test, feature = "test-support"))]
pub mod testkit;

#[cfg(test)]
mod tests;
