//! Linux only: runs the games (and `regedit`) through Proton or Wine.
//!
//! The default is a small built-in take on what umu-launcher does, written from Valve's published
//! interfaces rather than from umu's code:
//!
//! 1. GE-Proton is downloaded from its GitHub releases (SHA-512 checked) into
//!    `runtime/proton/<version>`.
//! 2. Proton's `toolmanifest.vdf` names the Steam Linux Runtime it was built for; that runtime is
//!    downloaded from repo.steampowered.com (SHA-256 checked) into `runtime/steamrt-<appid>`.
//! 3. Programs start as `<runtime>/_v2-entry-point --verb=waitforexitandrun -- <proton>/proton
//!    waitforexitandrun <program> <args>`, with the `STEAM_COMPAT_*` variables Proton reads.
//!
//! All three games share one prefix in `prefix/` (Proton's compat data folder; the Wine prefix is
//! `prefix/pfx`). A custom Proton folder or the system `wine` can be picked in the settings instead.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

use crate::launch_options::LaunchOptions;
use crate::settings::{RunnerKind, Settings};
use crate::{http, util};

const GE_PROTON_LATEST: &str = "https://api.github.com/repos/GloriousEggroll/proton-ge-custom/releases/latest";

/// Steam Linux Runtimes Proton builds can ask for, by Steam app id (`require_tool_appid`).
struct RuntimeSpec {
    appid: &'static str,
    name: &'static str,
    base: &'static str,
    archive: &'static str,
}

const RUNTIMES: [RuntimeSpec; 2] = [
    RuntimeSpec {
        appid: "1628350",
        name: "Steam Linux Runtime 3.0 (sniper)",
        base: "https://repo.steampowered.com/steamrt-images-sniper/snapshots/latest-container-runtime-depot/",
        archive: "SteamLinuxRuntime_sniper.tar.xz",
    },
    RuntimeSpec {
        appid: "4183110",
        name: "Steam Linux Runtime 4",
        base: "https://repo.steampowered.com/steamrt4/images/latest-container-runtime-depot/",
        archive: "SteamLinuxRuntime_4.tar.xz",
    },
];

/// Proton's compat data folder; the Wine prefix is `pfx` inside it.
pub fn prefix_dir() -> PathBuf {
    Path::new(&util::exe_dir()).join("prefix")
}

fn wine_prefix() -> PathBuf {
    prefix_dir().join("pfx")
}

fn runtime_root() -> PathBuf {
    Path::new(&util::exe_dir()).join("runtime")
}

/// `%APPDATA%` inside the prefix. Proton always runs as `steamuser`; plain Wine uses the login name.
pub fn app_data_dir() -> String {
    let user = if Settings::load().runner == RunnerKind::SystemWine {
        std::env::var("USER").unwrap_or_else(|_| "user".into())
    } else {
        "steamuser".into()
    };
    wine_prefix().join("drive_c/users").join(user).join("AppData/Roaming").to_string_lossy().into_owned()
}

/// The path a Linux file has inside Wine (`/home/a/x` -> `Z:\home\a\x`).
pub fn to_windows_path(p: &str) -> String {
    format!("Z:{}", p.replace('/', "\\"))
}

// ---------------------------------------------------------------------------
// What is installed

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct State {
    /// Managed GE-Proton version (folder name under `runtime/proton`).
    proton: String,
    /// Steam runtime app id -> installed VERSION.txt.
    runtimes: BTreeMap<String, String>,
}

fn state_file() -> PathBuf {
    runtime_root().join("state.json")
}

