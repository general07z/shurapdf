//! The diagnostic log: what the user does and what the app does in reply, written to the app's
//! log file (`<data dir>/logs/pdfcraft.log`, see `apps/pdfcraft/src/logging.rs`) so a tester can
//! send one file that shows exactly where something went wrong.
//!
//! Off unless chosen in Preferences ▸ General (`diagnostic_log`), and kept in the settings:
//! - [`Level::Numbers`] records commands, presses, edits, opens, saves, notices and timings, with
//!   a document's text and file names replaced by their length and kinds of character;
//! - [`Level::Text`] records the same with the text itself (a paragraph before and after an edit,
//!   what was typed, file names), for a tester who may share it.
//!
//! Never recorded at any level: passwords, certificate data, the control-channel token, image or
//! font bytes. Records go through `log` with the `pdfcraft_diag` target, which the default filter
//! (`pdfcraft*=info`) lets through; a record is only formatted when the log is on.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicU8, Ordering};

/// How much the diagnostic log records.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Level {
    /// Nothing.
    #[default]
    Off,
    /// Everything but a document's text and file names, which become counts by kind.
    Numbers,
    /// Everything, with a document's text and file names.
    Text,
}

impl Level {
    /// The settings and `set_option` name.
    pub fn name(self) -> &'static str {
        match self {
            Level::Off => "off",
            Level::Numbers => "numbers",
            Level::Text => "text",
        }
    }

    pub fn parse(name: &str) -> Option<Level> {
        match name {
            "off" => Some(Level::Off),
            "numbers" => Some(Level::Numbers),
            "text" => Some(Level::Text),
            _ => None,
        }
    }

    fn from_u8(v: u8) -> Level {
        match v {
            1 => Level::Numbers,
            2 => Level::Text,
            _ => Level::Off,
        }
    }

    fn as_u8(self) -> u8 {
        match self {
            Level::Off => 0,
            Level::Numbers => 1,
            Level::Text => 2,
        }
    }
}

static LEVEL: AtomicU8 = AtomicU8::new(0);

/// The level in force.
pub fn level() -> Level {
    Level::from_u8(LEVEL.load(Ordering::Relaxed))
}

/// Change the level; a change is itself recorded.
pub fn set(level: Level) {
    let before = Level::from_u8(LEVEL.swap(level.as_u8(), Ordering::Relaxed));
    if before != level {
        log::info!(
            target: "pdfcraft_diag",
            "diagnostic log {} (was {}); ShuraPDF {}, {} {}",
            level.name(),
            before.name(),
            pdfcraft_engine::build_info::version(),
            std::env::consts::OS,
            std::env::consts::ARCH
        );
    }
}

/// Whether anything is recorded.
pub fn on() -> bool {
    level() != Level::Off
}

/// Longest text written for one value, in characters.
const MAX_TEXT: usize = 4000;
/// Longest debug form written for one value, in bytes.
const MAX_DEBUG: usize = 1500;

/// A document's text as the level allows: quoted, with line breaks shown, at [`Level::Text`];
/// otherwise its length and the kinds of character it holds, in order of first appearance.
pub fn text(s: &str) -> String {
    if level() == Level::Text {
        let mut out = String::from("“");
        for (i, c) in s.chars().enumerate() {
            if i >= MAX_TEXT {
                out.push('…');
                break;
            }
            match c {
                '\n' => out.push('⏎'),
                '\r' => {}
                c if c.is_control() || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{061C}') => {
                    let _ = write!(out, "<U+{:04X}>", u32::from(c));
                }
                c => out.push(c),
            }
        }
        out.push('”');
        out
    } else {
        kinds(s)
    }
}

/// `s`'s length and the count of each kind of character in it (no character itself).
pub fn kinds(s: &str) -> String {
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for c in s.chars() {
        let k = kind(c);
        match counts.iter_mut().find(|e| e.0 == k) {
            Some(e) => e.1 += 1,
            None => counts.push((k, 1)),
        }
    }
    let parts: Vec<String> = counts.iter().map(|(k, n)| format!("{k} {n}")).collect();
    format!("[{} chars: {}]", s.chars().count(), parts.join(", "))
}

fn kind(c: char) -> &'static str {
    match c {
        ' ' => "space",
        '\n' | '\r' => "line break",
        '0'..='9' => "digit",
        '\u{0660}'..='\u{0669}' | '\u{06F0}'..='\u{06F9}' => "Arabic-Indic digit",
        '\u{064B}'..='\u{065F}' | '\u{0670}' => "Arabic mark",
        '\u{0600}'..='\u{06FF}' | '\u{0750}'..='\u{077F}' | '\u{FB50}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}' if c.is_alphabetic() => "Arabic",
        c if c.is_alphabetic() && c.is_ascii() => "Latin",
        c if c.is_alphabetic() => "other letter",
        c if c.is_whitespace() => "other space",
        c if c.is_control() || matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{061C}') => "control",
        _ => "punctuation",
    }
}

/// A file name or path as the level allows: itself at [`Level::Text`], else its extension.
pub fn name(s: &str) -> String {
    if level() == Level::Text {
        format!("“{s}”")
    } else {
        let ext = std::path::Path::new(s).extension().and_then(|e| e.to_str()).unwrap_or("");
        format!("[a .{ext} file]")
    }
}

