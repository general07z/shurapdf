use super::{Arc, BTreeMap, Color32, FontFamily, FontId, LineEditor, Rect};

/// The look shown for a paragraph: the family and weight guessed from its PDF font name.
fn source_look(base_font: &str, size: f64, color: [f64; 3], detected_bold: bool, detected_italic: bool) -> pdfcraft_engine::AddedText {
    use pdfcraft_engine::FontFamily as F;
    let name = base_font.to_ascii_lowercase();
    let family = if ["courier", "mono", "consolas", "menlo", "monaco", "lucida console"].iter().any(|s| name.contains(s)) {
        F::Courier
    } else if !name.contains("sans") && ["times", "serif", "roman", "cambria", "georgia", "palatino", "garamond"].iter().any(|s| name.contains(s)) {
        F::Times
    } else {
        F::Helvetica
    };
    pdfcraft_engine::AddedText {
        family,
        bold: detected_bold || ["bold", "black", "heavy", "semibold", "demi"].iter().any(|s| name.contains(s)),
        italic: detected_italic || ["italic", "oblique", "slanted"].iter().any(|s| name.contains(s)),
        size: (size * 10.0).round() / 10.0,
        color,
        ..Default::default()
    }
}

pub(super) fn look_of(b: &pdfcraft_engine::TextBlock) -> pdfcraft_engine::AddedText {
    source_look(&b.base_font, b.size, b.color, b.bold, b.italic)
}

pub(crate) fn editor_font(look: &pdfcraft_engine::AddedText, size: f32) -> FontId {
    let family = match (look.family, look.bold) {
        (pdfcraft_engine::FontFamily::Courier, _) => FontFamily::Monospace,
        (_, true) => FontFamily::Name("semibold".into()),
        (_, false) => FontFamily::Proportional,
    };
    FontId::new(size, family)
}

