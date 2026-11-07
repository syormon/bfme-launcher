//! Port of `BfmeWorkshopSyncManager`: switches the game installation to a given patch/mod
//! configuration by layering files from enhancements, the package, its dependencies and the
//! base game, then removes everything that is not part of the configuration.

use std::collections::HashSet;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

use anyhow::{Result, anyhow, bail};

use super::model::{Entry, WorkshopFile};
use super::*;
use super::hashes;
use crate::{bfme, hub, platform, util};

macro_rules! sync_error {
    ($name:ident) => {
        #[derive(Debug)]
        pub struct $name(pub String);
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
        impl std::error::Error for $name {}
    };
}

sync_error!(PackageNotSyncable);
sync_error!(GameNotInstalled);
sync_error!(EnhancementIncompatible);
sync_error!(FileMissing);
sync_error!(ScriptMissingRequirements);

pub type Progress<'a> = Option<&'a dyn Fn(i32, &str)>;

struct PlannedFile {
    file: WorkshopFile,
    entry_game: usize,
    entry_type: i32,
    is_base_file: bool,
    /// Where the file is written.
    destination: String,
}

fn guid_part(s: &str) -> &str {
    s.split(':').next().unwrap_or(s)
}

fn report(progress: Progress, p: i32, status: &str) {
    if let Some(cb) = progress {
        cb(p, status);
    }
    hub::sync_update(p, status);
}

pub fn sync(entry: &Entry, fast_compare: bool, enhancements: Option<Vec<String>>, progress: Progress) -> Result<()> {
    hub::sync_begin(entry);
    if let Some(cb) = progress {
        cb(0, "Preparing");
    }

    let result = sync_inner(entry.clone(), fast_compare, enhancements, progress);

    hub::sync_end();
    result
}

