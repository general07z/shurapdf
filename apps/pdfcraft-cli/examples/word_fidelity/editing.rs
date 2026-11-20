//! Arabic text editing checks: every Arabic paragraph of a PDF is edited the ways people edit
//! (a word added at either end, digits and brackets, Arabic-Indic digits, an English word, a
//! word deleted, punctuation), saved and reopened. An edit passes when the paragraph reads back
//! as typed, the page's other paragraphs are untouched, the typed words are found by search and
//! the page still renders.
//!
//! Each edit runs once keeping the paragraph's own font and once in each installed Arabic face
//! found on the machine ([`faces`]), through the same automation tools as the app's Edit tool.
//!
//! A real document's failures are reported by kind and count, and each with where it happened
//! (page, paragraph number, font, kind of edit) and, for a search failure, which kinds of
//! character the searched words hold, whether the unedited document finds them, how often each
//! word is found alone and how the edited page reads them (other spacing, reversed, characters
//! missing by kind): its text never reaches the summary.

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
/// Failures of a real document listed by where they happened, per kind.
const SHOWN_PRIVATE: usize = 25;

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
/// A chain failure: its kind, its detail (which may quote a synthetic document) and a note with
/// no text of the document, for a real one.
type ChainError = (&'static str, String, String);

fn plain(kind: &'static str) -> impl Fn(String) -> ChainError + Copy {
    move |e| (kind, e, String::new())
}

fn find_paragraph(auto: &mut Automation, doc: i64, page: usize, anchor: &str, after: &str) -> Result<(i64, String, [f64; 4]), ChainError> {
    let listed = tool(auto, "text_paragraphs", json!({ "doc": doc, "page": page })).map_err(plain("the edit failed"))?;
    let all: Vec<&Value> = listed.get("paragraphs").and_then(Value::as_array).into_iter().flatten().collect();
    let found = all.iter().find_map(|p| {
        let text = p.get("editor_text")?.as_str()?;
        norm(text).starts_with(anchor).then_some(())?;
        let rect = p.get("rect")?.as_array()?;
        let at = |i: usize| rect.get(i).and_then(Value::as_f64);
        Some((p.get("paragraph")?.as_i64()?, text.to_owned(), [at(0)?, at(1)?, at(2)?, at(3)?]))
    });
    if let Some(found) = found {
        return Ok(found);
    }
    // The paragraph sharing most of the anchor's characters, wherever they are.
    let texts: Vec<String> = all.iter().filter_map(|p| p.get("editor_text")?.as_str().map(norm)).collect();
    let shared = |t: &str| {
        let mut have: Vec<char> = t.chars().collect();
        anchor.chars().filter(|c| have.iter().position(|h| h == c).map(|i| have.swap_remove(i)).is_some()).count()
    };
    let closest = texts.iter().max_by_key(|t| shared(t)).cloned().unwrap_or_default();
    let kinds = |t: &str| {
        let mut out: Vec<&str> = Vec::new();
        for c in t.chars() {
            let k = if c.is_whitespace() { "space" } else { class_of(c) };
            if out.last() != Some(&k) {
                out.push(k);
            }
        }
        out.join(" ")
    };
    let head: String = closest.chars().take(anchor.chars().count()).collect();
    let where_ = match closest.find(anchor) {
        Some(at) => format!("it holds the anchor {} characters in", closest[..at].chars().count()),
        None if shared(&closest) == anchor.chars().count() => "it holds the anchor's characters, not in a row".to_owned(),
        None => format!("it holds {} of the anchor's {} characters", shared(&closest), anchor.chars().count()),
    };
    let note = format!(
        "lost after {after}; {} paragraphs on the page; the anchor reads {}; the closest paragraph starts {} ({} characters long); {where_}",
        texts.len(),
        kinds(anchor),
        kinds(&head),
        closest.chars().count()
    );
    Err(("the paragraph was lost", format!("after {after}, no paragraph starts “{anchor}”; closest: “{closest}”"), note))
}

/// What people do to one paragraph in a row, in one session, then save, reopen and check:
/// type at its end, type again (the second edit of a paragraph), move it, widen its box from the
/// left (the right edge of Arabic text stays), make it bold in the family's Bold, and reopen.
/// `face`: the first edit loads the installed face, as the app's installed-font choice does.
fn chain(work: &Path, page: usize, number: i64, text: &str, face: bool, bold: bool, others: &[String]) -> Result<(), ChainError> {
    let step = plain("the edit failed");
    let anchor: String = norm(text).chars().take(12).collect();
    let (mut auto, doc, _) = open(work, "in.pdf").map_err(step)?;
    let mut args = json!({ "doc": doc, "page": page, "paragraph": number, "text": format!("{} جديد", text.trim_end()) });
    if face {
        let font = tool(&mut auto, "text_font_import", json!({ "doc": doc, "page": page, "path": "face.ttf" })).map_err(step)?;
        args["font_resource"] = font.get("resource").cloned().ok_or_else(|| step("the face was not loaded".into()))?;
    }
    tool(&mut auto, "text_edit", args).map_err(step)?;
    // The second edit of the same paragraph, as the app sends it (no font of its own).
    let (n, current, _) = find_paragraph(&mut auto, doc, page, &anchor, "the first edit")?;
    let second = current.trim_end().replace(" جديد", " ثاني ٢");
    tool(&mut auto, "text_edit", json!({ "doc": doc, "page": page, "paragraph": n, "text": second })).map_err(plain("the second edit failed"))?;
    // Move it 20 pt right and 30 pt down.
    let (_, _, before) = find_paragraph(&mut auto, doc, page, &anchor, "the second edit")?;
    let listed = tool(&mut auto, "object_list", json!({ "doc": doc, "page": page })).map_err(plain("moving failed"))?;
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
        .ok_or(("moving failed", "the paragraph is not an object".to_owned(), String::new()))?;
    let mut mv = json!({ "doc": doc, "page": page, "objects": [{ "kind": kind, "index": object }], "offset": [20.0, 30.0] });
    if let Some(g) = listed.get("generation") {
        mv["generation"] = g.clone();
    }
    tool(&mut auto, "object_move", mv).map_err(plain("moving failed"))?;
    let (n, current, moved) = find_paragraph(&mut auto, doc, page, &anchor, "the move")?;
    if (moved[0] - before[0] - 20.0).abs() > 3.0 || (moved[1] - before[1] - 30.0).abs() > 3.0 {
        return Err(("moved to the wrong place", format!("by {:.1}, {:.1}", moved[0] - before[0], moved[1] - before[1]), String::new()));
    }
    // Widen the box 40 pt to the left (what the left handle does).
    tool(&mut auto, "text_edit", json!({ "doc": doc, "page": page, "paragraph": n, "width": moved[2] - moved[0] + 40.0, "dx": -40.0 }))
        .map_err(plain("widening failed"))?;
    let (n, current2, widened) = find_paragraph(&mut auto, doc, page, &anchor, "widening")?;
    if norm(&current2) != norm(&current) || widened[2] > moved[2] + 3.0 {
        return Err(("widening changed the text or moved it", format!("right edge {:.1} → {:.1}", moved[2], widened[2]), String::new()));
    }
    let mut expected = norm(&current2);
    if bold {
        let font = tool(&mut auto, "text_font_import", json!({ "doc": doc, "page": page, "path": "bold.ttf" })).map_err(plain("bold failed"))?;
        let resource = font.get("resource").cloned().ok_or(("bold failed", "the bold face was not loaded".to_owned(), String::new()))?;
        tool(&mut auto, "text_edit", json!({ "doc": doc, "page": page, "paragraph": n, "text": current2, "font_resource": resource }))
            .map_err(plain("bold failed"))?;
        let (_, after, _) = find_paragraph(&mut auto, doc, page, &anchor, "bold")?;
        expected = norm(&after);
    }
    tool(&mut auto, "doc_save", json!({ "doc": doc, "path": "out.pdf" })).map_err(step)?;
    drop(auto);
    let (mut auto, doc, _) = open(work, "out.pdf").map_err(plain("the saved file does not reopen"))?;
    let texts: Vec<String> =
        paragraphs(&mut auto, doc, page).map_err(plain("the saved file does not reopen"))?.iter().map(|(_, t)| norm(t)).collect();
    let joined = texts.join(" ");
    if !joined.contains(&expected) {
        return Err(("reads back differently after the chain", format!("wanted “{expected}”"), String::new()));
    }
    if let Some(gone) = others.iter().find(|o| !joined.contains(o.as_str())) {
        return Err(("another paragraph changed", format!("“{gone}”"), String::new()));
    }
    match std::fs::read(work.join("out.pdf")) {
        Ok(bytes) if renders(&bytes, page) => Ok(()),
        _ => Err(("the page does not render", String::new(), String::new())),
    }
}

/// The last two words typed on the last line of `new`, as a reader would search for them.
fn searched(new: &str) -> String {
    let last_line = norm(new.rsplit('\n').next().unwrap_or(new));
    let words: Vec<&str> = last_line.split(' ').collect();
    words.get(words.len().saturating_sub(2)..).map(|w| w.join(" ")).unwrap_or_default()
}

/// What a search failure can say about a real document without quoting it: the kinds of
/// character in the searched words, in order, each run of one kind named once (`Arabic, space,
/// digit`), and whether the unedited document (`in.pdf` in `work`) finds them.
fn search_note(work: &Path, page: usize, new: &str) -> String {
    let tail = searched(new);
    let kind = |c: char| match c {
        ' ' => "space",
        '0'..='9' => "digit",
        '\u{0660}'..='\u{0669}' | '\u{06F0}'..='\u{06F9}' => "Arabic-Indic digit",
        '\u{064B}'..='\u{065F}' | '\u{0670}' => "Arabic mark",
        '\u{0600}'..='\u{06FF}' | '\u{FB50}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}' if c.is_alphabetic() => "Arabic",
        c if c.is_alphabetic() => "Latin",
        c if c.is_whitespace() => "other space",
        c if c.is_control() || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{061C}') => "control",
        _ => "punctuation",
    };
    let mut kinds: Vec<&str> = Vec::new();
    for c in tail.chars() {
        let k = kind(c);
        if kinds.last() != Some(&k) {
            kinds.push(k);
        }
    }
    let before = open(work, "in.pdf")
        .and_then(|(mut auto, doc, _)| tool(&mut auto, "text_find", json!({ "doc": doc, "query": tail })))
        .map(|found| found.get("count").and_then(Value::as_u64).unwrap_or(0));
    let unedited = match before {
        Ok(0) => "not found in the unedited document either".to_owned(),
        Ok(n) => format!("found {n} times in the unedited document"),
        Err(_) => "the unedited document could not be searched".to_owned(),
    };
    format!("searched {}; {unedited}; {}", kinds.join(", "), after_edit(work, page, new, &tail))
}

/// The kind of character `c` is, as the summary names it.
fn class_of(c: char) -> &'static str {
    if c.is_ascii_digit() {
        "digit"
    } else if ('\u{0660}'..='\u{0669}').contains(&c) || ('\u{06F0}'..='\u{06F9}').contains(&c) {
        "Arabic-Indic digit"
    } else if c.is_alphabetic() && c.is_ascii() {
        "Latin"
    } else if c.is_alphabetic() {
        "Arabic"
    } else {
        "punctuation"
    }
}

