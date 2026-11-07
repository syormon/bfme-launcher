//! Port of WorkshopKit (the parts the launcher uses).

pub mod hashes;
pub mod model;
pub mod script;
pub mod sync;

use anyhow::{Result, anyhow};
use indexmap::IndexMap;

use crate::bfme::{self, Key};
use crate::http;
use crate::util::{self, combine, data_dir};
use model::{Entry, Preview};

pub const SERVER_HOST: &str = "https://bfmeladder.com";
pub const FILES_HOST: &str = "https://workshop-files.bfmeladder.com";

pub const IGNORED_GAME_FILES: &[&str] = &[
    "_zzlotr.big",
    "cs01.vp6",
    "nlc_logo.vp6",
    "te_logo.vp6",
    "newlinelogo.vp6",
    "ealogo.vp6",
    "242.vp6",
    "intel.vp6",
    "thx.vp6",
    "mod.txt",
    "timer_frame.png",
];

pub fn config_dir() -> String {
    combine(&[&data_dir(), "Config"])
}
pub fn library_dir() -> String {
    combine(&[&data_dir(), "Library"])
}
pub fn keybinds_dir(game_name: &str, name: &str) -> String {
    combine(&[&data_dir(), "Keybinds", &format!("{game_name}-{name}")])
}

#[derive(Clone, Debug, Default)]
pub struct GameReg {
    pub language: String,
    /// Install dir, trimmed of slashes and lower-cased.
    pub game_dir: String,
    pub data_dir: String,
}

pub type VirtualRegistry = [GameReg; 3];

pub fn virtual_registry() -> VirtualRegistry {
    let g = |i: usize| GameReg {
        language: bfme::language_to_code(&bfme::get(i, Key::Language)),
        game_dir: util::fold_case(util::trim_end_seps(&bfme::get(i, Key::InstallPath))),
        data_dir: bfme::get(i, Key::UserDataLeafName),
    };
    [g(0), g(1), g(2)]
}

pub fn mod_directory(game_dir: &str, name: &str, guid: &str) -> String {
    combine(&[
        &util::parent_dir_str(game_dir),
        "BFME Workshop",
        "Mods",
        &format!("{}-{}", util::sanitize_path_chars(name), guid),
    ])
}

// ---------------------------------------------------------------------------
// HTTP API (HttpUtils / DownloadManager / QueryManager)

fn api_url(path: &str, params: &[(&str, String)]) -> String {
    let mut url = format!("{SERVER_HOST}/api/{path}");
    if !params.is_empty() {
        let mut ser = url::form_urlencoded::Serializer::new(String::new());
        for (k, v) in params {
            ser.append_pair(k, if v.is_empty() { "~" } else { v });
        }
        url.push('?');
        url.push_str(&ser.finish());
    }
    url
}

const AUTH_HEADERS: &[(&str, &str)] = &[("AuthAccountUuid", "unauthenticated"), ("AuthAccountPassword", "")];

pub fn download(guid: &str) -> Result<Entry> {
    let entry: Option<Entry> = http::get_json(&api_url("workshop/download", &[("guid", guid.into())]), AUTH_HEADERS)?;
    entry.filter(|e| !e.guid.is_empty()).ok_or_else(|| anyhow!("The package {guid} was not found on the workshop."))
}

pub fn base_game(game: usize) -> Result<Entry> {
    download(&format!("original-{}", bfme::GAME_NAMES[game]))
}

pub fn query(keyword: &str, game: i32, ty: i32, sort_mode: i32, page: i32) -> Result<Vec<Preview>> {
    let url = api_url(
        "workshop/query",
        &[
            ("keyword", keyword.into()),
            ("game", game.to_string()),
            ("type", ty.to_string()),
            ("sortMode", sort_mode.to_string()),
            ("page", page.to_string()),
        ],
    );
    Ok(http::get_json::<Option<Vec<Preview>>>(&url, AUTH_HEADERS)?.unwrap_or_default())
}

pub fn get_preview(guid: &str) -> Result<Preview> {
    http::get_json(&api_url("workshop", &[("guid", guid.into())]), AUTH_HEADERS)
}

/// Download from the workshop, falling back to the local library.
pub fn get_package(guid: &str) -> Result<Entry> {
    download(guid).or_else(|_| library_get(guid))
}

// ---------------------------------------------------------------------------
// Library (BfmeWorkshopLibraryManager)

fn library_file(guid: &str) -> String {
    combine(&[&library_dir(), &format!("{guid}.json")])
}