fn load_state() -> State {
    std::fs::read_to_string(state_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_state(st: &State) -> Result<()> {
    std::fs::create_dir_all(runtime_root())?;
    std::fs::write(state_file(), serde_json::to_string_pretty(st)?)?;
    Ok(())
}

fn managed_proton_dir(st: &State) -> Option<PathBuf> {
    let dir = runtime_root().join("proton").join(&st.proton);
    (!st.proton.is_empty() && dir.join("proton").is_file()).then_some(dir)
}

/// Version of the managed GE-Proton, if it has been downloaded.
pub fn managed_version() -> Option<String> {
    let st = load_state();
    managed_proton_dir(&st).map(|_| st.proton)
}

fn required_runtime(proton: &Path) -> Result<Option<&'static RuntimeSpec>> {
    let manifest = std::fs::read_to_string(proton.join("toolmanifest.vdf")).unwrap_or_default();
    let appid = manifest.lines().find_map(|l| {
        let mut parts = l.split('"').filter(|p| !p.trim().is_empty());
        (parts.next()? == "require_tool_appid").then(|| parts.next().map(str::to_string))?
    });
    match appid {
        None => Ok(None),
        Some(id) => RUNTIMES
            .iter()
            .find(|r| r.appid == id)
            .map(Some)
            .ok_or_else(|| anyhow!("{} needs Steam runtime {id}, which bfme-launcher doesn't know how to download yet.", proton.display())),
    }
}

fn runtime_dir(spec: &RuntimeSpec) -> PathBuf {
    runtime_root().join(format!("steamrt-{}", spec.appid))
}

fn entry_point(dir: &Path) -> Option<PathBuf> {
    let p = dir.join("_v2-entry-point");
    p.is_file().then_some(p)
}

fn which(cmd: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")?.to_str()?.split(':').map(|d| Path::new(d).join(cmd)).find(|p| p.is_file())
}

fn prefix_ready() -> bool {
    wine_prefix().join("system.reg").is_file()
}

enum Tool {
    Wine(PathBuf),
    Proton { proton: PathBuf, runtime: Option<PathBuf> },
}

/// True when everything needed to start a game is already on disk (nothing to download).
pub fn is_ready() -> bool {
    let settings = Settings::load();
    let tool_ready = match settings.runner {
        RunnerKind::SystemWine => which("wine").is_some(),
        RunnerKind::CustomProton => {
            let p = PathBuf::from(&settings.custom_proton_path);
            p.join("proton").is_file() && runtime_present(&p)
        }
        RunnerKind::ManagedProton => managed_proton_dir(&load_state()).is_some_and(|p| runtime_present(&p)),
    };
    tool_ready && prefix_ready()
}

fn runtime_present(proton: &Path) -> bool {
    match required_runtime(proton) {
        Ok(None) => true,
        Ok(Some(spec)) => entry_point(&runtime_dir(spec)).is_some(),
        Err(_) => false,
    }
}

/// Downloads whatever is missing and creates the prefix. Cheap (no network) when all is in place.
pub fn ensure_ready(progress: &dyn Fn(i32, &str)) -> Result<()> {
    let tool = ensure_tool(progress, false)?;
    ensure_prefix(&tool, progress)
}

/// Updates the managed GE-Proton and its runtime to the latest releases. Returns the version.
pub fn update_managed(progress: &dyn Fn(i32, &str)) -> Result<String> {
    let mut st = load_state();
    let proton = install_latest_proton(&mut st, progress, true)?;
    ensure_runtime_for(&proton, progress, true)?;
    let tool = ensure_tool(progress, false)?;
    ensure_prefix(&tool, progress)?;
    Ok(load_state().proton)
}

fn ensure_tool(progress: &dyn Fn(i32, &str), update: bool) -> Result<Tool> {
    let settings = Settings::load();
    match settings.runner {
        RunnerKind::SystemWine => {
            let wine = which("wine").ok_or_else(|| {
                anyhow!("System Wine is selected in the settings, but no `wine` command was found. Install Wine, or switch to GE-Proton.")
            })?;
            Ok(Tool::Wine(wine))
        }
        RunnerKind::CustomProton => {
            let proton = PathBuf::from(&settings.custom_proton_path);
            if !proton.join("proton").is_file() {
                bail!("The custom Proton folder \"{}\" has no `proton` script. Pick a Proton folder in the settings.", proton.display());
            }
            let runtime = ensure_runtime_for(&proton, progress, update)?;
            Ok(Tool::Proton { proton, runtime })
        }
        RunnerKind::ManagedProton => {
            let mut st = load_state();
            let proton = match managed_proton_dir(&st) {
                Some(dir) if !update => dir,
                _ => install_latest_proton(&mut st, progress, update)?,
            };
            let runtime = ensure_runtime_for(&proton, progress, update)?;
            Ok(Tool::Proton { proton, runtime })
        }
    }
}

// ---------------------------------------------------------------------------
// Downloads

fn install_latest_proton(st: &mut State, progress: &dyn Fn(i32, &str), update: bool) -> Result<PathBuf> {
    progress(0, "Looking up the latest GE-Proton");
    let release: serde_json::Value = http::get_json(GE_PROTON_LATEST, &[("Accept", "application/vnd.github+json")])?;
    let tag = release["tag_name"].as_str().ok_or_else(|| anyhow!("unexpected GE-Proton release data"))?.to_string();
    if update && st.proton == tag && let Some(dir) = managed_proton_dir(st) {
        return Ok(dir);
    }
    let assets = release["assets"].as_array().cloned().unwrap_or_default();
    let asset = |pred: &dyn Fn(&str) -> bool| {
        assets.iter().find_map(|a| {
            let name = a["name"].as_str()?;
            if !pred(name) {
                return None;
            }
            Some((name.to_string(), a["browser_download_url"].as_str()?.to_string()))
        })
    };
    // Newer releases name the archive `<tag>-x86_64.tar.gz`, older ones `<tag>.tar.gz`.
    let (archive, url) = asset(&|n| n.ends_with("-x86_64.tar.gz"))
        .or_else(|| asset(&|n| n.ends_with(".tar.gz") && !n.contains("aarch64") && !n.contains("arm64")))
        .ok_or_else(|| anyhow!("{tag} has no x86_64 download"))?;
    let stem = archive.trim_end_matches(".tar.gz");
    let (_, sum_url) = asset(&|n| n == format!("{stem}.sha512sum")).ok_or_else(|| anyhow!("{tag} has no checksum file"))?;
    let expected = first_word(&http::get_string(&sum_url, &[])?);

    let label = format!("Downloading {tag}");
    let file = download(&url, &archive, &label, progress)?;
    verify(&file, &expected, Hash::Sha512)?;
    progress(100, &format!("Unpacking {tag}"));
    let dest = runtime_root().join("proton").join(&tag);
    extract(&file, &dest, Compression::Gzip)?;
    let _ = std::fs::remove_file(&file);

    // Keep only the version in use.
    let old = std::mem::replace(&mut st.proton, tag);
    save_state(st)?;
    if !old.is_empty() && old != st.proton {
        let _ = std::fs::remove_dir_all(runtime_root().join("proton").join(old));
    }
    Ok(dest)
}

fn ensure_runtime_for(proton: &Path, progress: &dyn Fn(i32, &str), update: bool) -> Result<Option<PathBuf>> {
    let Some(spec) = required_runtime(proton)? else { return Ok(None) };
    let dir = runtime_dir(spec);
    if entry_point(&dir).is_some() && !update {
        return Ok(Some(dir));
    }
    progress(0, &format!("Checking {}", spec.name));
    let version = http::get_string(&format!("{}VERSION.txt", spec.base), &[])?.trim().to_string();
    let mut st = load_state();
    if entry_point(&dir).is_some() && st.runtimes.get(spec.appid) == Some(&version) {
        return Ok(Some(dir));
    }
    let sums = http::get_string(&format!("{}SHA256SUMS", spec.base), &[])?;
    let expected = sums
        .lines()
        .find(|l| l.trim_end().ends_with(&format!(" *{}", spec.archive)) || l.trim_end().ends_with(&format!("  {}", spec.archive)))
        .map(first_word)
        .ok_or_else(|| anyhow!("no checksum for {} on the Steam runtime server", spec.archive))?;

    let file = download(&format!("{}{}", spec.base, spec.archive), spec.archive, &format!("Downloading {}", spec.name), progress)?;
    verify(&file, &expected, Hash::Sha256)?;
    progress(100, &format!("Unpacking {}", spec.name));
    extract(&file, &dir, Compression::Xz)?;
    let _ = std::fs::remove_file(&file);
    if entry_point(&dir).is_none() {
        bail!("{} was unpacked but has no _v2-entry-point", spec.name);
    }
    st.runtimes.insert(spec.appid.to_string(), version);
    save_state(&st)?;
    Ok(Some(dir))
}

fn first_word(s: &str) -> String {
    s.split_whitespace().next().unwrap_or("").to_lowercase()
}

fn download(url: &str, name: &str, label: &str, progress: &dyn Fn(i32, &str)) -> Result<PathBuf> {
    let dir = runtime_root().join("downloads");
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(name);
    http::get_file(url, &file.to_string_lossy(), |p| {
        progress(p, label);
        Ok(())
    })?;
    Ok(file)
}

enum Hash {
    Sha256,
    Sha512,
}

fn verify(file: &Path, expected: &str, hash: Hash) -> Result<()> {
    use sha2::Digest;
    fn digest<D: Digest>(mut f: impl Read) -> Result<String> {
        let mut h = D::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
    }
    let f = BufReader::new(std::fs::File::open(file)?);
    let actual = match hash {
        Hash::Sha256 => digest::<sha2::Sha256>(f)?,
        Hash::Sha512 => digest::<sha2::Sha512>(f)?,
    };
    if actual != expected {
        let _ = std::fs::remove_file(file);
        bail!("{} is corrupt (checksum mismatch). Please try again.", file.display());
    }
    Ok(())
}

enum Compression {
    Gzip,
    Xz,
}

/// Unpacks `archive` so that its single top-level folder becomes `dest` (replacing it).
fn extract(archive: &Path, dest: &Path, compression: Compression) -> Result<()> {
    let tmp = dest.with_extension("partial");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    let f = BufReader::new(std::fs::File::open(archive)?);
    let reader: Box<dyn Read> = match compression {
        Compression::Gzip => Box::new(flate2::read::GzDecoder::new(f)),
        Compression::Xz => Box::new(lzma_rust2::XzReader::new(f, true)),
    };
    let mut tar = tar::Archive::new(reader);
    tar.set_preserve_permissions(true);
    tar.set_preserve_mtime(true);
    tar.set_overwrite(true);
    tar.unpack(&tmp).with_context(|| format!("unpacking {}", archive.display()))?;

    let entries: Vec<PathBuf> = std::fs::read_dir(&tmp)?.flatten().map(|e| e.path()).collect();
    let content = match entries.as_slice() {
        [only] if only.is_dir() => only.clone(),
        _ => tmp.clone(),
    };
    let _ = std::fs::remove_dir_all(dest);
    std::fs::rename(&content, dest)?;
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

// ---------------------------------------------------------------------------
// Running programs

fn command(tool: &Tool, program: &str, args: &[String], cwd: &Path, opts: &LaunchOptions) -> Result<Command> {
    let mut argv: Vec<OsString> = opts.wrapper.iter().map(OsString::from).collect();
    let mut env: Vec<(String, OsString)> = vec![];
    match tool {
        Tool::Wine(wine) => {
            argv.push(wine.into());
            env.push(("WINEPREFIX".into(), wine_prefix().into()));
        }
        Tool::Proton { proton, runtime } => {
            if let Some(rt) = runtime {
                argv.push(entry_point(rt).ok_or_else(|| anyhow!("the Steam runtime in {} is incomplete", rt.display()))?.into());
                argv.push("--verb=waitforexitandrun".into());
                argv.push("--".into());
            }
            argv.push(proton.join("proton").into());
            argv.push("waitforexitandrun".into());

            // No Steam client: Proton only looks here for Steam's own libraries.
            let client = prefix_dir().join("steam");
            std::fs::create_dir_all(&client)?;
            let mut tools: Vec<OsString> = vec![proton.into()];
            tools.extend(runtime.iter().map(OsString::from));
            // Folders the runtime container must be able to see besides $HOME.
            let mut mounts: Vec<OsString> = vec![util::exe_dir().into(), util::data_dir().into(), cwd.into()];
            if let Some(parent) = cwd.parent() {
                mounts.push(parent.into());
            }
            env.push(("STEAM_COMPAT_DATA_PATH".into(), prefix_dir().into()));
            env.push(("STEAM_COMPAT_CLIENT_INSTALL_PATH".into(), client.into()));
            env.push(("STEAM_COMPAT_INSTALL_PATH".into(), cwd.into()));
            env.push(("STEAM_COMPAT_TOOL_PATHS".into(), join_paths(&tools)));
            env.push(("STEAM_COMPAT_MOUNTS".into(), join_paths(&mounts)));
            env.push(("STEAM_COMPAT_APP_ID".into(), "0".into()));
            env.push(("SteamAppId".into(), "0".into()));
            env.push(("SteamGameId".into(), "0".into()));
        }
    }
    argv.push(program.into());
    argv.extend(args.iter().map(OsString::from));

    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]).current_dir(cwd);
    for (k, v) in env {
        cmd.env(k, v);
    }
    // The user's own variables win.
    for (k, v) in &opts.env {
        cmd.env(k, v);
    }
    Ok(cmd)
}