/// Where `read`, which holds `want` once spaces are left out, spaces it differently: each
/// difference as the kinds of character on either side, with no text. At most four.
fn spacing_diff(read: &str, want: &str) -> String {
    let squeezed: Vec<(usize, char)> = read.char_indices().filter(|(_, c)| !c.is_whitespace()).collect();
    let target: Vec<char> = want.chars().filter(|c| !c.is_whitespace()).collect();
    let Some(at) = (0..=squeezed.len().saturating_sub(target.len()))
        .find(|&i| squeezed.get(i..i + target.len()).is_some_and(|w| w.iter().map(|(_, c)| *c).eq(target.iter().copied())))
    else {
        return "not placed".to_owned();
    };
    // Whether a space comes before the k-th non-space character, in the page text and as typed.
    let read_gap = |k: usize| match (squeezed.get(at + k - 1), squeezed.get(at + k)) {
        (Some((a, ca)), Some((b, _))) => read.get(a + ca.len_utf8()..*b).is_some_and(|g| !g.is_empty()),
        _ => false,
    };
    let want_chars: Vec<char> = want.chars().collect();
    let mut want_gap = vec![false; target.len()];
    let mut k = 0;
    for (i, c) in want_chars.iter().enumerate() {
        if c.is_whitespace() {
            continue;
        }
        if i > 0
            && want_chars.get(i - 1).is_some_and(|p| p.is_whitespace())
            && k > 0
            && let Some(g) = want_gap.get_mut(k)
        {
            *g = true;
        }
        k += 1;
    }
    let mut out: Vec<String> = Vec::new();
    for k in 1..target.len() {
        let (r, w) = (read_gap(k), want_gap.get(k).copied().unwrap_or(false));
        if r != w {
            let (a, b) = (target.get(k - 1).copied().map_or("?", class_of), target.get(k).copied().map_or("?", class_of));
            out.push(format!("space {} between {a} and {b}", if r { "added" } else { "missing" }));
        }
    }
    out.truncate(4);
    if out.is_empty() { "same spacing".to_owned() } else { out.join(", ") }
}

