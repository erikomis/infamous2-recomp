//! "Is there something newer?" for the Home screen, and the app update itself.
//!
//! Two separate questions:
//! - **Rebuild**: the installed game (install.json's `port_commit`, written by
//!   rebuild.py) was built from other port sources than the ones this app
//!   runs rebuild.py from (the bundled `port/BUILD_INFO.json`, or the git
//!   checkout in development). After the app is updated, that means the game
//!   should be rebuilt; rebuild.py then only redoes the steps whose inputs
//!   changed. Answered locally, no network.
//! - **New app version**: the update manifest (`latest.json`) the release
//!   workflows publish, compared with this app's version. Fetched with the
//!   system's `curl` from the first of `ENDPOINTS` that has one (the same
//!   list as `plugins.updater.endpoints` in tauri.conf.json): the fixed
//!   `updater` release, which `.github/workflows/updater.yml` points at the
//!   newest *published* version (pre-releases included, drafts never), then
//!   GitHub's "latest release" for when releases stop being pre-releases.
//!
//! **Download and install** (macOS, a packaged .app): tauri-plugin-updater
//! checks the same endpoints, downloads the `.app.tar.gz` the manifest names,
//! verifies it against the public key in tauri.conf.json (a minisign key
//! whose private half only the release workflow has; the signature must also
//! carry the announced version, `requireSignedVersion`) and swaps the bundle
//! in place. "Restart to finish" then relaunches into the new version, whose
//! Updates card offers the game rebuild as usual. Not while the game or a
//! rebuild runs, and not from the disk image or a translocated copy (macOS
//! runs a downloaded app from a read-only place until it is moved).
//!
//! `INFAMOUS2_NO_UPDATE_CHECK=1` turns all of it off; the setting "Check for
//! updates automatically" (on by default) only skips the check at start.

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// Where the update manifest is looked for, in order (tauri.conf.json's
/// `plugins.updater.endpoints` must be the same; a test checks).
pub const ENDPOINTS: [&str; 2] = [
    "https://github.com/erikomis/infamous2-recomp/releases/download/updater/latest.json",
    "https://github.com/erikomis/infamous2-recomp/releases/latest/download/latest.json",
];
/// Only links into the repository are offered to open (see `open_url`).
const RELEASES_PAGE: &str = "https://github.com/erikomis/infamous2-recomp/releases";
/// The manifest's entry for this build (the only one the releases carry).
const PLATFORM: &str = "darwin-aarch64";

#[derive(Serialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    app_version: String,
    /// Commit of the port sources this app rebuilds from.
    sources_commit: Option<String>,
    /// Commit the installed game was built from.
    installed_commit: Option<String>,
    /// Both are known and differ: the game should be rebuilt.
    rebuild_suggested: bool,
    /// INFAMOUS2_NO_UPDATE_CHECK is set: no check, no install.
    disabled: bool,
    /// Whether the online check ran (it is skipped when not asked or turned off).
    checked_online: bool,
    latest_version: Option<String>,
    latest_url: Option<String>,
    latest_notes: Option<String>,
    latest_date: Option<String>,
    newer_release: bool,
    check_error: Option<String>,
    /// Why "Download and install" cannot work here (None: it can):
    /// "platform" (not macOS on Apple Silicon, or no download for it),
    /// "dev" (not a packaged .app), "dmg" (running from the disk image),
    /// "translocated" (macOS runs it from a read-only copy until it is moved).
    install_blocker: Option<String>,
    /// A version installed in this session, waiting for the restart.
    installed_pending: Option<String>,
}

/// The repository root a rebuild.py path lives in (`<root>/infamous2-lle/tools/rebuild.py`).
fn repo_of(rebuild_py: &Path) -> Option<PathBuf> {
    Some(rebuild_py.parent()?.parent()?.parent()?.to_path_buf())
}

/// `port_commit` of a BUILD_INFO.json (stage_port.py), as rebuild.py copies it.
fn build_info_commit(text: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let c = v.get("port_commit")?.as_str()?.trim();
    (!c.is_empty()).then(|| c.to_string())
}

