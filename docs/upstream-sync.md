# Syncing with upstream (storytold/pdfcraft)

> **Status:** live · **Last updated:** 2026-11-17 · first sync in progress (branch `sync/upstream-2026-11`)

This fork keeps its own text-editing, Word-import, packaging and fidelity work (`docs/arabic-gaps.md`,
`docs/shura-setup.md`) on top of `storytold/pdfcraft`. This file is the plan and the rules, so a
sync is never started from memory.

## Rules
1. **Never merge upstream straight into `main`.** Merge into `sync/upstream-<year>-<month>`; `main` only
   takes the branch once every gate below passes.
2. In `crates/edit`, `crates/fonts`, `crates/ui-egui/src/tools/edit_text_ui*` and the Format text
   panel keep **our** code, then add what upstream's version needs to work with it.
3. Fork-only files stay as they are: `apps/pdfcraft-cli/examples/word_fidelity/`, `packaging/windows/`,
   `.github/workflows/shura-*.yml`, `docs/shura-setup.md`, `docs/arabic-gaps.md`, the `shuraword` pin.
4. Take upstream for crates we never touched (`render`, `redact`, `xfa`, `js`, `sign`, `cos`, `filters`,
   `crypt`, `vendor/*`, …).

## Gates (all must pass before the branch is merged)
- `cargo test` for the crates we changed (`edit`, `engine`, `automation`, `ui-egui`, `word-import`, `fonts`),
  `cargo xtask layers`, `cargo xtask assets`, clippy `-D warnings`.
- The Arabic editing matrix (`word_fidelity` editing checks, or `scratchpad/matrix/run.py`) on a real
  Word letter and a Word table PDF: **0 failures**. (Upstream alone, 2026-11-17: 183 of 183 and 200 of 366
  edits fail with "the BIZ UDPGothic fallback font has no glyph…".)
- The fidelity workflow on the Council's runner (Word connection checks, editing matrix, interface checks).

## Our changes that must survive (check each after the merge)
Word-PDF Arabic editing (installed face, shaped rows, `interp`/`grouping`/`styled` changes), table-cell
clip grouping, installed-font lookup and bold faces, move and resize of the open paragraph, one font list
(no Load font / Keep source fonts), a format change keeps its paragraph, live move preview, signature
background removal, the Word engine (Office automation, `ONE_AT_A_TIME`, timeouts) and its checks.

## After a sync
Send our general fixes to upstream as separate PRs (the owner decides): table-cell clip, moving a
redrawn paragraph, Bold on a selection, width handles of the open paragraph. Repeat every 2–4 weeks.

## Log
- 2026-11-17: fork base `3517d05` (2026-10-30). Upstream 6529 commits ahead (1160 files). Dry merge: 66
  conflicted files, 164 hunks (ui-egui 24 files, edit 12, engine 5, automation 3, forms 2, annot 2).
- 2026-11-17: merge started on `sync/upstream-2026-11`. Done: config, docs, packaging, attribution, parity;
  `create` (Word kind kept beside upstream's Office/LibreOffice kind), `engine`, `automation`, `export`
  (upstream's version taken; our Arabic reading-order test re-added, may need a port), `fonts`, `annot`/`forms`
  (OUR appearance code kept, upstream's Thai changes there dropped), `edit` except the re-port below.
  Upstream tracks clips itself (`Ts.clip`): our table-cell grouping now reads its bounds (`clip_bounds`).
  **Still to do:**
  1. `crates/edit/src/text_regions/styled.rs`: upstream's version was taken whole; re-apply OUR changes
     (`row_segments`, `one_look_one_source`/`merge_alike_rows`, `justify_gap`, `wrap_width`, `piece` in
     `shape_source`, RTL in `paragraph_geometry`, visual-order drawing and written order).
  2. `crates/edit/src/text/rewrite.rs` `ShapedRewrite::apply`: port to upstream's `ContentStream`/`paragraph_style`
     (3 arguments)/`AddedText { aligns, ink }`.
  3. `crates/ui-egui`: 23 files with conflict markers (edit_text_ui*, format_text_ui, panels, i18n, home, ...).
  4. Build, then the gates above. Our matrix must give 0 failures.