fn sync_inner(mut entry: Entry, fast: bool, enhancements: Option<Vec<String>>, progress: Progress) -> Result<()> {
    util::ensure_dir(&config_dir());
    let game = entry.game_idx();
    let keybinds = keybinds_dir(&entry.game_name(), &entry.name);
    let vr = virtual_registry();
    let mut active = active_enhancements(game);
    let script = script::run_if_scripted(&mut entry)?;
    let mut deps: Vec<Entry> = vec![];
    let mut files: Vec<PlannedFile> = vec![];

    match entry.r#type {
        0 | 1 | 4 => {
            save_active_patch(&entry)?;
            kill_game();

            prepare_runtime(progress)?;
            bfme::ensure_defaults(game);

            report(progress, 0, "Constructing dependency map");
            if entry.game == 2 {
                for (k, v) in active_enhancements(1) {
                    active.insert(k, v);
                }
            }
            if entry.game == 1 && bfme::is_installed(2) {
                for (k, v) in active_enhancements(2) {
                    active.insert(k, v);
                }
            }

            // Explicit dependencies.
            for d in entry.dependencies.iter().filter(|d| guid_part(d) != entry.guid) {
                deps.push(get_package(d)?);
            }
            resolve_implicit(&entry, &mut deps)?;

            // Explicit enhancements.
            if let Some(list) = &enhancements {
                let explicit: Vec<&String> = list
                    .iter()
                    .filter(|x| guid_part(x) != entry.guid && !deps.iter().any(|y| guid_part(x) == y.guid))
                    .collect();
                for e in explicit {
                    let pkg = get_package(e)?;
                    let compatible = pkg.dependencies.iter().any(|y| {
                        let matches = |g: &Entry| if y.contains(':') { format!("{}:{}", g.guid, g.version) == *y } else { g.guid == *y };
                        matches(&entry) || deps.iter().any(matches)
                    });
                    if !compatible {
                        return Err(EnhancementIncompatible(format!(
                            "'{}' is not compatible with '{}'.",
                            pkg.full_name(),
                            entry.full_name()
                        ))
                        .into());
                    }
                    deps.push(pkg);
                }
            }
            resolve_implicit(&entry, &mut deps)?;

            // A mod depending on a patch of another game makes that patch active for that game.
            if entry.r#type == 1
                && let Some(p) = deps.iter().find(|x| x.r#type == 0 && x.game != entry.game) {
                    save_active_patch(p)?;
                }

            // Disable incompatible enhancements (or all, if an explicit list was given).
            let versioned = format!("{}:{}", entry.guid, entry.version);
            let to_disable: Vec<Entry> = active
                .values()
                .filter(|x| {
                    (x.r#type == 2 && !x.dependencies.contains(&entry.guid) && !x.dependencies.contains(&versioned))
                        || x.game != entry.game
                        || enhancements.is_some()
                })
                .cloned()
                .collect();
            for e in &to_disable {
                disable_enhancement(e, &mut active);
            }

            // Enable enhancements that are (implicit) dependencies of this package.
            let to_enable: Vec<Entry> = deps
                .iter()
                .filter(|x| {
                    (x.r#type == 2 || x.r#type == 3)
                        && (x.dependencies.contains(&entry.guid)
                            || x.dependencies.contains(&versioned)
                            || deps.iter().any(|y| x.dependencies.contains(&y.guid))
                            || deps.iter().any(|y| x.dependencies.contains(&format!("{}:{}", y.guid, y.version))))
                })
                .cloned()
                .collect();
            for e in &to_enable {
                enable_enhancement(e, &mut active);
            }

            report(progress, 0, "Compiling file list");
            // Files added sooner have priority.
            let mut seen = HashSet::new();
            for e in active.values() {
                add_files(e, false, &mut files, &mut seen, &vr);
            }
            add_files(&entry, entry.is_base_game(), &mut files, &mut seen, &vr);
            for d in deps.iter().filter(|x| x.r#type == 0) {
                add_files(d, false, &mut files, &mut seen, &vr);
            }
            if !entry.is_base_game() {
                add_files(&base_game(game)?, true, &mut files, &mut seen, &vr);
            }
            if entry.game == 2 {
                add_files(&base_game(1)?, true, &mut files, &mut seen, &vr);
            }
            if entry.game == 1 && bfme::is_installed(2) {
                add_files(&base_game(2)?, true, &mut files, &mut seen, &vr);
            }

            check_games(&files, &vr, "mod/patch")?;
            download_all(&files, fast, progress)?;

            report(progress, 100, "Cleaning up");
            let mut games: Vec<usize> = files.iter().map(|f| f.entry_game).collect();
            games.sort();
            games.dedup();
            let maps_dir = combine(&[&util::app_data(), &vr[game].data_dir, "Maps", "Workshop"]);
            let mod_dir = mod_directory(&vr[game].game_dir, &entry.name, &entry.guid);
            for g in games {
                clean_up_files(g, &seen, &vr, &vr[g].game_dir)?;
                clean_up_files(g, &seen, &vr, &maps_dir)?;
                if entry.r#type == 1 {
                    clean_up_files(g, &seen, &vr, &mod_dir)?;
                }
            }

            if script.requirements.values().any(|v| !v) {
                return Err(ScriptMissingRequirements("This scripted package is missing one or more requirement.".into()).into());
            }

            report(progress, 100, "Finishing up");
            if entry.r#type == 1 {
                let path = if !script.files_directory.is_empty() { script.files_directory.clone() } else { mod_dir };
                save_active_mod(game, &path);
            } else {
                save_active_mod(game, "");
            }

            report(progress, 100, "Copying custom keybinds");
            util::ensure_dir(&keybinds);
            for f in util::list_files(&keybinds) {
                if util::extension(&f).eq_ignore_ascii_case(".big") {
                    let _ = util::copy_file(&f, &combine(&[&vr[game].game_dir, "lang", util::file_name(&f)]));
                }
            }
            let zz = combine(&[&keybinds, "_zzlotr.big"]);
            if util::exists(&zz) {
                let _ = util::copy_file(&zz, &combine(&[&vr[game].game_dir, "_zzlotr.big"]));
            }
        }
        3 => {
            kill_game();
            prepare_runtime(progress)?;
            bfme::ensure_defaults(game);
            report(progress, 0, "Compiling file list");
            let mut seen = HashSet::new();
            add_files(&entry, false, &mut files, &mut seen, &vr);
            check_games(&files, &vr, "map pack")?;
            enable_enhancement(&entry, &mut active);
            download_all(&files, fast, progress)?;
            report(progress, 100, "Cleaning up");
            let maps_dir = combine(&[&util::app_data(), &vr[game].data_dir, "Maps", "Workshop"]);
            let mut games: Vec<usize> = files.iter().map(|f| f.entry_game).collect();
            games.dedup();
            for g in games {
                clean_up_files(g, &seen, &vr, &maps_dir)?;
            }
        }
        _ => {
            return Err(PackageNotSyncable(format!(
                "\"{}\" is not a syncable package. It is only meant to be applied as an enhancement to other packages.",
                entry.name
            ))
            .into());
        }
    }
    Ok(())
}

