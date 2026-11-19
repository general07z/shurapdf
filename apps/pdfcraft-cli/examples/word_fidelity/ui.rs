//! The real interface on the Council's machine: ShuraPDF's own window code, drawn by the GPU
//! stack the machine has (`egui_kittest` + wgpu: the same renderer as the app), on each document's
//! PDF. It looks for what a person sees and no unit test can: empty boxes over letters while
//! editing, the open paragraph jumping after a format change, and how long drawing and moving
//! take.
//!
//! Per document, up to [`PARAGRAPHS`] Arabic paragraphs of a sentence or more are each opened for
//! typing, retyped, formatted with Bold twice and dragged, with the editor drawn each step.
//!
//! * **Boxes**: solid, square-ish dark blobs (letters have strokes; an empty glyph is a block) in
//!   the paragraph's area with the editor open, against the same area of the page as it opened.
//! * **Wrong paragraph**: after Bold the open paragraph must be the one that was open.
//! * **Speed**: milliseconds per drawn frame while typing and dragging, and until the page is
//!   drawn again after a move.
//!
//! A real document is reported as numbers only; a synthetic one also keeps its pictures.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::PdfCraftApp;

/// Arabic paragraphs tried per document.
const PARAGRAPHS: usize = 2;
/// The page is drawn at this many pixels per point.
const SCALE: f32 = 1.0;

/// What one document showed.
#[derive(Default)]
pub struct Outcome {
    pub name: String,
    pub paragraphs: usize,
    /// Boxes found with the editor open, and in the same areas before.
    pub boxes: usize,
    pub baseline: usize,
    /// Bold presses after which another paragraph was open.
    pub wrong_paragraph: usize,
    /// Numbers only (never text) about each such press: the open paragraph's number and box before
    /// and after, and how many paragraphs the page had.
    pub wrong_details: Vec<String>,
    /// Milliseconds per drawn frame (typing and dragging), and until the page is drawn after a move.
    pub frames: Vec<f64>,
    pub settled: Vec<f64>,
    pub error: Option<String>,
}

/// A paragraph to try: its box in points from the page's top left.
struct Target {
    rect: [f64; 4],
}

fn targets(pdf: &[u8], work: &Path) -> Result<(Vec<Target>, [f64; 2]), String> {
    use pdfcraft_automation::{Automation, Content};
    let _ = std::fs::create_dir_all(work);
    std::fs::write(work.join("ui.pdf"), pdf).map_err(|e| e.to_string())?;
    let mut auto = Automation::new().with_root(work).map_err(|e| e.to_string())?;
    let call = |auto: &mut Automation, tool: &str, args: serde_json::Value| -> Result<serde_json::Value, String> {
        auto.call(tool, &args)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find_map(|c| if let Content::Json(v) = c { Some(v) } else { None })
            .ok_or_else(|| format!("{tool}: nothing"))
    };
    let doc = call(&mut auto, "doc_open", serde_json::json!({ "path": "ui.pdf" }))?;
    let id = doc.get("doc").and_then(serde_json::Value::as_i64).ok_or("no document")?;
    let info = call(&mut auto, "doc_info", serde_json::json!({ "doc": id }))?;
    let first = info.get("pages").and_then(serde_json::Value::as_array).and_then(|p| p.first()).ok_or("no page size")?;
    let side = |key: &str, default: f64| first.get(key).and_then(serde_json::Value::as_f64).unwrap_or(default);
    let size = [side("width", 595.0), side("height", 842.0)];
    let listed = call(&mut auto, "text_paragraphs", serde_json::json!({ "doc": id, "page": 1 }))?;
    let found = listed
        .get("paragraphs")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let text = p.get("editor_text")?.as_str()?.to_owned();
            let r = p.get("rect")?.as_array()?;
            let at = |i: usize| r.get(i).and_then(serde_json::Value::as_f64);
            let rect = [at(0)?, at(1)?, at(2)?, at(3)?];
            let arabic = text.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c));
            (arabic && text.split_whitespace().collect::<Vec<_>>().join(" ").chars().count() >= 25 && rect[2] - rect[0] > 60.0)
                .then_some(Target { rect })
        })
        .take(PARAGRAPHS)
        .collect();
    Ok((found, size))
}

