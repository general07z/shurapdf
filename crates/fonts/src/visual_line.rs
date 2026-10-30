//! One line of mixed-script text in drawing order, as page text and form fields draw it: the
//! Unicode bidirectional algorithm (UAX #9) orders its runs, Arabic, Hebrew and Thai are shaped
//! with their craft-fonts faces, and the rest is left for a standard font. Shared by added page
//! text (pdfcraft-edit) and form-field appearances (pdfcraft-forms).

use unicode_bidi::{Level, ParagraphBidiInfo};

use crate::{
    GlyphError, ShapedCluster, ShapedScript, first_non_win_ansi, first_undrawable, helvetica_width, literal, shape_arabic, shape_hebrew, shape_thai,
    shaped_glyph, win_ansi,
};

/// A piece of a line, in drawing order.
#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    /// Shown in the caller's standard font (in drawing order: a run inside right-to-left text is
    /// already reversed and its brackets mirrored).
    Latin(String),
    /// Shaped glyph clusters, left to right.
    Shaped(Vec<ShapedCluster>),
}

/// Why a line could not be shaped: its script, the face's error and the text it was shaping.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeFailure {
    pub script: ShapedScript,
    pub error: GlyphError,
    pub text: String,
}

/// Thai letters, vowels, tone marks and digits.
pub fn is_thai(c: char) -> bool {
    matches!(u32::from(c), 0x0E00..=0x0E7F)
}

/// Arabic letters, marks, digits and punctuation: text with any of them is shaped with the
/// craft-fonts Arabic face.
pub fn is_arabic(c: char) -> bool {
    matches!(u32::from(c), 0x0600..=0x06FF | 0x0750..=0x077F | 0x0870..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFC)
}

/// Hebrew letters, points, cantillation marks and punctuation (and the presentation forms): text
/// with any of them is drawn with the craft-fonts Hebrew face.
pub fn is_hebrew(c: char) -> bool {
    matches!(u32::from(c), 0x0591..=0x05F4 | 0xFB1D..=0xFB4F)
}

/// Direction marks, embeddings, overrides and isolates: they steer the order and are not drawn.
pub fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// The mirrored form of a bracket drawn in right-to-left text.
pub fn mirrored(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        other => other,
    }
}

/// Whether a paragraph has right-to-left script and its first strong character is right to left
/// (its base direction).
pub fn paragraph_rtl(text: &str) -> bool {
    text.chars().any(|c| is_arabic(c) || is_hebrew(c)) && unicode_bidi::get_base_direction(text) == unicode_bidi::Direction::Rtl
}

/// `line` in drawing order with paragraph direction `rtl`: Arabic and Hebrew letters (with the
/// spaces and punctuation between them in a right-to-left run) and Thai are shaped; the rest is
/// left as [`Piece::Latin`].
pub fn visual_pieces(line: &str, rtl: bool) -> Result<Vec<Piece>, ShapeFailure> {
    // unicode-bidi indexes the first level of a line, so an empty one would panic.
    if line.is_empty() {
        return Ok(Vec::new());
    }
    let base = if rtl { Level::rtl() } else { Level::ltr() };
    let info = ParagraphBidiInfo::new(line, Some(base));
    let (levels, runs) = info.visual_runs(0..line.len());
    let mut pieces = Vec::new();
    for run in runs {
        let run_rtl = levels.get(run.start).is_some_and(Level::is_rtl);
        // Runs lie on character boundaries; a missing slice is skipped, never a panic.
        let Some(text) = line.get(run) else { continue };
        // Arabic and Hebrew letters take their face; so do spaces and punctuation between them
        // (Hebrew's when they follow Hebrew), which the shaper then mirrors. Latin text (and what
        // the faces lack) keeps the caller's font.
        let mut segments: Vec<(Option<ShapedScript>, String)> = Vec::new();
        for c in text.chars().filter(|c| !is_bidi_control(*c)) {
            let after_hebrew = segments.last().is_some_and(|(kind, _)| *kind == Some(ShapedScript::Hebrew));
            let script = if is_thai(c) {
                Some(ShapedScript::Thai)
            } else if is_hebrew(c) || (run_rtl && after_hebrew && !c.is_alphanumeric() && crate::hebrew_has(c)) {
                Some(ShapedScript::Hebrew)
            } else if is_arabic(c) || (run_rtl && !c.is_alphanumeric() && crate::arabic_has(c)) {
                Some(ShapedScript::Arabic)
            } else {
                None
            };
            match segments.last_mut() {
                Some((kind, s)) if *kind == script => s.push(c),
                _ => segments.push((script, c.to_string())),
            }
        }
        if run_rtl {
            segments.reverse();
        }
        for (script, s) in segments {
            pieces.push(if let Some(script) = script {
                let shaped = match script {
                    ShapedScript::Arabic => shape_arabic(&s, run_rtl),
                    ShapedScript::Hebrew => shape_hebrew(&s, run_rtl),
                    ShapedScript::Thai => shape_thai(&s),
                };
                Piece::Shaped(shaped.map_err(|error| ShapeFailure { script, error, text: s })?)
            } else if run_rtl {
                Piece::Latin(s.chars().rev().map(mirrored).collect())
            } else {
                Piece::Latin(s)
            });
        }
    }
    Ok(pieces)
}

