//! Linux implementation. The games run inside a Wine prefix (see `crate::runner`), so "the registry"
//! is a store kept by the launcher and imported into the prefix right before a game starts.

use std::path::{Path, PathBuf};

/// Kills every process whose executable name (with or without `.exe`) matches one of `names`.
/// Under Wine the Windows image path is argv[0] (`Z:\...\game.dat`), so both that and the kernel's
/// process name are checked.
pub fn kill_processes(names: &[&str]) {
    let Ok(rd) = std::fs::read_dir("/proc") else { return };
    let me = std::process::id();
    for e in rd.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        if pid == me {
            continue;
        }
        let comm = std::fs::read_to_string(e.path().join("comm")).unwrap_or_default();
        let cmdline = std::fs::read(e.path().join("cmdline")).unwrap_or_default();
        let argv0 = cmdline.split(|b| *b == 0).next().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
        let image = argv0.rsplit(['\\', '/']).next().unwrap_or("").to_string();
        let matches = |candidate: &str| {
            let c = candidate.trim();
            let stem = c.strip_suffix(".exe").or_else(|| c.strip_suffix(".EXE")).unwrap_or(c);
            !c.is_empty() && names.iter().any(|n| n.eq_ignore_ascii_case(stem) || n.eq_ignore_ascii_case(c))
        };
        if matches(&comm) || matches(&image) {
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGKILL);
            }
        }
    }
}

/// Wine/Proton can present any mode to the game, so offer the common ones.
pub fn supported_resolutions() -> Vec<String> {
    [
        (1024, 768), (1280, 720), (1280, 800), (1280, 1024), (1366, 768), (1440, 900), (1600, 900),
        (1680, 1050), (1920, 1080), (1920, 1200), (2560, 1080), (2560, 1440), (2560, 1600), (3440, 1440),
        (3840, 2160),
    ]
    .iter()
    .map(|(w, h)| format!("{w} {h}"))
    .collect()
}

/// Free bytes on the filesystem that holds `path` (walks up to the nearest existing folder).
pub fn free_space(path: &str) -> Option<u64> {
    let mut p = PathBuf::from(path);
    while !p.is_dir() {
        if !p.pop() {
            return None;
        }
    }
    let c = std::ffi::CString::new(p.to_string_lossy().as_bytes()).ok()?;
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return None;
    }
    Some(st.f_bavail as u64 * st.f_frsize as u64)
}

/// winit can't query monitors before the event loop runs; use a size that fits common screens.
pub fn work_area_logical() -> (f32, f32) {
    (1600.0, 1000.0)
}

/// Holds the lock file for the lifetime of the process.
pub struct InstanceGuard(#[allow(dead_code)] std::fs::File);

/// Returns `None` if another instance already holds `<exe dir>/.<name>.lock`.
pub fn acquire_single_instance(name: &str) -> Option<InstanceGuard> {
    let path = Path::new(&crate::util::exe_dir()).join(format!(".{name}.lock"));
    let Ok(file) = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path) else {
        return Some(InstanceGuard(tempfile_fallback()));
    };
    match file.try_lock() {
        Ok(()) => Some(InstanceGuard(file)),
        Err(std::fs::TryLockError::WouldBlock) => None,
        Err(_) => Some(InstanceGuard(file)),
    }
}

fn tempfile_fallback() -> std::fs::File {
    std::fs::File::open("/dev/null").expect("/dev/null")
}

/// Window managers don't let another process raise our window; the second instance just exits.
pub fn focus_existing_instance(_title: &str) {
    eprintln!("bfme-launcher is already running.");
}

