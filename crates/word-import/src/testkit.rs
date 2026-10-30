//! Synthetic fixtures for tests: Word packages built in code, never real documents.

use std::io::Write;

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// A minimal synthetic `.docx`: one right-to-left Arabic paragraph and one English paragraph.
pub fn synthetic_docx() -> Vec<u8> {
    docx(
        r#"<w:p><w:pPr><w:bidi/></w:pPr><w:r><w:rPr><w:rtl/></w:rPr><w:t>مجلس الشورى</w:t></w:r></w:p>
<w:p><w:r><w:t>Hello PDF</w:t></w:r></w:p>"#,
        None,
    )
}

/// A two-page synthetic Arabic document with what Council letters carry: a heading (outline
/// level 1, so a bookmark), a line mixing Arabic, Latin, European and Arabic-Indic digits, a
/// right-to-left table, a page break and a header repeated on every page.
pub fn rich_arabic_docx() -> Vec<u8> {
    let rtl_para =
        |text: &str| format!(r#"<w:p><w:pPr><w:bidi/></w:pPr><w:r><w:rPr><w:rtl/></w:rPr><w:t xml:space="preserve">{text}</w:t></w:r></w:p>"#);
    let cell = |text: &str| format!(r#"<w:tc><w:tcPr><w:tcW w:w="4000" w:type="dxa"/></w:tcPr>{}</w:tc>"#, rtl_para(text));
    let body = format!(
        r#"<w:p><w:pPr><w:bidi/><w:outlineLvl w:val="0"/></w:pPr><w:r><w:rPr><w:rtl/><w:sz w:val="36"/></w:rPr><w:t>جدول الأعمال</w:t></w:r></w:p>
{mixed}
<w:tbl><w:tblPr><w:bidiVisual/><w:tblW w:w="8000" w:type="dxa"/><w:tblBorders><w:top w:val="single" w:sz="4"/><w:bottom w:val="single" w:sz="4"/><w:insideH w:val="single" w:sz="4"/><w:insideV w:val="single" w:sz="4"/></w:tblBorders></w:tblPr>
<w:tblGrid><w:gridCol w:w="4000"/><w:gridCol w:w="4000"/></w:tblGrid>
<w:tr>{c1}{c2}</w:tr><w:tr>{c3}{c4}</w:tr></w:tbl>
<w:p><w:r><w:br w:type="page"/></w:r></w:p>
{second}"#,
        mixed = rtl_para("المادة ٣ من القانون رقم 12 لسنة 2026 (Law 12)"),
        c1 = cell("البند"),
        c2 = cell("القرار"),
        c3 = cell("الميزانية"),
        c4 = cell("موافقة"),
        second = rtl_para("الصفحة الثانية"),
    );
    docx(&body, Some(&rtl_para("الأمانة العامة")))
}

/// A `.docx` package around `body` (the children of `w:body`), with `header` (the children of
/// `w:hdr`) as the default header when given.
#[allow(clippy::unwrap_used)] // test fixture over in-memory buffers
pub fn docx(body: &str, header: Option<&str>) -> Vec<u8> {
    let header_ref = if header.is_some() { r#"<w:headerReference w:type="default" r:id="rIdH"/>"# } else { "" };
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="{W}" xmlns:r="{R}"><w:body>{body}<w:sectPr>{header_ref}<w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="708" w:footer="708"/><w:bidi/></w:sectPr></w:body></w:document>"#
    );
    let header_override = if header.is_some() {
        r#"<Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>"#
    } else {
        ""
    };
    let content_types = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
{header_override}</Types>"#
    );
    let package_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;
    let mut parts = vec![("[Content_Types].xml", content_types), ("_rels/.rels", package_rels.to_string()), ("word/document.xml", document)];
    if let Some(header) = header {
        parts.push((
            "word/header1.xml",
            format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:hdr xmlns:w="{W}" xmlns:r="{R}">{header}</w:hdr>"#),
        ));
        parts.push((
            "word/_rels/document.xml.rels",
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rIdH" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/>
</Relationships>"#
                .to_string(),
        ));
    }
    let mut out = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut out);
    for (name, xml) in parts {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    out.into_inner()
}
