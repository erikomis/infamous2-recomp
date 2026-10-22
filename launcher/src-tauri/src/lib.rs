//! Launcher core: settings, the rebuild (infamous2-lle/tools/rebuild.py) as a
//! child process whose output is streamed to the window, and "Play", which
//! starts the port's own run.sh with the settings mapped onto its environment
//! variables. No game code or data is part of the app: everything it shows or
//! starts comes from the player's own install folder.

mod app_icon;
mod input_map;
mod report;
mod saves;
mod trophies;
mod update;
mod zipfile;

use input_map::{Bindings, InputEnv};
use saves::{read_sfo, BackupMeta, Side, Slot};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::UNIX_EPOCH;
use tauri::{AppHandle, Emitter, Manager, State};

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

/// What the player can change. Each field maps onto one of the environment
/// variables `infamous2-lle/run.sh` and the runtime read (see `launch_env`).
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Launcher UI language: "pt-BR" or "en".
    pub ui_lang: String,
    /// Folder holding install.json (written by rebuild.py), or the root of a
    /// development checkout (infamous2-lle/build/Infamous2Recomp).
    pub install_dir: String,
    /// PS3_LANG: dub, subtitles and menus. "pt" selects the Brazilian files.
    pub game_lang: String,
    /// PS3_PATCHES: community patches by name, read from the player's own
    /// RPCS3 patch.yml (game_patches.cpp).
    pub patches: Vec<String>,
    /// PS3RECOMP_ENG_GPU_LIGHT: the SPU light pass on the GPU.
    pub gpu_light: bool,
    /// PS3RECOMP_ENG_GPU_SSAO: the SSAO job on the GPU (needs gpu_light).
    pub gpu_ssao: bool,
    /// PS3RECOMP_ENG_SKIP_FP: skip the occlusion-volume passes that black out facades.
    pub skip_occlusion: bool,
    /// PS3_DISPLAY_GAMMA when set; otherwise the game's own brightness setting.
    pub gamma_override: bool,
    pub gamma: f32,
    /// PS3_ANISO: anisotropic filtering, 0 (off), 2, 4, 8 or 16.
    pub aniso: u32,
    /// PS3_AA: "" (off), "fxaa" or "smaa".
    pub aa: String,
    /// PS3_SHARPEN: contrast-adaptive sharpening, 0 (off) to 1.
    pub sharpen: f32,
    /// PS3_VIBRANCE: -1 (muted) .. 0 (off) .. 1 (vivid).
    pub vibrance: f32,
    /// PS3_CONTRAST: 0.5 .. 1 (off) .. 1.5.
    pub contrast: f32,
    /// PS3_VIGNETTE and PS3_GRAIN: 0 (off) .. 1.
    pub vignette: f32,
    pub grain: f32,
    /// PS3_LUT: the player's colour LUT (ReShade's PNG layout), and how much
    /// of it (PS3_LUT_STRENGTH, 0..1).
    pub lut_path: String,
    pub lut_strength: f32,
    /// PS3_UPSCALE: "" (off) or "metalfx" (macOS 13+).
    pub upscale: String,
    /// PS3_MOUSE, PS3_MOUSE_SENS, PS3_MOUSE_INVERT_Y, PS3_MOUSE_INVERT_X,
    /// PS3_MOUSE_DEADZONE.
    pub mouse: bool,
    pub mouse_sens: f32,
    pub mouse_invert_y: bool,
    pub mouse_invert_x: bool,
    pub mouse_deadzone: u32,
    /// PS3_MOUSE_CAPTURE: a click captures the cursor (Esc/Cmd releases it).
    pub mouse_capture: bool,
    /// PS3_WINDOW_MODE: "windowed", "fullscreen" (macOS) or "borderless".
    pub window_mode: String,
    /// PS3_WINDOW_SIZE: "<w>x<h>" in points; empty = the game's 1280x720.
    pub window_size: String,
    /// The player's own ICON0.PNG in the Dock instead of the placeholder:
    /// the game's (PS3_WINDOW_ICON) and the launcher's own (app_icon.rs).
    pub game_icon: bool,
    /// PS3_DISPLAY: the screen's name (or its 1-based number when two share
    /// a name); empty = the main screen.
    pub display: String,
    /// PS3_VSYNC=0 when off.
    pub vsync: bool,
    /// PS3_AUDIO_VOLUME: master volume, 0-100.
    pub audio_volume: u32,
    /// PS3_AUDIO_DEVICE: output device name; empty = the system default.
    pub audio_device: String,
    /// PS3_RUMBLE=0 when off; PS3_RUMBLE_STRENGTH in percent (0-200).
    pub rumble: bool,
    pub rumble_strength: u32,
    /// PS3RECOMP_METAL_FPS=1: display and game fps in the window title.
    pub show_fps: bool,
    /// Extra KEY=VALUE lines (diagnostics), one per line; '#' starts a comment.
    pub extra_env: String,
    /// Remembered inputs of the install screen.
    pub game_paths: Vec<String>,
    pub rap_path: String,
    /// Decryption without RPCS3: the player's own key file (rebuild.py
    /// --keys), the firmware (--firmware: a dev_flash folder or
    /// PS3UPDAT.PUP; empty = RPCS3's) and the method (--decrypt-with).
    pub keys_path: String,
    pub firmware_path: String,
    pub decrypt_with: String,
    /// Automatic backups of the saves made before each "Play" that are kept
    /// (the newest ones); 0 turns them off.
    pub save_backups_keep: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            ui_lang: "pt-BR".into(),
            install_dir: default_install_dir().to_string_lossy().into_owned(),
            game_lang: "pt".into(),
            patches: Vec::new(),
            gpu_light: true,
            gpu_ssao: true,
            skip_occlusion: true,
            gamma_override: false,
            gamma: 1.0,
            aniso: 0,
            aa: String::new(),
            sharpen: 0.0,
            vibrance: 0.0,
            contrast: 1.0,
            vignette: 0.0,
            grain: 0.0,
            lut_path: String::new(),
            lut_strength: 1.0,
            upscale: String::new(),
            mouse: true,
            mouse_sens: 1.0,
            mouse_invert_y: false,
            mouse_invert_x: false,
            mouse_deadzone: 30,
            mouse_capture: true,
            window_mode: "windowed".into(),
            window_size: String::new(),
            game_icon: false,
            display: String::new(),
            vsync: true,
            audio_volume: 100,
            audio_device: String::new(),
            rumble: true,
            rumble_strength: 100,
            show_fps: false,
            extra_env: String::new(),
            game_paths: Vec::new(),
            rap_path: String::new(),
            keys_path: String::new(),
            firmware_path: String::new(),
            decrypt_with: "auto".into(),
            save_backups_keep: 10,
        }
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Where the port keeps its data on each system (the same folder run.sh uses
/// for saves: <data>/gamedata).
fn data_root() -> PathBuf {
    if cfg!(target_os = "macos") {
        home().join("Library/Application Support/infamous2-recomp")
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(home)
            .join("infamous2-recomp")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/share"))
            .join("infamous2-recomp")
    }
}

fn default_install_dir() -> PathBuf {
    data_root().join("install")
}

fn rpcs3_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        home().join("Library/Application Support/rpcs3")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".config"))
            .join("rpcs3")
    }
}

fn settings_path() -> PathBuf {
    data_root().join("launcher.json")
}

/// The controls file (PS3_INPUT_MAP), next to launcher.json; run.sh points
/// windowed runs at the same file.
fn input_map_path() -> PathBuf {
    data_root().join("input.toml")
}

/// "Report a problem" folders (report.rs).
fn reports_dir() -> PathBuf {
    data_root().join("reports")
}

/// The controls file as "Play" passes it (see `launch_env`).
fn input_env() -> InputEnv {
    let p = input_map_path();
    match fs::read_to_string(&p) {
        Ok(text) => InputEnv {
            path: Some(p.to_string_lossy().into_owned()),
            has_mouse_section: input_map::parse(&text).has_mouse_section,
        },
        Err(_) => InputEnv::default(),
    }
}

