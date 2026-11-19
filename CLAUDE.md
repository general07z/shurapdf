
# Fork goals: Arabic-first PDF workbench

This fork of PdfCraft targets an Arabic-first Acrobat Pro replacement. Upstream rules below still apply in full. Where they conflict, this section wins only on **priorities**, never on safety (never-panic, clean-room, assets, layering).

## Start every session here (in addition to upstream's list)
1. Read `docs/arabic-gaps.md` and `docs/shura-log.md`, then take the **first unchecked item in the work queue**. Do not pick upstream P2/P3 features unless that item needs them.
2. Before starting, check whether upstream already fixed it: `git fetch upstream && git log upstream/main --oneline -i --grep "<terms>"`. If upstream fixed it, merge upstream instead of re-implementing.

## Fork-specific rules
- **Arabic is the primary test language.** Every text feature you touch gets an Arabic test: shaped joining forms, mixed Arabic + Latin + digits in one line, Arabic-Indic digits (٠١٢٣), and copy/search order after save and reopen.
- **WordCraft** (pinned Git dependency on `storytold/wordcraft`, same commit as `general07z/shuraword`; local `../shuraword` only through an uncommitted `[patch]`, see `docs/shura-setup.md`) is used only through `crates/word-import` (`wordcraft-docx` + `wordcraft-pdf`). Never depend on `wordcraft-engine` or `wordcraft-ui-egui`. Keep the `krilla` `[patch.crates-io]` rev equal to WordCraft's at that commit.
- **No real Council documents in the repository**, in fixtures or in screenshots. Build synthetic fixtures in code (see `crates/word-import/src/testkit.rs`).
- **Never commit fonts**, including Windows Arabic fonts (Simplified Arabic, Traditional Arabic, Sakkal Majalla). Use system fonts at runtime and craft-fonts at build time.
- **No Adobe branding.** The UI follows Acrobat's layout, but never its icons, names or images.
- Fork PRs target this fork's `main`. Upstream-worthy, general fixes go as a separate PR to `storytold/pdfcraft`.
- **Two fork workflows**, the only exceptions to upstream's "CI only through craft-common-actions" rule below; change each only in its own PR.
  - `.github/workflows/shura-windows.yml` builds, signs and releases the Council's Windows packages through `packaging/windows/package.ps1`. Signing secrets and certificates follow `docs/code-signing.md`: never in the repository.
  - `.github/workflows/shura-word-fidelity.yml` scores the built-in Word engine against Microsoft Word on the Council's self-hosted Windows runner (labels `self-hosted, windows`; it is the repository's only self-hosted runner, so add a label before registering a second one). It stays `workflow_dispatch` only: no pull-request or push trigger may ever reach that runner. Its artifacts hold synthetic documents only; real documents on the runner are reported as numbered scores. Use it to measure every WordCraft layout change (`shuraword_rev` input builds a candidate commit before the rev bump).

## After every item
- Tick it in `docs/arabic-gaps.md` and update the matching `parity/acrobat-features.toml` entry, with honest notes.
- **Record it in `docs/shura-log.md`, in the same PR** (owner's rule, 2026-11-19): every feature added, every problem fixed (cause, the test that proves it, PR) and every problem found but not yet fixed (Known problems, without any real document's content). When a known problem is fixed, move it to Fixes. Read the log before an upstream merge and run its after-merge checklist after one.
- Run `cargo xtask parity --partial` and paste the changed lines in the PR description.

@AGENTS.md
