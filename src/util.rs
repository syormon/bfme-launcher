//! Small path / file helpers that mimic the .NET semantics the original code relied on
//! (string based paths, `Path.Combine`, lower-cased comparisons, ...).
//!
//! Paths are plain strings. On Windows they use `\`; on Linux they use `/`, and workshop file names
//! (which always use `\`) are converted when combined. Linux file systems are case-sensitive while
//! the games are not, so new game files are written in lower case into whatever existing folders
//! match case-insensitively (see [`resolve_ci`]).

use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use md5::{Digest, Md5};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Folder that contains the launcher executable. Everything the launcher itself persists lives here.
pub fn exe_dir() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_string_lossy().into_owned()))
        .unwrap_or_else(|| ".".into())
}

/// Default workshop folder: `<exe dir>\data`.
pub fn default_data_dir() -> String {
    combine(&[&exe_dir(), "data"])
}

static DATA_DIR_OVERRIDE: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

/// Sets the workshop folder chosen in the settings (empty = default).
pub fn set_data_dir(dir: &str) {
    *DATA_DIR_OVERRIDE.write().unwrap() = dir.trim().to_string();
}

/// Workshop folder: library, active patch state, keybind backups, download cache, DirectX files.
pub fn data_dir() -> String {
    let o = DATA_DIR_OVERRIDE.read().unwrap();
    if o.is_empty() { default_data_dir() } else { o.clone() }
}

