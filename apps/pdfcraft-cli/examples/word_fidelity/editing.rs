//! Arabic text editing checks: every Arabic paragraph of a PDF is edited the ways people edit
//! (a word added at either end, digits and brackets, Arabic-Indic digits, an English word, a
//! word deleted, punctuation), saved and reopened. An edit passes when the paragraph reads back
//! as typed, the page's other paragraphs are untouched, the typed words are found by search and
//! the page still renders.
//!
//! Each edit runs once keeping the paragraph's own font and once in each installed Arabic face
//! found on the machine ([`faces`]), through the same automation tools as the app's Edit tool.
//!
//! A real document's failures are reported by kind and count only: its text never reaches the
//! summary.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pdfcraft_automation::{Automation, Content};
use pdfcraft_render::{PageRenderer, RenderConfig, RenderRequest, RequestKind};
use serde_json::{Value, json};

/// Arabic paragraphs edited per document with the paragraph's own font, and with each face.
const PARAGRAPHS: usize = 40;
const PARAGRAPHS_PER_FACE: usize = 12;
/// Paragraphs per document and mode that also get the chain of edit, edit, move, widen, bold.
const CHAINS: usize = 4;
/// Installed faces tried, at most.
const FACES: usize = 3;
/// Failures of a synthetic document described in the summary, per kind.
const SHOWN: usize = 4;

/// An installed face that can be embedded: family name and file.
pub struct Face {
    pub family: String,
    pub path: PathBuf,
    /// The family's Bold, when installed.
    pub bold: Option<PathBuf>,
}

/// The Arabic faces Council documents use that are installed here and may be embedded, most
/// common first (DejaVu Sans stands in where there are no Windows fonts).
pub fn faces() -> Vec<Face> {
    const WANTED: [&str; 7] = ["Arial", "Sakkal Majalla", "Simplified Arabic", "Traditional Arabic", "Tahoma", "Times New Roman", "DejaVu Sans"];
    let found = pdfcraft_engine::text_fonts::discover(&pdfcraft_engine::text_fonts::installed_directories());
    WANTED
        .iter()
        .filter_map(|family| {
            // The regular weight: the shortest face name of the family.
            found.iter().filter(|f| f.family.eq_ignore_ascii_case(family)).min_by_key(|f| f.face.len())
        })
        .take(FACES)
        .map(|f| Face {
            family: f.family.clone(),
            path: f.path.clone(),
            bold: found
                .iter()
                .filter(|b| b.family == f.family && b.face.to_ascii_lowercase().contains("bold") && !b.face.to_ascii_lowercase().contains("italic"))
                .min_by_key(|b| b.face.len())
                .map(|b| b.path.clone()),
        })
        .collect()
}

/// The Council's usual Windows fonts and what happens to each when a paragraph is edited:
/// embeddable (the edited text keeps the font) or not installed / refused with the reason (it
/// changes to a stand-in). System fonts only, so it names them. Collections (`.ttc`) can't be
/// embedded and are counted.
pub fn font_report() -> String {
    const KNOWN: [(&str, &[&str]); 9] = [
        ("Arial", &["arial.ttf", "arialbd.ttf"]),
        ("Tahoma", &["tahoma.ttf", "tahomabd.ttf"]),
        ("Times New Roman", &["times.ttf", "timesbd.ttf"]),
        ("Sakkal Majalla", &["majalla.ttf", "majallab.ttf"]),
        ("Simplified Arabic", &["simpo.ttf", "simpbdo.ttf", "simpfxo.ttf"]),
        ("Traditional Arabic", &["trado.ttf", "tradbdo.ttf"]),
        ("Arabic Typesetting", &["arabtype.ttf"]),
        ("Segoe UI", &["segoeui.ttf", "segoeuib.ttf"]),
        ("Calibri", &["calibri.ttf", "calibrib.ttf"]),
    ];
    let dirs = pdfcraft_engine::text_fonts::installed_directories();
    let (mut files, mut collections) = (0usize, 0usize);
    for dir in &dirs {
        for path in std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()) {
            match path.extension().and_then(std::ffi::OsStr::to_str).map(str::to_ascii_lowercase).as_deref() {
                Some("ttf" | "otf") => files += 1,
                Some("ttc" | "otc") => collections += 1,
                _ => {}
            }
        }
    }
    let mut md = format!(
        "\n### Installed fonts\n\n{files} font files, {collections} collections (not embeddable). Per face: kept when a paragraph is edited, or why it is not.\n\n"
    );
    for (family, names) in KNOWN {
        let mut verdicts = Vec::new();
        for name in names {
            let found = dirs.iter().map(|d| d.join(name)).find(|p| p.is_file());
            let verdict = match found.map(|p| std::fs::read(p).map_err(|e| e.to_string())) {
                None => "not installed".to_owned(),
                Some(Err(e)) => format!("unreadable: {e}"),
                Some(Ok(bytes)) => match pdfcraft_engine::text_fonts::embeddable(&bytes) {
                    Ok(()) => "kept".to_owned(),
                    Err(why) => format!("refused: {why}"),
                },
            };
            verdicts.push(format!("{name} {verdict}"));
        }
        md.push_str(&format!("- {family}: {}\n", verdicts.join("; ")));
    }
    md
}