/// How the edited page (`out.pdf` in `work`) reads where the edited paragraph is, against the
/// searched words `tail`, in counts and yes or no only: how often each searched word is found
/// alone, and whether the page's text over the paragraph holds the words with other spacing,
/// reversed, or which kinds of character it lacks.
fn after_edit(work: &Path, page: usize, new: &str, tail: &str) -> String {
    let Ok((mut auto, doc, _)) = open(work, "out.pdf") else { return "the edited document could not be opened".to_owned() };
    let words: Vec<String> = tail
        .split(' ')
        .map(|w| {
            let n = tool(&mut auto, "text_find", json!({ "doc": doc, "query": w }))
                .ok()
                .and_then(|f| f.get("count").and_then(Value::as_u64))
                .unwrap_or(0);
            n.to_string()
        })
        .collect();
    let want = norm(new);
    let rect = tool(&mut auto, "text_paragraphs", json!({ "doc": doc, "page": page })).ok().and_then(|listed| {
        listed.get("paragraphs")?.as_array()?.iter().find_map(|p| {
            let text = norm(p.get("editor_text")?.as_str()?);
            (text == want || text.contains(&want)).then(|| p.get("rect").cloned()).flatten()
        })
    });
    let Some(rect) = rect else { return format!("each word found {} times; the paragraph is not listed", words.join(", ")) };
    let read = tool(&mut auto, "text_extract", json!({ "doc": doc, "pages": [page], "rect": rect }))
        .ok()
        .and_then(|e| e.get("pages")?.as_array()?.first()?.get("text")?.as_str().map(norm))
        .unwrap_or_default();
    let squeeze = |t: &str| t.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    let reversed: String = tail.chars().rev().collect();
    let how = if read.contains(tail) {
        "the page text over the paragraph holds them".to_owned()
    } else if squeeze(&read).contains(&squeeze(tail)) {
        format!("the page text over the paragraph holds them with other spacing ({})", spacing_diff(&read, tail))
    } else if read.contains(&reversed) || squeeze(&read).contains(&squeeze(&reversed)) {
        "the page text over the paragraph holds them reversed".to_owned()
    } else {
        let mut have: Vec<char> = read.chars().collect();
        let mut missing: Vec<&str> = Vec::new();
        for c in tail.chars().filter(|c| !c.is_whitespace()) {
            match have.iter().position(|h| *h == c) {
                Some(i) => {
                    have.swap_remove(i);
                }
                None => {
                    let k = if c.is_ascii_digit() {
                        "digit"
                    } else if c.is_alphabetic() && c.is_ascii() {
                        "Latin"
                    } else if c.is_alphabetic() {
                        "Arabic"
                    } else {
                        "punctuation"
                    };
                    if !missing.contains(&k) {
                        missing.push(k);
                    }
                }
            }
        }
        if missing.is_empty() {
            "the page text over the paragraph holds their characters in another order".to_owned()
        } else {
            format!("the page text over the paragraph lacks some {} characters", missing.join(", "))
        }
    };
    format!("each word found {} times; {how}; {}; {}", words.join(", "), drawn_spacing(work), glyphs_at_breaks(work, page, tail))
}