/// The commit rebuild.py would write into install.json from these sources:
/// BUILD_INFO.json when bundled, else `git rev-parse --short HEAD`.
fn sources_commit(rebuild_py: &Path) -> Option<String> {
    let root = repo_of(rebuild_py)?;
    if let Ok(text) = fs::read_to_string(root.join("BUILD_INFO.json")) {
        return build_info_commit(&text);
    }
    let out = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["rev-parse", "--short", "HEAD"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let c = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (out.status.success() && !c.is_empty()).then_some(c)
}

/// Same commit, whatever the abbreviation length and a `-dirty` mark.
fn same_commit(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.trim().trim_end_matches("-dirty").to_ascii_lowercase();
    let (a, b) = (norm(a), norm(b));
    !a.is_empty() && !b.is_empty() && (a.starts_with(&b) || b.starts_with(&a))
}

/// "v1.2.3" / "1.2" -> [1, 2, 3] / [1, 2]; anything else -> None.
fn version_parts(v: &str) -> Option<Vec<u64>> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let core = v.split(['-', '+']).next()?;
    core.split('.').map(|p| p.parse::<u64>().ok()).collect()
}

/// Whether `latest` is newer than `current`, as the updater decides it
/// (semver: a pre-release such as `1.0.0-rc1` comes before `1.0.0`).
fn is_newer(latest: &str, current: &str) -> bool {
    match (version_parts(latest), version_parts(current)) {
        (Some(mut a), Some(mut b)) => {
            let n = a.len().max(b.len());
            a.resize(n, 0);
            b.resize(n, 0);
            if a != b {
                return a > b;
            }
            // the same numbers: a release beats a pre-release of it
            let pre = |v: &str| v.split('+').next().unwrap_or("").contains('-');
            pre(current) && !pre(latest)
        }
        _ => false,
    }
}

/// What the app shows of an update manifest.
#[derive(Debug, PartialEq)]
struct Manifest {
    version: String,
    notes: Option<String>,
    pub_date: Option<String>,
    /// It has a download for this build (darwin-aarch64), with a signature.
    has_platform: bool,
}

/// An update manifest (`latest.json`, scripts/updater_json.py) in the shape
/// tauri-plugin-updater reads: version, notes, pub_date and
/// platforms.<os>-<arch>.{url, signature}.
fn parse_manifest(json: &str) -> Option<Manifest> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let version = v.get("version")?.as_str()?.trim().trim_start_matches('v').to_string();
    version_parts(&version)?;
    let text = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let has_platform = v.get("platforms").and_then(|p| p.get(PLATFORM)).is_some_and(|p| {
        p.get("url").and_then(|u| u.as_str()).is_some_and(|u| u.starts_with("https://"))
            && p.get("signature").and_then(|s| s.as_str()).is_some_and(|s| !s.trim().is_empty())
    });
    Some(Manifest { version, notes: text("notes"), pub_date: text("pub_date"), has_platform })
}

/// The release page of a version.
fn release_page(version: &str) -> String {
    format!("{RELEASES_PAGE}/tag/v{}", version.trim_start_matches('v'))
}

/// The first endpoint's manifest. Ok(None): none published yet (404 everywhere).
fn fetch_latest() -> Result<Option<Manifest>, String> {
    let mut last_err = None;
    for url in ENDPOINTS {
        let out = Command::new("curl")
            .args([
                "-sSL",
                "--max-time",
                "8",
                "-H",
                "Accept: application/json",
                "-H",
                "User-Agent: infamous2-recomp-launcher",
                "-w",
                "\n%{http_code}",
                url,
            ])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("curl: {e}"))?;
        let text = String::from_utf8_lossy(&out.stdout);
        let (body, code) = text.rsplit_once('\n').unwrap_or(("", text.as_ref()));
        match code.trim() {
            "200" => match parse_manifest(body) {
                Some(m) => return Ok(Some(m)),
                None => last_err = Some("unexpected answer from GitHub".to_string()),
            },
            "404" => {}
            "000" | "" => last_err = Some(String::from_utf8_lossy(&out.stderr).trim().to_string()),
            c => last_err = Some(format!("GitHub answered HTTP {c}")),
        }
    }
    match last_err {
        Some(e) => Err(e),
        None => Ok(None),
    }
}