/// Windows: the DirectX 9 runtime. Linux: Proton/Wine and the prefix (whose AppData holds the
/// games' Options.ini and maps).
fn prepare_runtime(progress: Progress) -> Result<()> {
    #[cfg(windows)]
    {
        report(progress, 0, "Installing DX9 runtime");
        crate::dx::ensure_runtimes();
    }
    #[cfg(not(windows))]
    crate::runner::ensure_ready(&|p, s| report(progress, p, s))?;
    Ok(())
}

fn resolve_implicit(entry: &Entry, deps: &mut Vec<Entry>) -> Result<()> {
    loop {
        let missing: Vec<String> = deps
            .iter()
            .flat_map(|x| x.dependencies.iter())
            .filter(|x| guid_part(x) != entry.guid && !deps.iter().any(|y| guid_part(x) == y.guid))
            .cloned()
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        for d in missing {
            if !deps.iter().any(|y| guid_part(&d) == y.guid) {
                deps.push(get_package(&d)?);
            }
        }
    }
}

fn check_games(files: &[PlannedFile], vr: &VirtualRegistry, what: &str) -> Result<()> {
    let mut games: Vec<usize> = files.iter().map(|f| f.entry_game).collect();
    games.sort();
    games.dedup();
    if games.iter().any(|g| vr[*g].game_dir.is_empty() || !util::dir_exists(&vr[*g].game_dir)) {
        return Err(GameNotInstalled(format!("One or more bfme games required by this {what} is not installed.")).into());
    }
    if games.iter().any(|g| util::trim_seps(&vr[*g].game_dir.replace('/', "\\")).split('\\').filter(|c| !c.is_empty()).count() <= 1) {
        return Err(GameNotInstalled(format!(
            "One or more bfme games required by this {what} is installed in a root directory (for example C: or D:). The sync was aborted as continuing could cause catastrophic damage. Your files are safe, nothing was modified! It is highly recommended that you change your bfme install directory!"
        ))
        .into());
    }
    Ok(())
}

/// How many files are verified at once. Hashing is mostly disk bound; a few readers keep an SSD
/// busy without thrashing a hard drive.
const VERIFY_THREADS: usize = 4;

fn download_all(files: &[PlannedFile], fast: bool, progress: Progress) -> Result<()> {
    let result = download_all_inner(files, fast, progress);
    hashes::save();
    result
}

