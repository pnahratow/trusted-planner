//! German words for an English UI.
//!
//! The English text in the template *is* the key: `{{ "Today" | t }}` looks up
//! "Today" and renders whatever German stands against it, or "Today" itself if
//! nothing does. That means English needs no file at all, a missing
//! translation degrades to correct English rather than to a bare identifier,
//! and the templates stay readable to someone who does not speak German.
//!
//! Everything else — code, comments, docs, database contents — stays English.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The source language. Its strings are already in the templates.
pub const DEFAULT: &str = "en";

/// What the settings page offers, and the only values accepted from it.
pub const LANGUAGES: &[(&str, &str)] = &[("en", "English"), ("de", "Deutsch")];

pub fn is_known(lang: &str) -> bool {
    LANGUAGES.iter().any(|(code, _)| *code == lang)
}

/// Where the `<lang>.json` files live. Read at render time, like templates, so
/// a wording fix is a file edit and a page reload.
#[derive(Clone)]
pub struct Locales {
    dir: PathBuf,
}

/// The words in force for one render.
#[derive(Clone)]
pub struct Locale {
    lang: String,
    words: HashMap<String, String>,
}

impl Locales {
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
        }
    }

    /// Loads the words for `lang`.
    ///
    /// A missing or malformed file is a warning, not an error: the page still
    /// renders, in English. A typo in a translation must never be able to take
    /// the planner down.
    pub fn load(&self, lang: &str) -> Locale {
        if lang == DEFAULT {
            return Locale::english();
        }

        let path = self.dir.join(format!("{lang}.json"));
        let words = match std::fs::read_to_string(&path) {
            Ok(raw) => match serde_json::from_str(&raw) {
                Ok(words) => words,
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "unreadable translations");
                    HashMap::new()
                }
            },
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "no translations for language");
                HashMap::new()
            }
        };

        Locale {
            lang: lang.to_string(),
            words,
        }
    }
}

impl Locale {
    pub fn english() -> Self {
        Self {
            lang: DEFAULT.to_string(),
            words: HashMap::new(),
        }
    }

    /// The translation of `text`, or `text` itself.
    pub fn t<'a>(&'a self, text: &'a str) -> &'a str {
        self.words.get(text).map_or(text, String::as_str)
    }

    /// For `<html lang>`, so a screen reader and the browser's own spelling
    /// checker know which language they are looking at.
    pub fn lang(&self) -> &str {
        &self.lang
    }
}

