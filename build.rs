fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/app.manifest");
    println!("cargo:rerun-if-changed=assets/app.debug.manifest");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // Release builds require elevation (the games keep their install info under HKLM),
        // debug builds run as invoker so they can be started from a normal shell.
        let manifest = if std::env::var("PROFILE").as_deref() == Ok("release") {
            "assets/app.manifest"
        } else {
            "assets/app.debug.manifest"
        };
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "BFME Launcher");
        res.set("FileDescription", "BFME Launcher");
        res.set_manifest_file(manifest);
        if let Err(e) = res.compile() {
            println!("cargo:warning=failed to embed Windows resources: {e}");
        }
    }
}
