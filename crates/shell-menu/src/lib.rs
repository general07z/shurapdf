//! The Windows 11 File Explorer menu entry "Convert to PDF with <product>" on Word documents.
//!
//! Windows 11's first-level menu shows only entries that a packaged app registers as an
//! `IExplorerCommand` COM class; the classic `shell` verbs the installer also writes stay under
//! "Show more options". This DLL is that class. A sparse MSIX package
//! (`packaging/windows/shell-menu/AppxManifest.xml`) registers it for `.docx`, `.doc` and `.docm`.
//! Invoking it starts `pdfcraft.exe --convert-to-pdf <file>` beside this DLL for each selected
//! file: the same conversion as the classic entry (`apps/pdfcraft/src/convert.rs`).
//!
//! The `unsafe` code is the COM exports and the shell calls; see `Cargo.toml`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

// The COM server, only in the DLL (`--features server`): the app and the CLI link this crate for
// [`register`] and [`manifest`] and export nothing.
#[cfg(all(windows, feature = "server"))]
mod server;

#[cfg(windows)]
pub mod register;

/// The command's class id, also in the package manifest (`desktop5:Verb Clsid`).
pub const CLSID: u128 = 0x5d3f63ed_560f_4798_b041_701ae5598c64;

/// The product's name as this build has it.
fn product() -> &'static str {
    option_env!("PDFCRAFT_PRODUCT_NAME").filter(|n| !n.is_empty()).unwrap_or("PdfCraft")
}

/// The entry's label.
pub fn title() -> String {
    format!("Convert to PDF with {}", product())
}

/// The sparse package's name: the product's letters and digits, then `.ShellMenu`.
pub fn package_name() -> String {
    let product: String = product().chars().filter(char::is_ascii_alphanumeric).collect();
    format!("{}.ShellMenu", if product.is_empty() { "PdfCraft" } else { &product })
}

/// The package's version: the app's own, numeric part only (as the MSI's), then `.0`.
pub fn package_version() -> [u16; 4] {
    let mut v = [0u16; 4];
    let numeric = env!("CARGO_PKG_VERSION").split(['-', '+']).next().unwrap_or("");
    for (slot, part) in v.iter_mut().zip(numeric.split('.')) {
        *slot = part.parse().unwrap_or(0);
    }
    v
}

