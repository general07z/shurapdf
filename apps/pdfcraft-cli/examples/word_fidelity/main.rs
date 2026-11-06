//! Word-fidelity comparison: converts Word documents with Microsoft Word and with the built-in
//! engine, renders both PDFs and measures how far apart they are, page by page.
//!
//! ```text
//! cargo run -p pdfcraft-cli --example word_fidelity -- OUT_DIR [--private DIR]
//! ```
//!
//! Runs on the Council's Windows machine with Office (`.github/workflows/shura-word-fidelity.yml`).
//! The synthetic documents (`pdfcraft_word_import::testkit::fidelity_corpus`) get their PDFs and a
//! picture per page (Word | built-in | difference: blue only in Word, red only built-in) in
//! `OUT_DIR/<name>/`. Documents in `--private DIR` (real ones, kept on that machine) get scores
//! only: no PDF, picture or file name leaves the machine. `OUT_DIR/summary.md` has the table.
//!
//! `WORD_FIDELITY_REFERENCE=builtin` compares the built-in engine with itself, to check the tool
//! where there is no Word.
//!
//! The score of a page is the share of inked pixels that have ink in the other render within
//! [`TOLERANCE`] pixels (both ways): 100% is the same page, a sub-point difference in anti-aliasing
//! still scores 100%, and a line moved by its own height scores about 0% for that line.
//!
//! On Windows (not in the self-check), it first checks the connection to Microsoft Word in the
//! situations users meet ([`checks`]), and exits 1 if one fails.

mod checks;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pdfcraft_render::{PageRenderer, RenderConfig, RenderRequest, RequestKind};
use pdfcraft_word_import::{WordEngine, WordImportOptions, set_engine, word_to_pdf};

/// Device pixels per point for the comparison renders.
const SCALE: f32 = 1.5;

/// One page rendered: width, height and whether each pixel is ink.
struct Ink {
    width: usize,
    height: usize,
    ink: Vec<bool>,
}

fn render(pdf: &[u8]) -> Result<Vec<Ink>, String> {
    let mut renderer = PageRenderer::new(Arc::new(pdf.to_vec()), RenderConfig::default());
    let mut pages = Vec::new();
    for page in 0..renderer.page_count() {
        let r = renderer.render(RenderRequest { page, kind: RequestKind::Pixels, tile: None, scale: SCALE, tag: 0, preview: false });
        if let Some(e) = r.error {
            return Err(format!("page {}: {e}", page + 1));
        }
        let ink = r
            .rgba
            .into_words()
            .into_iter()
            .map(|w| {
                let [red, green, blue, alpha] = w.to_ne_bytes();
                // Premultiplied: a transparent pixel is paper; dark and opaque is ink.
                alpha >= 128 && (u32::from(red) * 3 + u32::from(green) * 6 + u32::from(blue)) / 10 < 160
            })
            .collect();
        pages.push(Ink { width: r.width as usize, height: r.height as usize, ink });
    }
    Ok(pages)
}

/// Pixels (at [`SCALE`]) by which ink may be off and still count as matching: about 1.3 pt.
const TOLERANCE: usize = 2;

/// `p`'s ink grown by [`TOLERANCE`] pixels each way, on a `w`×`h` canvas.
fn grown(p: &Ink, w: usize, h: usize) -> Vec<bool> {
    let mut rows = vec![false; w * h];
    for y in 0..p.height.min(h) {
        for x in 0..p.width.min(w) {
            if p.ink.get(y * p.width + x).copied().unwrap_or(false) {
                for xx in x.saturating_sub(TOLERANCE)..(x + TOLERANCE + 1).min(w) {
                    if let Some(c) = rows.get_mut(y * w + xx) {
                        *c = true;
                    }
                }
            }
        }
    }
    let mut out = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            if rows.get(y * w + x).copied().unwrap_or(false) {
                for yy in y.saturating_sub(TOLERANCE)..(y + TOLERANCE + 1).min(h) {
                    if let Some(c) = out.get_mut(yy * w + x) {
                        *c = true;
                    }
                }
            }
        }
    }
    out
}