fn load_settings_file() -> Settings {
    fs::read_to_string(settings_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// The install
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
struct Manifest {
    title: String,
    app_ver: String,
    port_dir: String,
    run_sh: String,
    executable: String,
    vfs: String,
    dev_flash: String,
    icon: String,
    background: String,
    toolkit_commit: String,
    port_commit: String,
    built_at: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstallInfo {
    /// "install" (rebuild.py output) or "dev" (a development checkout).
    layout: String,
    title: String,
    app_ver: String,
    port_dir: String,
    run_sh: String,
    executable: String,
    vfs: String,
    dev_flash: String,
    icon: String,
    background: String,
    built_at: String,
    port_commit: String,
    toolkit_commit: String,
}

fn exe_name() -> &'static str {
    if cfg!(windows) {
        "Infamous2Recomp.exe"
    } else {
        "Infamous2Recomp"
    }
}

/// The game a development checkout built. On macOS that is the binary inside
/// build/Infamous2Recomp.app (the bundle is what lets Game Mode turn on;
/// run.sh starts it directly), with build/Infamous2Recomp a symlink to it;
/// a build from before the bundle has only the plain executable.
fn dev_executable(build: &Path) -> PathBuf {
    let app = build.join("Infamous2Recomp.app/Contents/MacOS").join(exe_name());
    if cfg!(target_os = "macos") && app.is_file() {
        app
    } else {
        build.join(exe_name())
    }
}

/// Read install.json, or recognise a development checkout (repo root or its
/// infamous2-lle folder) so a tree built by hand can be played too.
fn resolve_install(dir: &Path) -> Result<InstallInfo, String> {
    let manifest = dir.join("install.json");
    if manifest.is_file() {
        let m: Manifest = serde_json::from_str(
            &fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?,
        )
        .map_err(|e| format!("{}: {e}", manifest.display()))?;
        return Ok(InstallInfo {
            layout: "install".into(),
            title: m.title,
            app_ver: m.app_ver,
            port_dir: m.port_dir,
            run_sh: m.run_sh,
            executable: m.executable,
            vfs: m.vfs,
            dev_flash: m.dev_flash,
            icon: m.icon,
            background: m.background,
            built_at: m.built_at,
            port_commit: m.port_commit,
            toolkit_commit: m.toolkit_commit,
        });
    }
    let root = if dir.join("infamous2-lle").is_dir() {
        dir.to_path_buf()
    } else if dir.file_name().map_or(false, |n| n == "infamous2-lle") {
        dir.parent().map(Path::to_path_buf).unwrap_or_default()
    } else {
        return Err(format!("{}: no install.json", dir.display()));
    };
    let port = root.join("infamous2-lle");
    let exe = dev_executable(&port.join("build"));
    if !exe.is_file() {
        return Err(format!("{}: not built yet", exe.display()));
    }
    let vfs = root.join("infamous2/vfs");
    let title = vfs.join("PS3_GAME");
    let sfo = read_sfo(&title.join("PARAM.SFO")).unwrap_or_default();
    let s = |p: PathBuf| p.to_string_lossy().into_owned();
    Ok(InstallInfo {
        layout: "dev".into(),
        title: sfo.get("TITLE").cloned().unwrap_or_default(),
        app_ver: sfo.get("APP_VER").cloned().unwrap_or_default(),
        run_sh: s(port.join("run.sh")),
        port_dir: s(port),
        executable: s(exe),
        dev_flash: s(rpcs3_dir().join("dev_flash")),
        icon: s(title.join("ICON0.PNG")),
        background: s(title.join("PIC1.PNG")),
        vfs: s(vfs),
        built_at: String::new(),
        port_commit: String::new(),
        toolkit_commit: String::new(),
    })
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SaveInfo {
    title: String,
    subtitle: String,
    detail: String,
    /// Seconds since the epoch.
    modified: u64,
    folder: String,
}

/// The port's gamedata: PS3_GAMEDATA_ROOT from the extra variables, else
/// the folder run.sh points windowed play at.
fn gamedata_root(extra_env: &str) -> PathBuf {
    parse_env_lines(extra_env)
        .into_iter()
        .find(|(k, _)| k == "PS3_GAMEDATA_ROOT")
        .map(|(_, v)| PathBuf::from(v))
        .unwrap_or_else(|| data_root().join("gamedata"))
}

/// Where the saves manager works: the port's gamedata, RPCS3's users and
/// `<data>/save-backups`.
fn save_locations(s: &Settings) -> saves::Locations {
    saves::Locations {
        port_root: gamedata_root(&s.extra_env),
        rpcs3_home: rpcs3_dir().join("dev_hdd0/home"),
        backups: data_root().join("save-backups"),
    }
}

/// The most recent save in the port's gamedata.
fn last_save(s: &Settings) -> Option<SaveInfo> {
    saves::list_slots(&save_locations(s), Side::Port, false)
        .into_iter()
        .max_by_key(|x| x.modified)
        .map(|x| SaveInfo { title: x.title, subtitle: x.subtitle, detail: x.detail, modified: x.modified, folder: x.path })
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn data_url(path: &str, mime: &str) -> Option<String> {
    let mut f = fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(format!("data:{mime};base64,{}", base64(&buf)))
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}

// ---------------------------------------------------------------------------
// Processes
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct AppState {
    rebuild: Mutex<Option<u32>>,
    game: Mutex<Option<u32>>,
    setup: Mutex<Option<u32>>,
    /// The report folder of the game started in debug mode (kept after
    /// it closes, for "Open folder" and "Create .zip"), and whether that game
    /// is still running.
    report: Mutex<Option<(PathBuf, bool)>>,
    /// Where the last game this launcher started writes its output.
    last_log: Mutex<Option<PathBuf>>,
    /// The launcher's Dock icon as last set (app_icon::key; "" = the bundle's).
    dock_icon: Mutex<String>,
}

/// A GUI app on macOS starts with a bare PATH; Homebrew's tools (cmake,
/// ninja, python3.12) and the usual install places are added in front.
fn tool_path() -> String {
    let mut parts: Vec<String> = Vec::new();
    if cfg!(target_os = "macos") {
        parts.extend(["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin"].map(String::from));
    } else if cfg!(unix) {
        parts.push("/usr/local/bin".into());
    }
    if let Some(p) = std::env::var_os("PATH") {
        parts.extend(std::env::split_paths(&p).map(|p| p.to_string_lossy().into_owned()));
    }
    if cfg!(unix) {
        parts.extend(["/usr/bin", "/bin", "/usr/sbin", "/sbin"].map(String::from));
    }
    let mut seen = std::collections::HashSet::new();
    parts.retain(|p| seen.insert(p.clone()));
    parts.join(if cfg!(windows) { ";" } else { ":" })
}

/// The game builds and runs only on macOS so far (Metal); elsewhere the app
/// installs and opens but does not start a rebuild or the game.
/// INFAMOUS2_ANY_HOST=1 lets someone working on another port try anyway.
fn host_supported() -> bool {
    cfg!(target_os = "macos") || std::env::var_os("INFAMOUS2_ANY_HOST").is_some_and(|v| !v.is_empty())
}

fn require_supported_host() -> Result<(), String> {
    if host_supported() {
        Ok(())
    } else {
        Err(format!(
            "building and playing the game is not supported on {} yet (macOS only for now)",
            std::env::consts::OS
        ))
    }
}

/// `xcode-select -p` succeeds once the Command Line Tools (or Xcode) are in.
/// Until then /usr/bin/python3, git and clang are stubs that pop up Apple's
/// installer, so nothing may run them before this says yes.
fn clt_installed() -> bool {
    if !cfg!(target_os = "macos") {
        return true;
    }
    Command::new("xcode-select")
        .arg("-p")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map(|o| o.status.success() && Path::new(String::from_utf8_lossy(&o.stdout).trim()).is_dir())
        .unwrap_or(false)
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = tool_path();
    std::env::split_paths(&std::ffi::OsString::from(path))
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// A Python to run rebuild.py with (it picks the 3.10+ one for the venv
/// itself): Homebrew's first; the system stub only with the CLT installed.
fn find_python() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["python.exe", "python3.exe"]
    } else {
        &["python3.12", "python3.13", "python3.11", "python3.10", "python3"]
    };
    let clt = clt_installed();
    names
        .iter()
        .filter_map(|n| find_in_path(n))
        .find(|p| clt || !p.starts_with("/usr/bin"))
}

fn find_brew() -> Option<PathBuf> {
    ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
}

#[cfg(unix)]
fn own_process_group(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0); // so a cancel reaches the tools the script started too
}
#[cfg(not(unix))]
fn own_process_group(_cmd: &mut Command) {}

fn kill_tree(pid: u32) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill").arg("-TERM").arg(format!("-{pid}")).status();
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"]).status();
    }
}

/// rebuild.py: next to the sources in a development checkout (the launcher
/// lives in <repo>/launcher), or in the app's bundled resources (port/, staged by
/// launcher/scripts/stage_port.py).
fn rebuild_script(app: &AppHandle) -> Option<PathBuf> {
    let mut cands = Vec::new();
    if let Some(r) = std::env::var_os("INFAMOUS2_REPO") {
        cands.push(PathBuf::from(r).join("infamous2-lle/tools/rebuild.py"));
    }
    cands.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../infamous2-lle/tools/rebuild.py"));
    if let Ok(res) = app.path().resource_dir() {
        cands.push(res.join("port/infamous2-lle/tools/rebuild.py"));
    }
    cands.into_iter().find(|p| p.is_file()).map(|p| p.canonicalize().unwrap_or(p))
}

fn parse_env_lines(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let l = l.strip_prefix("export ").unwrap_or(l);
            let (k, v) = l.split_once('=')?;
            let k = k.trim();
            if k.is_empty() || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return None;
            }
            Some((k.to_string(), v.trim().trim_matches('"').to_string()))
        })
        .collect()
}

/// The environment "Play" adds on top of run.sh's defaults. run.sh uses
/// `${VAR-default}`, so an empty value turns a default off, exactly as on the
/// command line.
fn launch_env(s: &Settings, inst: &InstallInfo, input: &InputEnv) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = Vec::new();
    let mut set = |k: &str, v: String| env.push((k.to_string(), v));
    set("INFAMOUS2_VFS", inst.vfs.clone());
    if !inst.dev_flash.is_empty() {
        set("PS3_DEV_FLASH", inst.dev_flash.clone());
    }
    set("PS3_LANG", s.game_lang.clone());
    let patches: Vec<&str> = s.patches.iter().map(|p| p.trim()).filter(|p| !p.is_empty() && !p.contains(',')).collect();
    if !patches.is_empty() {
        set("PS3_PATCHES", patches.join(","));
    }
    if !s.gpu_light {
        set("PS3RECOMP_ENG_GPU_LIGHT", String::new());
    }
    if !s.gpu_light || !s.gpu_ssao {
        set("PS3RECOMP_ENG_GPU_SSAO", String::new()); // the GPU SSAO needs the GPU light pass
    }
    if !s.skip_occlusion {
        set("PS3RECOMP_ENG_SKIP_FP", String::new());
    }
    if s.gamma_override {
        set("PS3_DISPLAY_GAMMA", format!("{:.2}", s.gamma.clamp(0.8, 1.2)));
    }
    // Graphics options: only what differs from the game's own output.
    if s.aniso > 1 {
        set("PS3_ANISO", s.aniso.min(16).to_string());
    }
    if s.aa == "fxaa" || s.aa == "smaa" {
        set("PS3_AA", s.aa.clone());
    }
    if s.sharpen >= 0.005 {
        set("PS3_SHARPEN", format!("{:.2}", s.sharpen.min(1.0)));
    }
    if s.vibrance.abs() >= 0.005 {
        set("PS3_VIBRANCE", format!("{:.2}", s.vibrance.clamp(-1.0, 1.0)));
    }
    if (s.contrast - 1.0).abs() >= 0.005 && s.contrast > 0.0 {
        set("PS3_CONTRAST", format!("{:.2}", s.contrast.clamp(0.5, 1.5)));
    }
    if s.vignette >= 0.005 {
        set("PS3_VIGNETTE", format!("{:.2}", s.vignette.min(1.0)));
    }
    if s.grain >= 0.005 {
        set("PS3_GRAIN", format!("{:.2}", s.grain.min(1.0)));
    }
    if !s.lut_path.trim().is_empty() {
        set("PS3_LUT", s.lut_path.trim().to_string());
        if s.lut_strength < 0.995 {
            set("PS3_LUT_STRENGTH", format!("{:.2}", s.lut_strength.clamp(0.0, 1.0)));
        }
    }
    if s.upscale == "metalfx" {
        set("PS3_UPSCALE", "metalfx".into());
    }
    if let Some(p) = &input.path {
        set("PS3_INPUT_MAP", p.clone());
    }
    // The variables win over the file's [mouse]; when the file has one (a
    // hand edit), every mouse setting is passed so the launcher's stand.
    let all = input.has_mouse_section;
    if !s.mouse {
        set("PS3_MOUSE", "0".into());
    } else {
        if all {
            set("PS3_MOUSE", "1".into());
        }
        if all || (s.mouse_sens - 1.0).abs() > 1e-3 {
            set("PS3_MOUSE_SENS", format!("{:.2}", s.mouse_sens));
        }
        if all || s.mouse_invert_y {
            set("PS3_MOUSE_INVERT_Y", if s.mouse_invert_y { "1" } else { "0" }.into());
        }
        if all || s.mouse_invert_x {
            set("PS3_MOUSE_INVERT_X", if s.mouse_invert_x { "1" } else { "0" }.into());
        }
        if all || s.mouse_deadzone != 30 {
            set("PS3_MOUSE_DEADZONE", s.mouse_deadzone.min(120).to_string());
        }
        if all || !s.mouse_capture {
            set("PS3_MOUSE_CAPTURE", if s.mouse_capture { "1" } else { "0" }.into());
        }
    }
    if matches!(s.window_mode.as_str(), "fullscreen" | "borderless") {
        set("PS3_WINDOW_MODE", s.window_mode.clone());
    }
    if valid_window_size(&s.window_size) {
        set("PS3_WINDOW_SIZE", s.window_size.clone());
    }
    if s.game_icon {
        set("PS3_WINDOW_ICON", "game".into());
    }
    if !s.display.trim().is_empty() {
        set("PS3_DISPLAY", s.display.trim().to_string());
    }
    if !s.vsync {
        set("PS3_VSYNC", "0".into());
    }
    if s.audio_volume < 100 {
        set("PS3_AUDIO_VOLUME", s.audio_volume.to_string());
    }
    if !s.audio_device.trim().is_empty() {
        set("PS3_AUDIO_DEVICE", s.audio_device.trim().to_string());
    }
    if !s.rumble {
        set("PS3_RUMBLE", "0".into());
    } else if s.rumble_strength != 100 {
        set("PS3_RUMBLE_STRENGTH", s.rumble_strength.min(200).to_string());
    }
    if s.show_fps {
        set("PS3RECOMP_METAL_FPS", "1".into()); // exactly "1" (rsx_metal_backend.m)
    }
    // Normal play keeps the log quiet: with stderr going to a file the
    // runtime is verbose by default, ~500 [WAIT]/[evt] lines a second
    // written one by one from the game's threads (288 MB in a 2 h session).
    // Debug mode (report::DEBUG_ENV) and the extra variables can turn it on.
    set("PS3_VERBOSE", "0".into());
    env.extend(parse_env_lines(&s.extra_env));
    env
}

/// "<w>x<h>", as PS3_WINDOW_SIZE takes it (the window shrinks it to the screen).
fn valid_window_size(v: &str) -> bool {
    let mut it = v.splitn(2, 'x').map(|n| n.parse::<u32>().ok());
    matches!((it.next(), it.next()), (Some(Some(w)), Some(Some(h)))
        if (160..=16384).contains(&w) && (90..=16384).contains(&h))
}

