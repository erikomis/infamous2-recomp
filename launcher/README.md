# Launcher (Tauri)

A desktop app that takes the player from their own copy of inFAMOUS 2 to
"Play" without a terminal: it runs the rebuild on the player's machine, shows
its progress, keeps the game's settings and starts the game. It ships no game
code or files; the cover art on the home screen is read from the player's
installed copy (`PS3_GAME/ICON0.PNG`, `PIC1.PNG`).

The game builds and runs on macOS only for now (Metal). The app itself is
built for macOS, Windows and Linux; on Windows and Linux it installs and
opens, shows that building and playing are not supported there yet, and
keeps the Install/Play buttons disabled (see "Other platforms" in the
roadmap).

## What the player does

1. Install the app (`.dmg` on macOS) and open it.
2. **Requirements**: the app lists what the machine still needs
   (`rebuild.py --check-json`) and, with the player's OK, installs it:
   - Xcode Command Line Tools: opens Apple's installer (`xcode-select
     --install`); the screen updates by itself when it finishes.
   - CMake, Ninja, SDL2, glslang, SPIRV-Cross, Python 3.12: `brew install`
     of exactly those formulae, output shown in the window. Without
     Homebrew the app shows the official install command and brew.sh.
   - RPCS3 (decrypts the copy once) and the PS3 firmware inside it: not
     bundled; the app explains and links to rpcs3.net.
3. **Game folder**: pick RPCS3's `NPUA80638` folder (or RPCS3's data
   folder). If the game is installed in RPCS3 it is already filled in. The
   licence is found on its own: the app reads the content id from
   `USRDIR/EBOOT.BIN` and looks for `<content id>.rap` in every RPCS3 user's
   `exdata/` (the default RPCS3 folder and the one the copy sits in), next to
   the copy, and in Downloads, Desktop and Documents. Only when none is
   found does it ask for the `.rap` (under "More options", with PKG files,
   the install folder and the DLC key).
4. **Install**, then **Play** on the Home screen.

## Screens

- **Home / Início**: cover and title from the installed copy, version and
  build date, **Play/Jogar** (and Stop), the most recent save (title, progress,
  date), shortcuts to the saves folder and the game log.
- **Install / Instalar**, three steps: the machine's requirements (with the
  installs above), the game folder (licence found automatically; PKGs, the
  `.rap`, the install folder and the DLC key under "More options"), and the
  rebuild. **Check** runs `rebuild.py --dry-run` (requirements and plan,
  changes nothing); **Install** runs the rebuild with a progress bar, time
  estimate, step list and log; **Cancel** stops it and the tools it started.
- **Controls / Controles**: a table of the PS3 controls with their key,
  mouse button and controller button. Click a cell and press what should
  drive it (a key, a mouse button or the wheel, a controller button, or pick
  the controller input from a list when the web view sees no pad); an input
  bound elsewhere on the same device moves to the new control, × unbinds,
  **Restore defaults** brings back the built-in layout. While the screen is
  open, the rows light up for whatever is held (keys, mouse buttons over the
  table, a controller through the Gamepad API): the button test. Saved at
  once to `~/Library/Application Support/infamous2-recomp/input.toml`, which
  Play passes as `PS3_INPUT_MAP` (format: the main README, "Playing"). Below
  it, the mouse options: camera on/off, sensitivity, vertical and horizontal
  inversion, dead zone, cursor capture (`PS3_MOUSE*`).
- **Settings / Configurações**: game language (`PS3_LANG`), light pass and
  SSAO on the GPU, the occlusion-volume skip, a gamma override, the window
  mode and size (`PS3_WINDOW_MODE`, `PS3_WINDOW_SIZE`), the Dock icon
  (`PS3_WINDOW_ICON=game`), the install folder and extra environment
  variables; shows the equivalent command line. UI in Brazilian Portuguese
  (default) and English.

## How it fits together

```
launcher (Rust, src-tauri/src/lib.rs)
 ├─ Install → python3 infamous2-lle/tools/rebuild.py --game … --rap … --out …
 │            reads its PROGRESS/STEP-*/CHECK/… lines (src/progress.ts)
 └─ Play    → /bin/sh <install>/infamous2-lle/run.sh, settings as env vars