fn join_paths(parts: &[OsString]) -> OsString {
    let mut out = OsString::new();
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            out.push(":");
        }
        out.push(p);
    }
    out
}

fn ensure_prefix(tool: &Tool, progress: &dyn Fn(i32, &str)) -> Result<()> {
    if prefix_ready() {
        return Ok(());
    }
    progress(100, "Creating the Wine prefix");
    std::fs::create_dir_all(prefix_dir())?;
    let status = command(tool, "wineboot", &[], &prefix_dir(), &LaunchOptions::default())?
        .status()
        .context("starting Proton/Wine to create the prefix")?;
    if !prefix_ready() {
        bail!("Creating the Wine prefix in {} failed ({status}).", prefix_dir().display());
    }
    Ok(())
}

/// Writes the launcher's game registry into the prefix (`regedit /S`).
fn import_registry(tool: &Tool) -> Result<()> {
    let file = prefix_dir().join("registry.reg");
    std::fs::write(&file, crate::platform::reg::export_reg())?;
    let regedit = wine_prefix().join("drive_c/windows/regedit.exe");
    let program = if regedit.is_file() { regedit.to_string_lossy().into_owned() } else { "regedit".into() };
    let args = vec!["/S".to_string(), to_windows_path(&file.to_string_lossy())];
    let status = command(tool, &program, &args, &prefix_dir(), &LaunchOptions::default())?
        .status()
        .context("starting regedit")?;
    if !status.success() {
        bail!("Importing the game registry into the Wine prefix failed ({status}).");
    }
    Ok(())
}

