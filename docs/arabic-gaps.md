# Arabic PDF tool: gap tracker

> **Status:** live · **Last updated:** 2026-10-30 (W3–W5, W7, W8 done; W2 synthetic part) · **Baseline:** storytold/pdfcraft@f76dbb9 (2026-10-30) · **Source of truth for features:** `parity/acrobat-features.toml`
> Commit this file as `docs/arabic-gaps.md` in your fork. Update a row whenever its parity entry changes.

## Goals vs. what PdfCraft already has

| # | Goal | Status today | Parity IDs | What is missing |
|---|---|---|---|---|
| 1 | **Read PDF** | ✅ shipped | `core.open-pdf`, `core.open-aes`, `view.*` | nothing material |
| 2 | **Acrobat Pro-like interface** | ✅ shipped | `view.home-view`, `view.all-tools`, `view.thumbnails-panel` | Arabic UI mirroring is not fully qualified (see 8) |
| 3 | **Compress PDF** | ✅ shipped / 🟡 advanced | `optimize.reduce-file-size` ✅, `optimize.optimizer` 🟡 | presets, batch optimize, JBIG2 |
| 4 | **Bookmarks** | ✅ shipped | `organize.bookmark-*`, `organize.bookmarks-from-structure` | bookmark actions |
| 5 | **Signature** | ✅ shipped | `form.fill-sign-signature` (drawn/typed/image), `sign.pkcs12`, `sign.windows-cert-store`, `sign.visible-signature`, `sign.validate-all` | direct PKCS#11 (smart cards and tokens already sign through the Windows store when their driver registers a CNG key storage provider, with the card's own PIN dialog; **needs the owner:** insert the Council or eKey card, then Sign ▸ Use a certificate: if the card's certificate is listed, it works; if not, its middleware is CryptoAPI-only or PKCS#11-only and direct PKCS#11 is needed), timestamp server config, LTV is partial |
| 6 | **Word → PDF** | ✅ `.docx` and `.doc`, password-protected ones too; app, tools, CLI (synthetic tests) | `create.from-office` | `.rtf`/`.odt`, testing on real documents (W2) |
| 7 | **Combine PDF + Word** | ✅ Combine Files, Create multiple, add folder, `pdfcraft-cli combine` | `organize.combine-files` | a protected Word file in Combine is listed as unreadable (convert it first) |
| 8 | **Arabic writing and editing** | 🟡 **main gap** | see below | see below |
| 9 | **Edit PDF** | 🟡 mostly shipped | `edit.text-edit` 🟡, `edit.add-text` ✅, `edit.image-*` ✅ | lists, find/replace in edit mode, vector editing |

### Arabic in detail (goal 8)

| Capability | Status | Parity ID |
|---|---|---|
| Arabic interface translation (menus, dialogs, 6 plural forms) | 🟡 partial | `misc.localization` |
| Add **new** Arabic text (shaped, bidi, searchable, copy-paste in correct order) | ✅ | `edit.add-text` |
| Read, search and copy Arabic from existing PDFs | ✅ | `create.text-extraction` |
| Arabic in Word → PDF | ✅ shaped RTL, tables, headers, Arabic-Indic digits, heading bookmarks (synthetic tests) | `create.from-office` |
| **Typing** right-to-left inside the text box (caret moves RTL) | ✅ right-aligned box in the Arabic interface, caret moves on screen, Backspace one letter (kittest) | `misc.rtl-ui` |
| Bold/italic for added Arabic text | ✅ synthetic bold (fill + stroke) and italic (slant), as Word draws a one-weight face | `edit.add-text` |
| **Edit existing** Arabic text in a PDF (retype a paragraph) | 🟡 partial | `edit.rtl-cjk-editing` |
| Arabic caret and selection while editing | ❌ | `edit.rtl-cjk-editing` |
| Arabic fallback font when the PDF's font lacks a letter | ❌ | `edit.rtl-cjk-editing` |
| Fully mirrored RTL interface, qualified with real IME input | 🟡 menus, panes, rail, tool strip and tool rows mirrored; tab strip, some dialogs and real IME not yet | `misc.rtl-ui` |
| Arabic in form fields / Fill & Sign text | ❓ verify | `form.fill-sign-text` |
| Arabic OCR for scanned documents | ❓ verify (language selection is partial) | `ocr.ocr-languages` |

