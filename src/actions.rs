//! Launcher-level operations (port of the launcher's `BfmeSyncManager` and `BfmeLaunchManager`).
//! Everything here runs on a worker thread and reports back through [`crate::hub`].

use std::sync::{Arc, Mutex};

use crate::hub::{self, Busy, UiEvent};
use crate::settings::Settings;
use crate::util::combine;
use crate::workshop::{self, sync};
use crate::{bfme, launch_options, platform};

#[derive(Default)]
pub struct ProgressShared {
    pub progress: i32,
    pub status: String,
    pub done: bool,
}

pub type ProgressHandle = Arc<Mutex<ProgressShared>>;

pub fn spawn(f: impl FnOnce() + Send + 'static) {
    std::thread::spawn(f);
}

fn report_error(e: &anyhow::Error) {
    if let Some(x) = e.downcast_ref::<sync::EnhancementIncompatible>() {
        hub::send(UiEvent::Message { title: "COMPATIBILITY ERROR".into(), message: x.0.clone() });
    } else {
        hub::send(UiEvent::Error(hub::error_text(e)));
    }
}

pub fn sync_package(guid: &str, fast: bool, enhancements: Option<Vec<String>>, progress: Option<ProgressHandle>) {
    let _busy = Busy::new();
    let package = match workshop::get_package(guid) {
        Ok(p) => p,
        Err(e) => return report_error(&e),
    };
    let cb = progress.clone().map(|h| {
        move |p: i32, s: &str| {
            let mut g = h.lock().unwrap();
            g.progress = p;
            g.status = s.to_string();
            hub::repaint();
        }
    });
    let cb_ref: sync::Progress = cb.as_ref().map(|c| c as &dyn Fn(i32, &str));
    if let Err(e) = sync::sync(&package, fast, enhancements, cb_ref) {
        if e.downcast_ref::<sync::ScriptMissingRequirements>().is_some() {
            hub::send(UiEvent::ScriptedPackage(Box::new(package)));
        } else {
            report_error(&e);
        }
    }
    hub::send(UiEvent::LibraryChanged);
}

fn toggle_enhancement(guid: &str, enable: bool) {
    let _busy = Busy::new();
    let result = (|| -> anyhow::Result<()> {
        let package = workshop::get_package(guid)?;
        let game = package.game_idx();
        let active = workshop::active_enhancements(game);
        let patch = match workshop::active_patch(game) {
            Some(p) => p,
            None => workshop::base_game(game)?,
        };
        let mut list: Vec<String> = vec![];
        for e in active.values() {
            if e.guid.split(':').next() != Some(package.guid.as_str()) && !list.contains(&e.guid) {
                list.push(e.guid.clone());
            }
        }
        if enable {
            list.push(guid.to_string());
        }
        sync_package(&patch.guid, true, Some(list), None);
        Ok(())
    })();
    if let Err(e) = result {
        report_error(&e);
    }
}

pub fn enable_enhancement(guid: &str) {
    toggle_enhancement(guid, true);
}

pub fn disable_enhancement(guid: &str) {
    toggle_enhancement(guid, false);
}

pub fn install_game(game: usize, language: &str, location: &str) {
    let _busy = Busy::new();
    let result = (|| -> anyhow::Result<()> {
        bfme::create_new_install_registry(game, &combine(&[location, bfme::GAME_NAMES[game]]), language)?;
        if game == 2 && !bfme::is_installed(1) {
            bfme::create_new_install_registry(1, &combine(&[location, "BFME2"]), language)?;
        }
        sync::sync(&workshop::base_game(game)?, true, None, None)
    })();
    if let Err(e) = result {
        report_error(&e);
    }
    hub::send(UiEvent::LibraryChanged);
}

fn progress_cb(h: &ProgressHandle, status: &str) -> impl FnMut(i32) {
    let h = h.clone();
    let status = status.to_string();
    move |p| {
        let mut g = h.lock().unwrap();
        g.progress = p;
        g.status = status.clone();
        drop(g);
        hub::repaint();
    }
}

/// Moves a game to `to` (or just re-points it there when `to` already holds the game),
/// then updates its registry install path.
pub fn move_game(game: usize, to: &str, progress: ProgressHandle) {
    let _busy = Busy::new();
    let result = (|| -> anyhow::Result<()> {
        let from = crate::util::trim_end_seps(&bfme::install_path(game)).to_string();
        let to = to.trim().trim_end_matches(['\\', '/']).to_string();
        let exe = bfme::DEFAULT_GAME_EXECUTABLES[game];
        if !crate::util::exists(&combine(&[&to, exe])) {
            sync::kill_game();
            crate::util::move_dir(&from, &to, &mut progress_cb(&progress, "Moving game files"))
                .map_err(|e| anyhow::anyhow!("Failed to move {from} to {to}: {e}"))?;
        }
        bfme::set(game, bfme::Key::InstallPath, &format!("{to}{}", crate::util::SEP))?;
        bfme::ensure_defaults(game);
        Ok(())
    })();
    if let Err(e) = result {
        report_error(&e);
    }
    progress.lock().unwrap().done = true;
    hub::send(UiEvent::LibraryChanged);
}