/// The sparse package's `AppxManifest.xml`. `publisher` is the signing certificate's subject,
/// exactly (Windows refuses a package whose publisher differs from its signer). The package has
/// no files of its own besides its logos: the DLL and `pdfcraft.exe` are in the install folder,
/// its external location. `arch` is the DLL's (`x64`, `arm64`).
pub fn manifest(publisher: &str, arch: &str) -> String {
    let [a, b, c, d] = package_version();
    let clsid = clsid_string();
    let name = package_name();
    let product = xml_escape(product());
    let publisher = xml_escape(publisher);
    let title = xml_escape(&title());
    let arch = xml_escape(arch);
    let mut xml = String::new();
    for ext in WORD_EXTENSIONS {
        xml.push_str(&format!(r#"<desktop5:ItemType Type=".{ext}"><desktop5:Verb Id="ConvertToPdf" Clsid="{clsid}"/></desktop5:ItemType>"#));
    }
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
  xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
  xmlns:uap10="http://schemas.microsoft.com/appx/manifest/uap/windows10/10"
  xmlns:desktop4="http://schemas.microsoft.com/appx/manifest/desktop/windows10/4"
  xmlns:desktop5="http://schemas.microsoft.com/appx/manifest/desktop/windows10/5"
  xmlns:com="http://schemas.microsoft.com/appx/manifest/com/windows10"
  xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
  IgnorableNamespaces="uap uap10 desktop4 desktop5 com rescap">
  <Identity Name="{name}" Publisher="{publisher}" Version="{a}.{b}.{c}.{d}" ProcessorArchitecture="{arch}"/>
  <Properties>
    <DisplayName>{product}</DisplayName>
    <PublisherDisplayName>{product}</PublisherDisplayName>
    <Logo>Assets\logo50.png</Logo>
    <uap10:AllowExternalContent>true</uap10:AllowExternalContent>
  </Properties>
  <Resources><Resource Language="en-us"/></Resources>
  <Dependencies><TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.19041.0" MaxVersionTested="10.0.26100.0"/></Dependencies>
  <Capabilities><rescap:Capability Name="runFullTrust"/><rescap:Capability Name="unvirtualizedResources"/></Capabilities>
  <Applications>
    <Application Id="App" Executable="pdfcraft.exe" uap10:TrustLevel="mediumIL" uap10:RuntimeBehavior="win32App">
      <uap:VisualElements AppListEntry="none" DisplayName="{product}" Description="{title}" BackgroundColor="transparent" Square150x150Logo="Assets\logo150.png" Square44x44Logo="Assets\logo44.png"/>
      <Extensions>
        <desktop4:Extension Category="windows.fileExplorerContextMenus">
          <desktop4:FileExplorerContextMenus>{xml}</desktop4:FileExplorerContextMenus>
        </desktop4:Extension>
        <com:Extension Category="windows.comServer">
          <com:ComServer>
            <com:SurrogateServer DisplayName="{title}">
              <com:Class Id="{clsid}" Path="{DLL}" ThreadingModel="STA"/>
            </com:SurrogateServer>
          </com:ComServer>
        </com:Extension>
      </Extensions>
    </Application>
  </Applications>
</Package>
"#
    )
}

/// The DLL's file name, in the install folder beside `pdfcraft.exe`.
pub const DLL: &str = "pdfcraft_shell_menu.dll";

/// The extensions the entry is on, as the classic entry's (`pdfcraft.wxs`).
const WORD_EXTENSIONS: [&str; 3] = ["docx", "doc", "docm"];

/// [`CLSID`] in registry form, without braces.
pub fn clsid_string() -> String {
    let v = CLSID;
    format!("{:08X}-{:04X}-{:04X}-{:04X}-{:012X}", v >> 96, (v >> 80) & 0xFFFF, (v >> 64) & 0xFFFF, (v >> 48) & 0xFFFF, v & 0xFFFF_FFFF_FFFF)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

/// Whether `path` is a Word document the entry converts (by its extension, as the manifest
/// registers it; the conversion itself checks the content).
pub fn is_word(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| WORD_EXTENSIONS.iter().any(|w| e.eq_ignore_ascii_case(w)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_converts_word_documents_only() {
        assert!(is_word(std::path::Path::new("docs/مذكرة.DOCX")));
        assert!(is_word(std::path::Path::new("a.doc")) && is_word(std::path::Path::new("a.docm")));
        assert!(!is_word(std::path::Path::new("a.pdf")) && !is_word(std::path::Path::new("docx")));
        assert!(title().starts_with("Convert to PDF with "));
    }

    #[test]
    fn the_manifest_names_the_class_the_dll_serves_for_word_files() {
        assert_eq!(clsid_string(), "5D3F63ED-560F-4798-B041-701AE5598C64");
        let xml = manifest("CN=Shura Council, O=Shura Council, C=BH", "x64");
        assert!(xml.contains(r#"Publisher="CN=Shura Council, O=Shura Council, C=BH""#), "{xml}");
        assert!(xml.contains(&format!(r#"<com:Class Id="{}" Path="{DLL}""#, clsid_string())), "{xml}");
        for ext in ["docx", "doc", "docm"] {
            assert!(xml.contains(&format!(r#"Type=".{ext}"><desktop5:Verb Id="ConvertToPdf" Clsid="{}""#, clsid_string())), "{ext}");
        }
        assert!(xml.contains("<uap10:AllowExternalContent>true</uap10:AllowExternalContent>"));
        let [a, b, c, d] = package_version();
        assert_eq!(d, 0);
        assert!(xml.contains(&format!(r#"Name="{}" "#, package_name())) && xml.contains(&format!(r#"Version="{a}.{b}.{c}.0""#)));
        assert!(package_name().ends_with(".ShellMenu") && package_name().chars().all(|c| c.is_ascii_alphanumeric() || c == '.'));
    }

    #[test]
    fn the_manifest_escapes_the_publisher() {
        assert!(manifest(r#"CN="A & B""#, "x64").contains(r#"Publisher="CN=&quot;A &amp; B&quot;""#));
    }
}