/// Normalised form for comparing paths: lower case, `\` separators, no trailing separator.
pub fn norm_path(p: &str) -> String {
    p.trim().replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

/// True if `child` is `parent` or lies inside it.
pub fn path_within(child: &str, parent: &str) -> bool {
    let (c, p) = (norm_path(child), norm_path(parent));
    !p.is_empty() && (c == p || c.starts_with(&format!("{p}\\")))
}

pub fn dir_is_empty(p: &str) -> bool {
    fs::read_dir(p).map(|mut d| d.next().is_none()).unwrap_or(true)
}

fn count_files(p: &Path) -> usize {
    match fs::read_dir(p) {
        Ok(rd) => rd.flatten().map(|e| if e.path().is_dir() { count_files(&e.path()) } else { 1 }).sum(),
        Err(_) => 0,
    }
}

/// Moves everything inside `from` into `to` (merging, overwriting files), then removes `from`.
/// Uses a cheap rename where possible and falls back to copy + delete across drives.
/// `progress` receives 0-100.
pub fn move_dir(from: &str, to: &str, progress: &mut dyn FnMut(i32)) -> std::io::Result<()> {
    let (src, dst) = (Path::new(from), Path::new(to));
    if !src.is_dir() {
        return fs::create_dir_all(dst);
    }
    // Whole-folder rename when the target does not exist yet (same drive only).
    if !dst.exists() || (dst.is_dir() && dir_is_empty(to)) {
        if dst.exists() {
            let _ = fs::remove_dir(dst);
        }
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        if fs::rename(src, dst).is_ok() {
            progress(100);
            return Ok(());
        }
    }
    let total = count_files(src).max(1);
    let mut done = 0usize;
    fn walk(src: &Path, dst: &Path, done: &mut usize, total: usize, progress: &mut dyn FnMut(i32)) -> std::io::Result<()> {
        fs::create_dir_all(dst)?;
        for e in fs::read_dir(src)? {
            let e = e?;
            let (s, d) = (e.path(), dst.join(e.file_name()));
            if s.is_dir() {
                if !d.exists() && fs::rename(&s, &d).is_ok() {
                    *done += count_files(&d);
                } else {
                    walk(&s, &d, done, total, progress)?;
                }
            } else {
                if d.exists() {
                    fs::remove_file(&d)?;
                }
                if fs::rename(&s, &d).is_err() {
                    fs::copy(&s, &d)?;
                    fs::remove_file(&s)?;
                }
                *done += 1;
            }
            progress((*done * 100 / total) as i32);
        }
        fs::remove_dir(src)
    }
    walk(src, dst, &mut done, total, progress)?;
    progress(100);
    Ok(())
}

/// Path separator of the host.
#[cfg(windows)]
pub const SEP: char = '\\';
#[cfg(not(windows))]
pub const SEP: char = '/';

/// `%APPDATA%` (roaming) as the games see it. Only used for the games' own user-data folders
/// (maps, Options.ini). On Linux this is the AppData folder inside the Wine prefix.
#[cfg(windows)]
pub fn app_data() -> String {
    std::env::var("APPDATA").unwrap_or_else(|_| {
        let home = std::env::var("USERPROFILE").unwrap_or_default();
        format!("{home}\\AppData\\Roaming")
    })
}

#[cfg(not(windows))]
pub fn app_data() -> String {
    crate::runner::app_data_dir()
}

fn is_sep(c: char) -> bool {
    c == '\\' || c == '/'
}

#[cfg(windows)]
fn is_rooted(p: &str) -> bool {
    let b = p.as_bytes();
    p.starts_with('\\') || p.starts_with('/') || (b.len() >= 2 && b[1] == b':')
}

#[cfg(not(windows))]
fn is_rooted(p: &str) -> bool {
    p.starts_with('/')
}

/// Mirrors `System.IO.Path.Combine` (with the host's separator; `\` in parts become `/` on Linux).
pub fn combine(parts: &[&str]) -> String {
    let mut out = String::new();
    for part in parts {
        if part.is_empty() {
            continue;
        }
        #[cfg(not(windows))]
        let part = &part.replace('\\', "/");
        if is_rooted(part) || out.is_empty() {
            out = part.to_string();
            continue;
        }
        if !out.ends_with(['\\', '/', ':']) {
            out.push(SEP);
        }
        out.push_str(part);
    }
    out
}

/// `string.Join(SEP, path.Split(['\\', '/']).SkipLast(1))`
pub fn parent_dir_str(p: &str) -> String {
    let mut parts: Vec<&str> = p.split(is_sep).collect();
    parts.pop();
    parts.join(&SEP.to_string())
}

/// Trims separators on both ends. For relative names only: it would strip the leading `/` of a
/// Linux absolute path (use [`trim_end_seps`] for folders).
pub fn trim_seps(p: &str) -> &str {
    p.trim_matches('\\').trim_matches('/')
}

/// A folder path without trailing separators.
pub fn trim_end_seps(p: &str) -> &str {
    p.trim_end_matches(['\\', '/'])
}

/// Case folding for path comparisons that the original did with lower-cased strings: a no-op on
/// Linux, where the real spelling is needed to find the files.
pub fn fold_case(p: &str) -> String {
    if cfg!(windows) { p.to_lowercase() } else { p.to_string() }
}

/// On Linux, maps `path` onto the existing file system case-insensitively: every leading component
/// that exists under a different spelling (`Data` for `data`) is replaced by the existing one, the
/// rest is kept as given. Windows is case-insensitive already, so this is the identity there.
#[cfg_attr(windows, allow(dead_code))]
pub fn resolve_ci(path: &str) -> String {
    if cfg!(windows) {
        return path.to_string();
    }
    let mut cur = std::path::PathBuf::from("/");
    let mut parts = path.split('/').filter(|c| !c.is_empty()).peekable();
    while let Some(part) = parts.next() {
        let exact = cur.join(part);
        if exact.exists() {
            cur = exact;
            continue;
        }
        let found = fs::read_dir(&cur)
            .ok()
            .and_then(|rd| rd.flatten().map(|e| e.file_name()).find(|n| n.to_string_lossy().eq_ignore_ascii_case(part)));
        match found {
            Some(name) => cur.push(name),
            None => {
                cur.push(part);
                for rest in parts.by_ref() {
                    cur.push(rest);
                }
            }
        }
    }
    cur.to_string_lossy().into_owned()
}

pub fn file_name(p: &str) -> &str {
    p.rsplit(is_sep).next().unwrap_or(p)
}

/// `Path.GetExtension` (including the dot), or "" when there is none.
pub fn extension(p: &str) -> &str {
    let name = file_name(p);
    match name.rfind('.') {
        Some(i) => &name[i..],
        None => "",
    }
}

pub fn dir_name(p: &str) -> String {
    match p.rfind(is_sep) {
        Some(i) => p[..i].to_string(),
        None => String::new(),
    }
}

/// Replaces characters that `Path.GetInvalidPathChars()` reports on Windows.
pub fn sanitize_path_chars(s: &str) -> String {
    s.chars()
        .map(|c| {
            if (c as u32) < 32 || matches!(c, '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect()
}

pub fn exists(p: &str) -> bool {
    Path::new(p).is_file()
}

pub fn dir_exists(p: &str) -> bool {
    !p.is_empty() && Path::new(p).is_dir()
}

pub fn ensure_dir(p: &str) {
    if !p.is_empty() {
        let _ = fs::create_dir_all(p);
    }
}

pub fn size(p: &str) -> i64 {
    fs::metadata(p).map(|m| if m.is_file() { m.len() as i64 } else { -1 }).unwrap_or(-1)
}

/// Lower-case hex md5 of a file, or "-1" if it doesn't exist (same as the original).
pub fn md5_file(p: &str) -> String {
    let Ok(mut f) = fs::File::open(p) else {
        return "-1".into();
    };
    let mut hasher = Md5::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(_) => return "-1".into(),
        }
    }
    hex(&hasher.finalize())
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn read_text(p: &str, def: &str) -> String {
    match fs::read(p) {
        Ok(bytes) => {
            let s = String::from_utf8_lossy(&bytes).into_owned();
            s.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(s)
        }
        Err(_) => def.to_string(),
    }
}

pub fn write_text(p: &str, data: &str) {
    if let Ok(mut f) = fs::File::create(p) {
        let _ = f.write_all(data.as_bytes());
    }
}

pub fn read_json<T: DeserializeOwned>(p: &str) -> Option<T> {
    let text = fs::read_to_string(p).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

pub fn write_json<T: Serialize>(p: &str, value: &T) {
    if let Ok(text) = serde_json::to_string_pretty(value) {
        write_text(p, &text);
    }
}

pub fn file_contains(p: &str, needle: &str) -> bool {
    fs::read_to_string(p).map(|s| s.contains(needle)).unwrap_or(false)
}

/// Copy with overwrite, creating the destination directory if needed.
pub fn copy_file(from: &str, to: &str) -> std::io::Result<()> {
    let d = dir_name(to);
    if !d.is_empty() {
        let _ = fs::create_dir_all(&d);
    }
    fs::copy(from, to).map(|_| ())
}

/// Lists files of a directory as `root\name` strings (like `Directory.GetFiles`).
pub fn list_files(root: &str) -> Vec<String> {
    list_entries(root, false)
}

pub fn list_dirs(root: &str) -> Vec<String> {
    list_entries(root, true)
}

fn list_entries(root: &str, dirs: bool) -> Vec<String> {
    let Ok(rd) = fs::read_dir(root) else {
        return vec![];
    };
    let mut out = vec![];
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() == dirs {
            out.push(combine(&[root, &e.file_name().to_string_lossy()]));
        }
    }
    out
}

pub fn open_url(url: &str) {
    crate::platform::open_url(url);
}

pub fn open_folder(path: &str) {
    ensure_dir(path);
    crate::platform::open_folder(path);
}

pub fn set_clipboard(ctx: &egui::Context, text: &str) {
    ctx.copy_text(text.to_string());
}

/// `NumberUtils.IntAsPrettyNumber`
pub fn pretty_number(num: i64) -> String {
    if num <= 0 {
        return num.to_string();
    }
    let i = 10f64.powi(((num as f64).log10() - 2.0).max(0.0) as i32) as i64;
    let num = num / i * i;
    let fmt = |v: f64, suffix: &str| {
        let s = format!("{:.1}", (v * 10.0).floor() / 10.0);
        format!("{}{suffix}", s.trim_end_matches(".0"))
    };
    if num >= 1_000_000_000 {
        fmt(num as f64 / 1e9, "B")
    } else if num >= 1_000_000 {
        fmt(num as f64 / 1e6, "M")
    } else if num >= 1000 {
        fmt(num as f64 / 1e3, "K")
    } else {
        num.to_string()
    }
}

pub fn now_millis() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn time_ago(millis: i64) -> String {
    let secs = ((now_millis() - millis) / 1000).max(0);
    let mins = secs / 60;
    let hours = secs / 3600;
    let days = secs / 86400;
    if secs < 60 {
        if secs == 1 { "one second ago".into() } else { format!("{secs} seconds ago") }
    } else if secs < 3600 {
        format!("{mins} minutes ago")
    } else if secs < 7200 {
        "an hour ago".into()
    } else if secs < 86400 {
        format!("{hours} hours ago")
    } else if secs < 172800 {
        "yesterday".into()
    } else if secs < 2_592_000 {
        format!("{days} days ago")
    } else if secs < 31_104_000 {
        let m = days / 30;
        if m <= 1 { "one month ago".into() } else { format!("{m} months ago") }
    } else {
        let y = days / 365;
        if y <= 1 { "one year ago".into() } else { format!("{y} years ago") }
    }
}

pub fn format_size(size: i64) -> (String, &'static str) {
    let (v, unit) = if size >= 1_073_741_824 {
        (size as f64 / 1_073_741_824.0, "GB")
    } else if size >= 1_048_576 {
        (size as f64 / 1_048_576.0, "MB")
    } else {
        (size as f64 / 1024.0, "KB")
    };
    let rounded = (v * 100.0).round() / 100.0;
    (rounded.to_string().replace('.', ","), unit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(windows))]
    fn linux_paths() {
        assert_eq!(combine(&["/home/a/Games", "BFME2"]), "/home/a/Games/BFME2");
        assert_eq!(combine(&["/home/a/Games/", r"data\ini\x.ini"]), "/home/a/Games/data/ini/x.ini");
        assert_eq!(combine(&["/a", "/rooted"]), "/rooted");
        assert_eq!(parent_dir_str("/home/a/BFME2"), "/home/a");
        assert_eq!(trim_end_seps("/home/a/BFME2/"), "/home/a/BFME2");
        assert_eq!(fold_case("/Home/A"), "/Home/A");
    }

    #[test]
    #[cfg(not(windows))]
    fn resolve_ci_reuses_existing_spelling() {
        let root = std::env::temp_dir().join(format!("bfme-launcher-ci-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("Data/INI")).unwrap();
        let r = root.to_string_lossy();
        assert_eq!(resolve_ci(&format!("{r}/data/ini/new.ini")), format!("{r}/Data/INI/new.ini"));
        assert_eq!(resolve_ci(&format!("{r}/data/new/x.big")), format!("{r}/Data/new/x.big"));
        assert_eq!(resolve_ci(&format!("{r}/lang")), format!("{r}/lang"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    #[cfg(windows)]
    fn combine_matches_dotnet() {
        assert_eq!(combine(&[r"c:\games", "bfme1"]), r"c:\games\bfme1");
        assert_eq!(combine(&[r"c:\games\", "bfme1"]), r"c:\games\bfme1");
        assert_eq!(combine(&["c:", "BFME Workshop"]), "c:BFME Workshop");
        assert_eq!(combine(&[r"c:\a", r"\rooted"]), r"\rooted");
        assert_eq!(combine(&["", "x"]), "x");
    }

    #[test]
    #[cfg(windows)]
    fn path_helpers() {
        assert_eq!(parent_dir_str(r"c:\games\bfme1"), r"c:\games");
        assert_eq!(extension(r"data\x.big"), ".big");
        assert_eq!(extension(r"data.dir\noext"), "");
        assert_eq!(file_name(r"a/b\c.txt"), "c.txt");
        assert_eq!(sanitize_path_chars("a|b\"c"), "a_b_c");
    }

    #[test]
    fn path_checks() {
        assert!(path_within(r"C:\Games\BFME2\sub", r"c:\games\bfme2\"));
        assert!(path_within(r"C:\Games\BFME2", "C:/Games/BFME2"));
        assert!(!path_within(r"C:\Games\BFME2x", r"C:\Games\BFME2"));
        assert!(!path_within(r"C:\Games", ""));
    }

    #[test]
    fn move_dir_merges_and_removes_source() {
        let root = std::env::temp_dir().join(format!("bfme-launcher-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (src, dst) = (root.join("src"), root.join("dst"));
        fs::create_dir_all(src.join("a/b")).unwrap();
        fs::write(src.join("a/b/f.txt"), "new").unwrap();
        fs::write(src.join("top.txt"), "top").unwrap();
        fs::create_dir_all(dst.join("a")).unwrap();
        fs::write(dst.join("a/keep.txt"), "keep").unwrap();
        let mut last = 0;
        move_dir(&src.to_string_lossy(), &dst.to_string_lossy(), &mut |p| last = p).unwrap();
        assert_eq!(last, 100);
        assert!(!src.exists());
        assert_eq!(fs::read_to_string(dst.join("a/b/f.txt")).unwrap(), "new");
        assert_eq!(fs::read_to_string(dst.join("a/keep.txt")).unwrap(), "keep");
        assert_eq!(fs::read_to_string(dst.join("top.txt")).unwrap(), "top");

        // Whole-folder rename into a new location.
        let moved = root.join("moved");
        move_dir(&dst.to_string_lossy(), &moved.to_string_lossy(), &mut |_| {}).unwrap();
        assert!(!dst.exists() && moved.join("top.txt").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn pretty_numbers() {
        assert_eq!(pretty_number(999), "999");
        assert_eq!(pretty_number(1234), "1.2K");
        assert_eq!(pretty_number(50_000), "50K");
        assert_eq!(pretty_number(2_560_000), "2.5M");
    }
}