fn norm(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Solid square-ish dark blobs in `area` (x0, y0, x1, y1 pixels): the mark of a glyph that did not
/// draw. A letter is strokes (thin, mostly empty box); a missing glyph is a block.
fn solid_blobs(image: &image::RgbaImage, area: [u32; 4]) -> usize {
    let (w, h) = (image.width(), image.height());
    let (x0, y0, x1, y1) = (area[0].min(w), area[1].min(h), area[2].min(w), area[3].min(h));
    if x1 <= x0 || y1 <= y0 {
        return 0;
    }
    let (cw, ch) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let dark = |x: usize, y: usize| {
        let p = image.get_pixel(x0 + x as u32, y0 + y as u32);
        (u32::from(p[0]) * 3 + u32::from(p[1]) * 6 + u32::from(p[2])) / 10 < 90
    };
    let mut seen = vec![false; cw * ch];
    let mut count = 0;
    for sy in 0..ch {
        for sx in 0..cw {
            if seen.get(sy * cw + sx).copied().unwrap_or(true) || !dark(sx, sy) {
                continue;
            }
            // Flood fill (4-neighbour) the blob, tracking its box and pixel count.
            let (mut stack, mut area_px) = (vec![(sx, sy)], 0usize);
            let (mut bx0, mut by0, mut bx1, mut by1) = (sx, sy, sx, sy);
            if let Some(s) = seen.get_mut(sy * cw + sx) {
                *s = true;
            }
            while let Some((x, y)) = stack.pop() {
                area_px += 1;
                (bx0, by0, bx1, by1) = (bx0.min(x), by0.min(y), bx1.max(x), by1.max(y));
                for (nx, ny) in [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)] {
                    if nx < cw && ny < ch && !seen.get(ny * cw + nx).copied().unwrap_or(true) && dark(nx, ny) {
                        if let Some(s) = seen.get_mut(ny * cw + nx) {
                            *s = true;
                        }
                        stack.push((nx, ny));
                    }
                }
            }
            let (bw, bh) = (bx1 - bx0 + 1, by1 - by0 + 1);
            if bw >= 5 && bh >= 5 && bw <= 40 && bh <= 40 && area_px * 100 >= bw * bh * 80 {
                count += 1;
            }
        }
    }
    count
}