fn local_info(app_version: String, sources: Option<String>, installed: Option<String>) -> UpdateInfo {
    let installed = installed.filter(|c| !c.trim().is_empty());
    let rebuild_suggested = match (&sources, &installed) {
        (Some(s), Some(i)) => !same_commit(s, i),
        _ => false,
    };
    UpdateInfo { app_version, sources_commit: sources, installed_commit: installed, rebuild_suggested, ..Default::default() }
}

/// `INFAMOUS2_NO_UPDATE_CHECK` set to anything but empty or 0.
pub fn disabled() -> bool {
    std::env::var_os("INFAMOUS2_NO_UPDATE_CHECK").is_some_and(|v| !v.is_empty() && v != "0")
}

/// Why the bundle at `bundle` cannot be replaced in place (see UpdateInfo).
fn blocker_for(bundle: Option<&Path>) -> Option<&'static str> {
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return Some("platform");
    }
    let Some(b) = bundle else { return Some("dev") };
    if b.to_string_lossy().contains("/AppTranslocation/") {
        Some("translocated")
    } else if b.starts_with("/Volumes") && !writable_dir(b.parent()) {
        Some("dmg")
    } else {
        None
    }
}

/// Whether a file can be made in `dir` (not a mounted disk image).
fn writable_dir(dir: Option<&Path>) -> bool {
    let Some(dir) = dir else { return false };
    let probe = dir.join(format!(".infamous2-write-test-{}", std::process::id()));
    match fs::write(&probe, b"") {
        Ok(()) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn blocker() -> Option<&'static str> {
    blocker_for(crate::app_icon::app_bundle().as_deref())
}

#[tauri::command]
pub async fn check_updates(app: AppHandle, online: bool) -> UpdateInfo {
    let app_version = app.package_info().version.to_string();
    let sources = crate::rebuild_script(&app).and_then(|p| sources_commit(&p));
    let s = crate::load_settings_file();
    let installed = crate::resolve_install(Path::new(&s.install_dir)).ok().map(|i| i.port_commit);
    let mut info = local_info(app_version, sources, installed);
    info.disabled = disabled();
    info.installed_pending = app.state::<crate::AppState>().update_installed.lock().unwrap().clone();
    info.install_blocker = blocker().map(str::to_string);
    if online && !info.disabled {
        let r = tauri::async_runtime::spawn_blocking(fetch_latest).await.unwrap_or_else(|e| Err(e.to_string()));
        info.checked_online = true;
        match r {
            Ok(Some(m)) => {
                info.newer_release = is_newer(&m.version, &info.app_version);
                if !m.has_platform && info.install_blocker.is_none() {
                    info.install_blocker = Some("platform".into());
                }
                info.latest_url = Some(release_page(&m.version));
                info.latest_version = Some(m.version);
                info.latest_notes = m.notes;
                info.latest_date = m.pub_date;
            }
            Ok(None) => {}
            Err(e) => info.check_error = Some(e),
        }
    }
    info
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Progress {
    downloaded: u64,
    total: Option<u64>,
    /// "download", then "install" once the file is in and verified.
    phase: &'static str,
}

/// Clears AppState::updating however install_update ends.
struct Updating(AppHandle);

impl Drop for Updating {
    fn drop(&mut self) {
        *self.0.state::<crate::AppState>().updating.lock().unwrap() = false;
    }
}

/// "Download and install": the newest version from the manifest, verified
/// and swapped in (progress as `update-progress` events). Returns the
/// version now on disk; the app runs the old one until restart_app.
/// Errors that the window explains start with "blocked:<reason>".
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<String, String> {
    use tauri_plugin_updater::UpdaterExt;
    if disabled() {
        return Err("updates are turned off (INFAMOUS2_NO_UPDATE_CHECK)".into());
    }
    if let Some(b) = blocker() {
        return Err(format!("blocked:{b}"));
    }
    let guard = {
        let st = app.state::<crate::AppState>();
        if let Some(v) = st.update_installed.lock().unwrap().clone() {
            return Err(format!("version {v} is already installed: restart the app to finish"));
        }
        let mut updating = st.updating.lock().unwrap();
        if *updating {
            return Err("the update is already being installed".into());
        }
        if st.game.lock().unwrap().is_some() || crate::process_running(crate::GAME_PROCESS) {
            return Err("blocked:game".into());
        }
        if st.rebuild.lock().unwrap().is_some() {
            return Err("blocked:rebuild".into());
        }
        *updating = true;
        Updating(app.clone())
    };
    let updater = app.updater_builder().timeout(Duration::from_secs(300)).build().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "there is no newer version".to_string())?;
    let version = update.version.clone();
    let (a, b) = (app.clone(), app.clone());
    let mut got = 0u64;
    let mut last: Option<Instant> = None;
    update
        .download_and_install(
            move |chunk, total| {
                got += chunk as u64;
                if last.is_none_or(|t| t.elapsed() >= Duration::from_millis(100)) || total == Some(got) {
                    last = Some(Instant::now());
                    let _ = a.emit("update-progress", Progress { downloaded: got, total, phase: "download" });
                }
            },
            move || {
                let _ = b.emit("update-progress", Progress { downloaded: 0, total: None, phase: "install" });
            },
        )
        .await
        .map_err(|e| e.to_string())?;
    *app.state::<crate::AppState>().update_installed.lock().unwrap() = Some(version.clone());
    drop(guard);
    Ok(version)
}

/// "Restart to finish": relaunch into the installed version.
#[tauri::command]
pub fn restart_app(app: AppHandle) -> Result<(), String> {
    let st = app.state::<crate::AppState>();
    if st.game.lock().unwrap().is_some() {
        return Err("blocked:game".into());
    }
    if st.rebuild.lock().unwrap().is_some() {
        return Err("blocked:rebuild".into());
    }
    app.restart()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commits_match_across_abbreviations() {
        assert!(same_commit("77bb792", "77bb792"));
        assert!(same_commit("77bb792-dirty", "77bb7920a1"));
        assert!(same_commit("77BB792", "77bb792"));
        assert!(!same_commit("77bb792", "e75a505"));
        assert!(!same_commit("", "77bb792"));
    }

    #[test]
    fn rebuild_only_when_both_known_and_different() {
        let v = || "0.1.0".to_string();
        assert!(local_info(v(), Some("aaaa111".into()), Some("bbbb222".into())).rebuild_suggested);
        assert!(!local_info(v(), Some("aaaa111".into()), Some("aaaa111".into())).rebuild_suggested);
        // a development checkout's install has no commit; nothing bundled in `tauri dev` without git
        assert!(!local_info(v(), Some("aaaa111".into()), Some(String::new())).rebuild_suggested);
        assert!(!local_info(v(), None, Some("aaaa111".into())).rebuild_suggested);
    }

    #[test]
    fn versions() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("v0.1.1", "0.1"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("0.2.10", "0.2.9"));
        assert!(is_newer("0.2.7", "0.2.6"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.1", "0.1.0"));
        assert!(!is_newer("v0.0.9", "0.1.0"));
        assert!(!is_newer("0.2.6", "0.2.7"));
        assert!(!is_newer("nightly", "0.1.0"));
        assert!(is_newer("v0.2.0-rc1", "0.1.0"));
        // as semver (and so the updater) orders them
        assert!(is_newer("0.3.0", "0.3.0-rc1"));
        assert!(!is_newer("0.3.0-rc1", "0.3.0"));
        assert!(!is_newer("0.3.0+build5", "0.3.0"));
    }

    #[test]
    fn build_info() {
        assert_eq!(build_info_commit(r#"{"schema":1,"port_commit":"77bb792-dirty"}"#).as_deref(), Some("77bb792-dirty"));
        assert_eq!(build_info_commit(r#"{"schema":1,"port_commit":""}"#), None);
        assert_eq!(build_info_commit("not json"), None);
    }

    /// What scripts/updater_json.py writes (test_updater_json.py checks the
    /// script's side of it).
    const MANIFEST: &str = r#"{
      "version": "0.2.7",
      "notes": "Launcher v0.2.7.",
      "pub_date": "2026-10-23T12:00:00Z",
      "platforms": {
        "darwin-aarch64": {
          "url": "https://github.com/erikomis/infamous2-recomp/releases/download/v0.2.7/infamous2-recomp_0.2.7_aarch64.app.tar.gz",
          "signature": "dW50cnVzdGVk"
        }
      }
    }"#;

    #[test]
    fn manifest() {
        let m = parse_manifest(MANIFEST).unwrap();
        assert_eq!(
            m,
            Manifest {
                version: "0.2.7".into(),
                notes: Some("Launcher v0.2.7.".into()),
                pub_date: Some("2026-10-23T12:00:00Z".into()),
                has_platform: true,
            }
        );
        assert_eq!(release_page(&m.version), "https://github.com/erikomis/infamous2-recomp/releases/tag/v0.2.7");
        // a "v" is dropped; an entry without a signature does not count
        let m = parse_manifest(r#"{"version":"v0.3.0","platforms":{"darwin-aarch64":{"url":"https://x/y","signature":""}}}"#).unwrap();
        assert_eq!((m.version.as_str(), m.has_platform, m.notes), ("0.3.0", false, None));
        let other = r#"{"version":"0.3.0","platforms":{"windows-x86_64":{"url":"https://x","signature":"s"}}}"#;
        assert!(!parse_manifest(other).unwrap().has_platform);
        assert_eq!(parse_manifest(r#"{"version":"nightly"}"#), None);
        assert_eq!(parse_manifest(r#"{"message":"Not Found"}"#), None);
        assert_eq!(parse_manifest("Not Found"), None);
    }

    fn decode_b64(s: &str) -> Vec<u8> {
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        let (mut acc, mut bits) = (0u32, 0);
        for c in s.trim().bytes().filter(|&c| c != b'=') {
            acc = (acc << 6 | T.iter().position(|&t| t == c).expect("base64") as u32) & 0xffff;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
            }
        }
        out
    }

    /// The updater's settings in tauri.conf.json: the same endpoints as
    /// here, a minisign public key, the signed version required, and the
    /// updater artifacts only in the CI-only config (a local `tauri build`
    /// without the private key keeps working).
    #[test]
    fn updater_config() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let u = &conf["plugins"]["updater"];
        let eps: Vec<&str> = u["endpoints"].as_array().unwrap().iter().map(|e| e.as_str().unwrap()).collect();
        assert_eq!(eps, ENDPOINTS);
        assert_eq!(u["requireSignedVersion"], serde_json::Value::Bool(true));
        let text = String::from_utf8(decode_b64(u["pubkey"].as_str().unwrap())).unwrap();
        assert!(text.starts_with("untrusted comment: minisign public key"), "{text}");
        let raw = decode_b64(text.lines().nth(1).unwrap());
        assert_eq!(raw.len(), 42, "\"Ed\", the key id and the 32-byte key");
        assert_eq!(&raw[..2], b"Ed");
        assert!(conf["bundle"].get("createUpdaterArtifacts").is_none());
        let ci: serde_json::Value = serde_json::from_str(include_str!("../tauri.updater.conf.json")).unwrap();
        assert_eq!(ci["bundle"]["createUpdaterArtifacts"], serde_json::Value::Bool(true));
    }

    #[test]
    fn install_blockers() {
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            assert_eq!(blocker_for(None), Some("dev"));
            let t = Path::new("/private/var/folders/x/T/AppTranslocation/ABC/d/infamous2-recomp.app");
            assert_eq!(blocker_for(Some(t)), Some("translocated"));
            assert_eq!(blocker_for(Some(Path::new("/Volumes/infamous2-recomp-none/infamous2-recomp.app"))), Some("dmg"));
            assert_eq!(blocker_for(Some(&std::env::temp_dir().join("infamous2-recomp.app"))), None);
        } else {
            assert_eq!(blocker_for(None), Some("platform"));
        }
    }

    #[test]
    fn repo_root_of_rebuild_script() {
        assert_eq!(repo_of(Path::new("/r/port/infamous2-lle/tools/rebuild.py")), Some(PathBuf::from("/r/port")));
    }
}