/// Inked pixels of each page with ink of the other within [`TOLERANCE`], over all inked pixels
/// (1.0 when both pages are blank).
fn score(a: &Ink, b: &Ink) -> f64 {
    let (w, h) = (a.width.max(b.width), a.height.max(b.height));
    let (near_a, near_b) = (grown(a, w, h), grown(b, w, h));
    let at = |p: &Ink, x: usize, y: usize| x < p.width && y < p.height && p.ink.get(y * p.width + x).copied().unwrap_or(false);
    let (mut matched, mut inked) = (0u64, 0u64);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if at(a, x, y) {
                inked += 1;
                matched += u64::from(near_b.get(i).copied().unwrap_or(false));
            }
            if at(b, x, y) {
                inked += 1;
                matched += u64::from(near_a.get(i).copied().unwrap_or(false));
            }
        }
    }
    if inked == 0 { 1.0 } else { matched as f64 / inked as f64 }
}

/// Word | built-in | difference, side by side, as PNG.
fn picture(a: &Ink, b: &Ink, path: &Path) -> Result<(), String> {
    let (w, h) = (a.width.max(b.width), a.height.max(b.height));
    let gap = 12;
    let total_w = w * 3 + gap * 2;
    let mut rgb = vec![255u8; total_w * h * 3];
    let at = |p: &Ink, x: usize, y: usize| x < p.width && y < p.height && p.ink.get(y * p.width + x).copied().unwrap_or(false);
    let mut put = |x: usize, y: usize, c: [u8; 3]| {
        let i = (y * total_w + x) * 3;
        if let Some(px) = rgb.get_mut(i..i + 3) {
            px.copy_from_slice(&c);
        }
    };
    for y in 0..h {
        for x in 0..w {
            let (i, j) = (at(a, x, y), at(b, x, y));
            if i {
                put(x, y, [0, 0, 0]);
            }
            if j {
                put(w + gap + x, y, [0, 0, 0]);
            }
            let diff = match (i, j) {
                (true, true) => Some([0, 0, 0]),
                (true, false) => Some([30, 90, 230]),
                (false, true) => Some([230, 40, 40]),
                (false, false) => None,
            };
            if let Some(c) = diff {
                put(2 * (w + gap) + x, y, c);
            }
        }
    }
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), total_w as u32, h as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().and_then(|mut wr| wr.write_image_data(&rgb)).map_err(|e| e.to_string())
}

fn convert(bytes: &[u8], engine: WordEngine) -> Result<Vec<u8>, String> {
    set_engine(engine);
    word_to_pdf(bytes, &WordImportOptions::default()).map_err(|e| e.to_string())
}

/// The engine compared against: Microsoft Word, or the built-in engine itself when
/// `WORD_FIDELITY_REFERENCE=builtin` (a self-check without Word, where every page scores 100%).
fn reference() -> WordEngine {
    match std::env::var("WORD_FIDELITY_REFERENCE").as_deref() {
        Ok("builtin") => WordEngine::BuiltIn,
        _ => WordEngine::Word,
    }
}

/// One document's row: pages from each engine and each page's score.
struct Row {
    name: String,
    word_pages: usize,
    builtin_pages: usize,
    scores: Vec<f64>,
    error: Option<String>,
}