pub(super) fn color32(color: [f64; 3]) -> Color32 {
    Color32::from_rgb(
        (color[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (color[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (color[2].clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

/// The dark box behind text too pale to read on white.
pub(super) const DARK_EDITOR_FILL: Color32 = Color32::from_gray(0x26);

/// WCAG relative luminance of an sRGB colour.
pub(super) fn luminance(c: Color32) -> f32 {
    let channel = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
}

/// WCAG contrast ratio between two colours (1 to 21).
pub(super) fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// The inline editor's background for text drawn in `text`: white, or a dark box when the text
/// reads better on that (white or pale text, which would vanish on white).
pub(crate) fn editor_fill(text: Color32) -> Color32 {
    if contrast(text, Color32::WHITE) >= contrast(text, DARK_EDITOR_FILL) { Color32::WHITE } else { DARK_EDITOR_FILL }
}

/// One TextEdit lays out all source runs; its caret and selection span the whole region.
/// The interface families the paragraph editor draws a page's text with, by PDF font resource:
/// `fonts` for the text as it is, `joining` for Arabic. Word's Save as PDF embeds its fonts
/// without the tables that join Arabic letters (it draws the joined forms itself), so Arabic typed
/// or shown in such a program falls apart into separate letters. For Arabic the editor uses the
/// same face installed on this computer (Arial for `ArialMT`), else the interface's Arabic font.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SourceFonts {
    pub(crate) fonts: BTreeMap<String, String>,
    pub(crate) joining: BTreeMap<String, String>,
    /// The installed copy of a resource's face, when it can be embedded: a retyped Arabic
    /// paragraph is drawn with it (`BlockStyle::face`) and keeps the document's font.
    pub(crate) faces: BTreeMap<String, Arc<Vec<u8>>>,
    /// The installed bold face of each paragraph font that has one (Arial → Arial Bold): Bold
    /// draws the paragraph in it, as Word's own bold does, where a standard bold has no Arabic.
    pub(crate) bold_faces: BTreeMap<String, Arc<Vec<u8>>>,
}

impl SourceFonts {
    /// Every interface family these fonts name.
    pub(crate) fn families(&self) -> impl Iterator<Item = &String> {
        self.fonts.values().chain(self.joining.values())
    }
}

/// Whether a font program can join Arabic letters itself (it has glyph substitutions).
fn joins_arabic(bytes: &[u8]) -> bool {
    skrifa::FontRef::new(bytes).ok().and_then(|f| f.table_data(skrifa::raw::types::Tag::new(b"GSUB"))).is_some()
}

/// The keys to look a PDF font name up by in the installed fonts: the name itself, then (Word
/// names a synthetic bold or italic `TraditionalArabic,Bold`) the name without its style or
/// `MT` ending, which is the installed regular face.
fn style_names(name: &str) -> impl Iterator<Item = String> {
    let key = pdfcraft_fonts::embedding::face_key(name);
    let base = family_key(name);
    let mut keys = vec![key.clone()];
    // `Tajawal` is `Tajawal-Regular` installed.
    keys.push(format!("{key}regular"));
    if base != key {
        keys.push(base);
    }
    keys.into_iter()
}

/// The font name's key without any style or `MT` ending: `Arial-BoldMT` → `arial`.
fn family_key(name: &str) -> String {
    let key = pdfcraft_fonts::embedding::face_key(name);
    let mut base = key.as_str();
    while let Some(shorter) =
        ["bolditalic", "italic", "bold", "oblique", "regular", "mt"].iter().find_map(|suffix| base.strip_suffix(suffix).filter(|b| !b.is_empty()))
    {
        base = shorter;
    }
    base.to_owned()
}

/// The installed bold face of the font `name` (`ArialMT` → `Arial-BoldMT`, `Tajawal-Regular` →
/// `Tajawal-Bold`), read whole.
#[cfg(not(target_arch = "wasm32"))]
fn installed_bold_face(ctx: &egui::Context, name: &str) -> Option<Vec<u8>> {
    use std::io::Read;
    let files = crate::files::installed_font_files(ctx)?;
    let key = |f: &pdfcraft_engine::text_fonts::FontFile| pdfcraft_fonts::embedding::face_key(&f.face);
    // The family part of the name, without a style or `MT` ending.
    let base = family_key(name);
    let exact = [format!("{base}bold"), format!("{base}boldmt"), format!("{base}bd")];
    let regular = style_names(name).find_map(|k| files.iter().find(|f| key(f) == k));
    // The file named for it, else the family's plain Bold (not Semi/Extra/Black, Italic, …).
    let plain_bold = |f: &&pdfcraft_engine::text_fonts::FontFile| {
        let k = key(f);
        k.contains("bold")
            && !["semi", "demi", "extra", "ultra", "black", "heavy", "italic", "oblique", "light", "condensed", "narrow"]
                .iter()
                .any(|w| k.contains(w))
    };
    let file = files
        .iter()
        .find(|f| exact.contains(&key(f)))
        .or_else(|| regular.and_then(|r| files.iter().filter(|f| f.family == r.family).find(plain_bold)))
        .or_else(|| files.iter().filter(|f| key(f).starts_with(&base)).find(plain_bold))?;
    let mut bytes = Vec::new();
    std::fs::File::open(&file.path).ok()?.take(16 << 20).read_to_end(&mut bytes).ok()?;
    pdfcraft_fonts::embedding::validate_document_input(&bytes).is_ok().then_some(bytes)
}

/// The installed font file of the face `name` (a PDF font name, subset tag and all), read whole,
/// when the installed-font scan has finished and found it.
fn installed_face(ctx: &egui::Context, name: &str) -> Option<Vec<u8>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Read;
        let files = crate::files::installed_font_files(ctx)?;
        let file = style_names(name).find_map(|key| files.iter().find(|f| pdfcraft_fonts::embedding::face_key(&f.face) == key))?;
        let mut bytes = Vec::new();
        std::fs::File::open(&file.path).ok()?.take(16 << 20).read_to_end(&mut bytes).ok()?;
        joins_arabic(&bytes).then_some(bytes)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (ctx, name);
        None
    }
}

pub(crate) fn install_source_fonts(ctx: &egui::Context, doc: &pdfcraft_engine::Document, page: usize) -> SourceFonts {
    let programs = doc.text_editor_fonts(page);
    let mut definitions = ctx.fonts(|f| f.definitions().clone());
    // Only the current editor's document fonts live in the atlas. Never accumulate the
    // programs of previously opened files or add them to the interface's default families.
    definitions.font_data.retain(|name, _| !name.starts_with("pdf-source-"));
    definitions.families.retain(|family, _| !matches!(family, FontFamily::Name(name) if name.starts_with("pdf-source-")));
    let interface = definitions.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut out = SourceFonts::default();
    for (index, program) in programs.iter().enumerate() {
        let name = format!("pdf-source-{index}");
        definitions.font_data.insert(name.clone(), Arc::new(egui::FontData::from_owned(program.bytes.clone())));
        out.fonts.insert(program.resource.clone(), name);
    }
    for (index, program) in programs.iter().enumerate() {
        let Some(name) = out.fonts.get(&program.resource) else { continue };
        // Word-created PDFs often split one face into several subsets. Use all of the
        // document's matching subsets before falling back for newly inserted characters.
        let mut stack = vec![name.clone()];
        stack.extend(
            programs
                .iter()
                .filter(|p| {
                    pdfcraft_fonts::embedding::face_key(&p.face) == pdfcraft_fonts::embedding::face_key(&program.face)
                        && p.resource != program.resource
                })
                .filter_map(|p| out.fonts.get(&p.resource).cloned()),
        );
        stack.extend(interface.iter().cloned());
        definitions.families.insert(FontFamily::Name(name.clone().into()), stack.clone());
        // Arabic: the program itself when it joins letters, else the installed face, else the
        // interface's own (which does).
        // A complete face (an earlier edit embedded it) draws; a Word subset never does, even if it
        // has joining rules: it holds only the glyphs the page used, and a letter it lacks would
        // show as an empty box in the typing box.
        let joining = if joins_arabic(&program.bytes) && program.resource.starts_with("PCFull") {
            stack
        } else if let Some(bytes) = installed_face(ctx, &program.face) {
            let installed = format!("pdf-source-{index}-installed");
            if pdfcraft_fonts::embedding::validate_document_input(&bytes).is_ok() {
                out.faces.insert(program.resource.clone(), Arc::new(bytes.clone()));
            }
            definitions.font_data.insert(installed.clone(), Arc::new(egui::FontData::from_owned(bytes)));
            std::iter::once(installed).chain(interface.iter().cloned()).collect()
        } else {
            interface.clone()
        };
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(bold) = installed_bold_face(ctx, &program.face) {
            out.bold_faces.insert(program.resource.clone(), Arc::new(bold));
        }
        let joining_name = format!("pdf-source-{index}-joining");
        definitions.families.insert(FontFamily::Name(joining_name.clone().into()), joining);
        out.joining.insert(program.resource.clone(), joining_name);
    }
    if !out.fonts.is_empty() {
        ctx.set_fonts(definitions);
    }
    out
}

pub(super) fn editor_job(ui: &egui::Ui, ed: &LineEditor, text: &str, scale: f32, width: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    // An unchanged PDF row already has an authoritative line break. Do not rewrap it
    // because font rounding or a fallback face measures a fraction of a pixel wider.
    job.wrap.max_width = if text == ed.original && ed.look == ed.look0 && ed.extras == ed.extras0 { f32::INFINITY } else { width };
    if let Some((block, lines)) = &ed.source {
        let lefts: Vec<_> = block.rows.iter().filter_map(|r| r.first().and_then(|i| lines.get(*i)).map(|l| l.rect[0])).collect();
        let left_span = lefts.iter().copied().fold(f64::NEG_INFINITY, f64::max) - lefts.iter().copied().fold(f64::INFINITY, f64::min);
        let centered = block.rows.len() > 1
            && left_span > block.size * 0.5
            && block.rows.iter().all(|row| {
                let rect = row
                    .iter()
                    .filter_map(|i| lines.get(*i))
                    .map(|l| l.rect)
                    .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]);
                rect.is_some_and(|r| ((r[0] + r[2] - block.rect[0] - block.rect[2]) / 2.0).abs() < block.size)
            });
        if centered {
            job.halign = egui::Align::Center;
        }
        for (range, index) in block.edited_spans(text) {
            let Some(line) = lines.get(index) else { continue };
            let mut look = source_look(&line.base_font, line.size, line.color, line.bold, line.italic);
            look.size = line.size;
            if ed.look.family != ed.look0.family || ed.family_chosen {
                look.family = ed.look.family;
            }
            if ed.look.bold != ed.look0.bold {
                look.bold = ed.look.bold;
            }
            if ed.look.italic != ed.look0.italic {
                look.italic = ed.look.italic;
            }
            if ed.look.color != ed.look0.color {
                look.color = ed.look.color;
            }
            if ed.look.size != ed.look0.size {
                look.size = ed.look.size;
            }
            let mut font = editor_font(&look, (look.size as f32 * scale).clamp(1.0, 300.0));
            let original_face =
                ed.look.family == ed.look0.family && !ed.family_chosen && ed.look.bold == ed.look0.bold && ed.look.italic == ed.look0.italic;
            let piece = text.get(range.clone()).unwrap_or_default();
            // Arabic is drawn joined, in a face that can join it (see [`SourceFonts`]), and never
            // letter-spaced: a gap between joined letters breaks the word apart.
            let arabic = piece.chars().any(pdfcraft_fonts::visual_line::is_arabic) || line.text.chars().any(pdfcraft_fonts::visual_line::is_arabic);
            let source = if arabic { ed.fonts.joining.get(&line.font) } else { ed.fonts.fonts.get(&line.font) };
            let embedded = original_face && source.is_some();
            if embedded && let Some(name) = source {
                font.family = FontFamily::Name(name.clone().into());
            }
            let spacing = if original_face && !arabic {
                let native_width = ui.fonts_mut(|f| f.layout_no_wrap(line.text.clone(), font.clone(), Color32::BLACK).size().x);
                let target_width = ((line.rect[2] - line.rect[0]) * look.size / line.size.max(0.01)) as f32 * scale;
                (target_width - native_width) / line.text.chars().count().saturating_sub(1).max(1) as f32
            } else {
                0.0
            };
            let format = egui::TextFormat {
                font_id: font,
                extra_letter_spacing: spacing + if arabic { 0.0 } else { ed.extras.char_spacing as f32 * scale },
                color: color32(look.color),
                italics: look.italic && !embedded,
                ..Default::default()
            };
            append_joined(&mut job, piece, format);
        }
    } else {
        job.append(
            text,
            0.0,
            egui::TextFormat {
                font_id: editor_font(&ed.look, ed.size),
                color: color32(ed.look.color),
                italics: ed.look.italic,
                ..Default::default()
            },
        );
    }
    job
}

/// Append `text` to `job`, extending the last section when it has the same format: a word split
/// across the PDF's text pieces is then one piece of text to egui, which joins Arabic letters only
/// within a section (Word draws a line as many small pieces).
pub(super) fn append_joined(job: &mut egui::text::LayoutJob, text: &str, format: egui::TextFormat) {
    if let Some(last) = job.sections.last_mut()
        && last.format == format
        && last.byte_range.end.0 == job.text.len()
    {
        job.text.push_str(text);
        last.byte_range.end = egui::text::ByteIndex(job.text.len());
    } else {
        job.append(text, 0.0, format);
    }
}

/// `galley` drawn in no colour: its rows still place the caret and the selection.
pub(super) fn unseen(mut galley: Arc<egui::Galley>) -> Arc<egui::Galley> {
    for placed in &mut Arc::make_mut(&mut galley).rows {
        for vertex in &mut Arc::make_mut(&mut placed.row).visuals.mesh.vertices {
            vertex.color = Color32::TRANSPARENT;
        }
    }
    galley
}

/// Align the editable galley's baselines to the source rows instead of egui's interface
/// leading. Glyphs, meshes, caret and selection receive the same transform.
pub(super) fn editor_galley(ui: &egui::Ui, ed: &LineEditor, text: &str, scale: f32, width: f32) -> Arc<egui::Galley> {
    let mut job = editor_job(ui, ed, text, scale, width);
    let centered = job.halign == egui::Align::Center;
    job.halign = egui::Align::LEFT;
    let mut galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let Some((block, lines)) = &ed.source else { return galley };
    let sources: Vec<_> = block
        .rows
        .iter()
        .filter_map(|row| {
            let first = row.first().and_then(|i| lines.get(*i))?;
            let rect = row
                .iter()
                .filter_map(|i| lines.get(*i))
                .map(|l| l.rect)
                .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])?;
            Some((first.baseline()[1], rect))
        })
        .collect();
    let Some((first_baseline, first_rect)) = sources.first().copied() else { return galley };
    let leading = if ed.extras.line_spacing > 0.0 {
        ed.extras.line_spacing * ed.look.size
    } else {
        let fallback = if block.lines.len() == 1 && !text.contains('\n') {
            block.lines.first().and_then(|i| lines.get(*i)).map_or(block.size * 1.2, |line| line.wrap_leading())
        } else {
            block.size * 1.5
        };
        sources.windows(2).map(|r| (r[0].0 - r[1].0).abs()).rfind(|v| *v > 0.0).unwrap_or(fallback)
    };
    let g = Arc::make_mut(&mut galley);
    let mut bounds = Rect::NOTHING;
    let mut meshes = Rect::NOTHING;
    for (index, placed) in g.rows.iter_mut().enumerate() {
        let (last_baseline, last_rect) = sources.last().copied().unwrap_or((first_baseline, first_rect));
        let (baseline, source_rect) = sources
            .get(index)
            .copied()
            .unwrap_or((last_baseline - index.saturating_add(1).saturating_sub(sources.len()) as f64 * leading, last_rect));
        let changed_size = ed.look.size != ed.look0.size;
        let ratio = if changed_size { ed.look.size / ed.look0.size.max(0.01) } else { 1.0 };
        let ascent = (source_rect[3] - sources.get(index).map_or(last_baseline, |s| s.0)) as f32 * scale * ratio as f32;
        let descent = (sources.get(index).map_or(last_baseline, |s| s.0) - source_rect[1]) as f32 * scale * ratio as f32;
        let base_y = (block.rect[3] - baseline) as f32 * scale;
        let row = Arc::make_mut(&mut placed.row);
        placed.pos.y = base_y - ascent;
        placed.pos.x = if centered { (width - row.size.x) / 2.0 } else { (source_rect[0] - block.rect[0]) as f32 * scale };
        for glyph in &mut row.glyphs {
            let dy = ascent - glyph.pos.y;
            glyph.pos.y = ascent;
            glyph.font_ascent = ascent;
            glyph.font_height = ascent + descent;
            glyph.line_height = ascent + descent;
            if !glyph.uv_rect.is_nothing() {
                for vertex in row.visuals.mesh.vertices.iter_mut().skip(glyph.first_vertex as usize).take(4) {
                    vertex.pos.y += dy;
                }
            }
        }
        row.size.y = (ascent + descent).max(1.0);
        row.visuals.mesh_bounds = row.visuals.mesh.vertices.iter().fold(Rect::NOTHING, |r, v| r.union(Rect::from_min_max(v.pos, v.pos)));
        bounds = bounds.union(Rect::from_min_size(placed.pos, row.size));
        meshes = meshes.union(row.visuals.mesh_bounds.translate(placed.pos.to_vec2()));
    }
    g.rect = Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(bounds.right().max(0.0), bounds.bottom().max(0.0)));
    g.mesh_bounds = meshes;
    galley
}

