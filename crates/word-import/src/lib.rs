//! pdfcraft-word-import — Create PDF from a Word document.
//!
//! WordCraft reads the `.docx` (`wordcraft-docx`), lays it out (`wordcraft-layout`, called
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
    #[error("the Word file is password protected")]
    Encrypted,
    #[error("could not read the Word file: {0}")]
    Read(#[from] wordcraft_docx::DocxError),
    #[error("could not write the PDF: {0}")]
    Write(#[from] wordcraft_pdf::PdfError),
}

/// Options for the conversion (Acrobat's Create PDF ▸ Word settings, minimal set).
#[derive(Clone, Debug)]
pub struct WordImportOptions {
    /// Write a structure tree (tagged PDF) for accessibility.
    pub tagged: bool,
    /// Password for an encrypted `.docx`.
    pub password: Option<String>,
}

impl Default for WordImportOptions {
    fn default() -> Self {
        Self { tagged: true, password: None }
    }
}

/// Convert `.docx` bytes to PDF bytes.
pub fn docx_to_pdf(bytes: &[u8], opts: &WordImportOptions) -> Result<Vec<u8>, WordImportError> {
    if bytes.len() > MAX_WORD_FILE {
        return Err(WordImportError::TooLarge);
    }
    let doc = if wordcraft_docx::is_encrypted(bytes) {
        match opts.password.as_deref() {
            Some(pw) => wordcraft_docx::read_with_password(bytes, Some(pw))?,
            None => return Err(WordImportError::Encrypted),
        }
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
