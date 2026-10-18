//! Launcher core: settings, the rebuild (infamous2-lle/tools/rebuild.py) as a
//! child process whose output is streamed to the window, and "Play", which
//! starts the port's own run.sh with the settings mapped onto its environment
//! variables. No game code or data is part of the app: everything it shows or
//! starts comes from the player's own install folder.

mod input_map;
mod saves;
mod update;
mod zipfile;

use input_map::{Bindings, InputEnv};
use saves::{read_sfo, BackupMeta, Side, Slot};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Read};
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
    /// PS3RECOMP_ENG_GPU_LIGHT: the SPU light pass on the GPU.
    pub gpu_light: bool,
    /// PS3RECOMP_ENG_GPU_SSAO: the SSAO job on the GPU (needs gpu_light).
    pub gpu_ssao: bool,
    /// PS3RECOMP_ENG_SKIP_FP: skip the occlusion-volume passes that black out facades.
    pub skip_occlusion: bool,
    /// PS3_DISPLAY_GAMMA when set; otherwise the game's own brightness setting.
    pub gamma_override: bool,
    pub gamma: f32,
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
    /// PS3_WINDOW_ICON=game: the player's own ICON0.PNG in the Dock instead
    /// of the placeholder.
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
    /// Extra KEY=VALUE lines (diagnostics), one per line; '#' starts a comment.
    pub extra_env: String,
    /// Remembered inputs of the install screen.
    pub game_paths: Vec<String>,
    pub rap_path: String,
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
            gpu_light: true,
            gpu_ssao: true,
            skip_occlusion: true,
            gamma_override: false,
            gamma: 1.0,
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
            extra_env: String::new(),
            game_paths: Vec::new(),
            rap_path: String::new(),
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
    let exe = port.join("build").join(exe_name());
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
fn save_settings(settings: Settings) -> Result<(), String> {
    let p = settings_path();
    if let Some(d) = p.parent() {
        fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = p.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    fs::rename(&tmp, &p).map_err(|e| e.to_string())
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
}

#[tauri::command]
async fn get_status(app: AppHandle, state: State<'_, AppState>) -> Result<Status, String> {
    let s = load_settings_file();
    let dir = PathBuf::from(&s.install_dir);
    let (install, install_error) = match resolve_install(&dir) {
        Ok(i) => (Some(i), None),
        Err(e) => (None, Some(e)),
    };
    let icon = install.as_ref().and_then(|i| data_url(&i.icon, "image/png"));
    let background = install.as_ref().and_then(|i| data_url(&i.background, "image/png"));
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
    })
}

// ---------------------------------------------------------------------------
// Finding the player's copy and licence
// ---------------------------------------------------------------------------

const TITLE_ID: &str = "NPUA80638";

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

#[tauri::command]
async fn check_prereqs(app: AppHandle) -> Prereqs {
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
    let out = Command::new(&python)
        .arg(&script)
        .arg("--check-json")
        .env("PATH", tool_path())
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output();
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
    let mut s: Vec<String> = launch_env(&settings, &inst, &input_env())
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
}

#[tauri::command]
fn play(app: AppHandle, state: State<'_, AppState>) -> Result<PlayStarted, String> {
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
    let logs = data_root().join("logs");
    fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
    let log_path = logs.join("game.log");
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
    for (k, v) in launch_env(&s, &inst, &input_env()) {
        cmd.env(k, v);
    }
    own_process_group(&mut cmd);
    let mut child = cmd.spawn().map_err(|e| format!("{}: {e}", inst.run_sh))?;
    *slot = Some(child.id());
    drop(slot);
    let a = app.clone();
    thread::spawn(move || {
        let code = child.wait().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
        *a.state::<AppState>().game.lock().unwrap() = None;
        let _ = a.emit("game-exit", code);
    });
    Ok(PlayStarted { log: log_path.to_string_lossy().into_owned(), backup, backup_error })
}

#[tauri::command]
fn stop_game(state: State<'_, AppState>) {
    if let Some(pid) = *state.game.lock().unwrap() {
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

#[tauri::command]
fn log_paths() -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert("game".into(), data_root().join("logs/game.log").to_string_lossy().into_owned());
    m.insert("saves".into(), gamedata_root(&load_settings_file().extra_env).to_string_lossy().into_owned());
    m
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
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            get_input_map,
            save_input_map,
            get_status,
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
            saves_overview,
            saves_backup,
            saves_restore,
            saves_delete_backup,
            saves_import_rpcs3,
            saves_export_rpcs3,
            saves_delete_slot,
            saves_export_zip,
            saves_import_zip,
            list_outputs
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