/// A paragraph found again by the start of its text: its number, text and box (top-left origin).
fn find_paragraph(auto: &mut Automation, doc: i64, page: usize, anchor: &str) -> Result<(i64, String, [f64; 4]), (&'static str, String)> {
    let listed = tool(auto, "text_paragraphs", json!({ "doc": doc, "page": page })).map_err(|e| ("the edit failed", e))?;
    listed
        .get("paragraphs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find_map(|p| {
            let text = p.get("editor_text")?.as_str()?;
            norm(text).starts_with(anchor).then_some(())?;
            let rect = p.get("rect")?.as_array()?;
            let at = |i: usize| rect.get(i).and_then(Value::as_f64);
            Some((p.get("paragraph")?.as_i64()?, text.to_owned(), [at(0)?, at(1)?, at(2)?, at(3)?]))
        })
        .ok_or(("the paragraph was lost", format!("no paragraph starts “{anchor}”")))
}

/// What people do to one paragraph in a row, in one session, then save, reopen and check:
/// type at its end, type again (the second edit of a paragraph), move it, widen its box from the
/// left (the right edge of Arabic text stays), make it bold in the family's Bold, and reopen.
/// `face`: the first edit loads the installed face, as the app's installed-font choice does.
fn chain(work: &Path, page: usize, number: i64, text: &str, face: bool, bold: bool, others: &[String]) -> Result<(), (&'static str, String)> {
    let step = |e: String| ("the edit failed", e);
    let anchor: String = norm(text).chars().take(12).collect();
    let (mut auto, doc, _) = open(work, "in.pdf").map_err(step)?;
    let mut args = json!({ "doc": doc, "page": page, "paragraph": number, "text": format!("{} جديد", text.trim_end()) });
    if face {
        let font = tool(&mut auto, "text_font_import", json!({ "doc": doc, "page": page, "path": "face.ttf" })).map_err(step)?;
        args["font_resource"] = font.get("resource").cloned().ok_or_else(|| step("the face was not loaded".into()))?;
    }
    tool(&mut auto, "text_edit", args).map_err(step)?;
    // The second edit of the same paragraph, as the app sends it (no font of its own).
    let (n, current, _) = find_paragraph(&mut auto, doc, page, &anchor)?;
    let second = current.trim_end().replace(" جديد", " ثاني ٢");
    tool(&mut auto, "text_edit", json!({ "doc": doc, "page": page, "paragraph": n, "text": second })).map_err(|e| ("the second edit failed", e))?;
    // Move it 20 pt right and 30 pt down.
    let (_, _, before) = find_paragraph(&mut auto, doc, page, &anchor)?;
    let listed = tool(&mut auto, "object_list", json!({ "doc": doc, "page": page })).map_err(|e| ("moving failed", e))?;
    // The paragraph's object: the text object, or the added item an edit redrew it as, whose box
    // covers its box (the tightest one) (its text may read in another order than the paragraph's).
    let boxed = |o: &Value| o.get("rect").and_then(Value::as_array).map(|r| [0, 1, 2, 3].map(|i| r.get(i).and_then(Value::as_f64).unwrap_or(0.0)));
    let area = |r: [f64; 4]| ((r[2] - r[0]) * (r[3] - r[1])).max(0.0);
    let covers = |o: &Value| {
        let Some(r) = boxed(o) else { return false };
        let (w, h) = ((r[2].min(before[2]) - r[0].max(before[0])).max(0.0), (r[3].min(before[3]) - r[1].max(before[1])).max(0.0));
        area(before) > 0.0 && w * h / area(before) >= 0.8
    };
    let (kind, object) = listed
        .get("objects")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|o| matches!(o.get("kind").and_then(Value::as_str), Some("text" | "added")) && covers(o))
        .min_by(|a, b| area(boxed(a).unwrap_or([0.0; 4])).total_cmp(&area(boxed(b).unwrap_or([0.0; 4]))))
        .and_then(|o| Some((o.get("kind")?.as_str()?.to_owned(), o.get("index")?.clone())))
        .ok_or(("moving failed", "the paragraph is not an object".to_owned()))?;
    let mut mv = json!({ "doc": doc, "page": page, "objects": [{ "kind": kind, "index": object }], "offset": [20.0, 30.0] });
    if let Some(g) = listed.get("generation") {
        mv["generation"] = g.clone();
    }
    tool(&mut auto, "object_move", mv).map_err(|e| ("moving failed", e))?;
    let (n, current, moved) = find_paragraph(&mut auto, doc, page, &anchor)?;
    if (moved[0] - before[0] - 20.0).abs() > 3.0 || (moved[1] - before[1] - 30.0).abs() > 3.0 {
        return Err(("moved to the wrong place", format!("by {:.1}, {:.1}", moved[0] - before[0], moved[1] - before[1])));
    }
    // Widen the box 40 pt to the left (what the left handle does).
    tool(&mut auto, "text_edit", json!({ "doc": doc, "page": page, "paragraph": n, "width": moved[2] - moved[0] + 40.0, "dx": -40.0 }))
        .map_err(|e| ("widening failed", e))?;
    let (n, current2, widened) = find_paragraph(&mut auto, doc, page, &anchor)?;
    if norm(&current2) != norm(&current) || widened[2] > moved[2] + 3.0 {
        return Err(("widening changed the text or moved it", format!("right edge {:.1} → {:.1}", moved[2], widened[2])));
    }
    let mut expected = norm(&current2);
    if bold {
        let font = tool(&mut auto, "text_font_import", json!({ "doc": doc, "page": page, "path": "bold.ttf" })).map_err(|e| ("bold failed", e))?;
        let resource = font.get("resource").cloned().ok_or(("bold failed", "the bold face was not loaded".to_owned()))?;
        tool(&mut auto, "text_edit", json!({ "doc": doc, "page": page, "paragraph": n, "text": current2, "font_resource": resource }))
            .map_err(|e| ("bold failed", e))?;
        let (_, after, _) = find_paragraph(&mut auto, doc, page, &anchor)?;
        expected = norm(&after);
    }
    tool(&mut auto, "doc_save", json!({ "doc": doc, "path": "out.pdf" })).map_err(step)?;
    drop(auto);
    let (mut auto, doc, _) = open(work, "out.pdf").map_err(|e| ("the saved file does not reopen", e))?;
    let texts: Vec<String> =
        paragraphs(&mut auto, doc, page).map_err(|e| ("the saved file does not reopen", e))?.iter().map(|(_, t)| norm(t)).collect();
    let joined = texts.join(" ");
    if !joined.contains(&expected) {
        return Err(("reads back differently after the chain", format!("wanted “{expected}”")));
    }
    if let Some(gone) = others.iter().find(|o| !joined.contains(o.as_str())) {
        return Err(("another paragraph changed", format!("“{gone}”")));
    }
    match std::fs::read(work.join("out.pdf")) {
        Ok(bytes) if renders(&bytes, page) => Ok(()),
        _ => Err(("the page does not render", String::new())),
    }
}

