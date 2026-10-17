//! Launcher core: settings, the rebuild (infamous2-lle/tools/rebuild.py) as a
//! child process whose output is streamed to the window, and "Play", which
//! starts the port's own run.sh with the settings mapped onto its environment
//! variables. No game code or data is part of the app: everything it shows or
//! starts comes from the player's own install folder.

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
    /// PS3_MOUSE, PS3_MOUSE_SENS, PS3_MOUSE_INVERT_Y, PS3_MOUSE_DEADZONE.
    pub mouse: bool,
    pub mouse_sens: f32,
    pub mouse_invert_y: bool,
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
    /// Extra KEY=VALUE lines (diagnostics), one per line; '#' starts a comment.
    pub extra_env: String,
    /// Remembered inputs of the install screen.
    pub game_paths: Vec<String>,
    pub rap_path: String,
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
            mouse_deadzone: 30,
            mouse_capture: true,
            window_mode: "windowed".into(),
            window_size: String::new(),
            game_icon: false,
            extra_env: String::new(),
            game_paths: Vec::new(),
            rap_path: String::new(),
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

/// PARAM.SFO, binary (the game's) or the KEY=VALUE text the runtime writes
/// for saves.
fn read_sfo(path: &Path) -> Option<BTreeMap<String, String>> {
    let d = fs::read(path).ok()?;
    let mut out = BTreeMap::new();
    if d.len() >= 20 && &d[..4] == b"\0PSF" {
        let u32le = |o: usize| -> Option<u32> {
            Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?))
        };
        let u16le = |o: usize| -> Option<u16> {
            Some(u16::from_le_bytes(d.get(o..o + 2)?.try_into().ok()?))
        };
        let (kt, dt, n) = (u32le(8)? as usize, u32le(12)? as usize, u32le(16)? as usize);
        for k in 0..n.min(256) {
            let e = 20 + 16 * k;
            let (ko, fmt, len, off) = (u16le(e)? as usize, u16le(e + 2)?, u32le(e + 4)? as usize, u32le(e + 12)? as usize);
            let key_bytes = d.get(kt + ko..)?;
            let key_end = key_bytes.iter().position(|&b| b == 0)?;
            let key = String::from_utf8_lossy(&key_bytes[..key_end]).into_owned();
            let val = d.get(dt + off..dt + off + len)?;
            let v = if fmt == 0x404 && len >= 4 {
                u32::from_le_bytes(val[..4].try_into().ok()?).to_string()
            } else {
                String::from_utf8_lossy(val).trim_end_matches('\0').to_string()
            };
            out.insert(key, v);
        }
    } else {
        for line in String::from_utf8_lossy(&d).lines() {
            if let Some((k, v)) = line.split_once('=') {
                out.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }
    Some(out)
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

/// The most recent save in the folder run.sh points windowed play at.
fn last_save(extra_env: &str) -> Option<SaveInfo> {
    let root = parse_env_lines(extra_env)
        .into_iter()
        .find(|(k, _)| k == "PS3_GAMEDATA_ROOT")
        .map(|(_, v)| PathBuf::from(v))
        .unwrap_or_else(|| data_root().join("gamedata"));
    let saves = root.join("dev_hdd0/home/00000001/savedata");
    let mut best: Option<SaveInfo> = None;
    for e in fs::read_dir(&saves).ok()?.flatten() {
        let sfo_path = e.path().join("PARAM.SFO");
        let modified = fs::metadata(e.path().join("STATE"))
            .or_else(|_| fs::metadata(&sfo_path))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let Some(sfo) = read_sfo(&sfo_path) else { continue };
        if best.as_ref().map_or(true, |b| modified > b.modified) {
            best = Some(SaveInfo {
                title: sfo.get("TITLE").cloned().unwrap_or_default(),
                subtitle: sfo.get("SUB_TITLE").cloned().unwrap_or_default(),
                detail: sfo.get("DETAIL").cloned().unwrap_or_default(),
                modified,
                folder: e.path().to_string_lossy().into_owned(),
            });
        }
    }
    best
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
/// lives in <repo>/launcher), or in the app's bundled resources later on.
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
fn launch_env(s: &Settings, inst: &InstallInfo) -> Vec<(String, String)> {
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
    if !s.mouse {
        set("PS3_MOUSE", "0".into());
    } else {
        if (s.mouse_sens - 1.0).abs() > 1e-3 {
            set("PS3_MOUSE_SENS", format!("{:.2}", s.mouse_sens));
        }
        if s.mouse_invert_y {
            set("PS3_MOUSE_INVERT_Y", "1".into());
        }
        if s.mouse_deadzone != 30 {
            set("PS3_MOUSE_DEADZONE", s.mouse_deadzone.min(120).to_string());
        }
        if !s.mouse_capture {
            set("PS3_MOUSE_CAPTURE", "0".into());
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

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

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
        last_save: last_save(&s.extra_env),
        game_running: state.game.lock().unwrap().is_some(),
        rebuild_running: state.rebuild.lock().unwrap().is_some(),
        rebuild_script: rebuild_script(&app).map(|p| p.to_string_lossy().into_owned()),
        data_root: data_root().to_string_lossy().into_owned(),
        default_install_dir: default_install_dir().to_string_lossy().into_owned(),
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detected {
    game_dir: Option<String>,
    raps: Vec<String>,
}

/// The RPCS3 install of NPUA80638 and its licences, to prefill the form.
#[tauri::command]
fn detect_rpcs3() -> Detected {
    let r = rpcs3_dir();
    let game = r.join("dev_hdd0/game/NPUA80638");
    let mut raps: Vec<String> = fs::read_dir(r.join("dev_hdd0/home/00000001/exdata"))
        .map(|it| {
            it.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    let n = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    n.contains("NPUA80638") && n.to_lowercase().ends_with(".rap")
                })
                .map(|p| p.to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    raps.sort();
    Detected {
        game_dir: game.join("PARAM.SFO").is_file().then(|| game.to_string_lossy().into_owned()),
        raps,
    }
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
    let mut slot = state.rebuild.lock().unwrap();
    if slot.is_some() {
        return Err("a rebuild is already running".into());
    }
    let script = rebuild_script(&app).ok_or("rebuild.py not found (set INFAMOUS2_REPO to the repository)")?;
    let python = if cfg!(windows) { "python" } else { "python3" };
    let mut cmd = Command::new(python);
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
    let mut child = cmd.spawn().map_err(|e| format!("{python}: {e}"))?;
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
    let mut s: Vec<String> = launch_env(&settings, &inst)
        .into_iter()
        .map(|(k, v)| format!("{k}={}", shell_quote(&v)))
        .collect();
    s.push(shell_quote(&inst.run_sh));
    Ok(s.join(" "))
}

#[tauri::command]
fn play(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
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
    for (k, v) in launch_env(&s, &inst) {
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
    Ok(log_path.to_string_lossy().into_owned())
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
    m.insert("saves".into(), data_root().join("gamedata").to_string_lossy().into_owned());
    m
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            get_status,
            detect_rpcs3,
            start_rebuild,
            cancel_rebuild,
            launch_preview,
            play,
            stop_game,
            reveal,
            log_paths
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
        let mut s = Settings::default();
        let env: BTreeMap<_, _> = launch_env(&s, &inst).into_iter().collect();
        assert_eq!(env.get("PS3_LANG").map(String::as_str), Some("pt"));
        assert!(!env.contains_key("PS3RECOMP_ENG_GPU_LIGHT")); // run.sh default stays
        assert!(!env.contains_key("PS3_DISPLAY_GAMMA"));
        s.gpu_light = false;
        s.skip_occlusion = false;
        s.gamma_override = true;
        s.gamma = 1.5;
        s.mouse = false;
        let env: BTreeMap<_, _> = launch_env(&s, &inst).into_iter().collect();
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
        let env: BTreeMap<_, _> = launch_env(&s, &inst).into_iter().collect();
        assert_eq!(env.get("PS3_MOUSE_CAPTURE").map(String::as_str), Some("0"));
        assert_eq!(env.get("PS3_WINDOW_MODE").map(String::as_str), Some("borderless"));
        assert_eq!(env.get("PS3_WINDOW_SIZE").map(String::as_str), Some("1920x1080"));
        assert_eq!(env.get("PS3_WINDOW_ICON").map(String::as_str), Some("game"));
        s.window_size = "big".into();
        let env: BTreeMap<_, _> = launch_env(&s, &inst).into_iter().collect();
        assert!(!env.contains_key("PS3_WINDOW_SIZE"));
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