fn download_all_inner(files: &[PlannedFile], fast: bool, progress: Progress) -> Result<()> {
    // 1. Verify what is already in place, in parallel (files whose hash is cached cost one stat).
    let ok = verify_in_place(files, fast, progress)?;

    // 2. Fetch the rest one at a time, from the cache or the network.
    let missing: Vec<&PlannedFile> = files.iter().zip(&ok).filter(|(_, ok)| !**ok).map(|(f, _)| f).collect();
    let total: i64 = missing.iter().map(|f| f.file.size).sum::<i64>().max(1);
    let mut done: i64 = 0;
    let mut reported = -1;
    let mut order = missing;
    order.sort_by_key(|f| f.file.size);

    for f in order {
        let base = done;
        ensure_file_exists(f, fast, &mut |dl| {
            reported = ((base as f64 + f.file.size as f64 * (dl as f64 / 100.0)) / total as f64 * 100.0) as i32;
            report(progress, reported, &format!("{} ({dl}%)", f.file.name));
        })?;
        done += f.file.size;
        let p = (done as f64 / total as f64 * 100.0) as i32;
        if reported != p {
            reported = p;
            report(progress, reported, &f.file.name);
        }
    }
    Ok(())
}

/// Checks every planned file against what is on disk, on a few threads. Returns, per file,
/// whether it is already correct (and, for uncached base files, backed up to the cache).
fn verify_in_place(files: &[PlannedFile], fast: bool, progress: Progress) -> Result<Vec<bool>> {
    let total: i64 = files.iter().map(|f| f.file.size.max(0)).sum::<i64>().max(1);
    let checked = AtomicI64::new(0);
    let next = AtomicUsize::new(0);
    let finished = AtomicUsize::new(0);
    let results: Vec<std::sync::Mutex<Option<Result<bool>>>> = files.iter().map(|_| std::sync::Mutex::new(None)).collect();
    let threads = VERIFY_THREADS.min(files.len()).max(1);
    util::ensure_dir(&cache_dir());

    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(f) = files.get(i) else { break };
                    let r = in_place(f, fast);
                    checked.fetch_add(f.file.size.max(0), Ordering::Relaxed);
                    *results[i].lock().unwrap() = Some(r);
                }
                finished.fetch_add(1, Ordering::Relaxed);
            });
        }
        // The progress callback isn't Send, so this thread reports while the workers hash.
        let mut last = -1;
        loop {
            let p = (checked.load(Ordering::Relaxed) as f64 / total as f64 * 100.0) as i32;
            if p != last {
                last = p;
                report(progress, p, "Verifying files");
            }
            if finished.load(Ordering::Relaxed) == threads {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    });

    results.into_iter().map(|r| r.into_inner().unwrap().unwrap_or(Ok(false))).collect()
}

/// Whether the file is already correct at its destination. Downloads nothing.
fn in_place(pf: &PlannedFile, fast: bool) -> Result<bool> {
    let file = &pf.file;
    let ext = util::extension(&file.name);
    if ext == ".wps" {
        return Ok(true);
    }
    // Local sources (snapshots) are copied in step 2; it checks them itself.
    if !file.url.starts_with("http") {
        return Ok(false);
    }
    let use_size = fast && file.size > 0 && ext != ".dat" && ext != ".exe";
    let dest = &pf.destination;
    if !(util::exists(dest) && matches(dest, file, use_size)) {
        return Ok(false);
    }
    // Base files that are not cached by download keep a backup copy in the cache.
    let should_cache = if pf.is_base_file { ext == ".dat" || ext == ".exe" } else { true };
    if !should_cache && pf.entry_type != 1 {
        let cached = combine(&[&cache_dir(), &format!("{}.pfcache", file.guid)]);
        let needs_copy = !util::exists(&cached) || if use_size { util::size(&cached) != util::size(dest) } else { hashes::md5(&cached) != file.md5 };
        if needs_copy {
            util::copy_file(dest, &cached)?;
        }
    }
    Ok(true)
}

