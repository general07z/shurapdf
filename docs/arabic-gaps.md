# Arabic PDF tool: gap tracker

> **Status:** live · **Last updated:** 2026-10-30 (W3, W4 done; W2 synthetic part) · **Baseline:** storytold/pdfcraft@f76dbb9 (2026-10-30) · **Source of truth for features:** `parity/acrobat-features.toml`
> Commit this file as `docs/arabic-gaps.md` in your fork. Update a row whenever its parity entry changes.

## Goals vs. what PdfCraft already has

| # | Goal | Status today | Parity IDs | What is missing |
|---|---|---|---|---|
| 1 | **Read PDF** | ✅ shipped | `core.open-pdf`, `core.open-aes`, `view.*` | nothing material |
| 2 | **Acrobat Pro-like interface** | ✅ shipped | `view.home-view`, `view.all-tools`, `view.thumbnails-panel` | Arabic UI mirroring is not fully qualified (see 8) |
| 3 | **Compress PDF** | ✅ shipped / 🟡 advanced | `optimize.reduce-file-size` ✅, `optimize.optimizer` 🟡 | presets, batch optimize, JBIG2 |
| 4 | **Bookmarks** | ✅ shipped | `organize.bookmark-*`, `organize.bookmarks-from-structure` | bookmark actions |
| 5 | **Signature** | ✅ shipped | `form.fill-sign-signature` (drawn/typed/image), `sign.pkcs12`, `sign.windows-cert-store`, `sign.visible-signature`, `sign.validate-all` | PKCS#11 smart cards/tokens, timestamp server config, LTV is partial |
| 6 | **Word → PDF** | ✅ app, tools, CLI (synthetic tests) | `create.from-office` | `.doc`/`.rtf`, password prompt in the UI, testing on real documents (W2) |
| 7 | **Combine PDF + Word** | ✅ Combine Files, Create multiple, add folder, `pdfcraft-cli combine` | `organize.combine-files` | nothing for `.docx`; `.doc` is not supported |
| 8 | **Arabic writing and editing** | 🟡 **main gap** | see below | see below |
| 9 | **Edit PDF** | 🟡 mostly shipped | `edit.text-edit` 🟡, `edit.add-text` ✅, `edit.image-*` ✅ | lists, find/replace in edit mode, vector editing |

### Arabic in detail (goal 8)

| Capability | Status | Parity ID |
|---|---|---|
| Arabic interface translation (menus, dialogs, 6 plural forms) | 🟡 partial | `misc.localization` |
| Add **new** Arabic text (shaped, bidi, searchable, copy-paste in correct order) | ✅ | `edit.add-text` |
| Read, search and copy Arabic from existing PDFs | ✅ | `create.text-extraction` |
| Arabic in Word → PDF | ✅ shaped RTL, tables, headers, Arabic-Indic digits, heading bookmarks (synthetic tests) | `create.from-office` |
| **Typing** right-to-left inside the text box (caret moves RTL) | ❌ | `misc.rtl-ui` |
| Bold/italic for added Arabic text | ❌ (regular only) | `edit.add-text` notes |
| **Edit existing** Arabic text in a PDF (retype a paragraph) | 🟡 partial | `edit.rtl-cjk-editing` |
| Arabic caret and selection while editing | ❌ | `edit.rtl-cjk-editing` |
| Arabic fallback font when the PDF's font lacks a letter | ❌ | `edit.rtl-cjk-editing` |
| Fully mirrored RTL interface, qualified with real IME input | 🟡 | `misc.rtl-ui` |
| Arabic in form fields / Fill & Sign text | ❓ verify | `form.fill-sign-text` |
| Arabic OCR for scanned documents | ❓ verify (language selection is partial) | `ocr.ocr-languages` |

## Work queue (do in order, one PR each)

- [x] **W1** Create ▸ PDF from Word (#1, #2)
- [ ] **W2** Test W1 on 10 real (non-confidential) Council documents: Arabic letters, tables, headers/footers, page numbers, Arabic-Indic digits, logos. Save **synthetic** copies as regression fixtures, never real documents. *Synthetic part done:* `rich_arabic_docx` (heading, mixed Arabic/Latin/digits, RTL table, header on two pages). **Needs the owner:** convert real documents with `pdfcraft-cli combine in.docx in.docx --out x.pdf` or the app and report what looks wrong.
- [x] **W3** Combine accepts `.docx`: `SourceKind::Word` in `pdfcraft-create`, so Combine Files, Create multiple, Add folder, File ▸ Open and batch convert all take Word documents.
- [x] **W4** Automation: `doc_create` `from: "word"` (with `password`), `doc_create_multiple` and batch `convert` take `.docx`; `pdfcraft-cli combine` takes `.docx`.
- [ ] **W5** RTL typing in the Add Text box: the caret, Backspace/Delete, Home/End and arrow keys follow visual order (`misc.rtl-ui`). Start from WordCraft's RTL caret code (it already handles Arabic Backspace, #4757).
- [ ] **W6** Edit existing Arabic paragraphs: shaping when retyping, plus a fallback to the craft-fonts Arabic face (`edit.rtl-cjk-editing`).
- [ ] **W7** Bold Arabic in added text (synthetic bold like WordCraft's fill+stroke, or a bold Arabic face from craft-fonts).
- [ ] **W8** Default the interface language to Arabic for Council builds, then do a full RTL UI review with screenshots (`pdfcraft-cli ui … screenshot`).
- [ ] **W9** Arabic fonts the Council actually uses (Simplified Arabic, Traditional Arabic, Sakkal Majalla). These are Windows system fonts and **must not be committed**. Check that WordCraft picks them up from the system on Windows.
- [ ] **W10** Windows installer + internal deployment (packaging/ has the recipes).

## Definition of done for any item

1. A test that **fails before the fix** (shown by reverting the fix), at the right level (unit → engine → automation → UI).
2. The `parity/acrobat-features.toml` entry is updated, with honest `notes` saying what is still missing.
3. `cargo fmt --check`, `cargo clippy -p <crate> --all-targets -D warnings`, `cargo test -p <crate>`, `cargo xtask layers`, `cargo xtask assets`.
4. This file is updated: tick the box and change the status column.