```

- `rebuild.py` writes `<install>/install.json` (paths, version, commits); the
  launcher reads it. The default install folder is
  `~/Library/Application Support/infamous2-recomp/install`, next to the saves
  (`…/infamous2-recomp/gamedata`) that `run.sh` uses for windowed play.
- The install folder can also be the root of a development checkout (it must
  have `infamous2-lle/build/Infamous2Recomp`); Play then runs that tree's
  `run.sh` with its `../infamous2/vfs`.
- `run.sh` keeps every default. A setting only overrides it (`run.sh` uses
  `${VAR-default}`, so an empty value switches a default off).
- Settings live in `~/Library/Application Support/infamous2-recomp/launcher.json`;
  the game's output goes to `…/infamous2-recomp/logs/game.log`.
- The bindings live in `…/infamous2-recomp/input.toml` (`src-tauri/src/input_map.rs`
  reads and writes it). The launcher rewrites only `[keyboard]`,
  `[mouse_buttons]` and `[controller]` and keeps any other section as it was.
  The mouse options stay launcher settings passed as variables, which win
  over the file; if the file has a hand-written `[mouse]`, every mouse
  variable is passed so the screen's values are the ones that apply.
- The optional RAP key field (DLC only) is passed to `rebuild.py` in
  `PS3_RAP_KEY` for that run and never stored.
- A GUI app on macOS starts with a minimal `PATH`; the launcher puts
  `/opt/homebrew/bin` and `/usr/local/bin` in front for the rebuild and the game.

## Development

Needs Node 20+, Rust (stable) and the Xcode Command Line Tools. The rebuild
itself needs what `rebuild.py --dry-run` checks (Python 3.10+, CMake, Ninja,
SDL2, glslang, spirv-cross, RPCS3 with the firmware installed).

```sh
cd launcher
npm install
npm run tauri dev        # the app, with the frontend hot-reloaded
npm run tauri build      # .app and .dmg in src-tauri/target/release/bundle/
npm run typecheck        # frontend only
(cd src-tauri && cargo test)
```

In development the launcher finds `rebuild.py` relative to its own sources
(`../../infamous2-lle/tools/rebuild.py` from `src-tauri`); `INFAMOUS2_REPO=<repo>`
points it elsewhere. A packaged build finds it in its resources
(`port/infamous2-lle/tools/rebuild.py`, see below).
`INFAMOUS2_ANY_HOST=1` lets the launcher and `rebuild.py` try on a system
other than macOS (for work on the other ports).

## Packaging and releases

The bundles carry the port's own sources and tools, never anything from the
game. `scripts/stage_port.py` copies the git-tracked files `rebuild.py` needs
into `launcher/stage/port/` (ignored by git), in the repository's layout:

```
port/BUILD_INFO.json            commit the sources come from (no git in the bundle)
port/LICENSE, CREDITS.md
port/tools/setup_toolkit.sh     fetches and patches the ps3recomp toolkit
port/infamous2-lle/             CMakeLists.txt, main.cpp, ..., run.sh, config.toml
port/infamous2-lle/tools/       the *.py / *.sh rebuild.py copies (rebuild.py itself)
port/infamous2-lle/patches/     the toolkit patch
```

`src-tauri/tauri.bundle.conf.json` maps that folder to the `port/`
resource; it is passed with `--config` only when packaging, so `tauri dev`
and `cargo check` do not need the staging. `scripts/check_bundle.py`
unpacks every bundle (`.app`, `.dmg`, `.zip`, `.deb`, `.AppImage`, `.msi`,
NSIS `.exe`) and fails on licences and keys (`*.rap`, `*.rif`, `act.dat`,
`*.edat`), game code and data (`EBOOT*`, `*.self`, `*.sprx`, `*.prx`,
`*.elf`, `*.pkg`, `*.psarc`, `*.pup`, `PARAM.SFO`, `NPUA80638*`, folders
`dev_hdd0`, `dev_flash`, `PS3_GAME`, `USRDIR`), lifted output (`recompiled/`,
`lle/`, `lle_spu/`, `jobmods/`), SCE/PKG/PSF/PSAR/NPD headers and PowerPC
or SPU ELF files by content, any port file over 4 MB, and a missing
`rebuild.py`.

A local package (macOS):

```sh
python3 launcher/scripts/stage_port.py
cd launcher && npx tauri build --config src-tauri/tauri.bundle.conf.json
python3 scripts/check_bundle.py stage/port src-tauri/target/release/bundle/macos/*.app
```

### CI (`.github/workflows/launcher.yml`)

Runs on pushes to `main` and pull requests that touch `launcher/**`, the
staged port files (`infamous2-lle/*`, `infamous2-lle/tools/**`,
`infamous2-lle/patches/**`, `tools/setup_toolkit.sh`, `LICENSE`,
`CREDITS.md`) or the workflow, on `workflow_dispatch`, and on `v*` tags.

| Job | Runner | Bundles (artifact `infamous2-launcher-<job>`) |
|---|---|---|
| `macos-arm64` | `macos-14` | `infamous2-recomp_<ver>_aarch64.dmg`, `infamous2-recomp_<ver>_aarch64.app.zip` |
| `windows-x64` | `windows-latest` | `infamous2-recomp_<ver>_x64-setup.exe` (NSIS), `infamous2-recomp_<ver>_x64_en-US.msi` |
| `linux-x64` | `ubuntu-22.04` | `infamous2-recomp_<ver>_amd64.AppImage`, `infamous2-recomp_<ver>_amd64.deb` |

Each job stages the port, runs the staged `rebuild.py --check-json` and
`--dry-run` (the host check must fail on Windows/Linux), type-checks and
builds the frontend, runs `cargo test` (macOS, Linux), builds the bundles,
runs `check_bundle.py` on them, writes `SHA256SUMS-<job>.txt` and uploads
everything (kept 30 days). npm and cargo are cached. A `v*` tag also sets
the app version from the tag (`vX.Y.Z`) and the `release` job creates a
**draft** GitHub release with all artifacts attached (or updates the draft
on a rerun); publishing it stays a manual step.

### Signing (optional secrets)

Nothing is required: without secrets macOS gets an ad-hoc signature (the
first open needs right-click > Open, or System Settings > Privacy &
Security > Open Anyway) and the Windows installers are unsigned
(SmartScreen warns). With these repository secrets the workflow signs:

| Secret | Use |
|---|---|
| `APPLE_CERTIFICATE` | Developer ID Application certificate, `.p12` as base64 |
| `APPLE_CERTIFICATE_PASSWORD` | its password |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Name (TEAMID)` |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | notarization (app-specific password); only used together with the certificate |
| `WINDOWS_CERTIFICATE` | code-signing certificate, `.pfx` as base64 |
| `WINDOWS_CERTIFICATE_PASSWORD` | its password |

Only secrets that are set reach the build (Tauri would treat an empty
`APPLE_ID` as a request to notarize).

The icon is an original placeholder (`scripts/make_icon.py`, then
`npx tauri icon src-tauri/icons/app-icon.png`); no Sony art or logo.

## The rebuild script on its own

```sh
python3 infamous2-lle/tools/rebuild.py --dry-run \
    --game "$HOME/Library/Application Support/rpcs3/dev_hdd0/game/NPUA80638" \
    --rap  "$HOME/Library/Application Support/rpcs3/dev_hdd0/home/00000001/exdata/<content id>.rap"
```

`--help` lists the options (`--out`, `--force`, `--from STEP`, `--only STEP,…`,
`--toolkit`/`--venv` to reuse existing ones, `--jobs`, `--nice`).

## Not done yet

- Fullscreen/borderless and resolution: the runtime has no switch for them
  yet (the macOS window's green button gives fullscreen).
- The game still reads its options from environment variables; the plan is a
  `config.toml` the launcher writes and the game reads (see the roadmap).
- Signing: no certificates are configured yet, so macOS builds are ad-hoc
  signed and not notarized, and the Windows installers are unsigned.
- Building and playing on Windows and Linux: the app installs there but the
  game's renderer and window are macOS-only so far.
- Homebrew itself is not installed by the app (it needs the administrator
  password in Terminal); the app shows the command.
