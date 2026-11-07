//! Port of BfmeKit: registry handling, Options.ini, defaults.

use crate::platform::{self, RegValue, reg};
use crate::util::{self, app_data, combine};

pub const GAME_NAMES: [&str; 3] = ["BFME1", "BFME2", "RotWK"];

const DEFAULT_GAME_REGISTRY_KEYS: [&str; 3] = [
    r"Electronic Arts\EA Games\The Battle for Middle-earth",
    r"Electronic Arts\Electronic Arts\The Battle for Middle-earth II",
    r"Electronic Arts\Electronic Arts\The Lord of the Rings, The Rise of the Witch-king",
];

const DEPRECATED_GAME_REGISTRY_KEYS: [&str; 3] = [
    r"EA GAMES\The Battle for Middle-earth",
    r"Electronic Arts\The Battle for Middle-earth II",
    r"Electronic Arts\The Lord of the Rings, The Rise of the Witch-king",
];

const DEFAULT_USER_DATA_LEAF_NAMES: [&str; 3] = [
    "My Battle for Middle-earth Files",
    "My Battle for Middle-earth II Files",
    "My Rise of the Witch-king Files",
];

pub const DEFAULT_GAME_EXECUTABLES: [&str; 3] = ["lotrbfme.exe", "lotrbfme2.exe", "lotrbfme2ep1.exe"];

pub const DEFAULT_OPTIONS: &str = "AllHealthBars = yes
AlternateMouseSetup = no
AmbientVolume = 50.000000
AudioLOD = High
Brightness = 50
FixedStaticGameLOD = UltraHigh
FlashTutorial = 0
HasGotOnline = yes
HasSeenLogoMovies = yes
HeatEffects = yes
IdealStaticGameLOD = UltraHigh
IsThreadedLoad = yes
MovieVolume = 70.000000
MusicVolume = 70.000000
Resolution = 1920 1080
SFXVolume = 70.000000
ScrollFactor = 50
StaticGameLOD = UltraHigh
TimesInGame = 1
UnitDecals = yes
UseEAX3 = yes
VoiceVolume = 70.000000";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    InstallPath,
    Language,
    MapPackVersion,
    UseLocalUserMaps,
    UserDataLeafName,
    Version,
    Serial,
}

impl Key {
    fn value_name(self) -> &'static str {
        match self {
            Key::InstallPath => "InstallPath",
            Key::Language => "Language",
            Key::MapPackVersion => "MapPackVersion",
            Key::UseLocalUserMaps => "UseLocalUserMaps",
            Key::UserDataLeafName => "UserDataLeafName",
            Key::Version => "Version",
            Key::Serial => "",
        }
    }

    fn suffix(self) -> &'static str {
        if self == Key::Serial { r"\ergc" } else { "" }
    }
}

fn software(sub: &str) -> String {
    format!(r"SOFTWARE\WOW6432Node\{sub}")
}

pub fn get(game: usize, key: Key) -> String {
    ensure_fixed_registry(game);

    if key == Key::InstallPath
        && let Some(v) = reg::get_string(&software(DEPRECATED_GAME_REGISTRY_KEYS[game]), "Install Dir")
        && !v.is_empty()
    {
        return v;
    }

    let path = software(&format!("{}{}", DEFAULT_GAME_REGISTRY_KEYS[game], key.suffix()));
    reg::get_string(&path, key.value_name()).unwrap_or_default()
}

pub fn set(game: usize, key: Key, value: &str) -> std::io::Result<()> {
    let path = software(&format!("{}{}", DEFAULT_GAME_REGISTRY_KEYS[game], key.suffix()));
    reg::set(&path, key.value_name(), RegValue::Sz(value.to_string()))
}

fn set_dword(game: usize, key: Key, value: u32) -> std::io::Result<()> {
    let path = software(&format!("{}{}", DEFAULT_GAME_REGISTRY_KEYS[game], key.suffix()));
    reg::set(&path, key.value_name(), RegValue::Dword(value))
}

pub fn install_path(game: usize) -> String {
    get(game, Key::InstallPath)
}

pub fn is_installed(game: usize) -> bool {
    let p = install_path(game);
    !p.is_empty() && util::dir_exists(&p)
}

pub fn user_data_dir(game: usize) -> String {
    combine(&[&app_data(), &get(game, Key::UserDataLeafName)])
}

pub fn create_new_install_registry(game: usize, install_path: &str, language: &str) -> anyhow::Result<()> {
    let mut install_path = install_path.to_string();
    if !install_path.ends_with(util::SEP) {
        install_path.push(util::SEP);
    }
    std::fs::create_dir_all(&install_path)?;

    ensure_fixed_registry(game);
    set(game, Key::InstallPath, &install_path)?;
    set(game, Key::Language, language)?;
    set_dword(game, Key::MapPackVersion, 65536)?;
    set_dword(game, Key::UseLocalUserMaps, 0)?;
    set(game, Key::UserDataLeafName, DEFAULT_USER_DATA_LEAF_NAMES[game])?;
    set_dword(game, Key::Version, 65539)?;
    set(game, Key::Serial, &random_serial())?;
    ensure_defaults(game);
    Ok(())
}

