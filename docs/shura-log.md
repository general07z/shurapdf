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

### Diagnostic log
- Preferences ▸ General ▸ Diagnostic log: Off (default), Without document text, With document
  text, kept in the settings (`diagnostic_log`, also a `set_option`). It writes to the app's log
  file (`<data dir>/logs/pdfcraft.log`; "Show the log file" opens its folder): every command with
  its result and time, each press on a page with what lay under it and what took it, a paragraph
  opened for editing (rows, font, size, box, text), each edit with the page's paragraphs before
  and after it, its result, time and notices, opens, saves and every notice shown. Without
  document text, text and file names become counts by kind of character. Never: passwords,
  certificate data, the control token, font or image bytes (`crates/ui-egui/src/diag.rs`, #69).
  Testers send the file in the conversation; it never goes into the repository.
- The log also records, in the paragraph editor, every caret move and selection (characters,
  text, press position), what typing removed and inserted, and Format text choices (font, look
  against the paragraph's own, font shown); fonts by name, family and size, never their bytes,
  and any long list of numbers by its count (#73).

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
| #64 | Searching after an edit in an installed face failed in paragraphs with an email or web address (private-1: 21 of its 22 search failures) | Word draws the space after a paragraph as a run of its own, which belonged to no paragraph: the rewrite kept it, the wider row was drawn over it, and the page reader read it inside a word (`20 26`) | `a_rewritten_row_takes_its_trailing_space_run_with_it` |
| #66 | A blank run on a rewritten row's baseline but more than one em away, or at another size, stayed under the wider row | #64 took only a blank within one em and at the same size; now any blank on the row's baseline goes with it unless other text lies between. It did not reach private-1's 7 failures it was aimed at (run 44682679636: unchanged) | `a_rewritten_row_takes_a_far_trailing_space_run_with_it`, `a_space_run_beside_other_text_stays_when_a_row_is_rewritten` |
| #67 | Search after an edit in Arial or Tahoma failed in two private-1 paragraphs (7 of its 9 search failures): a blank glyph lay inside the typed words (`2026` read as `2 026`) | Found with this PR's new fidelity detail (run 44688480324): the blank was on the page before the edit, in no paragraph, at the middle of the row the edit added. The paragraph grew a row over the line below, where Word draws an empty paragraph's mark as a blank at its alignment point, and the rewrite kept it. Now the styled rewrite drops every blank-only run its drawn rows cover | `a_new_row_takes_the_blank_it_is_drawn_over` |
| #70 | Bold in a chain of edits "did not load" its Bold face (justified-arabic in Tahoma on the runner; locally in FreeSerif) | every shaped edit adds a font resource for its own glyph map, sharing the face's program; the editor's font list decoded and counted that program once per resource against its 32 MiB allowance, so after a few edits in a large face the Bold no longer fit and was left out of the list. Now a `PCFull` resource sharing a program already listed reuses its bytes, uncounted | `a_face_loaded_after_many_edits_is_listed` |
| #72 | Selecting a word in an Arabic paragraph opened for editing selected another word (owner's report, from the diagnostic log) | the unseen editor text that takes clicks was laid over the page's words in typing order, left to right, so a right-to-left row was mirrored: a click on the first word, at the right, landed on the last. Now words and characters are paired with the page's boxes in the order the layout shows them | `a_click_on_an_arabic_word_lands_on_that_word` |
| #72 | Bold changed the font list's name from Arial to `Arial-BoldMT`, as if another font had been chosen (owner's report) | the list showed the embedded bold face's PostScript name; now it shows the family, and the Bold button the weight | `the_font_list_names_the_family_whatever_its_weight` |
| #73 | "Show the log file" (and Reveal in File Explorer) said it could not show the file on Windows | Explorer exits with status 1 even when it opens the folder with the file selected; a started Explorer now counts as shown | `a_started_explorer_reveals_whatever_its_exit_status` |
| this PR | Bold on a Tajawal paragraph, with no Tajawal Bold installed, failed: "the BIZ UDPGothic fallback font has no glyph for ا" (owner's report) | with no installed bold face the editor asked for a standard bold font; its fallback faces have a bold Japanese face but no bold Arabic one. Now the paragraph keeps its own font and is drawn bold by stroking each glyph in its colour, 3% of the size (`BlockStyle::synthetic_bold`, `text_edit` `synthetic_bold`), as Word does. Bold on part of a paragraph still asks for a standard bold | `synthetic_bold_keeps_the_paragraphs_face_and_strokes_its_glyphs`, `bold_without_a_bold_face_keeps_the_font_and_is_synthetic`, `text_edit_draws_a_paragraph_in_synthetic_bold` |

## Known problems

- Arabic OCR: the built-in engine reads Latin only. Recommended: Windows OCR.
- Fidelity runner, editing stage (run 44543440607, `main` after #57–#59, compared with run
  44509927620 before them):
  - merged-table-page-break: 78 → 20 failures, all "add a word at the start" (`1 :`); 0 after
    #61 (run 44566311034).
  - justified-arabic: the widen failure is gone; the chain now reaches bold and fails there in
    Tahoma ("the bold face was not loaded"): the font list's allowance, fixed in #70; 0 failures
    after it (run 44729956957).
  - arabic-labels: still 1 (paragraph 4, Arial, chain): lost after the second edit, which
    leaves the paragraph without its leading Latin label "Site:" (run 44714354245). Passes
    locally with DejaVu Sans and other open faces, so it depends on Arial's metrics.
  - private-1: 25 before #64; 12 after it (run 44642509383); 5 after #67 (run 44700360978:
    search 2, lost paragraph 2, second edit 1). The 7 search failures #67 aimed at are gone.
    private-pdf-1: still 2 (lost paragraph). Remaining search failures: page 2, paragraph 4,
    Tahoma, punctuation (the typed characters read in another order); page 2, paragraph 10, own
    font (glyphs out of reading order; the unedited document has the same text unfindable).
    Lost paragraphs (page 1, paragraph 4, Arial and Sakkal Majalla, chain, in both private-1 and
    private-pdf-1): lost after widening, the paragraph (185 characters) then holding its first
    words' characters, not in a row (run 44714354245); not reproduced with open faces. The second
    edit (same paragraph, own font) not yet diagnosed.
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
