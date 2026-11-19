# ShuraPDF log

What this fork added, what it fixed, and what it knows is still wrong. Every fork PR updates this
file (see `CLAUDE.md`, "After every item"). Read it before an upstream merge; run the checklist at
the end after one. Real Council documents never appear here: they are named only as their
numbered fidelity scores (`private-1`, `private-pdf-1`, …).

PR numbers are `general07z/shurapdf` pull requests.

## Features

### Word to PDF
- Create ▸ PDF from Word with the built-in engine (WordCraft through `crates/word-import`) (#1, #2).
- `.doc` files, and a password prompt for protected Word files (#8).
- Word files in Combine, Create multiple and the automation tools (`doc_create from: "word"`,
  `doc_create_multiple`, batch convert) (#3).
- "Convert Word documents with": Automatic (Microsoft Word when it works, else the built-in
  engine), Microsoft Word, or Built-in. Word runs as a separate hidden copy, macros off, one
  conversion at a time, stopped when it hangs; every case is in `docs/shura-setup.md`, "How
  Convert Word documents with behaves" (#18, #21, #25, #29).

### Arabic text
- Bold and italic Arabic in added text; Arabic default language for Council builds (#4).
- Shaped Arabic in form fields and Fill & Sign (#9).
- Edit existing Arabic paragraphs: retyped and shaped in place (#11), in Word-made PDFs (reading
  order, row grouping, editor, installed fonts) (#26, #32), keeping the document's font when it is
  installed (#30), mixed bold/regular rows, justification and one-row growth (#34).
- Add Text offers installed fonts, shaped and embedded (#31).
- The open paragraph can be moved and resized, with width handles and a live preview; one font
  list with a scroll bar (#37–#42, #47).
- Bold toggles keep the selection and the paragraph open (#45, #46).
- Arabic paragraphs read right to left in Word exports (#15).

### Other
- ShuraPDF name, original icon, installer identity, updates from this repository's releases, a
  branded Help menu (#7, #12, #13, #17).
- High compression preset for Compress a PDF (#16).
- A signature image loses its paper background whatever its colour (#33).
- Windows build on GitHub Actions, English by default, release checksums (#6, #14, #19, #20, #27).
- Code signing: an internal certificate through the `WINDOWS_CERTIFICATE` secrets
  (`docs/code-signing.md`). SignPath Foundation signing is wired but inactive: SignPath declined
  the application (#49).

### Fidelity measurement (Council runner)
- `shura-word-fidelity.yml` scores the built-in engine against Microsoft Word, edits every Arabic
  paragraph (twice, move, widen, bold) and drives the real interface; four stages, one job each
  (#22–#24, #35, #41, #43, #48, #51, #53, #55).

### Dropped
- Mirrored right-to-left interface (#5, #10): removed with the upstream merge (#50) at the
  owner's request. Arabic typing in text boxes stays.

## Fixes

| PR | Problem | Cause | Test |
|---|---|---|---|
| #44 | Lines in different table cells joined into one paragraph | grouping ignored cell clips | `crates/edit` grouping tests |
| #52 | An Arabic paragraph split where a bracket wrapped | a row closing a bracket counted as a list item | `a_row_closing_a_bracket_from_the_row_above_is_not_a_list_item` |
| #54 | Right-aligned rows at a wide leading split into paragraphs | the repeated-leading check compared left edges only | `right_aligned_rows_at_a_wide_leading_are_one_paragraph` |
| #56 | Bold off a bold Arabic paragraph changed its font and leading | it fell back to the standard font, not the installed regular face | `bold_off_uses_the_installed_regular_face_of_the_paragraph_font` |
| #57 | A paragraph edited twice read back as its first row (justified-arabic) | the paragraph marker opened inside the first row's `/ActualText` span | `an_arabic_paragraph_edited_twice_reads_back_whole` |
| #58 | Digits typed into a table cell drawn on top of each other (merged-table-page-break) | typed text inherited Word's negative character spacing | `text_typed_into_a_squeezed_run_is_not_squeezed` |
| #61 | `1:` in a Word table cell opened for editing as `1 :`, and after an edit the colon was drawn over the number, so search failed (merged-table-page-break) | the space and colon Word draws left of the number, in visual order and squeezed, were read as drawn and kept their squeeze | `a_punctuation_run_in_an_arabic_row_reads_right_to_left`, `a_squeezed_colon_is_drawn_left_of_its_number_after_an_edit` |

## Known problems

- Arabic OCR: the built-in engine reads Latin only. Recommended: Windows OCR.
- Fidelity runner, editing stage (run 44543440607, `main` after #57–#59, compared with run
  44509927620 before them):
  - merged-table-page-break: 78 → 20 failures, all "add a word at the start" (`1 :`); 0 after
    #61 (run 44566311034).
  - justified-arabic: the widen failure is gone; the chain now reaches bold and fails there in
    Tahoma ("the bold face was not loaded"). Not yet diagnosed.
  - arabic-labels: still 1 (paragraph 4, Arial, chain: the paragraph was lost). Passes locally
    with DejaVu Sans, so it depends on Arial's metrics. Not yet diagnosed.
  - private-1: still 25 (search 22, lost paragraph 2, second edit 1); private-pdf-1: still 2 (lost
    paragraph). #57 and #58 did not reach their causes. Not yet diagnosed.
- The runner lacks Simplified Arabic and Traditional Arabic, so they are not measured.
- Not yet: kashida justification, joining across a style change inside a word, reflow of the
  paragraphs below an edit.

## After an upstream merge

1. Build, then `cargo test -p pdfcraft-edit` with `PDFCRAFT_TEST_ARABIC_FONT` set, and the
   fork's crates (`word-import`, `engine`, `automation`, `ui-egui`).
2. Every test named under Fixes still exists and passes.
3. Word to PDF: both engines, `.doc`, a protected file, Combine with a `.docx`.
4. Branding: no PdfCraft name in the UI, installer or update source.
5. `deny.toml` ignores and `xtask/baselines` still match.
6. Dispatch `shura-word-fidelity.yml` on `main` and compare with the last scores here.
