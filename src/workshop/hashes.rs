//! Remembers the MD5 of every file the sync has hashed, keyed by path and invalidated by the
//! file's size and modification time (plus its change time on Linux, which nothing can set
//! back). A re-sync then only hashes files that actually changed instead of every game file.
//!
//! Stored in `<data>/Cache/hashes.json`; losing it only costs one slow check.

use std::collections::HashMap;
use std::fs;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::util::{self, combine};

#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct Stamp {
    size: u64,
    mtime: i128,
    #[serde(default)]
    ctime: i128,
}

#[derive(Clone, Serialize, Deserialize)]
struct Known {
    #[serde(flatten)]
    stamp: Stamp,
    md5: String,
}

#[derive(Default)]
struct Store {
    /// The file the entries were read from; the data folder can be moved at runtime.
    file: String,
    entries: HashMap<String, Known>,
    dirty: bool,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);

fn store_file() -> String {
    combine(&[&util::data_dir(), "Cache", "hashes.json"])
}

fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut guard = STORE.lock().unwrap_or_else(|e| e.into_inner());
    let file = store_file();
    if guard.as_ref().is_none_or(|s| s.file != file) {
        let entries = fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        *guard = Some(Store { file, entries, dirty: false });
    }
    f(guard.as_mut().unwrap())
}

fn stamp(path: &str) -> Option<Stamp> {
    let m = fs::metadata(path).ok()?;
    if !m.is_file() {
        return None;
    }
    let nanos = |t: std::io::Result<std::time::SystemTime>| {
        t.ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as i128)
            .unwrap_or(0)
    };
    #[cfg(unix)]
    let ctime = {
        use std::os::unix::fs::MetadataExt;
        m.ctime() as i128 * 1_000_000_000 + m.ctime_nsec() as i128
    };
    #[cfg(not(unix))]
    let ctime = 0;
    Some(Stamp { size: m.len(), mtime: nanos(m.modified()), ctime })
}

/// `util::md5_file`, answered from the cache when the file hasn't changed since it was hashed.
/// Safe to call from several threads; the hashing itself runs outside the lock.
pub fn md5(path: &str) -> String {
    let Some(before) = stamp(path) else {
        return "-1".into();
    };
    let key = path.to_string();
    if let Some(hit) = with_store(|s| s.entries.get(&key).filter(|k| k.stamp == before).map(|k| k.md5.clone())) {
        return hit;
    }
    let md5 = util::md5_file(path);
    // Only remember it if the file didn't change while it was being read.
    if md5 != "-1" && stamp(path).as_ref() == Some(&before) {
        with_store(|s| {
            s.entries.insert(key, Known { stamp: before, md5: md5.clone() });
            s.dirty = true;
        });
    }
    md5
}

/// Writes the cache if anything new was hashed.
pub fn save() {
    with_store(|s| {
        if !s.dirty {
            return;
        }
        // Drop entries for files that no longer exist, so the cache doesn't grow forever.
        s.entries.retain(|p, _| std::path::Path::new(p).is_file());
        let Ok(json) = serde_json::to_vec(&s.entries) else { return };
        util::ensure_dir(&util::dir_name(&s.file));
        let tmp = format!("{}.tmp", s.file);
        if fs::write(&tmp, json).is_ok() && fs::rename(&tmp, &s.file).is_ok() {
            s.dirty = false;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rehashes_only_changed_files() {
        let dir = std::env::temp_dir().join(format!("bfme-launcher-hashes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.bin").to_string_lossy().into_owned();

        fs::write(&f, b"hello").unwrap();
        assert_eq!(md5(&f), "5d41402abc4b2a76b9719d911017c592");
        assert_eq!(md5(&f), "5d41402abc4b2a76b9719d911017c592");

        // Same size, new content: the stamp changes, so it is hashed again.
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(&f, b"world").unwrap();
        assert_eq!(md5(&f), "7d793037a0760186574b0282f2f435e7");

        assert_eq!(md5(&dir.join("missing").to_string_lossy()), "-1");
        let _ = fs::remove_dir_all(&dir);
    }
}
