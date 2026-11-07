//! Windows implementation: thin wrappers over the Win32 calls the launcher needs.

use std::mem::{size_of, zeroed};
use std::sync::atomic::{AtomicIsize, Ordering};

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HWND, INVALID_HANDLE_VALUE, RECT};
use windows_sys::Win32::Graphics::Gdi::{DEVMODEW, EnumDisplaySettingsW};
use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{CreateMutexW, OpenProcess, PROCESS_TERMINATE, TerminateProcess};
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, IsIconic, SPI_GETWORKAREA, SW_HIDE, SW_RESTORE, SW_SHOW, SetForegroundWindow, ShowWindow,
    SystemParametersInfoW,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// Kills every process whose image name (without `.exe`) matches one of `names`, case-insensitively.
pub fn kill_processes(names: &[&str]) {
    for (pid, name) in processes() {
        let stem = name.strip_suffix(".exe").or_else(|| name.strip_suffix(".EXE")).unwrap_or(&name);
        if names.iter().any(|n| n.eq_ignore_ascii_case(stem) || n.eq_ignore_ascii_case(&name)) {
            unsafe {
                let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
                if !h.is_null() {
                    TerminateProcess(h, 1);
                    CloseHandle(h);
                }
            }
        }
    }
}

fn processes() -> Vec<(u32, String)> {
    let mut out = vec![];
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut entry: PROCESSENTRY32W = zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                out.push((entry.th32ProcessID, from_wide(&entry.szExeFile)));
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    out
}

/// `SystemDisplayManager.GetAllSupportedResolutions`
pub fn supported_resolutions() -> Vec<String> {
    let mut res: Vec<(u32, u32)> = vec![];
    unsafe {
        let mut dm: DEVMODEW = zeroed();
        dm.dmSize = size_of::<DEVMODEW>() as u16;
        let mut i = 0;
        while EnumDisplaySettingsW(std::ptr::null(), i, &mut dm) != 0 {
            let fixed_output = dm.Anonymous1.Anonymous2.dmDisplayFixedOutput;
            if dm.dmDisplayFrequency == 60 && dm.dmBitsPerPel == 32 && fixed_output == 0 {
                res.push((dm.dmPelsWidth, dm.dmPelsHeight));
            }
            i += 1;
        }
    }
    res.sort();
    res.dedup();
    let skip = res.len().min(3);
    res.into_iter().skip(skip).map(|(w, h)| format!("{w} {h}")).collect()
}

/// Free bytes available on the volume that holds `path` (walks up to the nearest existing folder).
pub fn free_space(path: &str) -> Option<u64> {
    let mut p = std::path::PathBuf::from(path);
    while !p.is_dir() {
        if !p.pop() {
            return None;
        }
    }
    let w = wide(&p.to_string_lossy());
    let mut free: u64 = 0;
    let ok = unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(free)
}

/// Work area of the primary monitor in logical (96 dpi) pixels.
pub fn work_area_logical() -> (f32, f32) {
    unsafe {
        let mut r: RECT = zeroed();
        if SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut r as *mut _ as *mut _, 0) == 0 {
            return (1920.0, 1040.0);
        }
        let dpi = GetDpiForSystem().max(96) as f32 / 96.0;
        ((r.right - r.left) as f32 / dpi, (r.bottom - r.top) as f32 / dpi)
    }
}

/// Holds the single-instance mutex for the lifetime of the process.
pub struct InstanceGuard(#[allow(dead_code)] isize);

/// Returns `None` if another instance already owns the mutex.
pub fn acquire_single_instance(name: &str) -> Option<InstanceGuard> {
    let w = wide(name);
    unsafe {
        let h = CreateMutexW(std::ptr::null(), 1, w.as_ptr());
        if h.is_null() {
            return Some(InstanceGuard(0));
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(h);
            return None;
        }
        Some(InstanceGuard(h as isize))
    }
}

/// Brings the already running launcher (found by its window title) to the front.
pub fn focus_existing_instance(title: &str) {
    let w = wide(title);
    let h = unsafe { FindWindowW(std::ptr::null(), w.as_ptr()) };
    if !h.is_null() {
        show_and_focus(h as isize);
    }
}

fn show_and_focus(hwnd: isize) {
    let h = hwnd as HWND;
    unsafe {
        ShowWindow(h, SW_SHOW);
        if IsIconic(h) != 0 {
            ShowWindow(h, SW_RESTORE);
        }
        SetForegroundWindow(h);
    }
}

/// HWND of the main window, used to hide/show it around game launches.
static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);