/// Starts a game: brings the prefix's registry up to date, then runs `exe` with `args` in `cwd`.
/// `ensure_ready` must have succeeded before.
pub fn spawn_game(exe: &str, args: &[String], cwd: &str, opts: &LaunchOptions) -> Result<Child> {
    let tool = ensure_tool(&|_, _| {}, false)?;
    ensure_prefix(&tool, &|_, _| {})?;
    import_registry(&tool)?;
    command(&tool, exe, args, Path::new(cwd), opts)?.spawn().with_context(|| format!("starting {exe}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_paths() {
        assert_eq!(to_windows_path("/home/a/Games/BFME2/"), r"Z:\home\a\Games\BFME2\");
    }

    #[test]
    fn manifest_runtime_lookup() {
        let dir = std::env::temp_dir().join(format!("bfme-runner-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("toolmanifest.vdf"),
            "\"manifest\"\n{\n  \"version\" \"2\"\n  \"commandline\" \"/proton %verb%\"\n  \"require_tool_appid\" \"4183110\"\n}\n",
        )
        .unwrap();
        assert_eq!(required_runtime(&dir).unwrap().map(|r| r.appid), Some("4183110"));
        std::fs::write(dir.join("toolmanifest.vdf"), "\"manifest\"\n{\n  \"require_tool_appid\" \"1\"\n}\n").unwrap();
        assert!(required_runtime(&dir).is_err());
        std::fs::remove_file(dir.join("toolmanifest.vdf")).unwrap();
        assert!(required_runtime(&dir).unwrap().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The whole managed setup for real: downloads GE-Proton and its Steam Linux Runtime
    /// (~750 MB) next to the test binary, creates the prefix, imports a registry value and runs a
    /// Windows program in it. Run by hand: `bfme_launcher-<hash> --ignored --nocapture end_to_end`.
    #[test]
    #[ignore]
    fn end_to_end() {
        use crate::platform::{RegValue, reg};
        let t = std::time::Instant::now();
        let log = |p: i32, s: &str| println!("[{:>6.1}s] {p:>3}% {s}", t.elapsed().as_secs_f32());
        reg::set(r"SOFTWARE\bfme-launcher-test", "Probe", RegValue::Sz("/tmp/probe/".into())).unwrap();
        ensure_ready(&log).unwrap();
        println!("ready in {:.1}s, GE-Proton {:?}", t.elapsed().as_secs_f32(), managed_version());

        let cmd = wine_prefix().join("drive_c/windows/system32/reg.exe");
        let args: Vec<String> = ["query", r"HKLM\SOFTWARE\bfme-launcher-test", "/v", "Probe"].map(String::from).into();
        let status = spawn_game(&cmd.to_string_lossy(), &args, &prefix_dir().to_string_lossy(), &LaunchOptions::default())
            .unwrap()
            .wait()
            .unwrap();
        println!("reg query exited with {status} after {:.1}s", t.elapsed().as_secs_f32());
        let system_reg = std::fs::read_to_string(wine_prefix().join("system.reg")).unwrap();
        assert!(system_reg.to_lowercase().contains("bfme-launcher-test"), "registry value not imported");
        assert!(status.success());
    }
}
