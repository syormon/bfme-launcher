# bfme-launcher

A portable launcher for **The Battle for Middle-earth**, **The Battle for Middle-earth II** and **The Rise of the
Witch-king**, forked from the All-in-One Launcher of the [Bfme Foundation Project](https://github.com/MarcellVokk/BfmeFoundationProject).

It is a single portable executable: no installer, no self-update, no shortcuts, no uninstall entry, no tray icon. Put it in its
own folder and run it.

# Install

1. Grab a binary from the [releases](https://github.com/syormon/bfme-launcher/releases)
2. Run and install

## Linux

The launcher itself is a native Linux program; the games run through Proton:

- **GE-Proton (managed, default).** The first time a game starts (or a patch is installed), the launcher downloads
  the latest [GE-Proton](https://github.com/GloriousEggroll/proton-ge-custom) and the Steam Linux Runtime it was
  built for (about 750 MB together, checksums verified) and runs the game inside that runtime, the way Steam does.
  Nothing has to be installed beforehand and the games need no Steam account. Settings > General > **Update** fetches
  newer releases.
- **Custom Proton.** Any Proton folder, e.g. one installed by Steam or ProtonUp-Qt.
- **System Wine.** The `wine` on your `PATH`.

All three games share one Wine prefix next to the launcher. Nothing needs root: the games' registry entries are kept
by the launcher and imported into the prefix before each start.

Each game has **launch options** in Settings, in Steam's format:

```
PROTON_LOG=1 WINEDLLOVERRIDES="d3d9=n,b" gamemoderun %command% -nologo
```

Leading `NAME=value` words become environment variables, words before `%command%` wrap the game, and words after it
are passed to the game. Without `%command%`, everything that isn't a variable is a game argument. The same field works
on Windows (variables and arguments).

## Building

```bash
cargo build --release
```

- **Windows:** `target/release/bfme-launcher.exe` (embeds the DirectX 9, which is why its ~115MB)
> `--no-default-features` builds a small exe that skips the DirectX install.
- **Linux:** `target/release/bfme-launcher` (no DirectX bundle: Proton and Wine provide it)

## Differences from original

- linux support, with proton baked in (similar to umu-launcher). Defaults to `GE-Proton`
- dropped online/"Arena" all together
- portable launcher, no installer. Everything stays in the same folder as the executable. No tray functionality.
- game downloads and workshop downloads have selectable directories.
- more settings per each game (resolution, windowed, proton arguments, game installation folder)
- faster patch/mod switching. File hashes/checksums are now computed in parallel.
- no self updater

## Architecture

Everything the launcher persists lives next to the executable:

```
bfme-launcher(.exe)
settings.json          launcher language, workshop folder, windowed mode, launch options, Linux runner
data/                  workshop folder (can be moved in Settings > General)
  Config/              active patch / enhancements per game, DirectX flag
  Library/             workshop library
  Keybinds/            per-patch keybind backups
  Cache/               downloaded game files
  DirectX/             extracted DirectX 9 redistributable (Windows)
prefix/                Linux: Wine prefix (pfx/), the games' registry (registry.json)
runtime/               Linux: managed GE-Proton and Steam Linux Runtime
```

Games are installed to a folder chosen in the install dialog; the default is the launcher's folder
(`<launcher>/BFME1`, `<launcher>/BFME2`, `<launcher>/RotWK`). On Windows the only things written elsewhere are what the
games themselves require: their registry keys under `HKLM` and their user-data folders (maps, `Options.ini`) in
`%APPDATA%`. On Linux those live inside `prefix/`.

Both locations can be changed later in the settings: **General > Workshop folder** moves the `data/` contents to
another folder, and each game's **Installation folder** either moves the game into an empty folder or points the
launcher at a folder that already contains that game (files are moved with a rename when possible, copied across
drives otherwise).

Debug builds accept `BFME_LAUNCHER_DEBUG_VIEW=settings|settings-game<0-2>|about` to open a screen directly.

## Code map

| Area | Rust |
|---|---|
| Game registry, Options.ini, defaults (BfmeKit) | `src/bfme.rs` |
| Windows / Linux differences (processes, registry, shell, single instance) | `src/platform/` |
| Linux Proton / Wine runner | `src/runner.rs` |
| Launch options parsing | `src/launch_options.rs` |
| HTTP (`HttpMarshal`) | `src/http.rs` |
| Workshop library, queries, sync engine, `.wps` scripts (WorkshopKit) | `src/workshop/` |
| DirectX 9 runtime install (Windows) | `src/dx.rs` |
| Sync / launch actions, settings | `src/actions.rs`, `src/settings.rs` |
| UI, 13 languages | `src/ui/`, `src/i18n.rs`, `assets/lang/` |

## License and attribution

bfme-launcher's own code is MIT licensed, see [LICENSE](LICENSE).

It is a Rust port of the All-in-One Launcher from the
[Bfme Foundation Project](https://github.com/MarcellVokk/BfmeFoundationProject) by MarcellVokk and contributors. The
workshop sync engine, the game registry handling and the overall launcher flow are translated from their C# code, and
the translations in `assets/lang/` and the images in `assets/images/` and `assets/icon.ico` come from that project.
The original repository has no formal license; its README states that everything in it is open source, and that its
authors take no responsibility for what others do with it. Thanks to them for making it available.

Other bundled material keeps its own terms:

- `assets/dx9_redist.zip` is Microsoft's DirectX 9.0c End-User Runtime, redistributed under Microsoft's terms.
- The Battle for Middle-earth games, their names and artwork belong to their respective owners. This project is not
  affiliated with or endorsed by them, or by Valve or the GE-Proton project, whose software the Linux build downloads
  at runtime under their own licenses.