fn shell_quote(v: &str) -> String {
    if !v.is_empty() && v.chars().all(|c| c.is_ascii_alphanumeric() || "-_./:,=+".contains(c)) {
        v.to_string()
    } else {
        format!("'{}'", v.replace('\'', "'\\''"))
    }
}

/// The screens and audio outputs the Settings screen offers.
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Outputs {
    /// Screen names in the system's order (NSScreen.screens: the first has
    /// the menu bar), as the game's PS3_DISPLAY matches them.
    screens: Vec<String>,
    /// Output device names (CoreAudio's, which SDL uses for PS3_AUDIO_DEVICE).
    audio_outputs: Vec<String>,
}

/// The output devices in `system_profiler SPAudioDataType -json`: the items
/// that have output channels.
fn parse_audio_outputs(json: &str) -> Vec<String> {
    let v: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for group in v["SPAudioDataType"].as_array().into_iter().flatten() {
        for item in group["_items"].as_array().into_iter().flatten() {
            if item.get("coreaudio_device_output").is_some() {
                if let Some(n) = item["_name"].as_str() {
                    if !out.iter().any(|o| o == n) {
                        out.push(n.to_string());
                    }
                }
            }
        }
    }
    out
}

/// The names in the JSON array the screen probe prints.
fn parse_screens(json: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(json.trim()).unwrap_or_default()
}

fn command_stdout(cmd: &mut Command) -> Option<String> {
    let out = cmd.stderr(Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

#[cfg(target_os = "macos")]
fn list_outputs_host() -> Outputs {
    // AppKit's own names, the ones the Metal window matches (JXA needs no
    // extra dependency in the app).
    let screens = command_stdout(Command::new("/usr/bin/osascript").args([
        "-l",
        "JavaScript",
        "-e",
        "ObjC.import('AppKit'); JSON.stringify($.NSScreen.screens.js.map(s => s.localizedName.js))",
    ]))
    .map(|s| parse_screens(&s))
    .unwrap_or_default();
    let audio_outputs = command_stdout(Command::new("/usr/sbin/system_profiler").args(["SPAudioDataType", "-json"]))
        .map(|s| parse_audio_outputs(&s))
        .unwrap_or_default();
    Outputs { screens, audio_outputs }
}

#[cfg(not(target_os = "macos"))]
fn list_outputs_host() -> Outputs {
    Outputs::default()
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
async fn list_outputs() -> Outputs {
    tauri::async_runtime::spawn_blocking(list_outputs_host).await.unwrap_or_default()
}

#[tauri::command]
fn get_settings() -> Settings {
    load_settings_file()
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    let p = settings_path();
    if let Some(d) = p.parent() {
        fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    fs::rename(&tmp, &p).map_err(|e| e.to_string())?;
    sync_app_icon(&app); // "Your copy's icon in the Dock" may have changed
    Ok(())
}

/// The controls file for the Controls screen: what it binds (the screen
/// fills in the defaults for the rest), and where it is.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputMapView {
    path: String,
    exists: bool,
    bindings: Bindings,
}

#[tauri::command]
fn get_input_map() -> InputMapView {
    let p = input_map_path();
    let text = fs::read_to_string(&p);
    InputMapView {
        path: p.to_string_lossy().into_owned(),
        exists: text.is_ok(),
        bindings: text.map(|t| input_map::parse(&t).bindings).unwrap_or_default(),
    }
}

/// Write the bindings; the file's other sections ([mouse], anything newer)
/// stay as they were.
#[tauri::command]
fn save_input_map(bindings: Bindings) -> Result<String, String> {
    input_map::validate(&bindings)?;
    let p = input_map_path();
    let mut file = fs::read_to_string(&p).map(|t| input_map::parse(&t)).unwrap_or_default();
    file.bindings = bindings;
    if let Some(d) = p.parent() {
        fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("toml.tmp");
    fs::write(&tmp, input_map::render(&file)).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &p).map_err(|e| e.to_string())?;
    Ok(p.to_string_lossy().into_owned())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    install_dir: String,
    install: Option<InstallInfo>,
    install_error: Option<String>,
    icon: Option<String>,
    background: Option<String>,
    last_save: Option<SaveInfo>,
    game_running: bool,
    rebuild_running: bool,
    rebuild_script: Option<String>,
    data_root: String,
    default_install_dir: String,
    /// The game builds and runs here (macOS only for now).
    host_supported: bool,
    os: String,
    /// The .app the launcher runs from (macOS; none in a development build)
    /// and whether it has the custom Finder icon.
    app_bundle: Option<String>,
    finder_icon: bool,
}

/// The game's trophies with the player's unlocks (Game page).
#[tauri::command]
async fn list_trophies() -> Result<trophies::TrophyList, String> {
    let s = load_settings_file();
    let inst = resolve_install(&PathBuf::from(&s.install_dir))?;
    trophies::list(Path::new(&inst.vfs), &gamedata_root(&s.extra_env), &s.game_lang, true)
}

#[tauri::command]
async fn get_status(app: AppHandle, state: State<'_, AppState>) -> Result<Status, String> {
    let s = load_settings_file();
    let dir = PathBuf::from(&s.install_dir);
    let (install, install_error) = match resolve_install(&dir) {
        Ok(i) => (Some(i), None),
        Err(e) => (None, Some(e)),
    };
    // The art is the player's own (PS3_GAME/ICON0.PNG, PIC1.PNG), read here
    // at runtime: from the install, else from the game folder chosen on the
    // Install screen (before the first rebuild). Nothing ships with the app.
    let (icon_path, background_path) = match &install {
        Some(i) => (i.icon.clone(), i.background.clone()),
        None => copy_art(&s.game_paths),
    };
    let icon = data_url(&icon_path, "image/png");
    let background = data_url(&background_path, "image/png");
    sync_app_icon(&app);
    let bundle = app_icon::app_bundle().filter(|_| cfg!(target_os = "macos"));
    Ok(Status {
        install_dir: s.install_dir.clone(),
        install,
        install_error,
        icon,
        background,
        last_save: last_save(&s),
        game_running: state.game.lock().unwrap().is_some(),
        rebuild_running: state.rebuild.lock().unwrap().is_some(),
        rebuild_script: rebuild_script(&app).map(|p| p.to_string_lossy().into_owned()),
        data_root: data_root().to_string_lossy().into_owned(),
        default_install_dir: default_install_dir().to_string_lossy().into_owned(),
        host_supported: host_supported(),
        os: std::env::consts::OS.into(),
        finder_icon: bundle.as_deref().is_some_and(app_icon::has_finder_icon),
        app_bundle: bundle.map(|b| b.to_string_lossy().into_owned()),
    })
}

// ---------------------------------------------------------------------------
// The Dock and Finder icons from the player's copy (app_icon.rs)
// ---------------------------------------------------------------------------

/// The ICON0.PNG the icons are drawn from: the install's, else the game
/// folder chosen on the Install screen (as the Home hero).
fn icon_source(s: &Settings) -> Option<PathBuf> {
    let p = match resolve_install(Path::new(&s.install_dir)) {
        Ok(i) if Path::new(&i.icon).is_file() => i.icon,
        _ => copy_art(&s.game_paths).0,
    };
    Some(PathBuf::from(p)).filter(|p| p.is_file())
}

fn app_icon_cache() -> PathBuf {
    data_root().join(app_icon::CACHE_NAME)
}

/// The composed square icon when the setting is on and there is art (drawn
/// again when the copy's ICON0.PNG changed; the cached one when the copy is
/// out of reach).
fn composed_icon(s: &Settings) -> Option<PathBuf> {
    if !s.game_icon {
        return None;
    }
    app_icon::refresh(&app_icon_cache(), icon_source(s).as_deref())
}

/// Bring the launcher's Dock icon in line with the settings: the composed
/// icon, or the bundle's placeholder. Off the main thread (the drawing takes
/// a moment the first time); sets the Dock only when that changes.
fn sync_app_icon(app: &AppHandle) {
    if !cfg!(target_os = "macos") {
        return;
    }
    let app = app.clone();
    thread::spawn(move || {
        let icon = composed_icon(&load_settings_file());
        let key = app_icon::key(icon.as_deref());
        let state = app.state::<AppState>();
        let mut cur = state.dock_icon.lock().unwrap();
        if *cur != key {
            *cur = key;
            app_icon::apply_dock_icon(&app, icon);
        }
    });
}

/// PS3_WINDOW_ICON=game gets the same square icon as the launcher when it is
/// drawn (the toolkit centres the bare 320x176 ICON0 on a transparent square
/// otherwise; `=<file>` is its own option).
fn game_window_icon(s: &Settings, env: &mut [(String, String)]) {
    for (k, v) in env.iter_mut() {
        if k == "PS3_WINDOW_ICON" && v == "game" {
            if let Some(p) = composed_icon(s) {
                *v = p.to_string_lossy().into_owned();
            }
        }
    }
}

/// "Use as Finder icon": the composed icon on the installed .app (set), or
/// the bundle's own back (!set). Returns whether a custom icon is on now.
#[tauri::command]
async fn finder_icon(app: AppHandle, set: bool) -> Result<bool, String> {
    if !cfg!(target_os = "macos") {
        return Err("the Finder icon is macOS only".into());
    }
    let bundle = app_icon::app_bundle().ok_or("the launcher is not running from an .app (development build)")?;
    let icon = if set {
        let s = load_settings_file();
        let src = icon_source(&s);
        Some(app_icon::refresh(&app_icon_cache(), src.as_deref()).ok_or("no ICON0.PNG from your copy yet")?)
    } else {
        None
    };
    // NSWorkspace on the main thread, the answer back here
    let (tx, rx) = std::sync::mpsc::channel();
    let b = bundle.clone();
    app.run_on_main_thread(move || {
        let _ = tx.send(app_icon::set_finder_icon(&b, icon.as_deref()));
    })
    .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())??;
    Ok(app_icon::has_finder_icon(&bundle))
}

// ---------------------------------------------------------------------------
// Finding the player's copy and licence
// ---------------------------------------------------------------------------

const TITLE_ID: &str = "NPUA80638";

/// ICON0.PNG and PIC1.PNG of the first chosen game folder that is one (RPCS3's
/// NPUA80638 keeps them at its top; an extracted disc under PS3_GAME/).
fn copy_art(paths: &[String]) -> (String, String) {
    for p in paths.iter().map(|p| PathBuf::from(p.trim())).filter(|p| p.is_dir()) {
        let Some(g) = resolve_game_dir(&p) else { continue };
        for d in [g.clone(), g.join("PS3_GAME")] {
            if d.join("ICON0.PNG").is_file() {
                let s = |n: &str| d.join(n).to_string_lossy().into_owned();
                return (s("ICON0.PNG"), s("PIC1.PNG"));
            }
        }
    }
    (String::new(), String::new())
}

/// The NPUA80638 folder from whatever the player pointed at (the folder
/// itself, or RPCS3's data folder above it), as rebuild.py resolves it.
fn resolve_game_dir(p: &Path) -> Option<PathBuf> {
    [
        p.to_path_buf(),
        p.join(TITLE_ID),
        p.join("game").join(TITLE_ID),
        p.join("dev_hdd0/game").join(TITLE_ID),
    ]
    .into_iter()
    .find(|c| c.join("PARAM.SFO").is_file())
}

/// Content id (e.g. UP9000-NPUA80638_00-...) from the NPD header of an NPDRM
/// EBOOT.BIN, or from a PKG header. RPCS3 names the licence <content id>.rap.
fn content_id(path: &Path) -> Option<String> {
    let mut buf = vec![0u8; 0x2000];
    let n = fs::File::open(path).ok()?.read(&mut buf).ok()?;
    let d = &buf[..n];
    let field = |b: &[u8]| -> Option<String> {
        let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
        let s = String::from_utf8_lossy(&b[..end]).trim().to_string();
        (!s.is_empty() && s.is_ascii()).then_some(s)
    };
    if d.len() >= 0x60 && &d[..4] == b"\x7fPKG" {
        return field(&d[0x30..0x60]);
    }
    let i = d.windows(4).position(|w| w == b"NPD\0")?;
    field(d.get(i + 0x10..i + 0x40)?)
}

/// Every `exdata` folder of an RPCS3 data folder (one per PS3 user).
fn exdata_dirs(rpcs3_data: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(rpcs3_data.join("dev_hdd0/home"))
        .map(|it| it.flatten().map(|e| e.path().join("exdata")).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LicenceSearch {
    /// The NPUA80638 folder found from the picked path, if it is one.
    game_dir: Option<String>,
    title: Option<String>,
    app_ver: Option<String>,
    content_id: Option<String>,
    /// Matching .rap files, the best first (exact content id, then any
    /// NPUA80638 licence).
    raps: Vec<String>,
    /// Where it looked, for the message when nothing was found.
    searched: Vec<String>,
}

/// From the game folder (or PKGs) the player picked: the content id, and the
/// .rap wherever it usually is -- RPCS3's exdata (the default data folder and
/// the one the game folder sits in), next to the copy, Downloads/Desktop.
fn search_licence(paths: &[String]) -> LicenceSearch {
    let mut out = LicenceSearch::default();
    let mut dirs: Vec<PathBuf> = Vec::new();
    for raw in paths.iter().map(|p| PathBuf::from(p.trim())).filter(|p| !p.as_os_str().is_empty()) {
        if raw.is_file() {
            // a PKG: its content id, and the folder it sits in
            if out.content_id.is_none() {
                out.content_id = content_id(&raw);
            }
            if let Some(d) = raw.parent() {
                dirs.push(d.to_path_buf());
            }
            continue;
        }
        if let Some(g) = resolve_game_dir(&raw) {
            if out.game_dir.is_none() {
                let sfo = read_sfo(&g.join("PARAM.SFO")).unwrap_or_default();
                out.title = sfo.get("TITLE").cloned();
                out.app_ver = sfo.get("APP_VER").cloned();
                out.content_id = content_id(&g.join("USRDIR/EBOOT.BIN")).or(out.content_id.take());
                out.game_dir = Some(g.to_string_lossy().into_owned());
            }
            // <rpcs3 data>/dev_hdd0/game/NPUA80638: that RPCS3's licences
            if let Some(root) = g.parent().and_then(Path::parent).and_then(Path::parent) {
                if g.parent().and_then(Path::file_name).is_some_and(|n| n == "game") {
                    dirs.extend(exdata_dirs(root));
                }
            }
            dirs.push(g.clone());
            dirs.extend(g.ancestors().skip(1).take(2).map(Path::to_path_buf));
        }
        dirs.push(raw.clone());
        dirs.extend(raw.ancestors().skip(1).take(2).map(Path::to_path_buf));
    }
    dirs.extend(exdata_dirs(&rpcs3_dir()));
    for d in ["Downloads", "Desktop", "Documents"] {
        dirs.push(home().join(d));
    }
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| d.is_dir() && seen.insert(d.clone()));

    let want = out.content_id.as_ref().map(|c| format!("{}.rap", c.to_lowercase()));
    let mut exact = Vec::new();
    let mut other = Vec::new();
    for d in &dirs {
        out.searched.push(d.to_string_lossy().into_owned());
        let Ok(it) = fs::read_dir(d) else { continue };
        for e in it.flatten() {
            let p = e.path();
            let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !name.ends_with(".rap") || fs::metadata(&p).map(|m| m.len() != 16).unwrap_or(true) {
                continue;
            }
            let s = p.to_string_lossy().into_owned();
            if want.as_deref() == Some(name.as_str()) {
                exact.push(s);
            } else if name.contains(&TITLE_ID.to_lowercase()) {
                other.push(s);
            }
        }
    }
    exact.sort();
    other.sort();
    let mut seen = std::collections::HashSet::new();
    out.raps = exact.into_iter().chain(other).filter(|p| seen.insert(p.clone())).collect();
    out
}

/// The game folder and licence the player picked (or that RPCS3 has).
#[tauri::command]
fn find_licence(paths: Vec<String>) -> LicenceSearch {
    search_licence(&paths)
}

/// RPCS3's own install of NPUA80638 and its licence, to prefill the form.
#[tauri::command]
fn detect_rpcs3() -> LicenceSearch {
    let game = rpcs3_dir().join("dev_hdd0/game").join(TITLE_ID);
    if !game.join("PARAM.SFO").is_file() {
        return LicenceSearch::default();
    }
    search_licence(&[game.to_string_lossy().into_owned()])
}

// ---------------------------------------------------------------------------
// First run: what this machine still needs, and installing it
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CheckItem {
    level: String,
    name: String,
    message: String,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Prereqs {
    os: String,
    arch: String,
    /// The game builds and runs on this system (macOS only for now).
    supported: bool,
    /// macOS: Xcode Command Line Tools present (compiler, git, Python).
    clt: bool,
    homebrew: Option<String>,
    script: Option<String>,
    /// rebuild.py --check-json, item by item.
    checks: Vec<CheckItem>,
    /// Homebrew formulae that would fix the failed checks.
    brew_missing: Vec<String>,
    rpcs3_missing: bool,
    firmware_missing: bool,
    /// Nothing is missing.
    ok: bool,
    /// Why the checks could not run (no Python yet, ...).
    error: Option<String>,
}

/// Check name (rebuild.py) -> the Homebrew formula that provides it.
fn brew_formula(check: &str) -> Option<&'static str> {
    Some(match check {
        "python" => "python@3.12",
        "cmake" => "cmake",
        "ninja" => "ninja",
        "sdl2" => "sdl2",
        "glslang" => "glslang",
        "spirv-cross" => "spirv-cross",
        _ => return None,
    })
}

const BREW_ALLOWED: &[&str] = &["python@3.12", "cmake", "ninja", "sdl2", "glslang", "spirv-cross"];

/// `decrypt`: the Install screen's decryption choices, so the list shows
/// what this method needs (RPCS3, or the key file and what it lacks).
#[tauri::command]
async fn check_prereqs(app: AppHandle, decrypt: Option<DecryptOptions>) -> Prereqs {
    let mut p = Prereqs {
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        supported: host_supported(),
        clt: clt_installed(),
        homebrew: find_brew().map(|b| b.to_string_lossy().into_owned()),
        script: rebuild_script(&app).map(|s| s.to_string_lossy().into_owned()),
        ..Default::default()
    };
    if !p.supported {
        return p;
    }
    let Some(script) = p.script.clone() else {
        p.error = Some("rebuild.py not found".into());
        return p;
    };
    if cfg!(target_os = "macos") && !p.clt {
        p.checks.push(CheckItem {
            level: "fail".into(),
            name: "xcode-clt".into(),
            message: "Xcode Command Line Tools not installed".into(),
        });
    }
    let Some(python) = find_python() else {
        p.error = Some("no Python to run the checks yet".into());
        return p;
    };
    let mut cmd = Command::new(&python);
    cmd.arg(&script).arg("--check-json");
    if let Err(e) = apply_decrypt(&mut cmd, &decrypt.unwrap_or_default()) {
        p.error = Some(e);
        return p;
    }
    let out = cmd.env("PATH", tool_path()).stdin(Stdio::null()).stderr(Stdio::piped()).output();
    let out = match out {
        Ok(o) => o,
        Err(e) => {
            p.error = Some(format!("{}: {e}", python.display()));
            return p;
        }
    };
    #[derive(Deserialize)]
    struct Report {
        checks: Vec<CheckItem>,
    }
    match serde_json::from_slice::<Report>(&out.stdout) {
        Ok(r) => {
            // rebuild.py's own CLT check replaces the placeholder above
            p.checks.retain(|c| !r.checks.iter().any(|d| d.name == c.name));
            p.checks.extend(r.checks);
        }
        Err(e) => {
            let err = String::from_utf8_lossy(&out.stderr);
            p.error = Some(format!("rebuild.py --check-json: {e} {}", err.trim()));
            return p;
        }
    }
    for c in p.checks.iter().filter(|c| c.level == "fail") {
        if let Some(f) = brew_formula(&c.name) {
            if !p.brew_missing.iter().any(|x| x == f) {
                p.brew_missing.push(f.into());
            }
        }
        match c.name.as_str() {
            "rpcs3" => p.rpcs3_missing = true,
            "firmware" => p.firmware_missing = true,
            "xcode-clt" | "compiler" | "git" => p.clt = false,
            _ => {}
        }
    }
    p.ok = p.error.is_none() && !p.checks.iter().any(|c| c.level == "fail");
    p
}

/// Opens Apple's installer for the Command Line Tools (the player confirms there).
#[tauri::command]
fn install_clt() -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("only on macOS".into());
    }
    Command::new("xcode-select")
        .arg("--install")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("xcode-select: {e}"))
}

