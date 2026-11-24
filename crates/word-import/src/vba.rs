//! A VBA project (`vbaProject.bin`) holding one standard module given as source text, written from
//! the public specification ([MS-OVBA]) so the Word template's button can run a few lines of
//! macro code. Only what Word needs to compile the source is written: the project information,
//! its references (the VBA library, OLE Automation and Word's object library, [`REFERENCES`]), and
//! the module's compressed source with no compiled cache (`_VBA_PROJECT` version 0xFFFF), so Word
//! compiles it on first use.
//!
//! The source must be ASCII: it is stored in the project's code page (1252) and nothing here
//! converts it. Output is deterministic: the protection fields use fixed, not random, bytes.

use std::io::Write as _;

/// The project's and the module's code page (Windows-1252).
const CODE_PAGE: u16 = 1252;
/// Largest stream compressed here: one chunk of literals ([`compress`]).
const MAX_COMPRESSED_INPUT: usize = 3640;

/// The type libraries every Word project references, by name and registered library id
/// ([MS-OVBA] 2.1.1.8): VBA's own (`MsgBox`, `Chr$`, `Len`, `CreateObject`, `vbObjectError`…),
/// OLE Automation, and Word's object library. Without them Word loads the project but stops at
/// the first such call with "Can't find project or library" (the owner's Word, #85). Windows finds
/// each library by its GUID and version; the path is only a hint.
const REFERENCES: [(&str, &str); 3] = [
    (
        "VBA",
        "*\\G{000204EF-0000-0000-C000-000000000046}#4.2#9#C:\\Program Files\\Common Files\\Microsoft Shared\\VBA\\VBA7.1\\VBE7.DLL#Visual Basic For Applications",
    ),
    (
        "Word",
        "*\\G{00020905-0000-0000-C000-000000000046}#8.7#0#C:\\Program Files\\Microsoft Office\\root\\Office16\\MSWORD.OLB#Microsoft Word 16.0 Object Library",
    ),
    ("stdole", "*\\G{00020430-0000-0000-C000-000000000046}#2.0#0#C:\\Windows\\System32\\stdole2.tlb#OLE Automation"),
];

/// The document module every Word project has, as Word writes it for a template.
///
/// Its base class is Word's Document class by its GUID (`0{…}`). `1Normal.ThisDocument` (a class of
/// the Normal project) is a document's, which references Normal; a template has no such reference,
/// and real Word stopped on it with "Can't find project or library" (the Council runner, #87).
const THIS_DOCUMENT: &str = "Attribute VB_Name = \"ThisDocument\"\r\nAttribute VB_Base = \"0{00020906-0000-0000-C000-000000000046}\"\r\n\
Attribute VB_GlobalNameSpace = False\r\nAttribute VB_Creatable = False\r\nAttribute VB_PredeclaredId = True\r\n\
Attribute VB_Exposed = True\r\nAttribute VB_TemplateDerived = True\r\nAttribute VB_Customizable = True\r\n";

/// How a project is laid out: what the template uses ([`Layout::default`]). The other layouts are
/// the variations the Council runner loaded in real Word to find what it refused (#87); the tests
/// keep them buildable.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    /// `PROJECTSYSKIND`: 1 (32-bit Windows) or 3 (64-bit).
    pub sys_kind: u32,
    /// How many of [`REFERENCES`] to write, in order (VBA first).
    pub references: usize,
    /// Whether the project has Word's `ThisDocument` module.
    pub this_document: bool,
    /// Whether `ThisDocument` names its base class (`VB_Base`).
    pub vb_base: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self { sys_kind: 1, references: REFERENCES.len(), this_document: true, vb_base: true }
    }
}

/// The `vbaProject.bin` compound file for a project `project` with Word's `ThisDocument` module and
/// one standard module `module` whose source is `source` (without its `Attribute VB_Name` line,
/// which this adds).
#[cfg(test)]
pub(crate) fn project(project: &str, module: &str, source: &str) -> Result<Vec<u8>, String> {
    project_with(project, module, source, Layout::default())
}

