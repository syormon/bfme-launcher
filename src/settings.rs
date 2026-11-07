//! Launcher settings, stored as `settings.json` next to the executable.

use serde::{Deserialize, Serialize};

use crate::util::{self, combine};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub launcher_language: usize,
    /// Custom workshop folder; empty = `<exe dir>/data`.
    pub workshop_dir: String,
    /// Start BFME1 / BFME2 / RotWK in a window (`-win`) instead of fullscreen.
    pub windowed: [bool; 3],
    /// Per-game launch options, Steam style: `VAR=value wrapper %command% -extra -args`.
    pub launch_options: [String; 3],
    /// Linux: what runs the games.
    pub runner: RunnerKind,
    /// Linux: Proton folder used when `runner` is `CustomProton`.
    pub custom_proton_path: String,
}

/// How the games are run on Linux.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(windows, allow(dead_code))]
pub enum RunnerKind {
    /// GE-Proton downloaded and kept by the launcher, inside Valve's Steam Linux Runtime.
    #[default]
    ManagedProton,
    /// A Proton folder the user picked (e.g. one installed by Steam or ProtonUp-Qt).
    CustomProton,
    /// The `wine` on the PATH.
    SystemWine,
}

#[cfg_attr(windows, allow(dead_code))]
impl RunnerKind {
    pub const ALL: [RunnerKind; 3] = [RunnerKind::ManagedProton, RunnerKind::CustomProton, RunnerKind::SystemWine];

    pub fn label(self) -> &'static str {
        match self {
            RunnerKind::ManagedProton => "GE-Proton (managed)",
            RunnerKind::CustomProton => "Custom Proton",
            RunnerKind::SystemWine => "System Wine",
        }
    }
}

fn file() -> String {
    combine(&[&util::exe_dir(), "settings.json"])
}

impl Settings {
    pub fn load() -> Self {
        util::read_json(&file()).unwrap_or_default()
    }

    pub fn save(&self) {
        util::write_json(&file(), self);
    }
}