/// `brew install <formulae>` (only the ones the port needs), streamed to the
/// window as setup-line / setup-exit. The window asks the player first.
#[tauri::command]
fn install_brew(app: AppHandle, state: State<'_, AppState>, packages: Vec<String>) -> Result<(), String> {
    require_supported_host()?;
    if packages.is_empty() {
        return Err("nothing to install".into());
    }
    if let Some(bad) = packages.iter().find(|p| !BREW_ALLOWED.contains(&p.as_str())) {
        return Err(format!("{bad}: not a package this app installs"));
    }
    let brew = find_brew().ok_or("Homebrew not found")?;
    let mut slot = state.setup.lock().unwrap();
    if slot.is_some() {
        return Err("an install is already running".into());
    }
    let mut cmd = Command::new(&brew);
    cmd.arg("install")
        .args(&packages)
        .env("PATH", tool_path())
        .env("NONINTERACTIVE", "1")
        .env("HOMEBREW_NO_ENV_HINTS", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    own_process_group(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("{}: {e}", brew.display()))?;
    *slot = Some(child.id());
    drop(slot);
    let _ = app.emit("setup-line", format!("$ brew install {}", packages.join(" ")));
    let (stdout, stderr) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let a1 = app.clone();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = a1.emit("setup-line", line);
        }
    });
    let a2 = app.clone();
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = a2.emit("setup-line", line);
        }
    });
    thread::spawn(move || {
        let code = child.wait().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
        *app.state::<AppState>().setup.lock().unwrap() = None;
        let _ = app.emit("setup-exit", code);
    });
    Ok(())
}

#[tauri::command]
fn cancel_setup(state: State<'_, AppState>) {
    if let Some(pid) = *state.setup.lock().unwrap() {
        kill_tree(pid);
    }
}

/// Pages the app links to (download RPCS3, firmware how-to, Homebrew, the project).
const URL_ALLOWED: &[&str] = &[
    "https://rpcs3.net/",
    "https://brew.sh/",
    "https://www.playstation.com/",
    "https://github.com/erikomis/infamous2-recomp",
];

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !URL_ALLOWED.iter().any(|p| url.starts_with(p)) || url.chars().any(|c| c.is_whitespace() || c == '"') {
        return Err(format!("{url}: not a link this app opens"));
    }
    let mut cmd = if cfg!(target_os = "macos") {
        Command::new("open")
    } else if cfg!(windows) {
        let mut c = Command::new("rundll32");
        c.arg("url.dll,FileProtocolHandler");
        c
    } else {
        Command::new("xdg-open")
    };
    cmd.arg(&url).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// How rebuild.py decrypts: RPCS3, or tools/decrypt_self.py with the
/// player's own key file (no keys ship with the app or the repository).
#[derive(Deserialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct DecryptOptions {
    /// --keys: a key file in scetool's format.
    keys: String,
    /// --firmware: a dev_flash folder (or its sys/external) or PS3UPDAT.PUP;
    /// empty = RPCS3's dev_flash.
    firmware: String,
    /// --decrypt-with: "auto" (the key file when one is given), "python" or "rpcs3".
    decrypt_with: String,
}