fn random_serial() -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut out = String::new();
    while out.len() < 20 {
        for b in uuid::Uuid::new_v4().as_bytes() {
            if out.len() < 20 {
                out.push(CHARS[*b as usize % CHARS.len()] as char);
            }
        }
    }
    out
}

pub fn ensure_defaults(game: usize) {
    let exe = DEFAULT_GAME_EXECUTABLES[game];
    let install = install_path(game);
    {
        let app_paths = software(&format!(r"Microsoft\Windows\CurrentVersion\App Paths\{exe}"));
        reg::delete_tree(&app_paths);
        let _ = reg::set(&app_paths, "", RegValue::Sz(combine(&[&install, exe])));
        let _ = reg::set(&app_paths, "Game Registry", RegValue::Sz(format!(r"SOFTWARE\{}", DEFAULT_GAME_REGISTRY_KEYS[game])));
        let _ = reg::set(&app_paths, "Installed", RegValue::Dword(1));
        let _ = reg::set(&app_paths, "Path", RegValue::Sz(install.clone()));
        let _ = reg::set(&app_paths, "Restart", RegValue::Dword(0));
    }

    let data_dir = user_data_dir(game);
    util::ensure_dir(&data_dir);
    let options = combine(&[&data_dir, "Options.ini"]);
    let text = util::read_text(&options, "");
    if !util::exists(&options) || text.len() <= 6 || !text.contains("Resolution = ") {
        util::write_text(&options, DEFAULT_OPTIONS);
    }

    ensure_compatibility_settings(&combine(&[&install, exe]));
    ensure_compatibility_settings(&combine(&[&install, "game.dat"]));

    if game == 2 {
        ensure_defaults(1);
    }
}

/// Migrates the legacy "Install Dir" registry key to the new location (admin only).
fn ensure_fixed_registry(game: usize) {
    let old = software(DEPRECATED_GAME_REGISTRY_KEYS[game]);
    if !platform::has_admin() || !reg::exists(&old) {
        return;
    }
    let value = reg::get_string(&old, "Install Dir").unwrap_or_default();
    let _ = set(game, Key::InstallPath, &value);
    reg::delete_tree(&old);
}

pub fn ensure_compatibility_settings(game_path: &str) {
    platform::set_compat_layer(game_path);
}

pub fn language_to_code(language: &str) -> String {
    let l = language.to_lowercase();
    let code = match l.as_str() {
        "english" | "english uk" | "english us" => "EN",
        "french" => "FR",
        "german" => "DE",
        "italian" => "IT",
        "spanish" => "ES",
        "swedish" => "SV",
        "dutch" => "NL",
        "polish" => "PL",
        "norwegian" => "NO",
        "russian" => "RU",
        "turkish" => "TR",
        _ => return l,
    };
    code.to_string()
}

pub fn code_to_language(code: &str) -> String {
    let l = code.to_lowercase();
    let name = match l.as_str() {
        "en" => "English",
        "fr" => "French",
        "de" => "German",
        "it" => "Italian",
        "es" => "Spanish",
        "sv" => "Swedish",
        "nl" => "Dutch",
        "pl" => "Polish",
        "no" => "Norwegian",
        "ru" => "Russian",
        "tr" => "Turkish",
        _ => return l,
    };
    name.to_string()
}

/// Reads all values of an HKLM key (used by the workshop script interpreter).
pub fn hklm_values(path: &str) -> Option<Vec<(String, String)>> {
    reg::values(path)
}

pub fn hklm_string(path: &str, name: &str) -> Option<String> {
    reg::get_string(path, name)
}

// ---------------------------------------------------------------------------
// Options.ini (BfmeSettingsManager)

fn parse_options(text: &str) -> indexmap::IndexMap<String, String> {
    let mut map = indexmap::IndexMap::new();
    for line in text.split('\n') {
        if line.contains(" = ") {
            let mut parts = line.split(" = ");
            let k = parts.next().unwrap_or("").trim_matches('\r').to_string();
            let v = parts.next().unwrap_or("").trim_matches('\r').to_string();
            map.entry(k).or_insert(v);
        }
    }
    map
}

pub fn get_option(game: usize, name: &str) -> Option<String> {
    let file = combine(&[&user_data_dir(game), "Options.ini"]);
    if !util::exists(&file) {
        return None;
    }
    parse_options(&util::read_text(&file, "")).get(name).cloned()
}

pub fn set_option(game: usize, name: &str, value: &str) {
    let file = combine(&[&user_data_dir(game), "Options.ini"]);
    let text = if util::exists(&file) { util::read_text(&file, "") } else { DEFAULT_OPTIONS.to_string() };
    let mut map = parse_options(&text);
    map.insert(name.to_string(), value.to_string());
    let out: Vec<String> = map.iter().map(|(k, v)| format!("{k} = {v}")).collect();
    util::write_text(&file, &out.join("\n"));
}