/// Caret colour that stays visible on a field filled with `fill`: black on light fills, white on dark ones.
pub(crate) fn caret_color_on(fill: Color32) -> Color32 {
    let luma = 299 * u32::from(fill.r()) + 587 * u32::from(fill.g()) + 114 * u32::from(fill.b());
    if luma > 127_500 { Color32::BLACK } else { Color32::WHITE }
}

#[cfg(test)]
mod tests {
    use super::{append_joined, joins_arabic};

    /// Word draws an Arabic line as many small pieces; pieces in one format become one section,
    /// so egui joins letters across them. A different format still starts its own.
    #[test]
    fn pieces_in_one_format_are_one_section() {
        let mut job = egui::text::LayoutJob::default();
        let plain = egui::TextFormat::default();
        for piece in ["المو", "ضوع", ": "] {
            append_joined(&mut job, piece, plain.clone());
        }
        let bold = egui::TextFormat { color: egui::Color32::RED, ..plain };
        append_joined(&mut job, "طلب", bold);
        assert_eq!(job.text, "الموضوع: طلب");
        assert_eq!(job.sections.len(), 2);
        assert_eq!(job.sections[0].byte_range.end.0, "الموضوع: ".len());
    }

    /// A program with glyph substitutions joins Arabic itself; Word's embedded subsets have none.
    #[test]
    fn a_program_without_substitutions_cannot_join_arabic() {
        let full = include_bytes!("../../../../../assets/fonts/Inter-Regular.ttf").to_vec();
        assert!(joins_arabic(&full));
        let mut subset = full.clone();
        let tables = usize::from(u16::from_be_bytes([subset[4], subset[5]]));
        let gsub = (0..tables).map(|t| 12 + 16 * t).find(|&r| &subset[r..r + 4] == b"GSUB").unwrap();
        subset[gsub..gsub + 4].copy_from_slice(b"XSUB");
        assert!(!joins_arabic(&subset));
        assert!(!joins_arabic(b"not a font"));
    }
}

#[cfg(test)]
mod name_tests {
    use super::*;

    #[test]
    fn a_pdf_font_name_finds_its_installed_family_and_styles() {
        assert_eq!(family_key("ABCDEF+Arial-BoldMT"), "arial");
        assert_eq!(family_key("Tajawal-Regular"), "tajawal");
        assert_eq!(family_key("TraditionalArabic,Bold"), "traditionalarabic");
        assert_eq!(family_key("Bold"), "bold", "a name that is only a style is kept");
        let keys: Vec<String> = style_names("ArialMT").collect();
        assert_eq!(keys, ["arialmt", "arialmtregular", "arial"]);
        // Word's `Tajawal` (no style) is the installed `Tajawal-Regular`.
        assert!(style_names("Tajawal").any(|k| k == "tajawalregular"));
    }
}