/// Longest text drawn with [`Shaper`] (characters); longer text keeps the plain path.
pub const MAX_SHAPED_CHARS: usize = 4096;

/// Whether `text` has a script that is shaped rather than drawn with a standard font.
pub fn wants(text: &str) -> bool {
    text.chars().any(|c| is_arabic(c) || is_hebrew(c) || is_thai(c))
}

/// How an appearance's text in Arabic, Hebrew or Thai is drawn: each line in drawing order, the
/// shaped clusters as filled glyph outlines (an appearance stream adds no font objects of its
/// own), Latin pieces in the caller's standard font (Helvetica metrics, WinAnsi), and the line
/// wrapped in `/ActualText` so copying or searching gives the text as typed. Used by form-field
/// and comment appearances.
#[derive(Clone, Copy, Debug)]
pub struct Shaper {
    rtl: bool,
}

impl Shaper {
    /// A shaper for `text` when every line of it can be shaped (the faces are in this build and hold
    /// its letters); `None` otherwise, and the field keeps its plain drawing.
    pub fn for_text(text: &str) -> Option<Self> {
        if !wants(text) || text.chars().count() > MAX_SHAPED_CHARS {
            return None;
        }
        let shaper = Self { rtl: paragraph_rtl(text) };
        text.split(['\n', '\r']).all(|line| shaper.pieces(line).is_some()).then_some(shaper)
    }

    /// `line` in drawing order. Every line takes the field's paragraph direction (its first strong
    /// character's), as a wrapped paragraph's lines do.
    fn pieces(self, line: &str) -> Option<Vec<Piece>> {
        visual_pieces(line, self.rtl).ok()
    }

    /// The width of `line` at `size`: shaped clusters by their advances, the rest in Helvetica.
    pub fn width(self, line: &str, size: f64) -> f64 {
        match self.pieces(line) {
            Some(pieces) => pieces
                .iter()
                .map(|p| match p {
                    Piece::Latin(s) => helvetica_width(s, size),
                    Piece::Shaped(clusters) => clusters.iter().map(|c| c.advance * size).sum(),
                })
                .sum(),
            None => helvetica_width(line, size),
        }
    }

    /// The first character of `line` drawn as `?`: Latin pieces outside WinAnsi.
    pub fn first_unencodable(self, line: &str) -> Option<char> {
        self.pieces(line)?.iter().find_map(|p| match p {
            Piece::Latin(s) => first_non_win_ansi(s),
            Piece::Shaped(_) => None,
        })
    }

    /// Draw `line` with its left end at `[x, baseline y]`, `size` points: appended to `out`,
    /// which is inside a text object using `font` at `size`. The shaped glyphs are paths, so the
    /// text object is closed around them and reopened after; the fill colour (the `/DA` colour)
    /// carries over.
    pub fn draw(self, out: &mut Vec<u8>, line: &str, [x, y]: [f64; 2], size: f64, font: &str, color: &str) {
        let Some(pieces) = self.pieces(line) else { return };
        let logical: String = line.encode_utf16().map(|u| format!("{u:04X}")).collect();
        out.extend(format!("ET\n/Span <</ActualText <FEFF{logical}>>> BDC\n").bytes());
        let mut pen = x;
        for piece in &pieces {
            match piece {
                Piece::Latin(s) => {
                    out.extend(format!("BT\n/{font} {} Tf\n{color}\n1 0 0 1 {} {} Tm ", n(size), n(pen), n(y)).bytes());
                    out.extend(literal(&win_ansi(s)));
                    out.extend_from_slice(b" Tj\nET\n");
                    pen += helvetica_width(s, size);
                }
                Piece::Shaped(clusters) => {
                    for c in clusters {
                        for (id, [dx, dy]) in &c.glyphs {
                            let Ok(g) = shaped_glyph(c.script, *id) else { continue };
                            let mut path = String::new();
                            for contour in &g.contours {
                                for (j, p) in contour.iter().enumerate() {
                                    let (gx, gy) = (pen + (p[0] + dx) * size, y + (p[1] + dy) * size);
                                    path.push_str(&format!("{} {} {}\n", n(gx), n(gy), if j == 0 { "m" } else { "l" }));
                                }
                                if !contour.is_empty() {
                                    path.push_str("h\n");
                                }
                            }
                            if !path.is_empty() {
                                out.extend(path.bytes());
                                out.extend_from_slice(b"f\n");
                            }
                        }
                        pen += c.advance * size;
                    }
                }
            }
        }
        out.extend(format!("EMC\nBT\n/{font} {} Tf\n{color}\n", n(size)).bytes());
    }
}

/// A number for a content stream: three decimals at most, no trailing zeros.
fn n(v: f64) -> String {
    let s = format!("{:.3}", if v.is_finite() && v.abs() >= 5e-4 { v } else { 0.0 });
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".to_owned() } else { s.to_owned() }
}

/// [`first_undrawable`] for text an appearance draws through [`Shaper`] when it can: the first
/// character that would still come out as `?`.
pub fn first_undrawable_shaped(text: &str) -> Option<char> {
    match Shaper::for_text(text) {
        Some(shaper) => text.split(['\n', '\r']).find_map(|line| shaper.first_unencodable(line)),
        None => first_undrawable(text),
    }
}
