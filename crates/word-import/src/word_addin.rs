//! The Word template ShuraPDF puts in Word's startup folders, so Word shows a tab of the product's
//! own with a button that saves the open document as PDF beside it and opens that PDF in the app,
//! as Acrobat's add-in does.
//!
//! Word loads every template in its startup folders as a global template, and Office trusts them
//! by default. The tab has two buttons:
//! - "تحويل إلى PDF" runs [`MACRO`]: Word's own `ExportAsFixedFormat` writes `Name.pdf` beside the
//!   document (`Name (2).pdf` … when taken, never over another file), then the PDF opens in the app
//!   through its App Paths entry (`pdfcraft.exe`). It needs macros to run from Word's trusted
//!   startup folders, as they do unless a policy forbids it.
//! - "حفظ PDF…" is Word's own Save as PDF or XPS command (`idMso`), with no macro, so it works
//!   whatever the macro policy.
//!
//! The package is our own text, written here (no file made by Word is shipped); the macro is a
//! VBA project written from the specification ([`crate::vba`]).

use std::io::Write as _;

/// The macro module behind the first button. ASCII only (see [`crate::vba`]); the document's path
/// may hold any characters, which VBA keeps as Unicode and hands to COM unchanged.
pub const MACRO: &str = r#"Option Explicit

' ShuraPDF: save the active document as PDF beside it and open the PDF in ShuraPDF.
Public Sub ShuraPDF_Convert(control As Object)
    ShuraPDF_ConvertActive
End Sub

Public Sub ShuraPDF_ConvertActive()
    On Error GoTo Failed
    If Application.Documents.Count = 0 Then Exit Sub
    Dim doc As Object
    Set doc = Application.ActiveDocument
    ' A new document is saved first (wdDialogFileSaveAs = 84), as Word's own export asks.
    If Len(doc.Path) = 0 Then
        If Application.Dialogs(84).Show <> -1 Then Exit Sub
        If Len(doc.Path) = 0 Then Exit Sub
    End If
    Dim pdf As String
    pdf = ShuraPDF_Export(doc)
    CreateObject("Shell.Application").ShellExecute "pdfcraft.exe", Chr$(34) & pdf & Chr$(34), "", "open", 1
    Exit Sub
Failed:
    MsgBox "ShuraPDF: " & Err.Description, vbExclamation
End Sub

' The PDF of doc, saved beside it under a free name; its path. Also what the test on Word runs.
Public Function ShuraPDF_Export(doc As Object) As String
    Dim files As Object
    Set files = CreateObject("Scripting.FileSystemObject")
    Dim base As String
    base = files.BuildPath(doc.Path, files.GetBaseName(doc.FullName))
    Dim pdf As String
    pdf = base & ".pdf"
    Dim n As Long
    n = 2
    Do While files.FileExists(pdf)
        If n > 99 Then Err.Raise vbObjectError + 1, , "every name for the PDF is taken"
        pdf = base & " (" & n & ").pdf"
        n = n + 1
    Loop
    ' wdExportFormatPDF = 17, wdExportOptimizeForPrint = 0, wdExportAllDocument = 0,
    ' wdExportDocumentContent = 0, wdExportCreateHeadingBookmarks = 1.
    doc.ExportAsFixedFormat pdf, 17, False, 0, 0, 1, 1, 0, True, True, 1, True, True, False
    ShuraPDF_Export = pdf
End Function
"#;

/// The template, a macro-enabled `.dotm` package, with a ribbon tab named `product`.
pub fn template(product: &str) -> Result<Vec<u8>, String> {
    template_with(product, crate::vba::Layout::default())
}

/// [`template`] with its VBA project laid out as `layout` says: the Council runner loads such
/// variations in real Word to find what it refuses.
#[cfg(feature = "test-support")]
pub fn template_variant(product: &str, layout: crate::vba::Layout) -> Result<Vec<u8>, String> {
    template_with(product, layout)
}

fn template_with(product: &str, layout: crate::vba::Layout) -> Result<Vec<u8>, String> {
    // The module's name differs from the project's: VBA refuses a module named as its project.
    let project = crate::vba::project_with("ShuraPDF", "ShuraPDFMacros", MACRO, layout)?;
    let product = xml_escape(product);
    let parts: [(&str, String); 6] = [
        (
            "[Content_Types].xml",
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
                r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
                r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
                r#"<Default Extension="xml" ContentType="application/xml"/>"#,
                r#"<Default Extension="bin" ContentType="application/vnd.ms-office.vbaProject"/>"#,
                r#"<Override PartName="/word/document.xml" ContentType="application/vnd.ms-word.template.macroEnabledTemplate.main+xml"/>"#,
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
            "word/_rels/document.xml.rels",
            concat!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
                r#"<Relationship Id="rId1" Type="http://schemas.microsoft.com/office/2006/relationships/vbaProject" Target="vbaProject.bin"/>"#,
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
                    r#"<button id="ShuraPdfConvert" size="large" imageMso="FileSaveAsPdfOrXps" label="تحويل إلى PDF" onAction="ShuraPDF_Convert" screentip="تحويل إلى PDF" supertip="يحفظ المستند PDF بجانبه ويفتحه في {product}."/>"#,
                    r#"<button idMso="FileSaveAsPdfOrXps" label="حفظ PDF…" screentip="حفظ PDF…" supertip="أمر Word نفسه: يسأل عن الاسم والمكان."/>"#,
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
    out.start_file("word/vbaProject.bin", options).map_err(|e| e.to_string())?;
    out.write_all(&project).map_err(|e| e.to_string())?;
    out.finish().map(std::io::Cursor::into_inner).map_err(|e| e.to_string())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;

    fn part(package: &[u8], name: &str) -> Vec<u8> {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(package)).unwrap();
        let mut bytes = Vec::new();
        zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
        bytes
    }

    fn text(package: &[u8], name: &str) -> String {
        String::from_utf8(part(package, name)).unwrap()
    }

    #[test]
    fn the_template_is_a_macro_template_with_both_buttons() {
        let package = template("ShuraPDF").unwrap();
        let types = text(&package, "[Content_Types].xml");
        assert!(types.contains("macroEnabledTemplate.main+xml") && types.contains("vnd.ms-office.vbaProject"), "{types}");
        assert!(text(&package, "_rels/.rels").contains(r#"Target="customUI/customUI.xml""#), "the ribbon part is linked");
        assert!(text(&package, "word/_rels/document.xml.rels").contains(r#"Target="vbaProject.bin""#), "the macro is linked");
        let ui = text(&package, "customUI/customUI.xml");
        assert!(ui.contains(r#"label="ShuraPDF""#), "{ui}");
        assert!(ui.contains(r#"onAction="ShuraPDF_Convert""#), "the direct button runs the macro");
        assert!(ui.contains(r#"idMso="FileSaveAsPdfOrXps""#), "Word's own command stays as the fallback");
        assert!(MACRO.contains("Sub ShuraPDF_Convert(control As Object)"), "the button's callback exists");
        assert!(part(&package, "word/vbaProject.bin").starts_with(&[0xD0, 0xCF, 0x11, 0xE0]), "a compound file");
    }

    #[test]
    fn the_product_name_is_escaped() {
        let ui = text(&template("A & <B>").unwrap(), "customUI/customUI.xml");
        assert!(ui.contains(r#"label="A &amp; &lt;B&gt;""#), "{ui}");
    }
}