/// Moves the workshop folder (library, patch state, caches) to `to`; empty `to` = default location.
pub fn move_workshop(to: &str, progress: ProgressHandle) {
    let _busy = Busy::new();
    let from = crate::util::data_dir();
    let target = if to.is_empty() { crate::util::default_data_dir() } else { to.to_string() };
    match crate::util::move_dir(&from, &target, &mut progress_cb(&progress, "Moving workshop files")) {
        Ok(()) => {
            crate::util::set_data_dir(to);
            hub::send(UiEvent::WorkshopMoved(to.to_string()));
        }
        Err(e) => hub::send(UiEvent::Error(format!("Failed to move {from} to {target}: {e}"))),
    }
    progress.lock().unwrap().done = true;
    hub::send(UiEvent::LibraryChanged);
}

/// Resyncs the active patch after a game language change (`Settings.*NeedsResync`).
pub fn resync_language(game: usize, progress: ProgressHandle) {
    let ok = if game == 2 { bfme::is_installed(1) && bfme::is_installed(2) } else { bfme::is_installed(game) };
    if ok
        && let Some(active) = workshop::active_patch(game) {
            sync_package(&format!("{}:{}", active.guid, active.version), true, None, Some(progress.clone()));
        }
    progress.lock().unwrap().done = true;
    hub::repaint();
}

pub fn launch_game(game: usize) {
    let install = bfme::install_path(game);
    let exe = combine(&[&install, bfme::DEFAULT_GAME_EXECUTABLES[game]]);
    let settings = Settings::load();
    let opts = launch_options::parse(&settings.launch_options[game]);

    let mut args: Vec<String> = vec![];
    let mod_path = workshop::active_mod_path(game);
    if !mod_path.is_empty() {
        args.push("-mod".into());
        args.push(game_path(&mod_path));
    }
    if settings.windowed[game] {
        args.push("-win".into());
    }
    args.extend(opts.args.iter().cloned());
    bfme::ensure_compatibility_settings(&exe);
    bfme::ensure_compatibility_settings(&combine(&[&install, "game.dat"]));

    match start_game(&exe, &args, &install, &opts) {
        Ok(mut child) => {
            platform::hide_main_window();
            let _ = child.wait();
            platform::show_main_window();
            hub::send(UiEvent::GameExited);
        }
        Err(e) => hub::send(UiEvent::Error(format!("Failed to start {exe}\n{e:#}"))),
    }
}

/// A path as the game sees it (Linux paths become `Z:\...` inside Wine).
fn game_path(p: &str) -> String {
    #[cfg(target_os = "linux")]
    if p.starts_with('/') {
        return crate::runner::to_windows_path(p);
    }
    p.to_string()
}

#[cfg(windows)]
fn start_game(exe: &str, args: &[String], cwd: &str, opts: &launch_options::LaunchOptions) -> anyhow::Result<std::process::Child> {
    let mut cmd = match opts.wrapper.split_first() {
        Some((wrapper, rest)) => {
            let mut c = std::process::Command::new(wrapper);
            c.args(rest).arg(exe);
            c
        }
        None => std::process::Command::new(exe),
    };
    cmd.args(args).current_dir(cwd);
    for (k, v) in &opts.env {
        cmd.env(k, v);
    }
    Ok(cmd.spawn()?)
}

#[cfg(target_os = "linux")]
fn start_game(exe: &str, args: &[String], cwd: &str, opts: &launch_options::LaunchOptions) -> anyhow::Result<std::process::Child> {
    {
        let _busy = Busy::new();
        prepare_runner("PREPARING PROTON", "Downloading what is needed to run the game. This only happens once.")?;
    }
    crate::runner::spawn_game(exe, args, cwd, opts)
}

/// Linux: downloads Proton / the Steam runtime / creates the prefix if needed, showing a progress
/// popup only when there is actually something to do.
#[cfg(target_os = "linux")]
pub fn prepare_runner(title: &str, message: &str) -> anyhow::Result<()> {
    if crate::runner::is_ready() {
        return Ok(());
    }
    let handle = progress_popup(title, message);
    let result = crate::runner::ensure_ready(&progress_reporter(&handle));
    handle.lock().unwrap().done = true;
    hub::repaint();
    result
}

/// Linux: "Update" in the settings: newest GE-Proton and Steam runtime.
#[cfg(target_os = "linux")]
pub fn update_proton() {
    let _busy = Busy::new();
    let handle = progress_popup("UPDATING GE-PROTON", "Checking for a newer GE-Proton and Steam Linux Runtime.");
    let result = crate::runner::update_managed(&progress_reporter(&handle));
    handle.lock().unwrap().done = true;
    match result {
        Ok(v) => hub::send(UiEvent::Message { title: "GE-PROTON IS UP TO DATE".into(), message: format!("Using {v}.") }),
        Err(e) => hub::send(UiEvent::Error(format!("{e:#}"))),
    }
}

#[cfg(target_os = "linux")]
fn progress_popup(title: &str, message: &str) -> ProgressHandle {
    let handle: ProgressHandle = Arc::new(Mutex::new(ProgressShared::default()));
    hub::send(UiEvent::Progress { title: title.into(), message: message.into(), handle: handle.clone() });
    handle
}

#[cfg(target_os = "linux")]
fn progress_reporter(handle: &ProgressHandle) -> impl Fn(i32, &str) + use<> {
    let h = handle.clone();
    move |p, s| {
        let mut g = h.lock().unwrap();
        g.progress = p.clamp(0, 100);
        g.status = s.to_string();
        drop(g);
        hub::repaint();
    }
}