/// The two glyphs on either side of each word break the page reader puts inside the searched
/// words `tail` on the edited page, in numbers only, as fractions of the second glyph's height:
/// the horizontal gap between their boxes (negative where they overlap), the step between their
/// bottoms, and the ratio of their heights. At most four.
fn glyphs_at_breaks(work: &Path, page: usize, tail: &str) -> String {
    let Ok(bytes) = std::fs::read(work.join("out.pdf")) else { return "no glyphs".to_owned() };
    let mut renderer = PageRenderer::new(Arc::new(bytes), RenderConfig::default());
    let request = RenderRequest { page: page.saturating_sub(1), kind: RequestKind::Text, tile: None, scale: 1.0, tag: 0, preview: false };
    let Some(text) = renderer.render(request).text else { return "no glyphs".to_owned() };
    // The page's glyphs that are not blank, with whether the reader breaks a word before each.
    let mut seq: Vec<(usize, bool)> = Vec::new();
    let mut pending = false;
    for (i, g) in text.glyphs.iter().enumerate() {
        let brk = text.space_before.get(i).copied().unwrap_or(false) || (i > 0 && text.line_of.get(i) != text.line_of.get(i - 1));
        if g.text.chars().all(char::is_whitespace) {
            pending = true;
            continue;
        }
        seq.push((i, brk || pending));
        pending = false;
    }
    let want: Vec<char> = tail.chars().filter(|c| !c.is_whitespace()).collect();
    let chars: Vec<(usize, char, bool)> = seq
        .iter()
        .flat_map(|&(i, brk)| {
            text.glyphs.get(i).map(|g| g.text.chars().enumerate().map(move |(k, c)| (i, c, brk && k == 0)).collect::<Vec<_>>()).unwrap_or_default()
        })
        .collect();
    let Some(at) = (0..=chars.len().saturating_sub(want.len()))
        .find(|&s| chars.get(s..s + want.len()).is_some_and(|w| w.iter().map(|x| x.1).eq(want.iter().copied())))
    else {
        return "the glyphs are not in reading order".to_owned();
    };
    let mut typed_gap = vec![false; want.len()];
    let (mut k, mut space) = (0, false);
    for c in tail.chars() {
        if c.is_whitespace() {
            space = true;
            continue;
        }
        if let Some(g) = typed_gap.get_mut(k) {
            *g = space && k > 0;
        }
        space = false;
        k += 1;
    }
    let mut out = Vec::new();
    for k in 1..want.len() {
        let (Some(&(pi, _, _)), Some(&(ci, _, brk))) = (chars.get(at + k - 1), chars.get(at + k)) else { continue };
        if !brk || typed_gap.get(k).copied().unwrap_or(false) || pi == ci {
            continue;
        }
        let (Some(p), Some(c)) = (text.glyphs.get(pi), text.glyphs.get(ci)) else { continue };
        let h = (c.rect[3] - c.rect[1]).max(0.01);
        let gap = if c.rect[0] >= p.rect[0] { c.rect[0] - p.rect[2] } else { p.rect[0] - c.rect[2] };
        out.push(format!(
            "gap {:.2}, step {:.2}, height ratio {:.2}, drawn {} apart",
            gap / h,
            (c.rect[3] - p.rect[3]) / h,
            h / (p.rect[3] - p.rect[1]).max(0.01),
            ci.abs_diff(pi)
        ));
    }
    out.truncate(4);
    if out.is_empty() {
        return "no break inside the words".to_owned();
    }
    // Every glyph drawn from the first matched one to the last, in drawing order: its kind (a
    // blank by its code point) and its box's left and right edges and bottom, from the first
    // glyph's left edge and bottom, in its height.
    let (Some(first), Some(last)) = (chars.get(at).map(|c| c.0), chars.get(at + want.len() - 1).map(|c| c.0)) else { return out.join("; ") };
    let (lo, hi) = (first.min(last), first.max(last));
    let origin = text.glyphs.get(first).map_or([0.0; 4], |g| g.rect);
    let h = (origin[3] - origin[1]).max(0.01);
    let drawn: Vec<String> = (lo..=hi.min(lo + 40))
        .filter_map(|i| text.glyphs.get(i))
        .map(|g| {
            let kind = match g.text.chars().next() {
                Some(c) if c.is_whitespace() || c.is_control() => format!("U+{:04X}", u32::from(c)),
                Some(c) => class_of(c).to_owned(),
                None => "empty".to_owned(),
            };
            format!("{kind} {:.2}..{:.2}@{:.2}", (g.rect[0] - origin[0]) / h, (g.rect[2] - origin[0]) / h, (g.rect[3] - origin[3]) / h)
        })
        .collect();
    let blanks: Vec<String> = (lo..=hi.min(lo + 40))
        .filter_map(|i| text.glyphs.get(i))
        .filter(|g| g.text.chars().all(char::is_whitespace))
        .take(3)
        .map(|g| blank_source(work, page, g.rect, h))
        .collect();
    format!("at the breaks: {}; drawn: {}; blanks: {}", out.join("; "), drawn.join(", "), blanks.join("; "))
}