/// One document's result.
pub struct Outcome {
    pub name: String,
    pub edits: usize,
    /// Kind of failure and, for a synthetic document, what happened.
    pub failures: Vec<(&'static str, String)>,
    pub error: Option<String>,
}

fn tool(auto: &mut Automation, name: &str, args: Value) -> Result<Value, String> {
    let out = auto.call(name, &args).map_err(|e| e.to_string())?;
    out.into_iter()
        .find_map(|c| match c {
            Content::Json(v) => Some(v),
            Content::Png { .. } => None,
        })
        .ok_or_else(|| format!("{name} returned nothing"))
}

fn open(work: &Path, file: &str) -> Result<(Automation, i64, usize), String> {
    let mut auto = Automation::new().with_root(work).map_err(|e| e.to_string())?;
    let doc = tool(&mut auto, "doc_open", json!({ "path": file }))?;
    let id = doc.get("doc").and_then(Value::as_i64).ok_or("doc_open gave no document")?;
    let pages = doc.get("pages").and_then(Value::as_u64).and_then(|p| usize::try_from(p).ok()).unwrap_or(0);
    Ok((auto, id, pages))
}

/// A paragraph as listed: its number and its text in the editor's form.
fn paragraphs(auto: &mut Automation, doc: i64, page: usize) -> Result<Vec<(i64, String)>, String> {
    let listed = tool(auto, "text_paragraphs", json!({ "doc": doc, "page": page }))?;
    Ok(listed
        .get("paragraphs")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(|p| Some((p.get("paragraph")?.as_i64()?, p.get("editor_text")?.as_str()?.to_owned()))).collect())
        .unwrap_or_default())
}

