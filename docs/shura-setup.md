# ShuraPDF: setup

> **Status:** live · **Last updated:** 2026-10-30 · **Change:** the Word engine is now fetched from GitHub; `shuraword` is optional

Your projects on GitHub:

| Repository | Based on | Visibility | Role |
|---|---|---|---|
| [general07z/shurapdf](https://github.com/general07z/shurapdf) | `storytold/pdfcraft` | public | **The app you run**: PDF reader and editor (Acrobat Pro-like) |
| [general07z/shuraword](https://github.com/general07z/shuraword) | `storytold/wordcraft` | private | Your copy of the Word engine, needed only when you change the Word engine itself |

Related documents: `docs/arabic-gaps.md` (Arabic goals and work queue) and `CLAUDE.md` (rules for Claude Code).

> **Names:** only the **repository** names changed. The Rust crates inside keep their original names (`pdfcraft-engine`, `wordcraft-docx`, …), so every `cargo -p` command uses `pdfcraft-…`. Don't rename the crates: that would break pulling updates from upstream.

---

## 1. Get the project on your machine (one time)

```bash
mkdir ~/shura && cd ~/shura
```

```bash
git clone https://github.com/general07z/shurapdf
```

```bash
git clone https://github.com/storytold/craft-fonts
```

You do **not** need `shuraword` to build. Cargo downloads the Word engine itself (`storytold/wordcraft`, pinned in `Cargo.toml` to the same commit `shuraword` is on). This is because `shuraword` is private: GitHub's automated checks for the public `shurapdf` cannot read it, and the CI only accepts Git dependencies from `github.com/storytold`.

Add the original project as `upstream` so you can pull its fixes later (do this often; upstream moves fast):

```bash
cd ~/shura/shurapdf && git remote add upstream https://github.com/storytold/pdfcraft
```

```bash
git fetch upstream && git merge upstream/main
```

### Local shuraword (only when you change the Word engine)

Clone it next to `shurapdf`:

```bash
cd ~/shura && git clone https://github.com/general07z/shuraword
```

Then add these lines to the end of `shurapdf/Cargo.toml` **on your machine only**. Don't commit them: CI rejects path overrides.

```toml
[patch."https://github.com/storytold/wordcraft"]
wordcraft-docx = { path = "../shuraword/crates/docx" }
wordcraft-pdf = { path = "../shuraword/crates/pdf" }
```

When a change in `shuraword` should reach everyone, send it upstream to `storytold/wordcraft`, or make `shuraword` public and point the two `git =` lines in `Cargo.toml` at it (and allow it in `deny.toml` and the CI source check).

## 2. Download a ready build (no tools needed)

GitHub builds Windows packages on every push to `main` (`.github/workflows/shura-windows.yml`): **Actions ▸ ShuraPDF Windows build ▸ the latest run ▸ Artifacts ▸ ShuraPDF-windows-x64** (a zip holding the `.msi` installer and the portable `.zip`; kept 30 days). A tag such as `v0.15.0-shura.1` also makes a **Release** with the same files. Signing: `docs/code-signing.md`.

## 2b. Build, test and run yourself

Windows needs the MSVC build tools and `rustup`. The minimum is Rust 1.95.

```bash
cd ~/shura/shurapdf && cargo test -p pdfcraft-word-import -p pdfcraft-engine --lib -j 4
```

```bash
CRAFT_FONTS_DIR=../craft-fonts cargo run -p pdfcraft --release
```

In the app, go to **File ▸ Create ▸ Create PDF from Word…**, or **All tools ▸ Create ▸ Word document**.

A **Council build** that starts in Arabic for every user, whatever their Windows language (users can still change it in Preferences):

```bash
PDFCRAFT_DEFAULT_LANGUAGE=ar CRAFT_FONTS_DIR=../craft-fonts cargo build -p pdfcraft --release
```

On Windows PowerShell:

```powershell
$env:PDFCRAFT_DEFAULT_LANGUAGE="ar"; $env:CRAFT_FONTS_DIR="..\craft-fonts"; cargo build -p pdfcraft --release
```

`CRAFT_FONTS_DIR` matters for Arabic: without it the build has no Arabic face for text you add to a PDF (Word conversion uses the system fonts and doesn't need it).

To convert from the command line without the app:

```bash
cargo run -p pdfcraft-word-import --features test-support --example docx2pdf -- input.docx output.pdf
```

---

## 3. How Word import works (architecture)

```
 .docx bytes
    │
    ▼
 pdfcraft-word-import          ← NEW crate, layer L4 (same as pdfcraft-create)
    wordcraft_docx::read()        parse OOXML → wordcraft_doc::Document
    wordcraft_pdf::export()       wordcraft-layout (shaping, bidi, pagination) → PDF bytes
    │
    ▼
 pdfcraft-engine  Session::create_from_word()   ← validates: reopen with pdfcraft-cos + render check
    │
    ▼
 Session::open_new(name, bytes)  ← the existing API that Combine, Extract and Create already use
    │                              → native, unsaved, fully editable document
    ▼
 ui-egui  create_ui.rs  create_from_word_dialog()  ← File ▸ Create menu, All tools card
```

### Corrections to the original plan

- **`wordcraft-doc` + `wordcraft-layout` are not enough.** Layout makes positioned pages, not PDF bytes. You need `wordcraft-docx` (reader) and `wordcraft-pdf` (writer). `wordcraft-pdf` calls `wordcraft-layout` internally, and the other two crates come in automatically.
- **Do not depend on `wordcraft-engine`.** It pulls in WordCraft's whole session, proofing and UI. The two crates above are the narrow, stable API.
- **Workspace inheritance works across the two repositories.** The WordCraft crates use `version.workspace = true`. Cargo resolves that against **WordCraft's** own root `Cargo.toml` (for a Git or a path dependency alike), so nothing in WordCraft needs to change.
- **The one hidden trap, which the build caught:** WordCraft's `craft-pdf` needs a **patched `krilla`** that WordCraft installs through its own `[patch.crates-io]`. Patches never carry over to another workspace, so PdfCraft had to copy it. Without the patch the build fails with `SerializeSettings has no field group_color_space`. With it, PdfCraft's 383 engine tests still pass.
- **Where to put the trigger:** inject it at the **engine façade**, `Session::create_from_word` in `crates/engine/src/session/files.rs`, next to `create_from_images` and `create_from_text`. The UI only calls it. The reason is that PdfCraft's layering rule says frontends never touch parsing or writing crates. It also means the CLI, MCP and automation can reuse the same function later.

### Code (key parts)

**Root `Cargo.toml`:**
```toml
[workspace.dependencies]
pdfcraft-word-import = { path = "crates/word-import", version = "0.15.0" }
wordcraft-docx = { git = "https://github.com/storytold/wordcraft", rev = "5d063c5bc266dc0111b65c9f18c05db92675ce63" }
wordcraft-pdf  = { git = "https://github.com/storytold/wordcraft", rev = "5d063c5bc266dc0111b65c9f18c05db92675ce63" }

[patch.crates-io]
# WordCraft's PDF writer (craft-pdf) needs the craft-libs krilla fork; mirror WordCraft's own patch.
krilla = { git = "https://github.com/storytold/craft-libs", rev = "92d392cff132eaf53e13afff77f38b5c11b73aa0" }
```
Keep the `krilla` `rev` equal to the one in WordCraft's `Cargo.toml` at the pinned commit. When you move the WordCraft pin, check it again.

**Bridge (`crates/word-import/src/lib.rs`):**
```rust
pub fn docx_to_pdf(bytes: &[u8], opts: &WordImportOptions) -> Result<Vec<u8>, WordImportError> {
    if bytes.len() > MAX_WORD_FILE { return Err(WordImportError::TooLarge); }
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
```

**Engine (`crates/engine/src/session/files.rs`):**
```rust
pub fn create_from_word(&self, bytes: &[u8], password: Option<&str>) -> Result<Arc<Vec<u8>>, EditError> {
    let opts = pdfcraft_word_import::WordImportOptions { password: password.map(str::to_owned), ..Default::default() };
    let pdf = Arc::new(pdfcraft_word_import::docx_to_pdf(bytes, &opts)?);
    let doc = pdfcraft_cos::Document::open(pdf.clone()).map_err(|e| EditError::Reopen(e.to_string()))?;
    let expected = pdfcraft_organize::page_count(&doc)?;
    pdfcraft_render::check_pages(pdf.clone(), None, expected).map_err(|e| EditError::Reopen(e.to_string()))?;
    Ok(pdf)
}
```

**Opening it as a session document (UI or anywhere else):**
```rust
let bytes = session.create_from_word(&docx_bytes, None)?;
let id = session.open_new("Letter.pdf", bytes)?;   // dirty + unsaved, like any created file
```

### Verified

| Check | Result |
|---|---|
| `cargo test -p pdfcraft-word-import` | 2/2 pass (including garbage input returning an error, not a panic) |
| `cargo test -p pdfcraft-engine --lib` | 383 pass, 0 fail (the new end-to-end Arabic test included) |
| `cargo check -p pdfcraft-ui-egui` | builds |
| `cargo clippy -D warnings`, `cargo fmt --check` | clean |
| `cargo xtask layers` | OK, 38 crates (the new crate is registered at L4) |
| `pdffonts` | fonts embedded, subset, with Unicode maps |
| `pdftotext` | `مجلس الشورى` comes out in correct logical order |
| Visual render | Arabic letters are joined (shaped) and the paragraph is right-aligned |

Not yet verified: real Council documents (tables, headers and footers, Arabic fonts such as Simplified Arabic or Traditional Arabic), the Windows build, and the UI button clicked by hand.

---

## 4. The method (rules + checklist + oracle)

PdfCraft's real method does not use files named "GAP_TRACKER.md" or "qpdf only". It has four parts, and **your fork inherits all of them**:

1. **Rules:** `CLAUDE.md` + `AGENTS.md` (never panic, clean-room, layering, test-first, scoped builds).
2. **Machine-readable checklist:** `parity/acrobat-features.toml`, about 845 features, each `planned/partial/shipped` with **evidence = named tests**. `cargo xtask parity` prints the score.
3. **Ranked gap list:** `docs/gaps.md` (what to do next, in order).
4. **Oracles:** `crates/oracle`. External programs (qpdf, MuPDF, veraPDF, and others) check PdfCraft's output, and *a missing oracle is a declared skip, never a silent pass*. Install `qpdf` and `mupdf-tools` on your machine so these tests run.

Plus mechanical guards: `cargo xtask layers` (architecture) and `cargo xtask assets` (licensing).

**What this fork adds for your goals:** the "Fork goals" section at the top of `CLAUDE.md` and the work queue in `docs/arabic-gaps.md`. Each Claude Code session starts with:

> Read CLAUDE.md and docs/arabic-gaps.md. Take the first unchecked item in the work queue. Follow the protocol, prove the test fails before the fix, update the parity entry and the tracker, and open a PR.

---

## 5. Important cautions

- **Licence:** both projects are MIT OR Apache-2.0, so forking, modifying and internal use are allowed. Keep `LICENSE-*`, `NOTICE` and `ATTRIBUTION.*`.
- **"Looks like Acrobat Pro":** PdfCraft already copies Acrobat's *layout* (Home, All tools, right-hand panels). Never add Adobe icons, logos or names. That is a legal risk, and `cargo xtask assets` will fail.
- **Government use:** treat this as a pilot until it has passed testing on real Council documents and a security review. For legally binding signatures, use the Council's approved certificates (PKCS#12 or the Windows certificate store, both already supported). Do not rely on drawn signatures.
- **Upstream moves very fast** (hundreds of commits a week). Rebase your branch often. Consider opening the Word import as a PR to `storytold/pdfcraft`. If they merge it, you no longer have to maintain it.