pub fn set_main_window(cc: &eframe::CreationContext<'_>) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    if let Ok(h) = cc.window_handle()
        && let RawWindowHandle::Win32(w) = h.as_raw()
    {
        MAIN_HWND.store(w.hwnd.get(), Ordering::SeqCst);
    }
}

pub fn hide_main_window() {
    let hwnd = MAIN_HWND.load(Ordering::SeqCst);
    if hwnd != 0 {
        unsafe {
            ShowWindow(hwnd as HWND, SW_HIDE);
        }
    }
}

pub fn show_main_window() {
    let hwnd = MAIN_HWND.load(Ordering::SeqCst);
    if hwnd != 0 {
        show_and_focus(hwnd);
    }
}

/// The games keep their install info under HKLM, so Windows needs an elevated launcher.
pub fn has_admin() -> bool {
    unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 }
}

pub fn open_url(url: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let (verb, f) = (wide("open"), wide(url));
    unsafe {
        ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), f.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL);
    }
}

pub fn open_folder(path: &str) {
    let _ = std::process::Command::new("explorer.exe").arg(path).spawn();
}

/// Runs the game in Windows XP SP3 compatibility mode, like the original launcher.
pub fn set_compat_layer(game_path: &str) {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_ALL_ACCESS};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(k) = hkcu.open_subkey_with_flags(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers", KEY_ALL_ACCESS) {
        let _ = k.set_value(game_path, &"~ WINXPSP3".to_string());
    }
}

pub fn system_dir() -> String {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    format!("{root}\\System32")
}

pub fn system_x86_dir() -> String {
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
    format!("{root}\\SysWOW64")
}

/// `HKEY_LOCAL_MACHINE` access. Paths are relative to HKLM (e.g. `SOFTWARE\WOW6432Node\...`).
pub mod reg {
    use winreg::RegKey;
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};

    use super::super::RegValue;

    fn hklm() -> RegKey {
        RegKey::predef(HKEY_LOCAL_MACHINE)
    }

    pub fn get_string(path: &str, name: &str) -> Option<String> {
        hklm().open_subkey_with_flags(path, KEY_READ).ok()?.get_value::<String, _>(name).ok()
    }

    pub fn set(path: &str, name: &str, value: RegValue) -> std::io::Result<()> {
        let (k, _) = hklm().create_subkey(path)?;
        match value {
            RegValue::Sz(s) => k.set_value(name, &s),
            RegValue::Dword(d) => k.set_value(name, &d),
        }
    }

    pub fn delete_tree(path: &str) {
        let _ = hklm().delete_subkey_all(path);
    }

    pub fn exists(path: &str) -> bool {
        hklm().open_subkey_with_flags(path, KEY_READ).is_ok()
    }

    /// All values of a key as display strings (used by the workshop script interpreter).
    pub fn values(path: &str) -> Option<Vec<(String, String)>> {
        let k = hklm().open_subkey_with_flags(path, KEY_READ).ok()?;
        Some(k.enum_values().flatten().map(|(name, v)| (name, value_to_string(&v))).collect())
    }

    fn value_to_string(v: &winreg::RegValue) -> String {
        use winreg::enums::RegType::*;
        match v.vtype {
            REG_DWORD => u32::from_le_bytes(v.bytes.get(..4).and_then(|b| b.try_into().ok()).unwrap_or([0; 4])).to_string(),
            REG_QWORD => u64::from_le_bytes(v.bytes.get(..8).and_then(|b| b.try_into().ok()).unwrap_or([0; 8])).to_string(),
            _ => {
                let words: Vec<u16> = v.bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                super::from_wide(&words)
            }
        }
    }
}