fn norm(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn has_arabic(text: &str) -> bool {
    text.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c))
}

/// The edits tried on a paragraph: name and new text.
fn variants(text: &str) -> Vec<(&'static str, String)> {
    let end = text.trim_end();
    let mut all = vec![
        ("add a word at the end", format!("{end} جديد")),
        ("digits and brackets", format!("{end} (5) 2026")),
        ("Arabic-Indic digits", format!("{end} ٣٤٥")),
        ("English words", format!("{end} PDF file")),
        ("add a word at the start", format!("تجربة {}", text.trim_start())),
        ("punctuation", format!("{end}، نعم: «لا»؟")),
    ];
    let flat = norm(text);
    let words: Vec<&str> = flat.split(' ').collect();
    if let Some(kept) = words.len().checked_sub(1).filter(|n| *n >= 2).and_then(|n| words.get(..n)) {
        all.push(("delete the last word", kept.join(" ")));
    }
    all
}

fn renders(pdf: &[u8], page: usize) -> bool {
    let mut renderer = PageRenderer::new(Arc::new(pdf.to_vec()), RenderConfig::default());
    let request = RenderRequest { page: page.saturating_sub(1), kind: RequestKind::Pixels, tile: None, scale: 0.5, tag: 0, preview: false };
    renderer.render(request).error.is_none()
}

/// One edit, saved to `out.pdf` and read back: `Err` is the kind of failure and its detail.
fn edit(work: &Path, page: usize, paragraph: i64, new: &str, face: bool, others: &[String]) -> Result<(), (&'static str, String)> {
    let saved = work.join("out.pdf");
    let _ = std::fs::remove_file(&saved);
    let step = |e: String| ("the edit failed", e);
    let (mut auto, doc, _) = open(work, "in.pdf").map_err(step)?;
    let mut args = json!({ "doc": doc, "page": page, "paragraph": paragraph, "text": new });
    if face {
        let font = tool(&mut auto, "text_font_import", json!({ "doc": doc, "page": page, "path": "face.ttf" })).map_err(step)?;
        let resource = font.get("resource").and_then(Value::as_str).ok_or_else(|| step("the face was not loaded".into()))?;
        args["font_resource"] = json!(resource);
    }
    tool(&mut auto, "text_edit", args).map_err(step)?;
    tool(&mut auto, "doc_save", json!({ "doc": doc, "path": "out.pdf" })).map_err(step)?;
    drop(auto);

    let reopened = |e: String| ("the saved file does not reopen", e);
    let (mut auto, doc, _) = open(work, "out.pdf").map_err(reopened)?;
    let texts: Vec<String> = paragraphs(&mut auto, doc, page).map_err(reopened)?.iter().map(|(_, t)| norm(t)).collect();
    let (want, joined) = (norm(new), texts.join(" "));
    if !texts.contains(&want) && !joined.contains(&want) {
        let shared = |t: &String| t.split(' ').filter(|w| want.split(' ').any(|x| x == *w)).count();
        let got = texts.iter().max_by_key(|t| shared(t)).cloned().unwrap_or_default();
        return Err(("reads back differently", format!("typed “{want}”, reads “{got}”")));
    }
    if let Some(gone) = others.iter().find(|o| !joined.contains(o.as_str())) {
        return Err(("another paragraph changed", format!("“{gone}”")));
    }
    // The last two words typed on the last line, as a reader would search for them.
    let last_line = norm(new.rsplit('\n').next().unwrap_or(new));
    let words: Vec<&str> = last_line.split(' ').collect();
    let tail = words.get(words.len().saturating_sub(2)..).map(|w| w.join(" ")).unwrap_or_default();
    let found = tool(&mut auto, "text_find", json!({ "doc": doc, "query": tail })).map_err(|e| ("search fails", e))?;
    if found.get("count").and_then(Value::as_u64).unwrap_or(0) == 0 {
        return Err(("not found by search", format!("“{tail}”")));
    }
    match std::fs::read(&saved) {
        Ok(bytes) if renders(&bytes, page) => Ok(()),
        _ => Err(("the page does not render", String::new())),
    }
}