/// `value`'s debug form, cut at [`MAX_DEBUG`] bytes: formatting stops there, so a value holding
/// a font's or an image's bytes costs no more than its first part.
pub fn debug(value: &impl std::fmt::Debug) -> String {
    struct Bounded(String);
    impl std::fmt::Write for Bounded {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            let room = MAX_DEBUG.saturating_sub(self.0.len());
            if s.len() <= room {
                self.0.push_str(s);
                Ok(())
            } else {
                let mut end = room;
                while end > 0 && !s.is_char_boundary(end) {
                    end -= 1;
                }
                self.0.push_str(s.get(..end).unwrap_or(""));
                Err(std::fmt::Error)
            }
        }
    }
    let mut out = Bounded(String::new());
    if write!(out, "{value:?}").is_err() {
        out.0.push('…');
    }
    out.0
}

/// The name of an enum value's variant from its debug form (`EditTextBlock { .. }` →
/// `EditTextBlock`).
pub fn variant(value: &impl std::fmt::Debug) -> String {
    debug(value).chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect()
}

/// The app's log file, where these records go, when it exists: in the data folder the desktop app
/// opens it in (`apps/pdfcraft/src/main.rs`).
#[cfg(not(target_arch = "wasm32"))]
pub fn log_file() -> Option<std::path::PathBuf> {
    let dir = crate::portable::data_dir().map(std::path::Path::to_path_buf).or_else(|| eframe::storage_dir("PdfCraft"))?;
    let file = dir.join("logs").join("pdfcraft.log");
    file.is_file().then_some(file)
}

/// Record `args` when the log is on. Callers check [`on`] first when building the record costs
/// anything.
pub fn record(args: std::fmt::Arguments<'_>) {
    if on() {
        log::info!(target: "pdfcraft_diag", "{args}");
    }
}

/// Record a formatted line when the diagnostic log is on; the arguments are not evaluated when it
/// is off.
macro_rules! diag {
    ($($arg:tt)*) => {
        if $crate::diag::on() {
            $crate::diag::record(format_args!($($arg)*));
        }
    };
}
pub(crate) use diag;

#[cfg(test)]
mod tests {
    use super::*;

    /// Collects the diagnostic records this test's thread logs.
    struct Collect;
    static RECORDS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    impl log::Log for Collect {
        fn enabled(&self, m: &log::Metadata<'_>) -> bool {
            m.target() == "pdfcraft_diag"
        }
        fn log(&self, r: &log::Record<'_>) {
            if self.enabled(r.metadata()) {
                RECORDS.lock().unwrap().push(r.args().to_string());
            }
        }
        fn flush(&self) {}
    }

    fn records() -> Vec<String> {
        std::mem::take(&mut *RECORDS.lock().unwrap())
    }

    /// One test owns the global level, so tests running in parallel don't race on it.
    #[test]
    fn the_level_decides_what_text_and_names_show() {
        let _ = log::set_logger(&Collect);
        log::set_max_level(log::LevelFilter::Info);
        // Off: an edit records nothing.
        set(Level::Off);
        let mut app = crate::PdfCraftApp::new();
        let bytes = app.session.create_from_text("Diag", "Old text").unwrap();
        app.open_bytes("diag.pdf", None, bytes.as_ref().clone()).unwrap();
        records();
        let edit = |text: &str| pdfcraft_engine::Edit::EditTextBlock { page: 0, block: 0, text: text.into(), style: Default::default() };
        assert!(app.apply_edit(edit("New text")));
        assert!(records().is_empty(), "nothing is recorded while the log is off");
        // With text: the edit, the paragraph before and after it, and its result.
        set(Level::Text);
        assert!(app.apply_edit(edit("نص 12")));
        let log = records().join("\n");
        assert!(log.contains("EditTextBlock page 1 paragraph 1: text “نص 12”"), "{log}");
        assert!(log.contains("before, page 1 paragraph 1: “New text”"), "{log}");
        assert!(log.contains("after, page 1 paragraph 1: “نص 12”"), "{log}");
        assert!(log.contains("done in"), "{log}");
        // Without text: the same records, the text replaced by its kinds.
        set(Level::Numbers);
        assert!(app.apply_edit(edit("Third")));
        let log = records().join("\n");
        assert!(log.contains("text [5 chars: Latin 5]") && !log.contains("Third") && !log.contains("نص"), "{log}");

        set(Level::Numbers);
        assert_eq!(text("سم 12"), "[5 chars: Arabic 2, space 1, digit 2]");
        assert_eq!(name("C:\\docs\\letter.pdf"), "[a .pdf file]");
        set(Level::Text);
        assert_eq!(text("سم\n1:"), "“سم⏎1:”");
        assert_eq!(text("a\u{200F}b"), "“a<U+200F>b”");
        assert_eq!(name("letter.pdf"), "“letter.pdf”");
        set(Level::Off);
        assert!(!on());
    }

    #[test]
    fn debug_stops_at_its_bound() {
        let big = vec![7u8; 100_000];
        let out = debug(&big);
        assert!(out.len() <= MAX_DEBUG + '…'.len_utf8() && out.ends_with('…'), "{}", out.len());
        assert_eq!(variant(&Some(3)), "Some");
    }

    #[test]
    fn levels_round_trip_by_name() {
        for l in [Level::Off, Level::Numbers, Level::Text] {
            assert_eq!(Level::parse(l.name()), Some(l));
        }
        assert_eq!(Level::parse("all"), None);
    }
}