/// [`project`] laid out as `layout` says.
pub(crate) fn project_with(project: &str, module: &str, source: &str, layout: Layout) -> Result<Vec<u8>, String> {
    if ![project, module, source].iter().all(|s| s.is_ascii()) {
        return Err("VBA names and source must be ASCII".into());
    }
    if [project, module].iter().any(|s| s.is_empty() || s.len() > 31 || !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
        || module == "ThisDocument"
    {
        return Err("VBA names are 1 to 31 letters, digits or underscores".into());
    }
    // VBA refuses a module named as its project ("Name conflicts with existing module, project,
    // or object library"): Word then drops the whole project, and a button calling its macro
    // reports it can't be found (the owner's Word, with the template named after both).
    if project.eq_ignore_ascii_case(module) {
        return Err("a VBA module can't have its project's name".into());
    }
    let code = format!("Attribute VB_Name = \"{module}\"\r\n{}", source.replace("\r\n", "\n").replace('\n', "\r\n"));
    let module_stream = compress(code.as_bytes())?;
    let this_document: String = if layout.vb_base {
        THIS_DOCUMENT.to_owned()
    } else {
        THIS_DOCUMENT.replace("Attribute VB_Base = \"0{00020906-0000-0000-C000-000000000046}\"\r\n", "")
    };
    let document_stream = compress(this_document.as_bytes())?;
    let dir = compress(&dir_stream(project, module, layout))?;
    let id = "{9E394C0B-697E-4AEE-9FA6-446F51FB30DC}";
    let project_stream = project_stream(id, project, module, layout.this_document);
    let names = project_wm(module, layout.this_document);

    // Version 3 (512-byte sectors), the version Office writes its VBA projects in.
    let mut file = cfb::CompoundFile::create_with_version(cfb::Version::V3, std::io::Cursor::new(Vec::new())).map_err(|e| e.to_string())?;
    file.create_storage("/VBA").map_err(|e| e.to_string())?;
    let mut write = |path: &str, bytes: &[u8]| -> Result<(), String> {
        let mut stream = file.create_stream(path).map_err(|e| e.to_string())?;
        stream.write_all(bytes).map_err(|e| e.to_string())
    };
    // `_VBA_PROJECT`: reserved 0x61CC, version 0xFFFF (no compiled cache: compile the source),
    // reserved 0x00, reserved 0x0000.
    write("/VBA/_VBA_PROJECT", &[0xCC, 0x61, 0xFF, 0xFF, 0x00, 0x00, 0x00])?;
    write("/VBA/dir", &dir)?;
    if layout.this_document {
        write("/VBA/ThisDocument", &document_stream)?;
    }
    write(&format!("/VBA/{module}"), &module_stream)?;
    write("/PROJECT", project_stream.as_bytes())?;
    write("/PROJECTwm", &names)?;
    file.flush().map_err(|e| e.to_string())?;
    Ok(file.into_inner().into_inner())
}

/// [MS-OVBA] 2.4.1 compression of `data` as one chunk of literal tokens: every flag byte is 0, so
/// each byte is stored as it is. Valid for any decompressor, and enough for the small streams here.
fn compress(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() > MAX_COMPRESSED_INPUT {
        return Err(format!("a VBA stream of {} bytes is larger than this writer handles", data.len()));
    }
    // No data, no chunk: the signature byte alone.
    if data.is_empty() {
        return Ok(vec![0x01]);
    }
    let mut chunk = Vec::with_capacity(data.len() + data.len() / 8 + 1);
    for group in data.chunks(8) {
        chunk.push(0u8);
        chunk.extend_from_slice(group);
    }
    // Header: CompressedChunkSize - 3 (12 bits), signature 0b011, compressed flag 1.
    let size = chunk.len().checked_sub(1).and_then(|n| u16::try_from(n).ok()).ok_or("VBA chunk size out of range")?;
    let header = size | (0b011 << 12) | (1 << 15);
    let mut out = vec![0x01];
    out.extend_from_slice(&header.to_le_bytes());
    out.extend_from_slice(&chunk);
    Ok(out)
}

fn record(out: &mut Vec<u8>, id: u16, data: &[u8]) {
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(0).to_le_bytes());
    out.extend_from_slice(data);
}

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// The decompressed `dir` stream ([MS-OVBA] 2.3.4.2): project information, no references, one
/// procedural module whose source starts at offset 0 of its stream.
fn dir_stream(project: &str, module: &str, layout: Layout) -> Vec<u8> {
    let mut d = Vec::new();
    record(&mut d, 0x0001, &layout.sys_kind.to_le_bytes()); // PROJECTSYSKIND
    record(&mut d, 0x0002, &0x0409u32.to_le_bytes()); // PROJECTLCID
    record(&mut d, 0x0014, &0x0409u32.to_le_bytes()); // PROJECTLCIDINVOKE
    record(&mut d, 0x0003, &CODE_PAGE.to_le_bytes()); // PROJECTCODEPAGE
    record(&mut d, 0x0004, project.as_bytes()); // PROJECTNAME
    record(&mut d, 0x0005, &[]); // PROJECTDOCSTRING
    record(&mut d, 0x0040, &[]); //   ... its Unicode form
    record(&mut d, 0x0006, &[]); // PROJECTHELPFILEPATH
    record(&mut d, 0x003D, &[]); //   ... its second copy
    record(&mut d, 0x0007, &0u32.to_le_bytes()); // PROJECTHELPCONTEXT
    record(&mut d, 0x0008, &0u32.to_le_bytes()); // PROJECTLIBFLAGS
    // PROJECTVERSION: Id, Reserved (4), VersionMajor (u32), VersionMinor (u16).
    d.extend_from_slice(&0x0009u16.to_le_bytes());
    d.extend_from_slice(&4u32.to_le_bytes());
    d.extend_from_slice(&1u32.to_le_bytes());
    d.extend_from_slice(&0u16.to_le_bytes());
    record(&mut d, 0x000C, &[]); // PROJECTCONSTANTS
    record(&mut d, 0x003C, &[]); //   ... Unicode
    // PROJECTREFERENCES: each a REFERENCENAME (with its Unicode form after the reserved 0x003E)
    // and a REFERENCEREGISTERED (Size, SizeOfLibid, Libid, Reserved1 u32, Reserved2 u16).
    for (name, libid) in REFERENCES.iter().take(layout.references) {
        record(&mut d, 0x0016, name.as_bytes());
        d.extend_from_slice(&0x003Eu16.to_le_bytes());
        let unicode = utf16(name);
        d.extend_from_slice(&u32::try_from(unicode.len()).unwrap_or(0).to_le_bytes());
        d.extend_from_slice(&unicode);
        let mut registered = Vec::new();
        registered.extend_from_slice(&u32::try_from(libid.len()).unwrap_or(0).to_le_bytes());
        registered.extend_from_slice(libid.as_bytes());
        registered.extend_from_slice(&0u32.to_le_bytes());
        registered.extend_from_slice(&0u16.to_le_bytes());
        record(&mut d, 0x000D, &registered);
    }
    // PROJECTMODULES: Id, Size (2), Count; PROJECTCOOKIE: Id, Size (2), 0xFFFF.
    d.extend_from_slice(&0x000Fu16.to_le_bytes());
    d.extend_from_slice(&2u32.to_le_bytes());
    d.extend_from_slice(&(if layout.this_document { 2u16 } else { 1 }).to_le_bytes());
    d.extend_from_slice(&0x0013u16.to_le_bytes());
    d.extend_from_slice(&2u32.to_le_bytes());
    d.extend_from_slice(&0xFFFFu16.to_le_bytes());
    // Word's document module, then the standard one.
    for (name, document) in [("ThisDocument", true), (module, false)].into_iter().filter(|(_, document)| layout.this_document || !document) {
        record(&mut d, 0x0019, name.as_bytes()); // MODULENAME
        record(&mut d, 0x0047, &utf16(name)); // MODULENAMEUNICODE
        record(&mut d, 0x001A, name.as_bytes()); // MODULESTREAMNAME
        record(&mut d, 0x0032, &utf16(name)); //   ... Unicode
        record(&mut d, 0x001C, &[]); // MODULEDOCSTRING
        record(&mut d, 0x0048, &[]); //   ... Unicode
        record(&mut d, 0x0031, &0u32.to_le_bytes()); // MODULEOFFSET: the source starts at 0
        record(&mut d, 0x001E, &0u32.to_le_bytes()); // MODULEHELPCONTEXT
        record(&mut d, 0x002C, &0xFFFFu16.to_le_bytes()); // MODULECOOKIE
        record(&mut d, if document { 0x0022 } else { 0x0021 }, &[]); // MODULETYPE: document or procedural
        record(&mut d, 0x002B, &[]); // module terminator
    }
    record(&mut d, 0x0010, &[]); // dir terminator
    d
}