/// The rebuild.py arguments for `o` (both for --check-json and the run).
fn decrypt_args(o: &DecryptOptions) -> Result<Vec<String>, String> {
    let mut a = Vec::new();
    match o.decrypt_with.trim() {
        "" | "auto" => {}
        m @ ("python" | "rpcs3") => a.extend(["--decrypt-with".to_string(), m.to_string()]),
        m => return Err(format!("{m}: not a decryption method (auto, python, rpcs3)")),
    }
    for (flag, v) in [("--keys", &o.keys), ("--firmware", &o.firmware)] {
        let v = v.trim();
        if !v.is_empty() {
            a.extend([flag.to_string(), v.to_string()]);
        }
    }
    Ok(a)
}

/// rebuild.py with the decryption the screen shows: its $PS3_KEYS default
/// is dropped, so an empty key field means no key file.
fn apply_decrypt(cmd: &mut Command, o: &DecryptOptions) -> Result<(), String> {
    cmd.args(decrypt_args(o)?);
    cmd.env_remove("PS3_KEYS");
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RebuildRequest {
    games: Vec<String>,
    rap: String,
    out: String,
    dry_run: bool,
    force: bool,
    /// Optional, only for the DLC step; passed in the environment, never stored.
    rap_key: Option<String>,
    #[serde(default)]
    decrypt: DecryptOptions,
}

#[tauri::command]
fn start_rebuild(app: AppHandle, state: State<'_, AppState>, req: RebuildRequest) -> Result<(), String> {
    require_supported_host()?;
    let mut slot = state.rebuild.lock().unwrap();
    if slot.is_some() {
        return Err("a rebuild is already running".into());
    }
    let script = rebuild_script(&app).ok_or("rebuild.py not found (set INFAMOUS2_REPO to the repository)")?;
    let python = find_python().ok_or("Python 3 not found (install the Xcode Command Line Tools)")?;
    let mut cmd = Command::new(&python);
    cmd.arg("-u").arg(&script);
    for g in &req.games {
        cmd.arg("--game").arg(g);
    }
    if !req.rap.is_empty() {
        cmd.arg("--rap").arg(&req.rap);
    }
    if !req.out.is_empty() {
        cmd.arg("--out").arg(&req.out);
    }
    if req.dry_run {
        cmd.arg("--dry-run");
    }
    if req.force {
        cmd.arg("--force");
    }
    apply_decrypt(&mut cmd, &req.decrypt)?;
    cmd.arg("--nice").arg("10"); // the machine stays usable while it compiles
    cmd.env("PATH", tool_path());
    match req.rap_key.as_deref().map(str::trim) {
        Some(k) if !k.is_empty() => {
            cmd.env("PS3_RAP_KEY", k);
        }
        _ => {
            cmd.env_remove("PS3_RAP_KEY");
        }
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    own_process_group(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("{}: {e}", python.display()))?;
    *slot = Some(child.id());
    drop(slot);

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let a1 = app.clone();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = a1.emit("rebuild-line", line);
        }
    });
    let a2 = app.clone();
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let _ = a2.emit("rebuild-line", format!("LOG {line}"));
        }
    });
    let a3 = app.clone();
    thread::spawn(move || {
        let code = child.wait().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
        *a3.state::<AppState>().rebuild.lock().unwrap() = None;
        let _ = a3.emit("rebuild-exit", code);
    });
    Ok(())
}

#[tauri::command]
fn cancel_rebuild(state: State<'_, AppState>) {
    if let Some(pid) = *state.rebuild.lock().unwrap() {
        kill_tree(pid);
    }
}