/// Runs the editing checks on `pdf` in the folder `work`. `keep` (a synthetic document's folder)
/// receives the last passing result as `edited.pdf`, to look at.
pub fn run(name: &str, pdf: &[u8], work: &Path, faces: &[Face], keep: Option<&Path>) -> Outcome {
    let mut outcome = Outcome { name: name.to_owned(), edits: 0, failures: Vec::new(), error: None };
    let result = (|| -> Result<(), String> {
        std::fs::create_dir_all(work).map_err(|e| e.to_string())?;
        std::fs::write(work.join("in.pdf"), pdf).map_err(|e| e.to_string())?;
        let (mut auto, doc, pages) = open(work, "in.pdf")?;
        let mut listed = Vec::new();
        for page in 1..=pages {
            listed.push((page, paragraphs(&mut auto, doc, page)?));
        }
        drop(auto);
        // The paragraph's own font first, then each installed face.
        let modes = std::iter::once(None).chain(faces.iter().map(Some));
        for mode in modes {
            if let Some(face) = mode {
                std::fs::copy(&face.path, work.join("face.ttf")).map_err(|e| format!("{}: {e}", face.family))?;
            }
            let label = mode.map_or("own font", |f| f.family.as_str());
            let mut budget = if mode.is_some() { PARAGRAPHS_PER_FACE } else { PARAGRAPHS };
            let mut chains = CHAINS;
            for (page, list) in &listed {
                for (number, text) in list.iter().filter(|(_, t)| has_arabic(t)) {
                    if budget == 0 {
                        break;
                    }
                    budget -= 1;
                    let others: Vec<String> = list.iter().filter(|(n, _)| n != number).map(|(_, t)| norm(t)).collect();
                    // Only a sentence or more: one word beside its neighbours regroups with them.
                    if chains > 0 && norm(text).chars().count() >= 25 {
                        chains -= 1;
                        outcome.edits += 1;
                        let with_bold = mode.is_some_and(|f| f.bold.is_some());
                        if let Some(bold) = mode.and_then(|f| f.bold.as_ref()) {
                            let _ = std::fs::copy(bold, work.join("bold.ttf"));
                        }
                        if let Err((kind, detail)) = chain(work, *page, *number, text, mode.is_some(), with_bold, &others) {
                            outcome
                                .failures
                                .push((kind, format!("page {page}, paragraph {number}, {label}, edit twice, move, widen, bold: {detail}")));
                        }
                    }
                    for (variant, new) in variants(text) {
                        outcome.edits += 1;
                        match edit(work, *page, *number, &new, mode.is_some(), &others) {
                            Ok(()) => {
                                if let Some(dir) = keep {
                                    let _ = std::fs::copy(work.join("out.pdf"), dir.join("edited.pdf"));
                                }
                            }
                            Err((kind, detail)) => {
                                outcome.failures.push((kind, format!("page {page}, paragraph {number}, {label}, {variant}: {detail}")));
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    })();
    outcome.error = result.err();
    let _ = std::fs::remove_dir_all(work);
    outcome
}

/// The summary table. `public` says which documents (by name) may have their failures described.
pub fn table(outcomes: &[Outcome], faces: &[Face], public: impl Fn(&str) -> bool) -> String {
    let names: Vec<&str> = faces.iter().map(|f| f.family.as_str()).collect();
    let mut md = format!(
        "\n### Arabic text editing\n\nEach Arabic paragraph edited, saved and reopened, with its own font{}.\n\n| Document | Edits | Passed | Failed | Failures by kind |\n|---|---|---|---|---|\n",
        if names.is_empty() { String::new() } else { format!(" and in {}", names.join(", ")) }
    );
    let mut details = String::new();
    for o in outcomes {
        if let Some(e) = &o.error {
            // A real document's error could quote it: only a synthetic one's is shown.
            let why = if public(&o.name) { e.replace('|', "/") } else { "could not be checked".to_owned() };
            md.push_str(&format!("| {} | – | – | – | error: {why} |\n", o.name));
            continue;
        }
        let mut kinds: Vec<(&str, usize)> = Vec::new();
        for (kind, _) in &o.failures {
            match kinds.iter_mut().find(|(k, _)| k == kind) {
                Some((_, n)) => *n += 1,
                None => kinds.push((kind, 1)),
            }
        }
        let by_kind: Vec<String> = kinds.iter().map(|(k, n)| format!("{k}: {n}")).collect();
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            o.name,
            o.edits,
            o.edits.saturating_sub(o.failures.len()),
            o.failures.len(),
            if by_kind.is_empty() { "–".to_owned() } else { by_kind.join("; ") }
        ));
        if public(&o.name) {
            for (kind, _) in &kinds {
                for (_, detail) in o.failures.iter().filter(|(k, _)| k == kind).take(SHOWN) {
                    details.push_str(&format!("- {}: {kind}: {}\n", o.name, detail.replace('\n', " ⏎ ")));
                }
            }
        }
    }
    if !details.is_empty() {
        md.push_str("\nFailures in the synthetic documents:\n\n");
        md.push_str(&details);
    }
    md
}
