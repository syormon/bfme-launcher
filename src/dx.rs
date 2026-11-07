//! Port of `DirectXRuntimeManager`: installs the DirectX 9 runtime the games need.

use crate::util::{self, combine, data_dir};
use crate::platform as win32;

const DX_RUNTIME_VERSION: &str = "v1";

#[cfg(feature = "bundle-dx9")]
static DX9_REDIST: &[u8] = include_bytes!("../assets/dx9_redist.zip");

fn has_d3dx9(dir: &str) -> bool {
    util::exists(&combine(&[dir, "D3DX9_43.dll"])) || util::exists(&combine(&[dir, "d3dx9_43.dll"]))
}

pub fn ensure_runtimes() {
    let config_dir = combine(&[&data_dir(), "Config"]);
    let dx_dir = combine(&[&data_dir(), "DirectX"]);
    util::ensure_dir(&config_dir);
    util::ensure_dir(&dx_dir);

    let flag = combine(&[&config_dir, "dx_version.flag"]);
    let mut install = true;
    if util::exists(&flag) {
        let text = util::read_text(&flag, "");
        let v = text.split('\n').next().unwrap_or("");
        if v.is_empty() || v == DX_RUNTIME_VERSION {
            install = false;
        }
    }
    if !has_d3dx9(&win32::system_dir()) || !has_d3dx9(&win32::system_x86_dir()) {
        install = true;
    }

    if install {
        install_runtime(&dx_dir);
    }
    util::write_text(&flag, &format!("{DX_RUNTIME_VERSION}\n0"));
}

#[cfg(feature = "bundle-dx9")]
fn install_runtime(dx_dir: &str) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    for dir in [win32::system_dir(), win32::system_x86_dir()] {
        let _ = std::fs::remove_file(combine(&[&dir, "D3DX9_43.dll"]));
        let _ = std::fs::remove_file(combine(&[&dir, "d3dx9_43.dll"]));
    }

    let out_dir = combine(&[dx_dir, "dx9_redist"]);
    let cursor = std::io::Cursor::new(DX9_REDIST);
    match zip::ZipArchive::new(cursor) {
        Ok(mut archive) => {
            if let Err(e) = archive.extract(&out_dir) {
                eprintln!("failed to extract dx9 redist: {e}");
                return;
            }
        }
        Err(e) => {
            eprintln!("invalid dx9 redist archive: {e}");
            return;
        }
    }
    let setup = combine(&[&out_dir, "DXSETUP.exe"]);
    if util::exists(&setup) {
        let _ = std::process::Command::new(setup).arg("/silent").creation_flags(CREATE_NO_WINDOW).status();
    }
}

#[cfg(not(feature = "bundle-dx9"))]
fn install_runtime(_dx_dir: &str) {
    eprintln!("DirectX 9 runtime missing; this build was compiled without the bundled redistributable.");
}