/// The equivalent command line, shown in Settings.
#[tauri::command]
fn launch_preview(settings: Settings) -> Result<String, String> {
    let inst = resolve_install(Path::new(&settings.install_dir))?;
    let mut env = launch_env(&settings, &inst, &input_env());
    game_window_icon(&settings, &mut env);
    let mut s: Vec<String> = env
        .into_iter()
        .map(|(k, v)| format!("{k}={}", shell_quote(&v)))
        .collect();
    s.push(shell_quote(&inst.run_sh));
    Ok(s.join(" "))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayStarted {
    log: String,
    /// The backup of the saves made just before (none when nothing changed
    /// since the last one, or when they are turned off).
    backup: Option<String>,
    backup_error: Option<String>,
    /// Diagnostic mode: the report folder.
    report: Option<String>,
}

/// `diagnostic`: debug mode. Start the game with play_dump.sh's dump
/// variables and the problem logs (report::DEBUG_ENV) pointed at a new report
/// folder (its log goes there too), and keep its session index
/// (report::EVENTS): the start, a tick every 2 s, the captures, the exit and
/// a crash report. "Capture this frame" then dumps the frame on screen.
/// Without it, the usual Play.
#[tauri::command]
fn play(app: AppHandle, state: State<'_, AppState>, diagnostic: Option<bool>) -> Result<PlayStarted, String> {
    require_supported_host()?;
    let mut slot = state.game.lock().unwrap();
    if slot.is_some() {
        return Err("the game is already running".into());
    }
    let s = load_settings_file();
    let inst = resolve_install(Path::new(&s.install_dir))?;
    if !Path::new(&inst.executable).is_file() {
        return Err(format!("{}: not built", inst.executable));
    }
    let report_dir = if diagnostic.unwrap_or(false) {
        Some(report::create(&reports_dir(), &saves::stamp(now_secs()))?)
    } else {
        None
    };
    let log_path = match &report_dir {
        Some(d) => d.join(report::LOG),
        None => {
            let logs = data_root().join("logs");
            fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
            logs.join("game.log")
        }
    };
    let log = fs::File::create(&log_path).map_err(|e| e.to_string())?;
    let log2 = log.try_clone().map_err(|e| e.to_string())?;

    // the saves as they are now, before the game can touch them
    let (backup, backup_error) = match saves::backup_before_play(&save_locations(&s), s.save_backups_keep, now_secs()) {
        Ok(b) => (b.map(|b| b.id), None),
        Err(e) => (None, Some(e)),
    };

    // run.sh is the single source of the defaults; the settings only override.
    let mut cmd = Command::new("/bin/sh");
    cmd.arg(&inst.run_sh)
        .current_dir(&inst.port_dir)
        .env("PATH", tool_path())
        .env_remove("HEADLESS")
        .env_remove("PS3RECOMP_METAL_HEADLESS")
        .stdin(Stdio::null())
        .stdout(log)
        .stderr(log2);
    let mut env = launch_env(&s, &inst, &input_env());
    game_window_icon(&s, &mut env);
    if let Some(d) = &report_dir {
        env.extend(report::env(d)); // after the extra variables: the report's paths win
    }
    for (k, v) in &env {
        cmd.env(k, v);
    }
    own_process_group(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("{}: {e}", inst.run_sh))?;
    *slot = Some(child.id());
    drop(slot);
    *state.last_log.lock().unwrap() = Some(log_path.clone());
    let started = now_secs();
    if let Some(d) = &report_dir {
        *state.report.lock().unwrap() = Some((d.clone(), true));
        session_event(d, "start", &format!("pid={}", child.id()));
        // the machine and the build, for whoever reads the report (system_profiler takes a moment)
        let (d2, inst, now) = (d.clone(), inst.clone(), started);
        thread::spawn(move || {
            let _ = fs::write(d2.join(report::SYSTEM), system_info(&inst, &env, now));
        });
        spawn_ticker(app.clone(), d.clone());
    }
    let a = app.clone();
    let session = report_dir.clone();
    thread::spawn(move || {
        let status = child.wait();
        let code = status.as_ref().ok().and_then(|s| s.code()).unwrap_or(-1);
        let st = a.state::<AppState>();
        *st.game.lock().unwrap() = None;
        if let Some((_, live)) = st.report.lock().unwrap().as_mut() {
            *live = false;
        }
        let _ = a.emit("game-exit", code);
        if let Some(d) = session {
            session_exit(&d, status.ok(), started);
            let _ = a.emit("report-changed", ());
        }
    });
    Ok(PlayStarted {
        log: log_path.to_string_lossy().into_owned(),
        backup,
        backup_error,
        report: report_dir.map(|d| d.to_string_lossy().into_owned()),
    })
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// One line in a debug session's index (errors only cost the line).
fn session_event(dir: &Path, kind: &str, detail: &str) {
    let ms = now_ms();
    let _ = report::event(dir, ms, &utc_text(ms / 1000), kind, detail);
}

/// While the debug game runs: a tick every 2 s with the log's size (so the
/// triage can put a time on every line), and "taken" once the game has
/// consumed a capture's trigger.
fn spawn_ticker(app: AppHandle, dir: PathBuf) {
    thread::spawn(move || {
        let mut pending = false;
        loop {
            thread::sleep(std::time::Duration::from_secs(2));
            let live = matches!(app.state::<AppState>().report.lock().unwrap().as_ref(), Some((d, true)) if *d == dir);
            if !live {
                break;
            }
            let now_pending = dir.join(report::TRIGGER).exists();
            if pending && !now_pending {
                session_event(&dir, "taken", "");
            }
            pending = now_pending;
            session_event(&dir, "tick", "");
        }
    });
}

/// The end of a debug session: the exit status, and for a crash (any end
/// the player did not ask for with Stop) the macOS crash report, which
/// ReportCrash writes a few seconds after the process is gone.
fn session_exit(dir: &Path, status: Option<std::process::ExitStatus>, started: u64) {
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        status.and_then(|s| s.signal())
    };
    #[cfg(not(unix))]
    let signal: Option<i32> = None;
    let code = status.and_then(|s| s.code());
    let detail = match (code, signal) {
        (Some(c), _) => format!("code={c}"),
        (None, Some(sig)) => format!("signal={sig}"),
        _ => "unknown".into(),
    };
    session_event(dir, "exit", &detail);
    let stopped = fs::read_to_string(dir.join(report::EVENTS)).is_ok_and(|t| t.contains("\tstop\t"));
    let crashed = signal.is_some() || code.is_some_and(|c| c != 0);
    if !crashed || stopped || !cfg!(target_os = "macos") {
        return;
    }
    let diag = home().join("Library/Logs/DiagnosticReports");
    for _ in 0..20 {
        if let Some(p) = report::find_crash_report(&diag, exe_name(), started) {
            if fs::copy(&p, dir.join(report::CRASH)).is_ok() {
                session_event(dir, "crash-report", &format!("file={}", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
            }
            return;
        }
        thread::sleep(std::time::Duration::from_secs(1));
    }
}

fn command_line(program: &str, args: &[&str]) -> String {
    command_stdout(Command::new(program).args(args)).map(|s| s.trim().to_string()).unwrap_or_default()
}

/// UTC "YYYY-MM-DD HH:MM:SS".
fn utc_text(secs: u64) -> String {
    let st = saves::stamp(secs);
    format!("{}-{}-{} {}:{}:{}", &st[0..4], &st[4..6], &st[6..8], &st[9..11], &st[11..13], &st[13..15])
}

/// system.txt: what a report's reader needs to know about the machine, the
/// build and the settings (the variables run.sh got), with the home folder
/// shown as ~.
fn system_info(inst: &InstallInfo, env: &[(String, String)], now: u64) -> String {
    let mut t = String::new();
    let mut line = |k: &str, v: &str| {
        if !v.is_empty() {
            t.push_str(&format!("{k}: {v}\n"));
        }
    };
    line("report", &utc_text(now));
    line("launcher", env!("CARGO_PKG_VERSION"));
    line("os", &format!("{} {}", std::env::consts::OS, std::env::consts::ARCH));
    if cfg!(target_os = "macos") {
        line("macos", &format!("{} ({})", command_line("sw_vers", &["-productVersion"]), command_line("sw_vers", &["-buildVersion"])));
        line("model", &command_line("sysctl", &["-n", "hw.model"]));
        line("cpu", &command_line("sysctl", &["-n", "machdep.cpu.brand_string"]));
        let mem = command_line("sysctl", &["-n", "hw.memsize"]).parse::<u64>().unwrap_or(0);
        if mem > 0 {
            line("memory", &format!("{} GB", mem >> 30));
        }
        let gpus = command_stdout(Command::new("/usr/sbin/system_profiler").args(["SPDisplaysDataType", "-json"]))
            .and_then(|j| serde_json::from_str::<serde_json::Value>(&j).ok())
            .map(|v| {
                v["SPDisplaysDataType"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|g| g["sppci_model"].as_str().map(|m| match g["sppci_cores"].as_str() {
                        Some(c) => format!("{m} ({c} cores)"),
                        None => m.to_string(),
                    }))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        line("gpu", &gpus);
    }
    line("game", format!("{} {}", inst.title, inst.app_ver).trim());
    line("install", &inst.layout);
    line("built", &inst.built_at);
    line("port commit", &inst.port_commit);
    line("toolkit commit", &inst.toolkit_commit);
    t.push_str("\nenvironment given to run.sh:\n");
    for (k, v) in env {
        t.push_str(&format!("  {k}={}\n", shell_quote(v)));
    }
    let home = home().to_string_lossy().into_owned();
    if home.len() > 1 {
        t = t.replace(&home, "~");
    }
    t
}

/// The report this launcher started last (or the newest one on disk).
fn current_report(state: &State<'_, AppState>) -> Option<(PathBuf, bool)> {
    let r = state.report.lock().unwrap().clone();
    r.or_else(|| report::latest(&reports_dir()).map(|d| (d, false)))
}

#[tauri::command]
fn report_info(state: State<'_, AppState>) -> Option<report::Info> {
    current_report(&state).map(|(d, live)| report::info(&d, live))
}

/// "Capture this frame": only while the game started in debug mode runs.
#[tauri::command]
fn report_capture(state: State<'_, AppState>) -> Result<report::Info, String> {
    let game = state.game.lock().unwrap().is_some();
    match state.report.lock().unwrap().clone() {
        Some((d, true)) if game => {
            let ms = now_ms();
            report::capture(&d, &utc_text(ms / 1000), ms)?;
            Ok(report::info(&d, true))
        }
        _ => Err("the game was not started in debug mode".into()),
    }
}

/// A report by id, or the current one.
fn report_dir(state: &State<'_, AppState>, id: Option<&str>) -> Result<(PathBuf, bool), String> {
    let cur = current_report(state);
    match id {
        Some(id) => {
            let d = report::by_id(&reports_dir(), id)?;
            let live = cur.is_some_and(|(c, live)| live && c == d);
            Ok((d, live))
        }
        None => cur.ok_or_else(|| "no report yet: start the game in debug mode first".into()),
    }
}

/// log_triage.py: next to the sources in a development checkout, in the
/// app's bundled port, or in the install's own copy of the tools.
fn triage_script(app: &AppHandle) -> Option<PathBuf> {
    let mut cands = Vec::new();
    if let Some(r) = std::env::var_os("INFAMOUS2_REPO") {
        cands.push(PathBuf::from(r).join("infamous2-lle/tools/log_triage.py"));
    }
    cands.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../infamous2-lle/tools/log_triage.py"));
    if let Ok(res) = app.path().resource_dir() {
        cands.push(res.join("port/infamous2-lle/tools/log_triage.py"));
    }
    if let Ok(inst) = resolve_install(Path::new(&load_settings_file().install_dir)) {
        cands.push(Path::new(&inst.port_dir).join("tools/log_triage.py"));
    }
    cands.into_iter().find(|p| p.is_file()).map(|p| p.canonicalize().unwrap_or(p))
}

/// Run log_triage.py on a report folder (it writes triage.md / triage.json there).
fn run_triage(app: &AppHandle, dir: &Path) -> Result<(), String> {
    let script = triage_script(app).ok_or("log_triage.py not found (rebuild or update the app)")?;
    let py = find_python().ok_or("no Python 3 found (Advanced > Install lists what is missing)")?;
    let out = Command::new(py)
        .arg("-I")
        .arg(&script)
        .arg(dir)
        .arg("--quiet")
        .env("PATH", tool_path())
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{}: {e}", script.display()))?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(format!("log_triage.py: {}", err.trim().lines().last().unwrap_or("failed")))
    }
}

/// Advanced > Report a problem: the debug sessions on disk, newest first.
#[tauri::command]
async fn report_list(state: State<'_, AppState>) -> Result<Vec<report::Info>, String> {
    let cur = current_report(&state);
    tauri::async_runtime::spawn_blocking(move || {
        report::list(&reports_dir(), 40)
            .into_iter()
            .map(|d| {
                let live = cur.as_ref().is_some_and(|(c, live)| *live && *c == d);
                report::info(&d, live)
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())
}

/// "Analyse": run log_triage.py on a session and return its summary.
#[tauri::command]
async fn report_triage(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<report::Info, String> {
    let (dir, live) = report_dir(&state, Some(&id))?;
    tauri::async_runtime::spawn_blocking(move || {
        run_triage(&app, &dir)?;
        Ok(report::info(&dir, live))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Pack a report (by id, or the current one) into
/// `<reports>/infamous2-report-<id>.zip`, with the player's description (if
/// any) as notes.txt and a fresh triage (when Python is there; the .zip is
/// made without it otherwise).
#[tauri::command]
async fn report_zip(app: AppHandle, state: State<'_, AppState>, id: Option<String>, notes: Option<String>) -> Result<report::Info, String> {
    let (dir, live) = report_dir(&state, id.as_deref())?;
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(n) = notes.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
            fs::write(dir.join(report::NOTES), format!("{n}\n")).map_err(|e| e.to_string())?;
        }
        let _ = run_triage(&app, &dir);
        report::pack(&dir, &report::zip_path(&dir), &home().to_string_lossy())?;
        Ok(report::info(&dir, live))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn stop_game(state: State<'_, AppState>) {
    if let Some(pid) = *state.game.lock().unwrap() {
        if let Some((d, true)) = state.report.lock().unwrap().clone() {
            session_event(&d, "stop", ""); // the player ended it: not a crash
        }
        kill_tree(pid);
    }
}

/// Reveal a folder or file in Finder / Explorer / the file manager.
#[tauri::command]
fn reveal(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    let target = if p.exists() { p } else { p.parent().map(Path::to_path_buf).unwrap_or(p) };
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    Command::new(program).arg(&target).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// The game's log: the one the last game this launcher started writes to (a
/// report folder's in debug mode), else the usual logs/game.log.
fn game_log_path(state: &State<'_, AppState>) -> PathBuf {
    state.last_log.lock().unwrap().clone().unwrap_or_else(|| data_root().join("logs/game.log"))
}

/// The end of a log for the viewer: at most `max` bytes, starting at a line
/// boundary when it had to cut, invalid UTF-8 replaced.
fn tail_text(data: &[u8], cut: bool) -> String {
    let start = if cut { data.iter().position(|&b| b == b'\n').map_or(0, |i| i + 1) } else { 0 };
    String::from_utf8_lossy(&data[start..]).into_owned()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameLog {
    path: String,
    exists: bool,
    size: u64,
    /// Seconds since the epoch.
    modified: u64,
    text: String,
    /// Only the end of the file is in `text`.
    truncated: bool,
}

/// Advanced > Game log: the tail of the game's log.
#[tauri::command]
fn read_game_log(state: State<'_, AppState>, max_bytes: Option<u64>) -> Result<GameLog, String> {
    let path = game_log_path(&state);
    let max = max_bytes.unwrap_or(512 * 1024).clamp(4 * 1024, 8 * 1024 * 1024);
    let mut out = GameLog {
        path: path.to_string_lossy().into_owned(),
        exists: false,
        size: 0,
        modified: 0,
        text: String::new(),
        truncated: false,
    };
    let Ok(mut f) = fs::File::open(&path) else { return Ok(out) };
    let meta = f.metadata().map_err(|e| e.to_string())?;
    out.exists = true;
    out.size = meta.len();
    out.modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    out.truncated = meta.len() > max;
    if out.truncated {
        f.seek(SeekFrom::Start(meta.len() - max)).map_err(|e| e.to_string())?;
    }
    let mut buf = Vec::with_capacity(max.min(meta.len()) as usize);
    f.take(max).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    out.text = tail_text(&buf, out.truncated);
    Ok(out)
}

#[tauri::command]
fn log_paths(state: State<'_, AppState>) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    let log = game_log_path(&state);
    m.insert("game".into(), log.to_string_lossy().into_owned());
    m.insert("saves".into(), gamedata_root(&load_settings_file().extra_env).to_string_lossy().into_owned());
    m
}

// ---------------------------------------------------------------------------
// Shader cache
// ---------------------------------------------------------------------------

/// The port's shader cache (PS3_SHADER_CACHE, the toolkit's Metal backend):
/// the extra variables' value, else the folder run.sh points windowed play
/// at; None when the variables turn it off (empty or 0).
fn shader_cache_dir(extra_env: &str) -> Option<PathBuf> {
    match parse_env_lines(extra_env).into_iter().find(|(k, _)| k == "PS3_SHADER_CACHE") {
        Some((_, v)) if v.is_empty() || v == "0" => None,
        Some((_, v)) => Some(PathBuf::from(v)),
        None => Some(data_root().join("shader-cache")),
    }
}

/// Bytes and files under `dir` (0, 0 when it does not exist).
fn dir_usage(dir: &Path) -> (u64, u64) {
    let (mut bytes, mut files) = (0u64, 0u64);
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            match e.metadata() {
                Ok(m) if m.is_dir() => {
                    let (b, f) = dir_usage(&e.path());
                    bytes += b;
                    files += f;
                }
                Ok(m) => {
                    bytes += m.len();
                    files += 1;
                }
                Err(_) => {}
            }
        }
    }
    (bytes, files)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderCacheInfo {
    path: Option<String>,
    bytes: u64,
    files: u64,
}

fn shader_cache_info_now() -> ShaderCacheInfo {
    match shader_cache_dir(&load_settings_file().extra_env) {
        Some(d) => {
            let (bytes, files) = dir_usage(&d);
            ShaderCacheInfo { path: Some(d.to_string_lossy().into_owned()), bytes, files }
        }
        None => ShaderCacheInfo { path: None, bytes: 0, files: 0 },
    }
}

#[tauri::command]
fn shader_cache_info() -> ShaderCacheInfo {
    shader_cache_info_now()
}

/// Delete the cache (the game rebuilds it as it meets each shader again).
/// Refused while the game runs: it writes there.
#[tauri::command]
fn shader_cache_clear(state: State<'_, AppState>) -> Result<ShaderCacheInfo, String> {
    let game = state.game.lock().unwrap();
    if game.is_some() || process_running(GAME_PROCESS) {
        return Err("the game is running: close it before clearing the shader cache".into());
    }
    if let Some(d) = shader_cache_dir(&load_settings_file().extra_env) {
        if d.exists() {
            fs::remove_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
    }
    drop(game);
    Ok(shader_cache_info_now())
}

// ---------------------------------------------------------------------------
// Saves
// ---------------------------------------------------------------------------

/// A process with this exact name is running (the game started from a
/// terminal, RPCS3).
fn process_running(name: &str) -> bool {
    if cfg!(unix) {
        Command::new("pgrep")
            .args(["-x", name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    } else {
        false
    }
}

const GAME_PROCESS: &str = "Infamous2Recomp";

/// Run a saves operation with the game held off: refused while the game
/// runs (the one the launcher started, or one started from a terminal), and
/// "Play" waits for it to finish.
fn with_saves<T>(state: &State<'_, AppState>, f: impl FnOnce(&saves::Locations) -> Result<T, String>) -> Result<T, String> {
    let game = state.game.lock().unwrap();
    if game.is_some() || process_running(GAME_PROCESS) {
        return Err("the game is running: close it before changing saves".into());
    }
    let r = f(&save_locations(&load_settings_file()));
    drop(game);
    r
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavesOverview {
    port: Vec<Slot>,
    rpcs3: Vec<Slot>,
    rpcs3_users: Vec<String>,
    backups: Vec<BackupMeta>,
    port_dir: String,
    rpcs3_dir: String,
    backups_dir: String,
    game_running: bool,
    rpcs3_running: bool,
}

#[tauri::command]
async fn saves_overview(state: State<'_, AppState>) -> Result<SavesOverview, String> {
    let loc = save_locations(&load_settings_file());
    Ok(SavesOverview {
        port: saves::list_slots(&loc, Side::Port, true),
        rpcs3: saves::list_slots(&loc, Side::Rpcs3, true),
        rpcs3_users: saves::rpcs3_users(&loc),
        backups: saves::list_backups(&loc),
        port_dir: loc.port_savedata().to_string_lossy().into_owned(),
        rpcs3_dir: loc.rpcs3_home.to_string_lossy().into_owned(),
        backups_dir: loc.backups.to_string_lossy().into_owned(),
        game_running: state.game.lock().unwrap().is_some() || process_running(GAME_PROCESS),
        rpcs3_running: process_running("rpcs3") || process_running("RPCS3"),
    })
}

/// Back up one slot, or every slot of that side (and the port's trophies).
#[tauri::command]
async fn saves_backup(state: State<'_, AppState>, side: Side, user: String, slot: Option<String>) -> Result<Option<BackupMeta>, String> {
    with_saves(&state, |loc| {
        let one = slot.map(|s| vec![s]);
        saves::create_backup(loc, side, &user, one.as_deref(), one.is_none(), "manual", now_secs())
    })
}

#[tauri::command]
async fn saves_restore(state: State<'_, AppState>, id: String, slot: Option<String>) -> Result<saves::Report, String> {
    with_saves(&state, |loc| saves::restore_backup(loc, &id, slot.as_deref(), now_secs()))
}

#[tauri::command]
async fn saves_delete_backup(state: State<'_, AppState>, id: String) -> Result<(), String> {
    with_saves(&state, |loc| saves::delete_backup(loc, &id))
}

#[tauri::command]
async fn saves_import_rpcs3(state: State<'_, AppState>, user: String, slot: String) -> Result<saves::Report, String> {
    with_saves(&state, |loc| saves::import_from_rpcs3(loc, &user, &slot, now_secs()))
}

#[tauri::command]
async fn saves_export_rpcs3(state: State<'_, AppState>, slot: String, user: String) -> Result<saves::Report, String> {
    with_saves(&state, |loc| saves::export_to_rpcs3(loc, &slot, &user, now_secs()))
}

#[tauri::command]
async fn saves_delete_slot(state: State<'_, AppState>, side: Side, user: String, slot: String) -> Result<BackupMeta, String> {
    with_saves(&state, |loc| saves::delete_slot(loc, side, &user, &slot, now_secs()))
}

/// The port's slots (all, or one) in a .zip at `dest` (".zip" added).
#[tauri::command]
async fn saves_export_zip(state: State<'_, AppState>, slots: Option<Vec<String>>, dest: String) -> Result<String, String> {
    let mut dest = PathBuf::from(dest.trim());
    if dest.as_os_str().is_empty() {
        return Err("no file chosen".into());
    }
    if !dest.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) {
        dest.as_mut_os_string().push(".zip");
    }
    with_saves(&state, |loc| saves::export_zip(loc, slots.as_deref(), &dest, now_secs()))?;
    Ok(dest.to_string_lossy().into_owned())
}

#[tauri::command]
async fn saves_import_zip(state: State<'_, AppState>, path: String) -> Result<saves::Report, String> {
    with_saves(&state, |loc| saves::import_zip(loc, Path::new(path.trim()), now_secs()))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .setup(|app| {
            sync_app_icon(app.handle()); // the player's icon in the Dock from the start
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            get_input_map,
            save_input_map,
            get_status,
            list_trophies,
            update::check_updates,
            detect_rpcs3,
            find_licence,
            check_prereqs,
            install_clt,
            install_brew,
            cancel_setup,
            open_url,
            start_rebuild,
            cancel_rebuild,
            launch_preview,
            play,
            stop_game,
            reveal,
            log_paths,
            read_game_log,
            shader_cache_info,
            shader_cache_clear,
            saves_overview,
            saves_backup,
            saves_restore,
            saves_delete_backup,
            saves_import_rpcs3,
            saves_export_rpcs3,
            saves_delete_slot,
            saves_export_zip,
            saves_import_zip,
            list_outputs,
            report_info,
            report_list,
            report_triage,
            report_capture,
            report_zip,
            finder_icon
        ])
        .run(tauri::generate_context!())
        .expect("error while running the launcher");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn dev_executable_prefers_the_bundle() {
        let build = std::env::temp_dir().join(format!("i2l-devexe-{}", std::process::id()));
        let _ = fs::remove_dir_all(&build);
        fs::create_dir_all(&build).unwrap();
        // A build from before the bundle: only the plain executable.
        assert_eq!(dev_executable(&build), build.join(exe_name()));
        let macos = build.join("Infamous2Recomp.app/Contents/MacOS");
        fs::create_dir_all(&macos).unwrap();
        fs::write(macos.join(exe_name()), b"").unwrap();
        let want = if cfg!(target_os = "macos") {
            macos.join(exe_name())
        } else {
            build.join(exe_name())
        };
        assert_eq!(dev_executable(&build), want);
        let _ = fs::remove_dir_all(&build);
    }

    #[test]
    fn env_lines() {
        let v = parse_env_lines("# c\nexport A=1\nB = \"x y\"\nbad line\n1-2=3\n");
        assert_eq!(v, vec![("A".into(), "1".into()), ("B".into(), "x y".into())]);
    }

    #[test]
    fn env_from_settings() {
        let inst = InstallInfo {
            layout: "install".into(),
            title: String::new(),
            app_ver: String::new(),
            port_dir: "/p".into(),
            run_sh: "/p/run.sh".into(),
            executable: "/p/build/x".into(),
            vfs: "/v".into(),
            dev_flash: "/f".into(),
            icon: String::new(),
            background: String::new(),
            built_at: String::new(),
            port_commit: String::new(),
            toolkit_commit: String::new(),
        };
        let none = InputEnv::default();
        let mut s = Settings::default();
        let env: BTreeMap<_, _> = launch_env(&s, &inst, &none).into_iter().collect();
        assert_eq!(env.get("PS3_LANG").map(String::as_str), Some("pt"));
        assert!(!env.contains_key("PS3RECOMP_ENG_GPU_LIGHT")); // run.sh default stays
        assert!(!env.contains_key("PS3_DISPLAY_GAMMA"));
        s.gpu_light = false;
        s.skip_occlusion = false;
        s.gamma_override = true;
        s.gamma = 1.5;
        s.mouse = false;
        let env: BTreeMap<_, _> = launch_env(&s, &inst, &none).into_iter().collect();
        assert_eq!(env.get("PS3RECOMP_ENG_GPU_LIGHT").map(String::as_str), Some(""));
        assert_eq!(env.get("PS3RECOMP_ENG_GPU_SSAO").map(String::as_str), Some(""));
        assert_eq!(env.get("PS3RECOMP_ENG_SKIP_FP").map(String::as_str), Some(""));
        assert_eq!(env.get("PS3_DISPLAY_GAMMA").map(String::as_str), Some("1.20"));
        assert_eq!(env.get("PS3_MOUSE").map(String::as_str), Some("0"));
        assert!(!env.contains_key("PS3_WINDOW_MODE")); // windowed is the default
        assert!(!env.contains_key("PS3_WINDOW_SIZE"));
        assert!(!env.contains_key("PS3_MOUSE_CAPTURE")); // no mouse, nothing to capture
        s.mouse = true;
        s.mouse_capture = false;
        s.window_mode = "borderless".into();
        s.window_size = "1920x1080".into();
        s.game_icon = true;
        let env: BTreeMap<_, _> = launch_env(&s, &inst, &none).into_iter().collect();
        assert_eq!(env.get("PS3_MOUSE_CAPTURE").map(String::as_str), Some("0"));
        assert_eq!(env.get("PS3_WINDOW_MODE").map(String::as_str), Some("borderless"));
        assert_eq!(env.get("PS3_WINDOW_SIZE").map(String::as_str), Some("1920x1080"));
        assert_eq!(env.get("PS3_WINDOW_ICON").map(String::as_str), Some("game"));
        s.window_size = "big".into();
        let env: BTreeMap<_, _> = launch_env(&s, &inst, &none).into_iter().collect();
        assert!(!env.contains_key("PS3_WINDOW_SIZE"));
        assert!(!env.contains_key("PS3_INPUT_MAP")); // no controls file yet
        assert!(!env.contains_key("PS3_MOUSE_SENS"));
    }

    #[test]
    fn env_display_audio_rumble() {
        let inst = InstallInfo {
            layout: "install".into(),
            title: String::new(),
            app_ver: String::new(),
            port_dir: "/p".into(),
            run_sh: "/p/run.sh".into(),
            executable: "/p/build/x".into(),
            vfs: "/v".into(),
            dev_flash: String::new(),
            icon: String::new(),
            background: String::new(),
            built_at: String::new(),
            port_commit: String::new(),
            toolkit_commit: String::new(),
        };
        let none = InputEnv::default();
        let mut s = Settings::default();
        let env: BTreeMap<_, _> = launch_env(&s, &inst, &none).into_iter().collect();
        for k in ["PS3_DISPLAY", "PS3_VSYNC", "PS3_AUDIO_VOLUME", "PS3_AUDIO_DEVICE", "PS3_RUMBLE", "PS3_RUMBLE_STRENGTH"] {
            assert!(!env.contains_key(k), "{k} is a default and must stay out");
        }
        s.display = "DELL U2720Q".into();
        s.vsync = false;
        s.audio_volume = 40;
        s.audio_device = " Erik's AirPods ".into();
        s.rumble_strength = 150;
        let env: BTreeMap<_, _> = launch_env(&s, &inst, &none).into_iter().collect();
        assert_eq!(env.get("PS3_DISPLAY").map(String::as_str), Some("DELL U2720Q"));
        assert_eq!(env.get("PS3_VSYNC").map(String::as_str), Some("0"));
        assert_eq!(env.get("PS3_AUDIO_VOLUME").map(String::as_str), Some("40"));
        assert_eq!(env.get("PS3_AUDIO_DEVICE").map(String::as_str), Some("Erik's AirPods"));
        assert_eq!(env.get("PS3_RUMBLE_STRENGTH").map(String::as_str), Some("150"));
        assert!(!env.contains_key("PS3_RUMBLE"));
        s.rumble = false;
        s.audio_volume = 0;
        let env: BTreeMap<_, _> = launch_env(&s, &inst, &none).into_iter().collect();
        assert_eq!(env.get("PS3_RUMBLE").map(String::as_str), Some("0"));
        assert!(!env.contains_key("PS3_RUMBLE_STRENGTH")); // off: the strength is moot
        assert_eq!(env.get("PS3_AUDIO_VOLUME").map(String::as_str), Some("0"));
        assert!(!env.contains_key("PS3RECOMP_METAL_FPS"));
        s.show_fps = true;
        let env: BTreeMap<_, _> = launch_env(&s, &inst, &none).into_iter().collect();
        assert_eq!(env.get("PS3RECOMP_METAL_FPS").map(String::as_str), Some("1"));
    }

    #[test]
    fn decryption_arguments() {
        let o = |keys: &str, firmware: &str, m: &str| DecryptOptions { keys: keys.into(), firmware: firmware.into(), decrypt_with: m.into() };
        assert!(decrypt_args(&DecryptOptions::default()).unwrap().is_empty());
        assert!(decrypt_args(&o("", "", "auto")).unwrap().is_empty());
        assert_eq!(
            decrypt_args(&o(" /k/keys ", "/d/PS3UPDAT.PUP", "python")).unwrap(),
            ["--decrypt-with", "python", "--keys", "/k/keys", "--firmware", "/d/PS3UPDAT.PUP"]
        );
        assert_eq!(decrypt_args(&o("", "/f/dev_flash", "rpcs3")).unwrap(), ["--decrypt-with", "rpcs3", "--firmware", "/f/dev_flash"]);
        assert!(decrypt_args(&o("", "", "--force")).is_err());
        // the request from the window: no "decrypt" at all is the old behaviour
        let r: RebuildRequest = serde_json::from_str(r#"{"games":[],"rap":"","out":"","dryRun":true,"force":false,"rapKey":null}"#).unwrap();
        assert!(decrypt_args(&r.decrypt).unwrap().is_empty());
        let r: RebuildRequest = serde_json::from_str(
            r#"{"games":[],"rap":"","out":"","dryRun":true,"force":false,"decrypt":{"keys":"/k","firmware":"","decryptWith":"python"}}"#,
        )
        .unwrap();
        assert_eq!(decrypt_args(&r.decrypt).unwrap(), ["--decrypt-with", "python", "--keys", "/k"]);
        // an old launcher.json without the new fields
        let s: Settings = serde_json::from_str(r#"{"uiLang":"en"}"#).unwrap();
        assert_eq!((s.decrypt_with.as_str(), s.show_fps, s.keys_path.as_str()), ("auto", false, ""));
    }

    #[test]
    fn env_graphics_options() {
        let inst = InstallInfo {
            layout: "install".into(),
            title: String::new(),
            app_ver: String::new(),
            port_dir: "/p".into(),
            run_sh: "/p/run.sh".into(),
            executable: "/p/build/x".into(),
            vfs: "/v".into(),
            dev_flash: String::new(),
            icon: String::new(),
            background: String::new(),
            built_at: String::new(),
            port_commit: String::new(),
            toolkit_commit: String::new(),
        };
        let none = InputEnv::default();
        let mut s = Settings::default();
        let env: BTreeMap<_, _> = launch_env(&s, &inst, &none).into_iter().collect();
        assert_eq!(env.get("PS3_VERBOSE").map(String::as_str), Some("0"), "normal play logs quietly");
        for k in ["PS3_ANISO", "PS3_AA", "PS3_SHARPEN", "PS3_UPSCALE"] {
            assert!(!env.contains_key(k), "{k} is a default and must stay out");
        }
        s.aniso = 32;
        s.aa = "fxaa".into();
        s.sharpen = 0.5;
        s.upscale = "metalfx".into();
        let env: BTreeMap<_, _> = launch_env(&s, &inst, &none).into_iter().collect();
        assert_eq!(env.get("PS3_ANISO").map(String::as_str), Some("16"));
        assert_eq!(env.get("PS3_AA").map(String::as_str), Some("fxaa"));
        assert_eq!(env.get("PS3_SHARPEN").map(String::as_str), Some("0.50"));
        assert_eq!(env.get("PS3_UPSCALE").map(String::as_str), Some("metalfx"));
        s.aa = "smaa".into();
        let env: BTreeMap<_, _> = launch_env(&s, &inst, &none).into_iter().collect();
        assert_eq!(env.get("PS3_AA").map(String::as_str), Some("smaa"));
        s.aa = "bogus".into();
        let env: BTreeMap<_, _> = launch_env(&s, &inst, &none).into_iter().collect();
        assert!(!env.contains_key("PS3_AA"), "an unknown AA mode stays out");
        s.aniso = 1;
        s.sharpen = 0.0;
        s.vibrance = 0.4;
        s.contrast = 1.2;
        let env2: BTreeMap<_, _> = launch_env(&s, &inst, &none).into_iter().collect();
        assert_eq!(env2.get("PS3_VIBRANCE").map(String::as_str), Some("0.40"));
        assert_eq!(env2.get("PS3_CONTRAST").map(String::as_str), Some("1.20"));
        s.vibrance = 0.0;
        s.contrast = 1.0;
        s.vignette = 0.5;
        s.lut_path = "/x/lut.png".into();
        s.lut_strength = 0.6;
        let env3: BTreeMap<_, _> = launch_env(&s, &inst, &none).into_iter().collect();
        assert_eq!(env3.get("PS3_VIGNETTE").map(String::as_str), Some("0.50"));
        assert_eq!(env3.get("PS3_LUT").map(String::as_str), Some("/x/lut.png"));
        assert_eq!(env3.get("PS3_LUT_STRENGTH").map(String::as_str), Some("0.60"));
        assert!(!env3.contains_key("PS3_GRAIN"));
        s.vignette = 0.0;
        s.lut_path = String::new();
        let env: BTreeMap<_, _> = launch_env(&s, &inst, &none).into_iter().collect();
        assert!(!env.contains_key("PS3_ANISO"));
        assert!(!env.contains_key("PS3_SHARPEN"));
    }

    #[test]
    fn audio_outputs_from_system_profiler() {
        let json = r#"{"SPAudioDataType":[{"_items":[
            {"_name":"Microfone (MacBook Pro)","coreaudio_device_input":1},
            {"_name":"Alto-falantes (MacBook Pro)","coreaudio_device_output":2,
             "coreaudio_default_audio_output_device":"spaudio_yes"},
            {"_name":"Erik's AirPods Pro","coreaudio_device_input":1,"coreaudio_device_output":2}
        ],"_name":"coreaudio_device"}]}"#;
        assert_eq!(parse_audio_outputs(json), vec!["Alto-falantes (MacBook Pro)", "Erik's AirPods Pro"]);
        assert!(parse_audio_outputs("not json").is_empty());
        assert_eq!(parse_screens("[\"Built-in Retina Display\",\"DELL U2720Q\"]\n"), vec!["Built-in Retina Display", "DELL U2720Q"]);
        assert!(parse_screens("").is_empty());
    }

    #[test]
    fn env_with_a_controls_file() {
        let inst = InstallInfo {
            layout: "install".into(),
            title: String::new(),
            app_ver: String::new(),
            port_dir: "/p".into(),
            run_sh: "/p/run.sh".into(),
            executable: "/p/build/x".into(),
            vfs: "/v".into(),
            dev_flash: String::new(),
            icon: String::new(),
            background: String::new(),
            built_at: String::new(),
            port_commit: String::new(),
            toolkit_commit: String::new(),
        };
        let mut s = Settings::default();
        let file = InputEnv { path: Some("/d/input.toml".into()), has_mouse_section: false };
        let env: BTreeMap<_, _> = launch_env(&s, &inst, &file).into_iter().collect();
        assert_eq!(env.get("PS3_INPUT_MAP").map(String::as_str), Some("/d/input.toml"));
        assert!(!env.contains_key("PS3_MOUSE_SENS")); // defaults stay out of the environment
        s.mouse_invert_x = true;
        let env: BTreeMap<_, _> = launch_env(&s, &inst, &file).into_iter().collect();
        assert_eq!(env.get("PS3_MOUSE_INVERT_X").map(String::as_str), Some("1"));
        // a [mouse] in the file: every mouse setting is passed, so the launcher's win
        let file = InputEnv { path: Some("/d/input.toml".into()), has_mouse_section: true };
        let env: BTreeMap<_, _> = launch_env(&s, &inst, &file).into_iter().collect();
        assert_eq!(env.get("PS3_MOUSE").map(String::as_str), Some("1"));
        assert_eq!(env.get("PS3_MOUSE_SENS").map(String::as_str), Some("1.00"));
        assert_eq!(env.get("PS3_MOUSE_INVERT_Y").map(String::as_str), Some("0"));
        assert_eq!(env.get("PS3_MOUSE_DEADZONE").map(String::as_str), Some("30"));
        assert_eq!(env.get("PS3_MOUSE_CAPTURE").map(String::as_str), Some("1"));
    }

    #[test]
    fn licence_found_from_the_game_folder() {
        let root = std::env::temp_dir().join(format!("i2l-rap-{}", std::process::id()));
        let game = root.join("dev_hdd0/game/NPUA80638");
        let exdata = root.join("dev_hdd0/home/00000001/exdata");
        fs::create_dir_all(game.join("USRDIR")).unwrap();
        fs::create_dir_all(&exdata).unwrap();
        fs::write(game.join("PARAM.SFO"), "TITLE=inFAMOUS 2\nAPP_VER=01.03\n").unwrap();
        let cid = "UP9000-NPUA80638_00-TESTTESTTEST0001";
        let mut eboot = vec![0u8; 0x200];
        eboot[0x80..0x84].copy_from_slice(b"NPD\0");
        eboot[0x90..0x90 + cid.len()].copy_from_slice(cid.as_bytes());
        fs::write(game.join("USRDIR/EBOOT.BIN"), &eboot).unwrap();
        fs::write(exdata.join("EP0000-NPUA80638_00-OTHER0000000000.rap"), [0u8; 16]).unwrap();
        fs::write(exdata.join(format!("{cid}.rap")), [1u8; 16]).unwrap();
        fs::write(exdata.join("UP9000-NPUA80638_00-BROKEN.rap"), [0u8; 3]).unwrap(); // not 16 bytes

        assert_eq!(content_id(&game.join("USRDIR/EBOOT.BIN")).as_deref(), Some(cid));
        // picking RPCS3's data folder or the game folder itself both work
        for pick in [root.clone(), game.clone()] {
            let r = search_licence(&[pick.to_string_lossy().into_owned()]);
            assert_eq!(r.game_dir.as_deref(), Some(game.to_string_lossy().as_ref()));
            assert_eq!(r.content_id.as_deref(), Some(cid));
            assert_eq!(r.app_ver.as_deref(), Some("01.03"));
            assert!(r.raps[0].ends_with(&format!("{cid}.rap")), "{:?}", r.raps);
            assert!(r.raps.iter().any(|p| p.ends_with("OTHER0000000000.rap")));
            assert!(!r.raps.iter().any(|p| p.contains("BROKEN")));
        }
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn content_id_from_pkg_header() {
        let p = std::env::temp_dir().join(format!("i2l-pkg-{}.pkg", std::process::id()));
        let mut h = vec![0u8; 0x100];
        h[..4].copy_from_slice(b"\x7fPKG");
        h[0x30..0x30 + 19].copy_from_slice(b"UP9000-NPUA80638_00");
        fs::write(&p, &h).unwrap();
        assert_eq!(content_id(&p).as_deref(), Some("UP9000-NPUA80638_00"));
        fs::remove_file(&p).unwrap();
    }

    #[test]
    fn brew_formulae_are_allowed() {
        for c in ["python", "cmake", "ninja", "sdl2", "glslang", "spirv-cross"] {
            assert!(BREW_ALLOWED.contains(&brew_formula(c).unwrap()));
        }
        assert!(brew_formula("rpcs3").is_none());
    }

    #[test]
    fn log_tail_starts_at_a_line() {
        assert_eq!(tail_text(b"whole\nfile\n", false), "whole\nfile\n");
        assert_eq!(tail_text(b"rtial line\nnext\nlast", true), "next\nlast");
        assert_eq!(tail_text(b"no newline at all", true), "no newline at all");
        assert_eq!(tail_text(b"x\n\xff\xfeok\n", true), "\u{fffd}\u{fffd}ok\n");
    }

    #[test]
    fn art_from_the_chosen_game_folder() {
        let root = std::env::temp_dir().join(format!("i2l-art-{}", std::process::id()));
        let game = root.join("dev_hdd0/game/NPUA80638");
        fs::create_dir_all(&game).unwrap();
        fs::write(game.join("PARAM.SFO"), "TITLE=inFAMOUS 2\n").unwrap();
        assert_eq!(copy_art(&[root.to_string_lossy().into_owned()]), (String::new(), String::new()));
        fs::write(game.join("ICON0.PNG"), b"png").unwrap();
        let (icon, pic) = copy_art(&["/nonexistent".into(), root.to_string_lossy().into_owned()]);
        assert!(icon.ends_with("NPUA80638/ICON0.PNG"), "{icon}");
        assert!(pic.ends_with("NPUA80638/PIC1.PNG"), "{pic}");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn text_sfo() {
        let dir = std::env::temp_dir().join(format!("i2l-sfo-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("PARAM.SFO");
        fs::write(&p, "TITLE=inFAMOUS 2\nSUB_TITLE=(Auto) 3. Forward\nDETAIL=1%\n").unwrap();
        let m = read_sfo(&p).unwrap();
        assert_eq!(m.get("SUB_TITLE").map(String::as_str), Some("(Auto) 3. Forward"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