pub fn library_query(keyword: &str, game: i32, ty: i32, page: i32) -> Vec<Preview> {
    let flag = combine(&[&config_dir(), "library_version.flag"]);
    util::ensure_dir(&config_dir());
    if util::read_text(&flag, "") != "2" {
        let _ = std::fs::remove_dir_all(library_dir());
        util::write_text(&flag, "2");
    }
    util::ensure_dir(&library_dir());

    let mut files: Vec<(std::time::SystemTime, String)> = util::list_files(&library_dir())
        .into_iter()
        .map(|f| {
            let t = std::fs::metadata(&f).and_then(|m| m.created()).unwrap_or(std::time::UNIX_EPOCH);
            (t, f)
        })
        .collect();
    files.sort_by_key(|(t, _)| *t);

    let mut entries: Vec<Preview> =
        files.iter().filter_map(|(_, f)| util::read_json::<Entry>(f)).map(|e| e.preview()).collect();

    for i in 0..3 {
        for guid in [format!("original-{}", bfme::GAME_NAMES[i]), format!("official-{i}")] {
            if !entries.iter().any(|e| e.guid == guid)
                && let Ok(e) = download(&guid) {
                    library_add_or_update(&e);
                    entries.push(e.preview());
                }
        }
    }

    let kw = keyword.to_lowercase();
    entries.retain(|x| !x.guid.is_empty());
    // Equivalent to the chained stable OrderByDescending calls of the original.
    entries.sort_by_key(|x| {
        std::cmp::Reverse((
            x.guid.starts_with("exp-original-"),
            x.guid.starts_with("original-"),
            x.guid.starts_with("official-"),
        ))
    });
    entries.retain(|x| {
        (kw.is_empty() || x.name.to_lowercase().contains(&kw) || x.description.to_lowercase().contains(&kw))
            && (game == -1 || x.game == game)
            && match ty {
                -1 => true,
                -2 => x.r#type == 0 || x.r#type == 1,
                -3 => x.r#type == 2 || x.r#type == 3,
                t => x.r#type == t,
            }
    });

    if page == -1 {
        return entries;
    }
    entries.into_iter().skip((page * 25) as usize).take(25).collect()
}

pub fn library_get(guid: &str) -> Result<Entry> {
    util::read_json(&library_file(guid))
        .ok_or_else(|| anyhow!("The package with guid {guid} was not found in the library!"))
}

pub fn library_add_or_update(entry: &Entry) {
    util::ensure_dir(&library_dir());
    if entry.r#type <= 1 {
        util::ensure_dir(&keybinds_dir(&entry.game_name(), &entry.name));
    }
    util::write_json(&library_file(&entry.guid), entry);
}

pub fn library_remove(guid: &str) {
    let _ = std::fs::remove_file(library_file(guid));
}

pub fn is_in_library(guid: &str) -> bool {
    util::exists(&library_file(guid))
}

// ---------------------------------------------------------------------------
// Active state (BfmeWorkshopManager / ConfigUtils)

fn active_patch_file(game: usize) -> String {
    combine(&[&config_dir(), &format!("active_patch_{game}.json")])
}
fn active_enhancements_file(game: usize) -> String {
    combine(&[&config_dir(), &format!("active_enhancements_{game}.json")])
}

pub fn active_patch(game: usize) -> Option<Entry> {
    util::read_json::<Option<Entry>>(&active_patch_file(game)).flatten()
}

pub fn active_enhancements(game: usize) -> IndexMap<String, Entry> {
    util::read_json(&active_enhancements_file(game)).unwrap_or_default()
}

pub fn active_mod_path(game: usize) -> String {
    util::read_text(&combine(&[&bfme::install_path(game), "mod.txt"]), "")
}

pub fn is_patch_active(game: usize, guid: &str) -> bool {
    util::file_contains(&active_patch_file(game), &format!("\"Guid\": \"{guid}\""))
}

pub fn is_enhancement_active(game: usize, guid: &str) -> bool {
    util::file_contains(&active_enhancements_file(game), &format!("\"Guid\": \"{guid}\""))
}

fn write_enhancements(game: i32, active: &IndexMap<String, Entry>) {
    let filtered: IndexMap<&String, &Entry> = active.iter().filter(|(_, v)| v.game == game).collect();
    util::write_json(&active_enhancements_file(game.clamp(0, 2) as usize), &filtered);
}

pub(crate) fn disable_enhancement(e: &Entry, active: &mut IndexMap<String, Entry>) {
    active.shift_remove(&e.guid);
    write_enhancements(e.game, active);
}

pub(crate) fn enable_enhancement(e: &Entry, active: &mut IndexMap<String, Entry>) {
    active.insert(e.guid.clone(), e.clone());
    write_enhancements(e.game, active);
}

pub(crate) fn save_active_patch(patch: &Entry) -> Result<()> {
    util::write_json(&active_patch_file(patch.game_idx()), patch);
    if patch.game == 2 {
        util::write_json(&active_patch_file(1), &base_game(1)?);
    }
    if patch.game == 1 && bfme::is_installed(2) {
        util::write_json(&active_patch_file(2), &base_game(2)?);
    }
    Ok(())
}

pub(crate) fn save_active_mod(game: usize, mod_path: &str) {
    let f = combine(&[&bfme::install_path(game), "mod.txt"]);
    if !mod_path.is_empty() {
        util::write_text(&f, mod_path);
    } else {
        let _ = std::fs::remove_file(f);
    }
}
