//! `cargo run -p pdfcraft-word-import --example docx2pdf -- in.docx out.pdf`
//! (no input: converts the built-in synthetic Arabic/English sample).
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (input, output) = match args.as_slice() {
        [i, o] => (std::fs::read(i)?, o.clone()),
        [o] => (pdfcraft_word_import::testkit::synthetic_docx(), o.clone()),
        _ => return Err("usage: docx2pdf [in.docx] out.pdf".into()),
    };
    let pdf = pdfcraft_word_import::word_to_pdf(&input, &Default::default())?;
    std::fs::write(&output, pdf)?;
    Ok(())
}
