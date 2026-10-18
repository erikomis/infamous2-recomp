//! "Is there something newer?" for the Home screen.
//!
//! Two separate questions:
//! - **Rebuild**: the installed game (install.json's `port_commit`, written by
//!   rebuild.py) was built from other port sources than the ones this app
//!   runs rebuild.py from (the bundled `port/BUILD_INFO.json`, or the git
//!   checkout in development). After the app is updated, that means the game
//!   should be rebuilt; rebuild.py then only redoes the steps whose inputs
//!   changed. Answered locally, no network.
//! - **New release**: the newest published GitHub release of the repository,
//!   compared with this app's version. One unauthenticated request to the
//!   GitHub API (through the system's `curl`), only when the Home screen asks;
//!   `INFAMOUS2_NO_UPDATE_CHECK=1` turns it off. Drafts and pre-releases are
//!   never offered (GitHub's "latest" endpoint skips them).

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tauri::AppHandle;

const RELEASES_API: &str = "https://api.github.com/repos/erikomis/infamous2-recomp/releases/latest";
/// Only links into the repository are offered to open (see `open_url`).
const RELEASES_PAGE: &str = "https://github.com/erikomis/infamous2-recomp/releases";

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
    /// Whether the online check ran (it is skipped when not asked or turned off).
    checked_online: bool,
    latest_version: Option<String>,
    latest_url: Option<String>,
    newer_release: bool,
    check_error: Option<String>,
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

fn is_newer(latest: &str, current: &str) -> bool {
    match (version_parts(latest), version_parts(current)) {
        (Some(mut a), Some(mut b)) => {
            let n = a.len().max(b.len());
            a.resize(n, 0);
            b.resize(n, 0);
            a > b
        }
        _ => false,
    }
}

/// tag and page of a GitHub "latest release" answer (None for a draft or
/// pre-release, or a page outside the repository).
fn parse_release(json: &str) -> Option<(String, String)> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    if v.get("draft").and_then(|d| d.as_bool()) == Some(true)
        || v.get("prerelease").and_then(|d| d.as_bool()) == Some(true)
    {
        return None;
    }
    let tag = v.get("tag_name")?.as_str()?.trim().to_string();
    let url = v
        .get("html_url")
        .and_then(|u| u.as_str())
        .filter(|u| u.starts_with(RELEASES_PAGE) && !u.chars().any(|c| c.is_whitespace() || c == '"'))
        .map(str::to_string)
        .unwrap_or_else(|| RELEASES_PAGE.to_string());
    (!tag.is_empty()).then_some((tag, url))
}

/// Ok(None): the repository has no published release yet (404).
fn fetch_latest() -> Result<Option<(String, String)>, String> {
    let out = Command::new("curl")
        .args([
            "-sSL",
            "--max-time",
            "8",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: infamous2-recomp-launcher",
            "-w",
            "\n%{http_code}",
            RELEASES_API,
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, code) = text.rsplit_once('\n').unwrap_or(("", text.as_ref()));
    match code.trim() {
        "200" => parse_release(body).map(Some).ok_or_else(|| "unexpected answer from GitHub".to_string()),
        "404" => Ok(None),
        "000" | "" => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        c => Err(format!("GitHub answered HTTP {c}")),
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

#[tauri::command]
pub async fn check_updates(app: AppHandle, online: bool) -> UpdateInfo {
    let app_version = app.package_info().version.to_string();
    let sources = crate::rebuild_script(&app).and_then(|p| sources_commit(&p));
    let s = crate::load_settings_file();
    let installed = crate::resolve_install(Path::new(&s.install_dir)).ok().map(|i| i.port_commit);
    let mut info = local_info(app_version, sources, installed);
    if online && std::env::var_os("INFAMOUS2_NO_UPDATE_CHECK").map_or(true, |v| v.is_empty() || v == "0") {
        let r = tauri::async_runtime::spawn_blocking(fetch_latest).await.unwrap_or_else(|e| Err(e.to_string()));
        info.checked_online = true;
        match r {
            Ok(Some((tag, url))) => {
                info.newer_release = is_newer(&tag, &info.app_version);
                info.latest_version = Some(tag);
                info.latest_url = Some(url);
            }
            Ok(None) => {}
            Err(e) => info.check_error = Some(e),
        }
    }
    info
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
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.1", "0.1.0"));
        assert!(!is_newer("v0.0.9", "0.1.0"));
        assert!(!is_newer("nightly", "0.1.0"));
        assert!(is_newer("v0.2.0-rc1", "0.1.0"));
    }

    #[test]
    fn build_info() {
        assert_eq!(build_info_commit(r#"{"schema":1,"port_commit":"77bb792-dirty"}"#).as_deref(), Some("77bb792-dirty"));
        assert_eq!(build_info_commit(r#"{"schema":1,"port_commit":""}"#), None);
        assert_eq!(build_info_commit("not json"), None);
    }

    #[test]
    fn release_answer() {
        let r = parse_release(
            r#"{"tag_name":"v0.2.0","html_url":"https://github.com/erikomis/infamous2-recomp/releases/tag/v0.2.0","draft":false,"prerelease":false}"#,
        );
        assert_eq!(
            r,
            Some(("v0.2.0".into(), "https://github.com/erikomis/infamous2-recomp/releases/tag/v0.2.0".into()))
        );
        assert_eq!(parse_release(r#"{"tag_name":"v0.2.0","draft":true}"#), None);
        assert_eq!(parse_release(r#"{"tag_name":"v0.2.0","prerelease":true}"#), None);
        // a page elsewhere is replaced by the repository's releases page
        assert_eq!(
            parse_release(r#"{"tag_name":"v0.2.0","html_url":"https://example.com/x"}"#).map(|r| r.1).as_deref(),
            Some(RELEASES_PAGE)
        );
        assert_eq!(parse_release(r#"{"message":"Not Found"}"#), None);
    }

    #[test]
    fn repo_root_of_rebuild_script() {
        assert_eq!(
            repo_of(Path::new("/r/port/infamous2-lle/tools/rebuild.py")),
            Some(PathBuf::from("/r/port"))
        );
    }
}