fn compare(name: &str, bytes: &[u8], out: Option<&Path>) -> Row {
    let mut row = Row { name: name.to_owned(), word_pages: 0, builtin_pages: 0, scores: Vec::new(), error: None };
    let result = (|| -> Result<(), String> {
        let word = convert(bytes, reference()).map_err(|e| format!("Microsoft Word: {e}"))?;
        let builtin = convert(bytes, WordEngine::BuiltIn).map_err(|e| format!("built-in engine: {e}"))?;
        let (wp, bp) = (render(&word)?, render(&builtin)?);
        row.word_pages = wp.len();
        row.builtin_pages = bp.len();
        if let Some(dir) = out {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            std::fs::write(dir.join("word.pdf"), &word).map_err(|e| e.to_string())?;
            std::fs::write(dir.join("builtin.pdf"), &builtin).map_err(|e| e.to_string())?;
        }
        for (i, (a, b)) in wp.iter().zip(&bp).enumerate() {
            row.scores.push(score(a, b));
            if let Some(dir) = out {
                picture(a, b, &dir.join(format!("page-{:02}.png", i + 1)))?;
            }
        }
        Ok(())
    })();
    row.error = result.err();
    row
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some(checks::CONVERT_ONE) {
        checks::convert_one();
    }
    let mut args = std::env::args().skip(1);
    let Some(out) = args.next().map(PathBuf::from) else {
        eprintln!("usage: word_fidelity OUT_DIR [--private DIR]");
        std::process::exit(2);
    };
    let private = match (args.next().as_deref(), args.next()) {
        (Some("--private"), Some(dir)) => Some(PathBuf::from(dir)),
        _ => None,
    };
    if let Err(e) = std::fs::create_dir_all(&out) {
        eprintln!("{}: {e}", out.display());
        std::process::exit(2);
    }
    let self_check = reference() == WordEngine::BuiltIn;
    let connection = if cfg!(windows) && !self_check { checks::run(&out) } else { Vec::new() };
    let mut rows: Vec<Row> =
        pdfcraft_word_import::testkit::fidelity_corpus().into_iter().map(|(name, bytes)| compare(name, &bytes, Some(&out.join(name)))).collect();
    if let Some(dir) = private.filter(|d| d.is_dir()) {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("docx") || x.eq_ignore_ascii_case("doc")))
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        for (i, path) in files.iter().enumerate() {
            // Scores only, under a number: nothing of a real document leaves the machine.
            let name = format!("private-{}", i + 1);
            match std::fs::read(path) {
                Ok(bytes) => rows.push(compare(&name, &bytes, None)),
                Err(e) => rows.push(Row { name, word_pages: 0, builtin_pages: 0, scores: Vec::new(), error: Some(e.kind().to_string()) }),
            }
        }
    }
    let mut md = String::from("| Document | Pages (Word / built-in) | Average | Lowest page | Per page |\n|---|---|---|---|---|\n");
    let mut word_failed = false;
    for r in &rows {
        if let Some(e) = &r.error {
            word_failed |= e.starts_with("Microsoft Word");
            md.push_str(&format!("| {} | – | – | – | error: {} |\n", r.name, e.replace('|', "/")));
            continue;
        }
        let avg = if r.scores.is_empty() { 0.0 } else { r.scores.iter().sum::<f64>() / r.scores.len() as f64 };
        let (low_i, low) = r.scores.iter().copied().enumerate().fold((0, 1.0f64), |m, (i, s)| if s < m.1 { (i, s) } else { m });
        let pages: Vec<String> = r.scores.iter().map(|s| format!("{:.0}", s * 100.0)).collect();
        md.push_str(&format!(
            "| {} | {} / {} | {:.1}% | {:.1}% (p. {}) | {} |\n",
            r.name,
            r.word_pages,
            r.builtin_pages,
            avg * 100.0,
            low * 100.0,
            low_i + 1,
            pages.join(" ")
        ));
    }
    if !connection.is_empty() {
        md.push_str(&checks::table(&connection));
    }
    print!("{md}");
    if let Err(e) = std::fs::write(out.join("summary.md"), &md) {
        eprintln!("summary.md: {e}");
    }
    // In GitHub Actions, the table also goes on the run's page.
    if let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY") {
        use std::io::Write;
        let appended = std::fs::OpenOptions::new().append(true).create(true).open(path).and_then(|mut f| f.write_all(md.as_bytes()));
        if let Err(e) = appended {
            eprintln!("step summary: {e}");
        }
    }
    if connection.iter().any(|c| !c.passed) {
        eprintln!("A Word connection check failed: see the table above");
        std::process::exit(1);
    }
    if word_failed {
        eprintln!("Microsoft Word could not convert: see the errors above");
        std::process::exit(1);
    }
}