## Work queue (do in order, one PR each)

- [x] **W1** Create ▸ PDF from Word (#1, #2)
- [ ] **W2** Test W1 on 10 real (non-confidential) Council documents: Arabic letters, tables, headers/footers, page numbers, Arabic-Indic digits, logos. Save **synthetic** copies as regression fixtures, never real documents. *Synthetic part done:* `rich_arabic_docx` (heading, mixed Arabic/Latin/digits, RTL table, header on two pages). **Needs the owner:** convert real documents with `pdfcraft-cli combine in.docx in.docx --out x.pdf` or the app and report what looks wrong.
- [x] **W3** Combine accepts `.docx`: `SourceKind::Word` in `pdfcraft-create`, so Combine Files, Create multiple, Add folder, File ▸ Open and batch convert all take Word documents.
- [x] **W4** Automation: `doc_create` `from: "word"` (with `password`), `doc_create_multiple` and batch `convert` take `.docx`; `pdfcraft-cli combine` takes `.docx`.
- [x] **W5** Right-to-left interface and typing (`misc.rtl-ui`, partial): menus from the right, Tools pane right, rail and side panel left, Select/Hand bar right, tool rows icon-right; a new text box starts right-aligned and the caret moves on screen in Arabic. Home page mirrored too (headings, tool cards from the right, card text and "Use now" at the right edge, no overlap with Arabic's taller lines). **Follow-ups:** the tab strip stays left to right for now (its drag-reorder and scrolling assume it, and egui's scroll area has no right-to-left mode, so it needs its own change); check dialogs one by one with `--example shot -- --language ar`; try real Windows Arabic keyboard input (IME) on a Council PC.
- [ ] **W6** Edit existing Arabic paragraphs: shaping when retyping, plus a fallback to the craft-fonts Arabic face (`edit.rtl-cjk-editing`). *Assessed:* large (the existing-text editor in `crates/edit`); after W5, which it shares the caret work with.
- [x] **W7** Bold and italic Arabic (also Hebrew, Thai) in added text: synthetic bold (fill + stroke, 3% of the size) and italic (0.21 slant), as WordCraft and Word do. A real bold Arabic face (craft-fonts has Regular only) would look better still.
- [x] **W8** Council builds default to Arabic: build with `PDFCRAFT_DEFAULT_LANGUAGE=ar` and a fresh install starts in Arabic whatever the Windows language (saved settings still win; Preferences ▸ Language can change it). RTL review done by screenshot: text and translation good, "Window" menu translated; the layout is not mirrored yet (moved to W5).
- [ ] **W9** Arabic fonts the Council actually uses (Simplified Arabic, Traditional Arabic, Sakkal Majalla). These are Windows system fonts and **must not be committed**. *Code checked:* WordCraft reads `%WINDIR%\Fonts` and `%LOCALAPPDATA%\Microsoft\Windows\Fonts` and matches a document's font by name, so these resolve on Council PCs. **Needs the owner:** on a Council PC, convert a .docx set in Simplified Arabic and run `pdffonts` (or File ▸ Properties ▸ Fonts) on the PDF: the font must say SimplifiedArabic, embedded.
- [ ] **W10** Windows installer + internal deployment. *Build done:* `.github/workflows/shura-windows.yml` builds the MSI and portable zip (Arabic by default, craft-fonts with their OFL notices) on every push to `main`, signs them when the `WINDOWS_CERTIFICATE` secrets exist (`docs/code-signing.md`; pilot certificate script `packaging/windows/new-signing-certificate.ps1`), and publishes a release for a `v*` tag. **Needs the owner:** create the pilot certificate (or ask ICT for one from the Council CA), push a tag, and decide deployment (Intune/SCCM/GPO). The installer still says "PdfCraft" (product name and icon in `packaging/windows/*.wxs`).

## Definition of done for any item

1. A test that **fails before the fix** (shown by reverting the fix), at the right level (unit → engine → automation → UI).
2. The `parity/acrobat-features.toml` entry is updated, with honest `notes` saying what is still missing.
3. `cargo fmt --check`, `cargo clippy -p <crate> --all-targets -D warnings`, `cargo test -p <crate>`, `cargo xtask layers`, `cargo xtask assets`.
4. This file is updated: tick the box and change the status column.