/// Where a blank glyph drawn at `rect` (view space) on the edited page comes from, in numbers and
/// kinds only: whether the unedited page draws a blank at the same place (a run the edit kept) and
/// the lines of each page that hold that place, by length, kinds of character, font (the edited
/// paragraph's or another) and size against `h`, the line height of the typed words.
fn blank_source(work: &Path, page: usize, rect: [f32; 4], h: f32) -> String {
    let near = |r: &[f32; 4]| (r[0] - rect[0]).abs() < h * 0.05 && (r[3] - rect[3]).abs() < h * 0.2;
    let before = std::fs::read(work.join("in.pdf")).ok().and_then(|bytes| {
        let mut renderer = PageRenderer::new(Arc::new(bytes), RenderConfig::default());
        let request = RenderRequest { page: page.saturating_sub(1), kind: RequestKind::Text, tile: None, scale: 1.0, tag: 0, preview: false };
        renderer.render(request).text
    });
    let kept = match before {
        Some(t) => match t.glyphs.iter().find(|g| near(&g.rect)) {
            Some(g) if g.text.chars().all(char::is_whitespace) => "the unedited page draws a blank there",
            Some(_) => "the unedited page draws a glyph there, not a blank",
            None => "the unedited page draws nothing there",
        },
        None => "the unedited page was not read",
    };
    let lines = |file: &str| -> String {
        let Ok((mut auto, doc, _)) = open(work, file) else { return "not opened".to_owned() };
        let font = tool(&mut auto, "text_paragraphs", json!({ "doc": doc, "page": page }))
            .ok()
            .and_then(|p| {
                p.get("paragraphs")?.as_array()?.iter().find_map(|p| {
                    let r: Vec<f64> = p.get("rect")?.as_array()?.iter().filter_map(Value::as_f64).collect();
                    let inside = r.len() == 4
                        && f64::from(rect[0]) >= r[0] - 1.0
                        && f64::from(rect[2]) <= r[2] + 1.0
                        && f64::from(rect[3]) >= r[1]
                        && f64::from(rect[1]) <= r[3];
                    inside.then(|| p.get("font")?.as_str().map(str::to_owned)).flatten()
                })
            })
            .unwrap_or_default();
        let Ok(listed) = tool(&mut auto, "text_lines", json!({ "doc": doc, "page": page })) else { return "not listed".to_owned() };
        let hits: Vec<String> = listed
            .get("lines")
            .and_then(Value::as_array)
            .map(|all| {
                all.iter()
                    .filter_map(|l| {
                        let r: Vec<f64> = l.get("rect")?.as_array()?.iter().filter_map(Value::as_f64).collect();
                        let (x, y) = (f64::from(rect[0] + rect[2]) / 2.0, f64::from(rect[1] + rect[3]) / 2.0);
                        (r.len() == 4 && x >= r[0] && x <= r[2] && y >= r[1] && y <= r[3]).then_some(())?;
                        let text = l.get("text")?.as_str()?;
                        let mut counts: Vec<(&str, usize)> = Vec::new();
                        for c in text.chars() {
                            let k = if c.is_whitespace() { "blank" } else { class_of(c) };
                            match counts.iter_mut().find(|e| e.0 == k) {
                                Some(e) => e.1 += 1,
                                None => counts.push((k, 1)),
                            }
                        }
                        let which =
                            if l.get("font").and_then(Value::as_str) == Some(font.as_str()) { "the paragraph's font" } else { "another font" };
                        let size = l.get("size").and_then(Value::as_f64).unwrap_or(0.0) / f64::from(h.max(0.01));
                        let kinds: Vec<String> = counts.iter().map(|(k, n)| format!("{k} {n}")).collect();
                        let edge = |v: f64| (v - f64::from(rect[0])) / f64::from(h.max(0.01));
                        Some(format!(
                            "a line of {} ({}), {which}, size {size:.2}, {:.2}..{:.2}",
                            text.chars().count(),
                            kinds.join(", "),
                            edge(r[0]),
                            edge(r[2])
                        ))
                    })
                    .take(2)
                    .collect()
            })
            .unwrap_or_default();
        if hits.is_empty() { "in no listed line".to_owned() } else { hits.join(" and ") }
    };
    format!("{kept}; edited page: {}; unedited page: {}", lines("out.pdf"), lines("in.pdf"))
}

