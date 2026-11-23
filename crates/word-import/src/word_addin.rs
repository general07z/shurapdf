//! The Word template the Windows installer puts in Office's startup folder, so Word shows a tab of
//! the product's own with a button to save the open document as PDF, as Acrobat's add-in does.
//!
//! Word loads every template in its startup folder as a global template for every user of the PC,
//! and that folder is one of Office's trusted locations by default. This template holds no macro:
//! its button is Word's own "Save as PDF or XPS" command (`idMso="FileSaveAsPdfOrXps"`), so it
//! works whatever the macro policy, and Word itself writes the PDF.
//!
//! The package is our own text, written here (no file made by Word is shipped).

use std::io::Write as _;

/// The template, a `.dotx` package, with a ribbon tab named `product`.
pub fn template(product: &str) -> Result<Vec<u8>, String> {
    let product = xml_escape(product);
    let parts: [(&str, String); 5] = [
        (
            "[Content_Types].xml",
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
                r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
                r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
                r#"<Default Extension="xml" ContentType="application/xml"/>"#,
                r#"<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.template.main+xml"/>"#,
                r#"</Types>"#
            )
            .to_owned(),
        ),
        (
            "_rels/.rels",
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
                r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>"#,
                r#"<Relationship Id="rId2" Type="http://schemas.microsoft.com/office/2007/relationships/ui/extensibility" Target="customUI/customUI.xml"/>"#,
                r#"</Relationships>"#
            )
            .to_owned(),
        ),
        (
            "word/document.xml",
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p/></w:body></w:document>"#
            )
            .to_owned(),
        ),
        (
            "customUI/customUI.xml",
            format!(
                concat!(
                    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
                    r#"<customUI xmlns="http://schemas.microsoft.com/office/2006/01/customui">"#,
                    r#"<ribbon><tabs><tab id="ShuraPdfTab" label="{product}">"#,
                    r#"<group id="ShuraPdfGroup" label="PDF">"#,
                    r#"<button idMso="FileSaveAsPdfOrXps" size="large" label="تحويل إلى PDF" screentip="تحويل إلى PDF" supertip="يحفظ المستند بصيغة PDF بمحرك Word نفسه."/>"#,
                    r#"</group></tab></tabs></ribbon></customUI>"#
                ),
                product = product
            ),
        ),
        (
            "docProps/app.xml",
            format!(
                concat!(
                    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
                    r#"<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>{product}</Application></Properties>"#
                ),
                product = product
            ),
        ),
    ];
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, body) in &parts {
        out.start_file(*name, options).map_err(|e| e.to_string())?;
        out.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
    }
    out.finish().map(std::io::Cursor::into_inner).map_err(|e| e.to_string())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;

    fn part(package: &[u8], name: &str) -> String {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(package)).unwrap();
        let mut text = String::new();
        zip.by_name(name).unwrap().read_to_string(&mut text).unwrap();
        text
    }

    #[test]
    fn the_template_is_a_word_template_with_a_pdf_button_and_no_macro() {
        let package = template("ShuraPDF").unwrap();
        let types = part(&package, "[Content_Types].xml");
        assert!(types.contains("wordprocessingml.template.main+xml"), "a .dotx: {types}");
        assert!(!types.contains("macroEnabled") && !types.contains("vbaProject"), "no macro");
        assert!(part(&package, "_rels/.rels").contains(r#"Target="customUI/customUI.xml""#), "the ribbon part is linked");
        let ui = part(&package, "customUI/customUI.xml");
        assert!(ui.contains(r#"label="ShuraPDF""#) && ui.contains(r#"idMso="FileSaveAsPdfOrXps""#), "{ui}");
        assert!(!ui.contains("onAction"), "Word's own command, no callback");
    }

    #[test]
    fn the_product_name_is_escaped() {
        let ui = part(&template("A & <B>").unwrap(), "customUI/customUI.xml");
        assert!(ui.contains(r#"label="A &amp; &lt;B&gt;""#), "{ui}");
    }
}