fn add_files(entry: &Entry, is_base: bool, files: &mut Vec<PlannedFile>, seen: &mut HashSet<String>, vr: &VirtualRegistry) {
    let dest_dir = entry.destination_directory(vr);
    let lang = &vr[entry.game_idx()].language;
    for file in &entry.files {
        // Windows: the whole path lower-cased, as the original did. Linux: the folders as they exist
        // on disk (any spelling), new parts in lower case, so every package lands in the same folders.
        #[cfg(windows)]
        let destination = combine(&[&dest_dir, &file.name]).to_lowercase().trim_start_matches(['\\', '/']).to_string();
        #[cfg(not(windows))]
        let destination = util::resolve_ci(&combine(&[&dest_dir, &file.name.to_lowercase()]));
        // Lower case, for the case-insensitive comparisons the games expect.
        let key = destination.to_lowercase();
        if IGNORED_GAME_FILES.contains(&util::file_name(&file.name).to_lowercase().as_str()) {
            continue;
        }
        if file.language != "ALL" && !file.language.is_empty() && !file.language.split(' ').any(|l| l == lang) {
            continue;
        }
        if !seen.insert(key.clone()) {
            continue;
        }
        files.push(PlannedFile {
            file: file.clone(),
            entry_game: entry.game_idx(),
            entry_type: entry.r#type,
            is_base_file: is_base,
            destination,
        });
    }
}

/// Download cache, kept with the rest of the launcher data next to the exe.
fn cache_dir() -> String {
    combine(&[&util::data_dir(), "Cache"])
}

fn matches(path: &str, file: &WorkshopFile, use_size: bool) -> bool {
    if use_size { util::size(path) == file.size } else { hashes::md5(path) == file.md5 }
}

fn ensure_file_exists(pf: &PlannedFile, fast: bool, on_progress: &mut dyn FnMut(i32)) -> Result<()> {
    let file = &pf.file;
    let ext = util::extension(&file.name);
    let cache_dir = cache_dir();
    let use_size = fast && file.size > 0 && ext != ".dat" && ext != ".exe";
    let should_cache = if pf.is_base_file { ext == ".dat" || ext == ".exe" } else { true };
    let dest = &pf.destination;
    let cached = combine(&[&cache_dir, &format!("{}.pfcache", file.guid)]);
    let is_mod = pf.entry_type == 1;

    if ext == ".wps" {
        return Ok(());
    }
    util::ensure_dir(&cache_dir);
    util::ensure_dir(&util::dir_name(dest));

    // Local source (snapshots).
    if !file.url.starts_with("http") {
        if !util::exists(&file.url) {
            return Err(FileMissing(format!("The file {} is missing.", file.url)).into());
        }
        let src = file.url.to_lowercase();
        if src.trim_start_matches(['\\', '/']) != dest.to_lowercase().trim_start_matches(['\\', '/']) {
            if util::exists(dest) && matches(dest, file, use_size) {
                return Ok(());
            }
            util::copy_file(&file.url, dest)?;
        }
        return Ok(());
    }

    // Already in place?
    if util::exists(dest) && matches(dest, file, use_size) {
        if !should_cache && !is_mod {
            let needs_copy = !util::exists(&cached)
                || if use_size { util::size(&cached) != util::size(dest) } else { hashes::md5(&cached) != file.md5 };
            if needs_copy {
                util::copy_file(dest, &cached)?;
            }
        }
        return Ok(());
    }

    // Try the caches.
    if !is_mod
        && util::exists(&cached) && matches(&cached, file, use_size) {
            util::copy_file(&cached, dest)?;
            return Ok(());
        }

    let target = if should_cache && !is_mod { cached.clone() } else { dest.clone() };
    crate::http::get_file(&file.url, &target, |p| {
        on_progress(p);
        Ok(())
    })?;
    if should_cache && !is_mod {
        util::copy_file(&cached, dest)?;
    }
    Ok(())
}