/// [MS-OVBA] 2.4.3 data encryption of `data`, as the `PROJECT` stream's CMG, DPB and GC values
/// hold it (hex). `seed` stands for the specification's random byte; fixed, the output is too.
fn encrypt(project_id: &str, seed: u8, data: &[u8]) -> String {
    let key = project_id.bytes().fold(0u8, u8::wrapping_add);
    let version_enc = seed ^ 2;
    let key_enc = seed ^ key;
    let mut out = vec![seed, version_enc, key_enc];
    let (mut unencrypted1, mut encrypted1, mut encrypted2) = (key, key_enc, version_enc);
    let mut push = |byte: u8, out: &mut Vec<u8>| {
        let enc = byte ^ encrypted2.wrapping_add(unencrypted1);
        out.push(enc);
        encrypted2 = encrypted1;
        encrypted1 = enc;
        unencrypted1 = byte;
    };
    let ignored = usize::from((seed & 6) / 2);
    for i in 0..ignored {
        push(0x5A ^ u8::try_from(i).unwrap_or(0), &mut out);
    }
    for byte in u32::try_from(data.len()).unwrap_or(0).to_le_bytes() {
        push(byte, &mut out);
    }
    for byte in data {
        push(*byte, &mut out);
    }
    out.iter().map(|b| format!("{b:02X}")).collect()
}