pub fn set_main_window(_cc: &eframe::CreationContext<'_>) {}

pub fn hide_main_window() {
    crate::hub::viewport_cmd(egui::ViewportCommand::Minimized(true));
}

pub fn show_main_window() {
    crate::hub::viewport_cmd(egui::ViewportCommand::Minimized(false));
    crate::hub::viewport_cmd(egui::ViewportCommand::Focus);
}

/// Nothing on Linux needs root: the registry lives in the launcher's own Wine prefix.
pub fn has_admin() -> bool {
    true
}

pub fn open_url(url: &str) {
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

pub fn open_folder(path: &str) {
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

/// Windows compatibility layers don't exist under Wine.
pub fn set_compat_layer(_game_path: &str) {}

/// The game registry, stored as JSON next to the Wine prefix (`prefix/registry.json`) and exported as
/// a `.reg` file that `regedit` imports into the prefix before each launch. Paths are relative to
/// HKLM, exactly like on Windows.
pub mod reg {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use serde::{Deserialize, Serialize};

    use super::super::RegValue;

    #[derive(Default, Serialize, Deserialize)]
    struct Key {
        /// Path with its original spelling (the map key is lower-cased).
        path: String,
        values: BTreeMap<String, RegValue>,
    }

    type Store = BTreeMap<String, Key>;

    static STORE: Mutex<Option<Store>> = Mutex::new(None);

    fn file() -> std::path::PathBuf {
        crate::runner::prefix_dir().join("registry.json")
    }

    fn with<T>(f: impl FnOnce(&mut Store) -> T, save: bool) -> T {
        let mut guard = STORE.lock().unwrap();
        let store = guard.get_or_insert_with(|| {
            std::fs::read_to_string(file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
        });
        let out = f(store);
        if save {
            let _ = std::fs::create_dir_all(crate::runner::prefix_dir());
            if let Ok(text) = serde_json::to_string_pretty(store) {
                let _ = std::fs::write(file(), text);
            }
        }
        out
    }

    fn norm(path: &str) -> String {
        path.trim_matches('\\').to_lowercase()
    }

    fn find<'a>(values: &'a BTreeMap<String, RegValue>, name: &str) -> Option<&'a RegValue> {
        values.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v)
    }

    pub fn get_string(path: &str, name: &str) -> Option<String> {
        with(|s| s.get(&norm(path)).and_then(|k| find(&k.values, name)).map(RegValue::to_display), false)
    }

    pub fn set(path: &str, name: &str, value: RegValue) -> std::io::Result<()> {
        with(
            |s| {
                let key = s.entry(norm(path)).or_insert_with(|| Key { path: path.trim_matches('\\').to_string(), ..Default::default() });
                let existing = key.values.keys().find(|k| k.eq_ignore_ascii_case(name)).cloned();
                key.values.insert(existing.unwrap_or_else(|| name.to_string()), value);
            },
            true,
        );
        Ok(())
    }

    pub fn delete_tree(path: &str) {
        let p = norm(path);
        with(|s| s.retain(|k, _| *k != p && !k.starts_with(&format!("{p}\\"))), true);
    }

    pub fn exists(path: &str) -> bool {
        with(|s| s.contains_key(&norm(path)), false)
    }

    pub fn values(path: &str) -> Option<Vec<(String, String)>> {
        with(|s| s.get(&norm(path)).map(|k| k.values.iter().map(|(n, v)| (n.clone(), v.to_display())).collect()), false)
    }

    /// The whole store as a `regedit` file (UTF-16LE with BOM, like Windows' own exports).
    /// String values holding a Linux path are written as the `Z:\` path Wine maps it to.
    pub fn export_reg() -> Vec<u8> {
        let text = with(
            |s| {
                let mut out = String::from("Windows Registry Editor Version 5.00\r\n\r\n");
                for key in s.values() {
                    out.push_str(&format!("[HKEY_LOCAL_MACHINE\\{}]\r\n", key.path));
                    for (name, value) in &key.values {
                        let lhs = if name.is_empty() { "@".to_string() } else { format!("\"{}\"", escape(name)) };
                        let rhs = match value {
                            RegValue::Sz(v) if v.starts_with('/') => format!("\"{}\"", escape(&crate::runner::to_windows_path(v))),
                            RegValue::Sz(v) => format!("\"{}\"", escape(v)),
                            RegValue::Dword(d) => format!("dword:{d:08x}"),
                        };
                        out.push_str(&format!("{lhs}={rhs}\r\n"));
                    }
                    out.push_str("\r\n");
                }
                out
            },
            false,
        );
        let mut bytes = vec![0xFF, 0xFE];
        for u in text.encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        bytes
    }

    fn escape(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"")
    }
}