/// `keys` holds the lower-cased destination of every planned file.
fn clean_up_files(game: usize, keys: &HashSet<String>, vr: &VirtualRegistry, root: &str) -> Result<()> {
    if !util::dir_exists(root) {
        return Ok(());
    }
    if game != 2
        && !vr[2].game_dir.is_empty()
        && util::dir_exists(&vr[2].game_dir)
        && util::norm_path(root) == util::norm_path(&vr[2].game_dir)
    {
        return Ok(());
    }
    if root.replace('/', "\\").rsplit('\\').next() == Some("BFME Workshop") {
        return Ok(());
    }

    for f in util::list_files(root) {
        let lower = f.to_lowercase();
        if !IGNORED_GAME_FILES.contains(&util::file_name(&lower)) && !keys.contains(&lower) {
            std::fs::remove_file(&f).map_err(|e| anyhow!("Failed to delete {f}: {e}"))?;
        }
    }
    for d in util::list_dirs(root) {
        clean_up_files(game, keys, vr, &d)?;
    }
    if util::list_files(root).is_empty() && util::list_dirs(root).is_empty() {
        let _ = std::fs::remove_dir_all(root);
    }
    Ok(())
}

pub fn kill_game() {
    platform::kill_processes(&["game.dat", "lotrbfme", "lotrbfme2", "lotrbfme2ep1"]);
}

/// `BfmeWorkshopSyncManager.CreateSnapshot`
pub fn create_snapshot(game: usize) -> Result<Entry> {
    let vr = virtual_registry();
    let game_dir = vr[game].game_dir.clone();
    if game_dir.is_empty() || !util::dir_exists(&game_dir) {
        bail!("The game is not installed.");
    }
    let cache_dir = cache_dir();
    let base = base_game(game)?;
    let mut files = vec![];
    util::ensure_dir(&cache_dir);

    fn scan(folder: &str, game: usize, vr: &VirtualRegistry, base: &Entry, cache: &str, out: &mut Vec<WorkshopFile>) -> Result<()> {
        if game == 1
            && !vr[2].game_dir.is_empty()
            && util::dir_exists(&vr[2].game_dir)
            && util::norm_path(folder) == util::norm_path(&vr[2].game_dir)
        {
            return Ok(());
        }
        if folder.replace('/', "\\").rsplit('\\').next() == Some("BFME Workshop") {
            return Ok(());
        }
        for f in util::list_files(folder) {
            // Path relative to the game folder, lower case with `\` like workshop file names.
            let rel = f.get(vr[game].game_dir.len()..).unwrap_or(&f);
            let name = util::trim_seps(&rel.to_lowercase().replace('/', "\\")).to_string();
            let md5 = hashes::md5(&f);
            if base.files.iter().any(|x| x.name.to_lowercase() == name && x.md5 == md5) {
                continue;
            }
            let cached = combine(&[cache, &format!("{md5}.pfcache")]);
            util::copy_file(&f, &cached)?;
            out.push(WorkshopFile {
                guid: md5.clone(),
                name,
                url: cached,
                md5,
                language: "ALL".into(),
                size: util::size(&f),
            });
        }
        for d in util::list_dirs(folder) {
            scan(&d, game, vr, base, cache, out)?;
        }
        Ok(())
    }
    let scanned = scan(&game_dir, game, &vr, &base, &cache_dir, &mut files);
    hashes::save();
    scanned?;

    let now = chrono::Local::now().format("%Y.%m.%d %H:%M:%S").to_string();
    let gname = bfme::GAME_NAMES[game];
    Ok(Entry {
        guid: format!("snapshot-{gname}-{}", uuid::Uuid::new_v4()),
        name: format!("{gname} Snapshot"),
        version: now.clone(),
        description: format!("This snapshot was created on {now}."),
        artwork_url: format!("{FILES_HOST}/snapshot-artwork.png"),
        author: "Me".into(),
        game: game as i32,
        r#type: 4,
        creation_time: util::now_millis(),
        files,
        ..Default::default()
    })
}
