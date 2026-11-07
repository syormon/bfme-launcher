//! Everything that differs between operating systems, behind one API:
//!
//! * Windows: Win32 calls and the real registry (`windows.rs`).
//! * Linux: `/proc`, `xdg-open` and a registry that lives in the launcher and is imported into the
//!   Wine prefix before the game starts (`linux.rs`).
//!
//! Both modules export the same functions; callers use `crate::platform::...` and never `cfg`.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

/// Registry value as the games and the original launcher use them.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum RegValue {
    Sz(String),
    Dword(u32),
}

#[cfg_attr(windows, allow(dead_code))]
impl RegValue {
    pub fn to_display(&self) -> String {
        match self {
            RegValue::Sz(s) => s.clone(),
            RegValue::Dword(d) => d.to_string(),
        }
    }
}
