//! Launcher translations (converted from the original WPF resource dictionaries).

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

pub const LANGUAGE_NAMES: [&str; 13] = [
    "English", "Deutsch", "Magyar", "Français", "Italiano", "Español", "Svenska", "Türkçe", "Nederlands", "Polski",
    "Norsk", "Русский", "العربية",
];

const FILES: [&str; 13] = [
    include_str!("../assets/lang/en.txt"),
    include_str!("../assets/lang/de.txt"),
    include_str!("../assets/lang/hu.txt"),
    include_str!("../assets/lang/fr.txt"),
    include_str!("../assets/lang/it.txt"),
    include_str!("../assets/lang/es.txt"),
    include_str!("../assets/lang/sv.txt"),
    include_str!("../assets/lang/tr.txt"),
    include_str!("../assets/lang/nl.txt"),
    include_str!("../assets/lang/pl.txt"),
    include_str!("../assets/lang/no.txt"),
    include_str!("../assets/lang/ru.txt"),
    include_str!("../assets/lang/ar.txt"),
];

fn parse(text: &'static str) -> HashMap<&'static str, &'static str> {
    text.lines()
        .filter_map(|l| l.trim_start_matches('\u{feff}').split_once('='))
        .collect()
}

fn tables() -> &'static Vec<HashMap<&'static str, &'static str>> {
    static T: OnceLock<Vec<HashMap<&'static str, &'static str>>> = OnceLock::new();
    T.get_or_init(|| FILES.iter().map(|f| parse(f)).collect())
}

static CURRENT: RwLock<usize> = RwLock::new(0);

pub fn set_language(idx: usize) {
    *CURRENT.write().unwrap() = idx.min(FILES.len() - 1);
}

pub fn language() -> usize {
    *CURRENT.read().unwrap()
}

/// Looks up a translation key, falling back to English and then to the key itself.
pub fn t(key: &str) -> &'static str {
    let tables = tables();
    let idx = language();
    if let Some(v) = tables[idx].get(key) {
        return v;
    }
    if let Some(v) = tables[0].get(key) {
        return v;
    }
    static MISSING: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let mut missing = MISSING.lock().unwrap();
    if let Some(k) = missing.iter().find(|k| **k == key) {
        return k;
    }
    let leaked: &'static str = Box::leak(key.to_string().into_boxed_str());
    missing.push(leaked);
    leaked
}

/// Expands `{Key}` placeholders the way the original popups did.
pub fn expand(s: &str) -> String {
    let mut out = String::new();
    for (i, part) in s.split('{').enumerate() {
        if i > 0
            && let Some((key, rest)) = part.split_once('}') {
                out.push_str(t(key));
                out.push_str(rest);
                continue;
            }
        out.push_str(part);
    }
    out
}