/// The `PROJECT` stream ([MS-OVBA] 2.3.1): the project's id, its module, name, and no protection
/// (state 0), no password (one zero byte) and visible (0xFF).
fn project_stream(id: &str, project: &str, module: &str, this_document: bool) -> String {
    let (document, workspace) = if this_document { ("Document=ThisDocument/&H00000000\r\n", "ThisDocument=0, 0, 0, 0, C\r\n") } else { ("", "") };
    format!(
        "ID=\"{id}\"\r\n{document}Module={module}\r\nName=\"{project}\"\r\nHelpContextID=\"0\"\r\nVersionCompatible32=\"393222000\"\r\n\
         CMG=\"{cmg}\"\r\nDPB=\"{dpb}\"\r\nGC=\"{gc}\"\r\n\r\n\
         [Host Extender Info]\r\n&H00000001={{3832D640-CF90-11CF-8E43-00A0C911005A}};VBE;&H00000000\r\n\r\n\
         [Workspace]\r\n{workspace}{module}=0, 0, 0, 0, C\r\n",
        cmg = encrypt(id, 0x33, &0u32.to_le_bytes()),
        dpb = encrypt(id, 0x35, &[0x00]),
        gc = encrypt(id, 0x37, &[0xFF]),
    )
}

/// The `PROJECTwm` stream ([MS-OVBA] 2.3.3): each module's name in the code page and in UTF-16.
fn project_wm(module: &str, this_document: bool) -> Vec<u8> {
    let mut out = Vec::new();
    for name in ["ThisDocument", module].into_iter().filter(|n| this_document || *n != "ThisDocument") {
        out.extend_from_slice(name.as_bytes());
        out.push(0);
        out.extend_from_slice(&utf16(name));
        out.extend_from_slice(&[0, 0]);
    }
    out.extend_from_slice(&[0, 0]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;

    /// [MS-OVBA] 2.4.1.3.1 decompression, written apart from [`compress`] to check it.
    fn decompress(data: &[u8]) -> Vec<u8> {
        assert_eq!(data[0], 1, "signature byte");
        let mut out = Vec::new();
        let mut at = 1;
        while at + 2 <= data.len() {
            let header = u16::from_le_bytes([data[at], data[at + 1]]);
            assert_eq!((header >> 12) & 0b111, 0b011, "chunk signature");
            let size = usize::from(header & 0x0FFF) + 3;
            let end = at + size;
            let mut i = at + 2;
            assert!(header & 0x8000 != 0, "compressed chunk");
            while i < end {
                let flags = data[i];
                i += 1;
                for bit in 0..8 {
                    if i >= end {
                        break;
                    }
                    assert_eq!(flags >> bit & 1, 0, "literal tokens only");
                    out.push(data[i]);
                    i += 1;
                }
            }
            at = end;
        }
        out
    }

    #[test]
    fn compression_round_trips() {
        for len in [0, 1, 7, 8, 9, 100, 3640] {
            let data: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
            assert_eq!(decompress(&compress(&data).unwrap()), data, "{len}");
        }
        assert!(compress(&[0; 3641]).is_err());
    }

    #[test]
    fn the_project_holds_the_module_source() {
        let bin = project("ShuraPDF", "ShuraPDFMacros", "Sub Hello()\nEnd Sub\n").unwrap();
        let mut file = cfb::CompoundFile::open(std::io::Cursor::new(bin)).unwrap();
        assert_eq!(file.version(), cfb::Version::V3, "Office's own version");
        let mut read = |p: &str| {
            let mut v = Vec::new();
            file.open_stream(p).unwrap().read_to_end(&mut v).unwrap();
            v
        };
        let source = String::from_utf8(decompress(&read("/VBA/ShuraPDFMacros"))).unwrap();
        assert_eq!(source, "Attribute VB_Name = \"ShuraPDFMacros\"\r\nSub Hello()\r\nEnd Sub\r\n");
        let dir = decompress(&read("/VBA/dir"));
        assert!(dir.windows(8).any(|w| w == b"ShuraPDF"), "the module is named in dir");
        // The VBA library is referenced, else Word stops at MsgBox with "Can't find project or
        // library" (the owner's Word), and its record says how long its library id is.
        let vba = b"*\\G{000204EF-0000-0000-C000-000000000046}";
        let at = dir.windows(vba.len()).position(|w| w == vba).expect("the VBA library is referenced");
        let size = u32::from_le_bytes(dir[at - 4..at].try_into().unwrap()) as usize;
        assert!(dir[at..at + size].ends_with(b"#Visual Basic For Applications"));
        assert_eq!(u16::from_le_bytes([dir[at - 10], dir[at - 9]]), 0x000D, "REFERENCEREGISTERED");
        assert_eq!(u32::from_le_bytes(dir[at - 8..at - 4].try_into().unwrap()) as usize, size + 10, "its size: length, id, reserved");
        for library in [&b"{00020905-0000-0000-C000-000000000046}"[..], b"{00020430-0000-0000-C000-000000000046}"] {
            assert!(dir.windows(library.len()).any(|w| w == library), "Word and OLE Automation too");
        }
        assert_eq!(read("/VBA/_VBA_PROJECT"), [0xCC, 0x61, 0xFF, 0xFF, 0, 0, 0]);
        let project_text = String::from_utf8(read("/PROJECT")).unwrap();
        assert!(
            project_text.contains("Module=ShuraPDFMacros\r\n")
                && project_text.contains("Name=\"ShuraPDF\"\r\n")
                && project_text.contains("Document=ThisDocument/&H00000000\r\n"),
            "{project_text}"
        );
        assert!(String::from_utf8(decompress(&read("/VBA/ThisDocument"))).unwrap().contains("VB_Base = \"0{00020906-0000-0000-C000-000000000046}\""));
    }

    #[test]
    fn non_ascii_source_and_bad_names_are_refused() {
        assert!(project("P", "M", "' تحويل").is_err());
        assert!(project("P", "Bad Name", "").is_err());
        // VBA refuses it, and Word drops the whole project (the owner's report).
        assert!(project("ShuraPDF", "ShuraPDF", "").is_err());
        assert!(project("ShuraPDF", "shurapdf", "").is_err());
    }
}