fn settle(h: &mut Harness<'static, PdfCraftApp>) {
    let start = Instant::now();
    h.run_steps(3);
    while start.elapsed() < Duration::from_secs(15) {
        h.run_steps(2);
        if !h.state().render_pending() {
            h.run_steps(2);
            if !h.state().render_pending() {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn open_paragraph(h: &Harness<'static, PdfCraftApp>) -> Option<String> {
    h.state().views.first().and_then(|v| v.line_editor.as_ref()).map(|e| norm(&e.text))
}

/// Runs the checks on `pdf`; `keep` (a synthetic document's folder) receives the pictures.
pub fn run(name: &str, pdf: &[u8], work: &Path, keep: Option<&Path>) -> Outcome {
    let mut outcome = Outcome { name: name.to_owned(), ..Outcome::default() };
    let result = (|| -> Result<(), String> {
        let (found, size) = targets(pdf, work)?;
        let bytes = pdf.to_vec();
        for (n, target) in found.iter().enumerate() {
            let data = bytes.clone();
            let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1000.0)).with_pixels_per_point(SCALE).build_eframe(move |_cc| {
                let mut app = PdfCraftApp::new();
                let _ = app.set_option("language", "en");
                let _ = app.open_bytes("ui.pdf", None, data.clone());
                app
            });
            h.run_steps(6);
            if !h.state_mut().execute("edit.edit_text") {
                return Err("the Edit tool did not start".into());
            }
            // Installed fonts are found in the background: let that finish before opening.
            for _ in 0..30 {
                h.run_steps(2);
                std::thread::sleep(Duration::from_millis(100));
            }
            settle(&mut h);
            let Some(page) = h.state().views.first().and_then(|v| v.page_screen_rect(0)) else { return Err("the page is not on screen".into()) };
            let k = page.width() / size[0] as f32;
            let screen = |x: f64, y: f64| egui::pos2(page.left() + x as f32 * k, page.top() + y as f32 * k);
            let area = [
                (page.left() + target.rect[0] as f32 * k - 12.0).max(0.0) as u32,
                (page.top() + target.rect[1] as f32 * k - 12.0).max(0.0) as u32,
                (page.left() + target.rect[2] as f32 * k + 12.0) as u32,
                (page.top() + target.rect[3] as f32 * k + 12.0) as u32,
            ];
            let before = h.render()?;
            outcome.baseline += solid_blobs(&before, area);
            // Open it with a click on its middle.
            let centre = screen((target.rect[0] + target.rect[2]) / 2.0, (target.rect[1] + target.rect[3]) / 2.0);
            h.hover_at(centre);
            h.run_steps(2);
            h.drag_at(centre);
            h.run_steps(1);
            h.drop_at(centre);
            h.run_steps(6);
            let Some(opened) = open_paragraph(&h) else { return Err("a click did not open the paragraph".into()) };
            outcome.paragraphs += 1;
            // Type at its end: each drawn frame is timed.
            h.key_press_modifiers(Modifiers::COMMAND, Key::End);
            h.run_steps(2);
            for word in [" جديد", " ثاني"] {
                let t = Instant::now();
                h.event(egui::Event::Text(word.into()));
                h.run_steps(3);
                let _ = h.render()?;
                outcome.frames.push(t.elapsed().as_secs_f64() * 1000.0);
            }
            settle(&mut h);
            let typed = h.render()?;
            outcome.boxes += solid_blobs(&typed, area);
            if let Some(dir) = keep {
                let _ = std::fs::create_dir_all(dir);
                let _ = typed.save(dir.join(format!("ui-{}-typed.png", n + 1)));
            }
            // Bold twice: the same paragraph must stay open.
            let expected = open_paragraph(&h).unwrap_or(opened);
            let shape = |h: &Harness<'static, PdfCraftApp>| -> (Option<usize>, Vec<[f64; 4]>) {
                let state = h.state();
                let block = state.views.first().and_then(|v| v.line_editor.as_ref()).map(|e| e.block);
                let rects = state
                    .views
                    .first()
                    .and_then(|v| state.session.get(v.id))
                    .map(|d| d.text_blocks(0).iter().map(|b| b.rect).collect())
                    .unwrap_or_default();
                (block, rects)
            };
            for press in 1..=2 {
                let (was, was_rects) = shape(&h);
                let before_rows = was.and_then(|b| was_rects.get(b)).map(|r| split_numbers(&h, *r)).unwrap_or_default();
                h.get_by_label("Bold").click();
                settle(&mut h);
                if open_paragraph(&h).as_deref() != Some(expected.as_str()) {
                    outcome.wrong_paragraph += 1;
                    let (now, now_rects) = shape(&h);
                    let boxed = |r: Option<&[f64; 4]>| r.map_or("none".to_owned(), |r| format!("{:.0},{:.0},{:.0},{:.0}", r[0], r[1], r[2], r[3]));
                    outcome.wrong_details.push(format!(
                        "paragraph {} press {press}: open #{:?} box {} -> open #{:?} box {}; paragraphs {} -> {}",
                        n + 1,
                        was,
                        boxed(was.and_then(|b| was_rects.get(b))),
                        now,
                        boxed(now.and_then(|b| now_rects.get(b))),
                        was_rects.len(),
                        now_rects.len()
                    ));
                    outcome.wrong_details.extend(before_rows.iter().map(|r| format!("  before{}", r.trim_start_matches(' '))));
                    if let Some(old) = was.and_then(|b| was_rects.get(b)).copied() {
                        outcome.wrong_details.extend(split_numbers(&h, old));
                    }
                }
            }
            let bolded = h.render()?;
            outcome.boxes += solid_blobs(&bolded, area);
            if let Some(dir) = keep {
                let _ = bolded.save(dir.join(format!("ui-{}-bold.png", n + 1)));
            }
            // Close it, then drag the paragraph: every drawn frame is timed, and the time until the
            // page is drawn again after the drop.
            h.key_press(Key::Escape);
            settle(&mut h);
            let from = centre;
            h.hover_at(from);
            h.run_steps(2);
            h.drag_at(from);
            h.run_steps(1);
            for step in 1..=10 {
                let t = Instant::now();
                h.hover_at(from + egui::vec2(8.0 * step as f32, 5.0 * step as f32));
                h.run_steps(1);
                let _ = h.render()?;
                outcome.frames.push(t.elapsed().as_secs_f64() * 1000.0);
            }
            let t = Instant::now();
            h.drop_at(from + egui::vec2(80.0, 50.0));
            settle(&mut h);
            outcome.settled.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        Ok(())
    })();
    outcome.error = result.err();
    outcome
}

/// Numbers only (never text) about the paragraphs now covering `old`, the open paragraph's box
/// before the press: per paragraph its box and, per row, the top of its first line, the size,
/// bold, its left and right edges and number of characters, and the class of the row's first character (digit, Latin, Arabic, bullet, other) with
/// whether the row opens like a list marker ("12)", "a.", "•").
fn split_numbers(h: &Harness<'static, PdfCraftApp>, old: [f64; 4]) -> Vec<String> {
    let state = h.state();
    let Some(doc) = state.views.first().and_then(|v| state.session.get(v.id)) else { return Vec::new() };
    let lines = doc.text_lines(0);
    let overlaps = |r: &[f64; 4]| r[0] < old[2] && r[2] > old[0] && r[1] < old[3] && r[3] > old[1];
    let class = |text: &str| -> String {
        let t = text.trim_start();
        let first = t.chars().next();
        let kind = match first {
            None => "empty",
            Some(c) if c.is_ascii_digit() => "digit",
            Some(c) if c.is_ascii_alphabetic() => "latin",
            Some(c) if ('\u{0600}'..='\u{06FF}').contains(&c) => "arabic",
            Some('•' | '◦' | '▪' | '‣' | '⁃' | '*' | '-' | '–' | '—') => "bullet",
            Some(_) => "other",
        };
        let lead: String = t.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        let after = t.chars().nth(lead.chars().count());
        let marker = !lead.is_empty() && lead.len() <= 3 && matches!(after, Some('.' | ')'));
        format!("{kind}{}", if marker { "+marker" } else { "" })
    };
    doc.text_blocks(0)
        .iter()
        .enumerate()
        .filter(|(_, b)| overlaps(&b.rect))
        .map(|(i, b)| {
            let rows: Vec<String> = b
                .rows
                .iter()
                .filter_map(|row| {
                    let first = row.first().and_then(|n| lines.get(*n))?;
                    let text: String = row.iter().filter_map(|n| lines.get(*n)).map(|l| l.text.as_str()).collect();
                    let left = row.iter().filter_map(|n| lines.get(*n)).map(|l| l.rect[0]).fold(f64::INFINITY, f64::min);
                    let right = row.iter().filter_map(|n| lines.get(*n)).map(|l| l.rect[2]).fold(f64::NEG_INFINITY, f64::max);
                    Some(format!(
                        "y{:.1} x{:.0}-{:.0} c{} s{:.1}{} {}",
                        first.rect[1],
                        left,
                        right,
                        text.chars().count(),
                        first.size,
                        if first.bold { " b" } else { "" },
                        class(&text)
                    ))
                })
                .collect();
            format!("  now #{i} box {:.0},{:.0},{:.0},{:.0} rows [{}]", b.rect[0], b.rect[1], b.rect[2], b.rect[3], rows.join("; "))
        })
        .collect()
}

fn percentile(values: &[f64], p: f64) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    v.get(((v.len() as f64 - 1.0) * p).round() as usize).copied().unwrap_or(0.0)
}

/// The table: `backend` names the graphics backend the run used (or "default").
pub fn table(outcomes: &[Outcome], backend: &str, public: impl Fn(&str) -> bool) -> String {
    let mut md = format!(
        "\n### Interface, drawn by the machine's graphics ({backend})\n\nThe app's own window code on each document's first Arabic paragraphs: opened, retyped, Bold twice, dragged. Boxes: solid square blobs over the text with the editor open, against before. Frame times include drawing the whole window.\n\n| Document | Paragraphs | Boxes (open / before) | Bold on another paragraph | Frame ms (median / 95% / max) | After a move, until drawn (ms) |\n|---|---|---|---|---|---|\n"
    );
    for o in outcomes {
        if let Some(e) = &o.error {
            let why = if public(&o.name) { e.replace('|', "/") } else { "could not be checked".to_owned() };
            md.push_str(&format!("| {} | – | – | – | – | error: {why} |\n", o.name));
            continue;
        }
        md.push_str(&format!(
            "| {} | {} | {} / {} | {} | {:.0} / {:.0} / {:.0} | {} |\n",
            o.name,
            o.paragraphs,
            o.boxes,
            o.baseline * 2,
            o.wrong_paragraph,
            percentile(&o.frames, 0.5),
            percentile(&o.frames, 0.95),
            percentile(&o.frames, 1.0),
            o.settled.iter().map(|s| format!("{s:.0}")).collect::<Vec<_>>().join(" ")
        ));
    }
    // Numbers about a Bold that opened another paragraph; a real document's are shown too (no text).
    for o in outcomes {
        for d in &o.wrong_details {
            md.push_str(&format!("\n- {}: {d}", o.name));
        }
    }
    md.push('\n');
    md
}

/// Whether a run found a failure: boxes beyond those before, or Bold on another paragraph.
pub fn failed(outcomes: &[Outcome]) -> bool {
    outcomes.iter().any(|o| o.error.is_some() || o.wrong_paragraph > 0 || o.boxes > o.baseline * 2)
}

/// The input list a child run reads: `name<TAB>path` per line.
pub fn read_inputs(list: &Path) -> Vec<(String, PathBuf)> {
    std::fs::read_to_string(list)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('\t').map(|(n, p)| (n.to_owned(), PathBuf::from(p))))
        .collect()
}