/// The spacing the edit drew with, from the shaped runs it wrote into `out.pdf` (the content
/// streams that mark text with `/ActualText`), in numbers only: each character spacing (`Tc`) and
/// word spacing (`Tw`) set, and the widest gap a `TJ` adjustment opens between two glyphs, in
/// thousandths of the font size. Text, glyph codes and fonts are left out.
fn drawn_spacing(work: &Path) -> String {
    use pdfcraft_cos::{DecodeBudget, Document, Object};
    let Ok(bytes) = std::fs::read(work.join("out.pdf")) else { return "no edited file".to_owned() };
    let Ok(doc) = Document::open(Arc::new(bytes)) else { return "the edited file does not open".to_owned() };
    let mut budget = DecodeBudget::page();
    let (mut tc, mut tw, mut widest, mut wide) = (Vec::<String>::new(), Vec::<String>::new(), 0.0_f64, 0usize);
    for num in doc.object_numbers().into_iter().take(20_000) {
        let Ok(o) = doc.try_get(num) else { continue };
        let Object::Stream(st) = &*o else { continue };
        let Ok(data) = budget.decode(st) else { continue };
        if !data.windows(11).any(|w| w == b"/ActualText") {
            continue;
        }
        let mut operands: Vec<f64> = Vec::new();
        let (mut i, mut in_array, mut array_gaps) = (0usize, false, Vec::<f64>::new());
        while let Some(&b) = data.get(i) {
            match b {
                b'(' => {
                    // A string: skip it, with its escapes and nested brackets.
                    let mut depth = 0usize;
                    while let Some(&c) = data.get(i) {
                        match c {
                            b'\\' => i += 1,
                            b'(' => depth += 1,
                            b')' => {
                                depth = depth.saturating_sub(1);
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                    i += 1;
                }
                b'<' if data.get(i + 1) != Some(&b'<') => {
                    while data.get(i).is_some_and(|c| *c != b'>') {
                        i += 1;
                    }
                    i += 1;
                }
                b'[' => {
                    in_array = true;
                    array_gaps.clear();
                    i += 1;
                }
                b']' => {
                    in_array = false;
                    i += 1;
                }
                b'-' | b'+' | b'.' | b'0'..=b'9' => {
                    let start = i;
                    while data.get(i).is_some_and(|c| matches!(c, b'-' | b'+' | b'.' | b'0'..=b'9')) {
                        i += 1;
                    }
                    let n = std::str::from_utf8(data.get(start..i).unwrap_or_default()).ok().and_then(|t| t.parse::<f64>().ok());
                    if let Some(n) = n {
                        if in_array {
                            array_gaps.push(n);
                        } else {
                            operands.push(n);
                        }
                    }
                }
                b'a'..=b'z' | b'A'..=b'Z' | b'\'' | b'"' => {
                    let start = i;
                    while data.get(i).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'*' || *c == b'\'' || *c == b'"') {
                        i += 1;
                    }
                    let op = data.get(start..i).unwrap_or_default();
                    let last = operands.last().copied();
                    match (op, last) {
                        (b"Tc", Some(v)) if !tc.contains(&format!("{v}")) => tc.push(format!("{v}")),
                        (b"Tw", Some(v)) if !tw.contains(&format!("{v}")) => tw.push(format!("{v}")),
                        (b"TJ", _) => {
                            for g in &array_gaps {
                                // A negative adjustment moves the next glyph right: a gap.
                                if -g > widest {
                                    widest = -g;
                                }
                                if -g > 250.0 {
                                    wide += 1;
                                }
                            }
                            array_gaps.clear();
                        }
                        _ => {}
                    }
                    operands.clear();
                }
                b'/' => {
                    i += 1;
                    while data.get(i).is_some_and(|c| !c.is_ascii_whitespace() && !b"/[]()<>".contains(c)) {
                        i += 1;
                    }
                }
                _ => i += 1,
            }
        }
    }
    tc.truncate(8);
    tw.truncate(8);
    format!("drawn with Tc {}, Tw {}, widest TJ gap {widest:.0}, gaps over 250: {wide}", tc.join("/"), tw.join("/"))
}

/// One document's result.
pub struct Outcome {
    pub name: String,
    pub edits: usize,
    /// Kind of failure, what happened (shown for a synthetic document only) and where, with no
    /// text of the document (shown for every document).
    pub failures: Vec<(&'static str, String, String)>,
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
    let tail = searched(new);
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
                        if let Err((kind, detail, note)) = chain(work, *page, *number, text, mode.is_some(), with_bold, &others) {
                            let mut place = format!("page {page}, paragraph {number}, {label}, edit twice, move, widen, bold");
                            let full = format!("{place}: {detail}");
                            if !note.is_empty() {
                                place = format!("{place}: {note}");
                            }
                            outcome.failures.push((kind, full, place));
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
                                let mut place = format!("page {page}, paragraph {number}, {label}, {variant}");
                                if kind == "not found by search" {
                                    place = format!("{place}: {}", search_note(work, *page, &new));
                                }
                                outcome.failures.push((kind, format!("{place}: {detail}"), place));
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
    let (mut details, mut private) = (String::new(), String::new());
    for o in outcomes {
        if let Some(e) = &o.error {
            // A real document's error could quote it: only a synthetic one's is shown.
            let why = if public(&o.name) { e.replace('|', "/") } else { "could not be checked".to_owned() };
            md.push_str(&format!("| {} | – | – | – | error: {why} |\n", o.name));
            continue;
        }
        let mut kinds: Vec<(&str, usize)> = Vec::new();
        for (kind, _, _) in &o.failures {
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
        // A real document's failure is described in full only when it is numbers (where a
        // paragraph went, where its edge went); otherwise by where it happened alone, with no
        // text of it.
        const NUMBERS_ONLY: [&str; 2] = ["moved to the wrong place", "widening changed the text or moved it"];
        let open = public(&o.name);
        for (kind, _) in &kinds {
            let full = open || NUMBERS_ONLY.contains(kind);
            let shown = if open { SHOWN } else { SHOWN_PRIVATE };
            for (_, detail, place) in o.failures.iter().filter(|(k, _, _)| k == kind).take(shown) {
                let text = if full { detail } else { place };
                let list = if open { &mut details } else { &mut private };
                list.push_str(&format!("- {}: {kind}: {}\n", o.name, text.replace('\n', " ⏎ ")));
            }
        }
    }
    if !details.is_empty() {
        md.push_str("\nFailures in the synthetic documents:\n\n");
        md.push_str(&details);
    }
    if !private.is_empty() {
        md.push_str("\nWhere the real documents failed (no text of them):\n\n");
        md.push_str(&private);
    }
    md
}
