# Launcher (Tauri)

A desktop app that takes the player from their own copy of inFAMOUS 2 to
"Play" without a terminal: it runs the rebuild on the player's machine, shows
its progress, keeps the game's settings and starts the game. It ships no game
code or files; the cover art on the home screen is read from the player's
installed copy (`PS3_GAME/ICON0.PNG`, `PIC1.PNG`).

macOS first (Metal). The same interface is meant for Windows and Linux.

## Screens

- **Home / Início**: cover and title from the installed copy, version and
  build date, **Play/Jogar** (and Stop), the most recent save (title, progress,
  date), shortcuts to the saves folder and the game log.
- **Install / Instalar**: the game copy (RPCS3's `NPUA80638` folder, or PKGs),
  the `.rap` licence and the install folder; "Find in RPCS3" fills them in.
  **Check** runs `rebuild.py --dry-run` (requirements and plan, changes
  nothing); **Install** runs the rebuild with a progress bar, time estimate,
  step list and log; **Cancel** stops it and the tools it started.
- **Settings / Configurações**: game language (`PS3_LANG`), light pass and
  SSAO on the GPU, the occlusion-volume skip, a gamma override, the window
  mode and size (`PS3_WINDOW_MODE`, `PS3_WINDOW_SIZE`), the Dock icon
  (`PS3_WINDOW_ICON=game`), mouse options (cursor capture: `PS3_MOUSE_CAPTURE`),
  the install folder and extra environment variables; shows the equivalent
  command line. UI in Brazilian Portuguese (default) and English.

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
points it elsewhere. A packaged build will need the port sources bundled as
Tauri resources (`port/infamous2-lle/...`, already looked up there).

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
- Packaging: bundling the port sources as resources, code signing, Windows
  and Linux builds.