/// Fills `{name}` placeholders in a translated pattern.
///
/// Dates need this: "3 Sep" is "3. Sep" in German, so the *order and
/// punctuation* have to be translatable, not only the words. Three patterns
/// use it and nothing else does, which is exactly as much templating as this
/// needs.
pub fn fill(pattern: &str, vars: &[(&str, &str)]) -> String {
    let mut out = pattern.to_string();
    for (name, value) in vars {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locale(pairs: &[(&str, &str)]) -> Locale {
        Locale {
            lang: "de".into(),
            words: pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        }
    }

    #[test]
    fn an_untranslated_string_stays_english() {
        let de = locale(&[("Today", "Heute")]);
        assert_eq!(de.t("Today"), "Heute");
        assert_eq!(
            de.t("Add a task"),
            "Add a task",
            "a gap in the file must read as English, not as a broken key"
        );
    }

    #[test]
    fn english_needs_no_file() {
        let en = Locale::english();
        assert_eq!(en.t("Today"), "Today");
        assert_eq!(en.lang(), "en");
    }

    #[test]
    fn a_missing_file_degrades_to_english() {
        let locales = Locales::new(Path::new("/nonexistent"));
        let loc = locales.load("de");
        assert_eq!(loc.t("Today"), "Today");
        assert_eq!(loc.lang(), "de", "still German for `<html lang>`");
    }

    #[test]
    fn patterns_place_the_parts_where_the_language_puts_them() {
        assert_eq!(
            fill("{day} {month}", &[("day", "3"), ("month", "Sep")]),
            "3 Sep"
        );
        assert_eq!(
            fill("{day}. {month}", &[("day", "3"), ("month", "Sep")]),
            "3. Sep"
        );
        assert_eq!(
            fill(
                "{weekday}, {day}. {month}",
                &[
                    ("weekday", "Donnerstag"),
                    ("day", "3"),
                    ("month", "September")
                ]
            ),
            "Donnerstag, 3. September"
        );
    }

    /// Strings the Rust side asks for by something other than a literal, which
    /// the scans below cannot see.
    const INDIRECT_KEYS: &[&str] = &[
        // `loc.t(queries::OVERDUE_LIST_NAME)` — a constant, not a literal.
        "Todo",
        // Theme values, which reach the template as data rather than literals.
        "system", "light", "dark",
    ];

    /// Every `.t("...")` in the source. The date words and the handful above
    /// are asked for indirectly; everything else the Rust side translates is a
    /// literal, and a literal can be found.
    fn keys_used_in_rust() -> Vec<String> {
        let mut keys = Vec::new();
        let mut dirs = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(dir).expect("src/") {
                let path = entry.expect("readable entry").path();
                if path.is_dir() {
                    dirs.push(path);
                    continue;
                }
                let source = std::fs::read_to_string(&path).expect("readable source");
                for line in source.lines() {
                    // Comments talk *about* the call as often as they sit next
                    // to one, and this file's own doc comment is the proof.
                    if line.trim_start().starts_with("//") {
                        continue;
                    }
                    for chunk in line.split(".t(\"").skip(1) {
                        if let Some(literal) = chunk.split('"').next() {
                            keys.push(literal.to_string());
                        }
                    }
                }
            }
        }
        keys
    }

    /// Everything the app can ask for, from wherever it asks.
    fn all_keys() -> Vec<String> {
        keys_used_in_templates()
            .into_iter()
            .chain(keys_used_in_rust())
            .chain(INDIRECT_KEYS.iter().map(|k| (*k).to_string()))
            .chain(
                crate::calendar::all_date_words()
                    .into_iter()
                    .map(str::to_string),
            )
            .collect()
    }

    /// Every `{{ "..." | t }}` in every template.
    fn keys_used_in_templates() -> Vec<String> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
        let mut keys = Vec::new();
        for entry in std::fs::read_dir(dir).expect("templates/") {
            let path = entry.expect("readable entry").path();
            let source = std::fs::read_to_string(&path).expect("readable template");
            for chunk in source.split("{{").skip(1) {
                let Some(expr) = chunk.split("}}").next() else {
                    continue;
                };
                if !expr.contains("| t") {
                    continue;
                }
                // Both quote styles appear: single inside HTML attributes,
                // double elsewhere.
                for quote in ['"', '\''] {
                    let mut parts = expr.split(quote);
                    parts.next();
                    while let Some(literal) = parts.next() {
                        keys.push(literal.to_string());
                        if parts.next().is_none() {
                            break;
                        }
                    }
                }
            }
        }
        keys
    }

    fn german() -> HashMap<String, String> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("locales/de.json");
        let raw = std::fs::read_to_string(path).expect("locales/de.json");
        serde_json::from_str(&raw).expect("de.json is valid JSON")
    }

    /// The point of the whole scheme is that a missing translation is invisible
    /// — it renders as English — so only a test can say the file is complete.
    #[test]
    fn german_translates_every_string_the_app_shows() {
        let de = german();
        let mut missing: Vec<String> = all_keys()
            .into_iter()
            .filter(|key| !de.contains_key(key))
            .collect();
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "untranslated: {missing:#?}");
    }

    /// The failure this scheme invites: writing a string straight into a
    /// template and forgetting `| t`. Nothing breaks, nothing looks wrong in
    /// English, and the German page quietly keeps an English word in it.
    #[test]
    fn no_template_shows_a_string_it_did_not_mark_for_translation() {
        // Entities and punctuation carry no language; the app's own name is
        // not translated.
        let ignore = |text: &str| {
            text.starts_with('&')
                || text == "Trusted&nbsp;Planner"
                || !text.chars().any(char::is_alphabetic)
        };

        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates");
        let mut bare = Vec::new();
        for entry in std::fs::read_dir(dir).expect("templates/") {
            let path = entry.expect("readable entry").path();
            let source = std::fs::read_to_string(&path).expect("readable template");
            let name = path
                .file_name()
                .expect("a file")
                .to_string_lossy()
                .to_string();

            // Text between tags, skipping anything holding an expression.
            for chunk in source.split('>').skip(1) {
                let Some(text) = chunk.split('<').next() else {
                    continue;
                };
                let text = text.trim();
                if text.is_empty() || text.contains('{') || ignore(text) {
                    continue;
                }
                bare.push(format!("{name}: {text}"));
            }
        }
        bare.sort();
        assert!(bare.is_empty(), "not marked with `| t`: {bare:#?}");
    }

    /// And the other way round: a key nothing looks up any more is a string
    /// that moved or was reworded, and the German next to it is stale.
    #[test]
    fn german_has_nothing_left_over() {
        let known: std::collections::HashSet<String> = all_keys().into_iter().collect();
        let mut stray: Vec<String> = german()
            .into_keys()
            .filter(|key| !known.contains(key))
            .collect();
        stray.sort();
        assert!(stray.is_empty(), "no longer used: {stray:#?}");
    }

    #[test]
    fn only_the_offered_languages_are_accepted() {
        assert!(is_known("en") && is_known("de"));
        assert!(!is_known("fr"), "the settings form must not store anything");
        assert!(!is_known("../etc/passwd"), "the code becomes a file name");
    }
}
