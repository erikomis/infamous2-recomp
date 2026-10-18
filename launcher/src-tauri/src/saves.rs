//! The saves manager: the game's save slots in the port's gamedata and in
//! RPCS3, timestamped backups, restore, RPCS3 import/export and .zip
//! archives.
//!
//! A slot is a flat folder (`BCUS98125_INFAMOUS2_0`: PARAM.SFO, ICON0.PNG,
//! STATE, maybe PARAM.PFD). The two sides differ only in the metadata:
//! - the port (ps3recomp's cellSaveData) writes and reads PARAM.SFO as
//!   `KEY=VALUE` text (TITLE, SUB_TITLE, DETAIL, ATTRIBUTE, LIST_PARAM) and
//!   keeps the names of the files the game wrote as SECUREFILE in PARAM.PFD,
//!   one per line;
//! - RPCS3 writes a binary PSF PARAM.SFO with the full key set, and one
//!   integer `*<file>` key per file (1 = secure); it writes no PARAM.PFD.
//!
//! The game's own files (STATE, ICON0.PNG) are copied byte for byte; only the
//! metadata is converted when a slot crosses sides. Every operation that
//! replaces or removes a slot backs it up first, and a slot is swapped in
//! whole (built next to it, then renamed), so a failed copy leaves the old
//! one in place.
//!
//! Everything here takes its folders from `Locations`, so the tests run on
//! temporary directories.

use crate::zipfile;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Save folders of this game: the disc id the game uses for its saves (the
/// PSN release NPUA80638 saves as BCUS98125 too) and the PSN id.
pub const GAME_PREFIXES: &[&str] = &["BCUS98125", "NPUA80638"];
/// The PS3 user the port's cellSaveData writes under.
pub const PORT_USER: &str = "00000001";
const MANIFEST: &str = "backup.json";
const ZIP_MANIFEST: &str = "infamous2-saves.json";
/// The slot's metadata, never a game file.
const SFO: &str = "PARAM.SFO";
const PFD: &str = "PARAM.PFD";
/// Files cellSaveData types as content, never SECUREFILE.
const CONTENT_FILES: &[&str] = &["ICON0.PNG", "ICON1.PAM", "PIC1.PNG", "SND0.AT3"];

pub struct Locations {
    /// The port's gamedata (PS3_GAMEDATA_ROOT): saves and trophies.
    pub port_root: PathBuf,
    /// RPCS3's `dev_hdd0/home` (one folder per PS3 user).
    pub rpcs3_home: PathBuf,
    /// Where backups go (`<data>/save-backups`).
    pub backups: PathBuf,
}

impl Locations {
    pub fn port_savedata(&self) -> PathBuf {
        self.port_root.join("dev_hdd0/home").join(PORT_USER).join("savedata")
    }
    pub fn port_trophies(&self) -> PathBuf {
        self.port_root.join("trophies")
    }
    pub fn rpcs3_savedata(&self, user: &str) -> PathBuf {
        self.rpcs3_home.join(user).join("savedata")
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    #[default]
    Port,
    Rpcs3,
}

impl Side {
    fn label(self) -> &'static str {
        match self {
            Side::Port => "port",
            Side::Rpcs3 => "rpcs3",
        }
    }
}

// ---------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------

/// A save folder of this game, with a name that is safe to join onto a path.
pub fn valid_slot(name: &str) -> bool {
    name.len() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && GAME_PREFIXES.iter().any(|p| name.len() > p.len() && name.starts_with(p))
}

/// A PS3 user folder: eight digits.
pub fn valid_user(u: &str) -> bool {
    u.len() == 8 && u.bytes().all(|b| b.is_ascii_digit())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('.')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// One path component from an archive or a PFD line.
fn safe_component(n: &str) -> bool {
    !n.is_empty() && n != "." && n != ".." && n.len() <= 255 && !n.contains(['/', '\\', '\0', ':'])
}

fn savedata_dir(loc: &Locations, side: Side, user: &str) -> Result<PathBuf, String> {
    match side {
        Side::Port => Ok(loc.port_savedata()),
        Side::Rpcs3 if valid_user(user) => Ok(loc.rpcs3_savedata(user)),
        Side::Rpcs3 => Err(format!("{user:?}: not an RPCS3 user folder")),
    }
}

fn check_slot(name: &str) -> Result<(), String> {
    if valid_slot(name) {
        Ok(())
    } else {
        Err(format!("{name:?}: not an inFAMOUS 2 save folder"))
    }
}

// ---------------------------------------------------------------------------
// PARAM.SFO
// ---------------------------------------------------------------------------

pub const PSF_UTF8_SPECIAL: u16 = 0x0004;
pub const PSF_UTF8: u16 = 0x0204;
pub const PSF_INT: u16 = 0x0404;

/// One key of a binary PSF (PARAM.SFO), kept whole so a rewrite changes only
/// the keys it means to.
#[derive(Clone, Debug, PartialEq)]
pub struct PsfEntry {
    pub key: String,
    pub fmt: u16,
    /// Bytes used (strings include their NUL).
    pub data: Vec<u8>,
    pub max_len: u32,
}

impl PsfEntry {
    fn int(key: &str, v: u32) -> Self {
        PsfEntry { key: key.into(), fmt: PSF_INT, data: v.to_le_bytes().to_vec(), max_len: 4 }
    }
    fn string(key: &str, v: &str, max_len: u32) -> Self {
        let mut e = PsfEntry { key: key.into(), fmt: PSF_UTF8, data: Vec::new(), max_len };
        e.set_str(v);
        e
    }
    /// A NUL-terminated UTF-8 value, cut (on a character boundary) to fit.
    fn set_str(&mut self, v: &str) {
        let room = (self.max_len as usize).saturating_sub(1);
        let mut end = v.len().min(room);
        while !v.is_char_boundary(end) {
            end -= 1;
        }
        self.data = v.as_bytes()[..end].to_vec();
        self.data.push(0);
    }
    fn as_int(&self) -> Option<u32> {
        (self.fmt == PSF_INT && self.data.len() >= 4).then(|| u32::from_le_bytes(self.data[..4].try_into().unwrap()))
    }
    fn as_string(&self) -> String {
        match self.as_int() {
            Some(n) => n.to_string(),
            None => String::from_utf8_lossy(&self.data).trim_end_matches('\0').to_string(),
        }
    }
}

pub fn psf_parse(d: &[u8]) -> Option<Vec<PsfEntry>> {
    if d.len() < 20 || &d[..4] != b"\0PSF" {
        return None;
    }
    let u32le = |o: usize| -> Option<u32> { Some(u32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?)) };
    let u16le = |o: usize| -> Option<u16> { Some(u16::from_le_bytes(d.get(o..o + 2)?.try_into().ok()?)) };
    let (kt, dt, n) = (u32le(8)? as usize, u32le(12)? as usize, u32le(16)? as usize);
    let mut out = Vec::new();
    for k in 0..n.min(512) {
        let e = 20 + 16 * k;
        let (ko, fmt, len, max, off) =
            (u16le(e)? as usize, u16le(e + 2)?, u32le(e + 4)? as usize, u32le(e + 8)?, u32le(e + 12)? as usize);
        let key_bytes = d.get(kt + ko..)?;
        let key_end = key_bytes.iter().position(|&b| b == 0)?;
        let key = String::from_utf8_lossy(&key_bytes[..key_end]).into_owned();
        let data = d.get(dt + off..dt + off + len)?.to_vec();
        out.push(PsfEntry { key, fmt, data, max_len: max.max(len as u32) });
    }
    Some(out)
}

/// A PSF as the PS3 (and RPCS3) lays it out: keys sorted, key table padded
/// to 4 bytes, each value padded to its max length.
pub fn psf_write(entries: &[PsfEntry]) -> Vec<u8> {
    let mut es: Vec<&PsfEntry> = entries.iter().collect();
    es.sort_by(|a, b| a.key.cmp(&b.key));
    let mut keys = Vec::new();
    let mut index = Vec::new();
    let mut data = Vec::new();
    for e in &es {
        let max = (e.max_len as usize).max(e.data.len());
        index.extend_from_slice(&(keys.len() as u16).to_le_bytes());
        index.extend_from_slice(&e.fmt.to_le_bytes());
        index.extend_from_slice(&(e.data.len() as u32).to_le_bytes());
        index.extend_from_slice(&(max as u32).to_le_bytes());
        index.extend_from_slice(&(data.len() as u32).to_le_bytes());
        keys.extend_from_slice(e.key.as_bytes());
        keys.push(0);
        data.extend_from_slice(&e.data);
        data.resize(data.len() + max - e.data.len(), 0);
    }
    while keys.len() % 4 != 0 {
        keys.push(0);
    }
    let kt = 20 + index.len();
    let dt = kt + keys.len();
    let mut out = Vec::with_capacity(dt + data.len());
    out.extend_from_slice(b"\0PSF");
    out.extend_from_slice(&0x0101u32.to_le_bytes());
    out.extend_from_slice(&(kt as u32).to_le_bytes());
    out.extend_from_slice(&(dt as u32).to_le_bytes());
    out.extend_from_slice(&(es.len() as u32).to_le_bytes());
    out.extend_from_slice(&index);
    out.extend_from_slice(&keys);
    out.extend_from_slice(&data);
    out
}

/// The port's text PARAM.SFO. A line that does not start with `KEY=` belongs
/// to the value above it (DETAIL holds newlines), so the values come back as
/// the game wrote them.
fn parse_text_sfo(text: &str) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut last: Option<String> = None;
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let key = line
            .split_once('=')
            .map(|(k, _)| k)
            .filter(|k| !k.is_empty() && k.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'));
        match (key, &last) {
            (Some(k), _) => {
                out.insert(k.to_string(), line[k.len() + 1..].to_string());
                last = Some(k.to_string());
            }
            (None, Some(k)) => {
                let v = out.get_mut(k).unwrap();
                v.push('\n');
                v.push_str(line);
            }
            (None, None) => {}
        }
    }
    // the file's own final newline is not part of the last value
    if let Some(v) = last.and_then(|k| out.get_mut(&k)) {
        if v.ends_with('\n') {
            v.pop();
        }
    }
    out
}

/// PARAM.SFO as key -> value: binary PSF (the game's, RPCS3's saves) or the
/// port's KEY=VALUE text.
pub fn sfo_values(d: &[u8]) -> BTreeMap<String, String> {
    match psf_parse(d) {
        Some(es) => es.into_iter().map(|e| (e.key.clone(), e.as_string())).collect(),
        None => parse_text_sfo(&String::from_utf8_lossy(d)),
    }
}

pub fn read_sfo(path: &Path) -> Option<BTreeMap<String, String>> {
    fs::read(path).ok().map(|d| sfo_values(&d))
}

/// The text PARAM.SFO the port's cellSaveData writes (and reads back).
fn port_sfo_text(v: &BTreeMap<String, String>) -> String {
    let g = |k: &str| v.get(k).map(String::as_str).unwrap_or("");
    let list = v.get("LIST_PARAM").or_else(|| v.get("SAVEDATA_LIST_PARAM")).map(String::as_str).unwrap_or("");
    format!(
        "TITLE={}\nSUB_TITLE={}\nDETAIL={}\nATTRIBUTE={}\nLIST_PARAM={}\n",
        g("TITLE"),
        g("SUB_TITLE"),
        g("DETAIL"),
        g("ATTRIBUTE").parse::<u32>().unwrap_or(0),
        list
    )
}

/// A PARAM.SFO for RPCS3 with the keys RPCS3 itself writes for a save.
fn rpcs3_sfo_new(slot: &str, v: &BTreeMap<String, String>, files: &[(String, bool)]) -> Vec<u8> {
    let g = |k: &str| v.get(k).map(String::as_str).unwrap_or("");
    let mut es = vec![
        PsfEntry { key: "ACCOUNT_ID".into(), fmt: PSF_UTF8_SPECIAL, data: b"0000000000000000".to_vec(), max_len: 16 },
        PsfEntry::int("ATTRIBUTE", g("ATTRIBUTE").parse().unwrap_or(0)),
        PsfEntry::string("CATEGORY", "SD", 4),
        PsfEntry::string("DETAIL", g("DETAIL"), 1024),
        PsfEntry::string("PARAMS", "", 1024),
        PsfEntry::string("PARAMS2", "", 12),
        PsfEntry::int("PARENTAL_LEVEL", 0),
        PsfEntry::string("SAVEDATA_DIRECTORY", slot, 32),
        PsfEntry::string("SAVEDATA_LIST_PARAM", v.get("LIST_PARAM").map(String::as_str).unwrap_or(""), 8),
        PsfEntry::string("SUB_TITLE", g("SUB_TITLE"), 128),
        PsfEntry::string("TITLE", g("TITLE"), 128),
    ];
    let blist: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    let blist = blist.join("/");
    es.push(PsfEntry::string("RPCS3_BLIST", &blist, (blist.len() as u32 + 1).div_ceil(4) * 4));
    for (n, secure) in files {
        es.push(PsfEntry::int(&format!("*{n}"), u32::from(*secure)));
    }
    psf_write(&es)
}

/// RPCS3's own PARAM.SFO for a slot, kept whole: only the titles shown in its
/// save list follow the port's save, and files it has no `*<file>` key for
/// get one.
fn rpcs3_sfo_update(mut es: Vec<PsfEntry>, v: &BTreeMap<String, String>, files: &[(String, bool)]) -> Vec<u8> {
    for e in es.iter_mut() {
        if matches!(e.key.as_str(), "TITLE" | "SUB_TITLE" | "DETAIL") && e.fmt == PSF_UTF8 {
            if let Some(val) = v.get(&e.key).filter(|s| !s.is_empty()) {
                e.set_str(val);
            }
        }
    }
    for (n, secure) in files {
        let k = format!("*{n}");
        if !es.iter().any(|e| e.key == k) {
            es.push(PsfEntry::int(&k, u32::from(*secure)));
        }
    }
    psf_write(&es)
}

/// The port's PARAM.PFD (file names, one per line), if `d` is one.
fn text_pfd(d: &[u8]) -> Option<Vec<String>> {
    let s = std::str::from_utf8(d).ok()?;
    let names: Vec<String> = s.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
    names.iter().all(|n| safe_component(n) && n.len() < 64).then_some(names)
}

// ---------------------------------------------------------------------------
// Files
// ---------------------------------------------------------------------------

fn io<T>(r: std::io::Result<T>, p: &Path) -> Result<T, String> {
    r.map_err(|e| format!("{}: {e}", p.display()))
}

fn mtime(p: &Path) -> u64 {
    fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn set_mtime(p: &Path, secs: u64) {
    if secs == 0 {
        return;
    }
    if let Ok(f) = fs::OpenOptions::new().write(true).open(p) {
        let _ = f.set_modified(UNIX_EPOCH + std::time::Duration::from_secs(secs));
    }
}

/// Copy a file and keep its modification time (the date the list shows).
fn copy_file(src: &Path, dst: &Path) -> Result<(), String> {
    io(fs::copy(src, dst), src)?;
    if let Ok(t) = fs::metadata(src).and_then(|m| m.modified()) {
        if let Ok(f) = fs::OpenOptions::new().write(true).open(dst) {
            let _ = f.set_modified(t);
        }
    }
    Ok(())
}

/// Copy a folder tree (links are skipped), keeping modification times.
fn copy_tree(src: &Path, dst: &Path, skip: &[&str]) -> Result<(), String> {
    io(fs::create_dir_all(dst), dst)?;
    for e in io(fs::read_dir(src), src)?.flatten() {
        let name = e.file_name();
        if skip.iter().any(|s| name == *s) {
            continue;
        }
        let Ok(ft) = e.file_type() else { continue };
        let (from, to) = (e.path(), dst.join(&name));
        if ft.is_dir() {
            copy_tree(&from, &to, &[])?;
        } else if ft.is_file() {
            copy_file(&from, &to)?;
        }
    }
    Ok(())
}

/// Every regular file under `dir`, as ('/'-joined relative path, full path).
fn walk(dir: &Path) -> Vec<(String, PathBuf)> {
    fn go(base: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) {
        let Ok(it) = fs::read_dir(base) else { return };
        for e in it.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            let name = e.file_name().to_string_lossy().into_owned();
            let r = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            if ft.is_dir() {
                go(&e.path(), &r, out);
            } else if ft.is_file() {
                out.push((r, e.path()));
            }
        }
    }
    let mut out = Vec::new();
    go(dir, "", &mut out);
    out.sort();
    out
}

fn tree_size(dir: &Path) -> u64 {
    walk(dir).iter().filter_map(|(_, p)| fs::metadata(p).ok()).map(|m| m.len()).sum()
}

/// FNV-1a, to tell two STATE files (or two backups) apart.
fn fnv64(d: &[u8]) -> u64 {
    d.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// Contents of `dir` as (path under `prefix`, hash) pairs.
fn digest(dir: &Path, prefix: &str, out: &mut Vec<(String, u64)>) {
    for (rel, p) in walk(dir) {
        out.push((format!("{prefix}/{rel}"), fs::read(&p).map(|d| fnv64(&d)).unwrap_or(0)));
    }
}

/// Swap a slot (or the trophies folder) in whole: build the new one next to
/// it, move the old one aside, rename, and only then delete the old one. A
/// failure on the way leaves the old one where it was.
fn replace_dir(dest: &Path, fill: impl FnOnce(&Path) -> Result<(), String>) -> Result<(), String> {
    let parent = dest.parent().ok_or("no parent folder")?;
    let name = dest.file_name().ok_or("no folder name")?.to_string_lossy().into_owned();
    io(fs::create_dir_all(parent), parent)?;
    let tmp = parent.join(format!(".{name}.incoming"));
    let old = parent.join(format!(".{name}.previous"));
    let _ = fs::remove_dir_all(&tmp);
    io(fs::create_dir_all(&tmp), &tmp)?;
    if let Err(e) = fill(&tmp) {
        let _ = fs::remove_dir_all(&tmp);
        return Err(e);
    }
    let had_old = dest.exists();
    if had_old {
        let _ = fs::remove_dir_all(&old);
        if let Err(e) = fs::rename(dest, &old) {
            let _ = fs::remove_dir_all(&tmp);
            return Err(format!("{}: {e}", dest.display()));
        }
    }
    if let Err(e) = fs::rename(&tmp, dest) {
        if had_old {
            let _ = fs::rename(&old, dest);
        }
        let _ = fs::remove_dir_all(&tmp);
        return Err(format!("{}: {e}", dest.display()));
    }
    if had_old {
        let _ = fs::remove_dir_all(&old);
    }
    Ok(())
}

/// UTC "YYYYMMDD-HHMMSS" for backup folder names.
fn stamp(secs: u64) -> String {
    let (y, m, d) = zipfile::civil_from_days((secs / 86_400) as i64);
    let r = secs % 86_400;
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}", r / 3600, r % 3600 / 60, r % 60)
}

// ---------------------------------------------------------------------------
// Slots
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    pub side: Side,
    /// RPCS3 user folder ("00000001" for the port).
    pub user: String,
    pub name: String,
    pub path: String,
    pub title: String,
    pub subtitle: String,
    pub detail: String,
    /// Seconds since the epoch: the STATE file's date.
    pub modified: u64,
    pub size: u64,
    pub files: u32,
    /// ICON0.PNG as a data: URL.
    pub icon: Option<String>,
    /// Hash of STATE (the progress itself), to compare the two sides.
    pub state_hash: Option<String>,
    /// "text" (the port's), "binary" (RPCS3's) or "none".
    pub sfo: String,
}

fn read_slot(dir: &Path, side: Side, user: &str, icons: bool) -> Option<Slot> {
    let name = dir.file_name()?.to_string_lossy().into_owned();
    if !valid_slot(&name) || !dir.is_dir() {
        return None;
    }
    let files = walk(dir);
    let raw = fs::read(dir.join(SFO)).ok();
    let v = raw.as_deref().map(sfo_values).unwrap_or_default();
    let modified = [dir.join("STATE"), dir.join(SFO)]
        .iter()
        .map(|p| mtime(p))
        .find(|&t| t > 0)
        .unwrap_or_else(|| files.iter().map(|(_, p)| mtime(p)).max().unwrap_or(0));
    let icon = if icons {
        fs::read(dir.join("ICON0.PNG")).ok().map(|d| format!("data:image/png;base64,{}", crate::base64(&d)))
    } else {
        None
    };
    Some(Slot {
        side,
        user: user.to_string(),
        path: dir.to_string_lossy().into_owned(),
        title: v.get("TITLE").cloned().unwrap_or_default(),
        subtitle: v.get("SUB_TITLE").cloned().unwrap_or_default(),
        detail: v.get("DETAIL").cloned().unwrap_or_default(),
        modified,
        size: files.iter().filter_map(|(_, p)| fs::metadata(p).ok()).map(|m| m.len()).sum(),
        files: files.len() as u32,
        icon,
        state_hash: fs::read(dir.join("STATE")).ok().map(|d| format!("{:016x}", fnv64(&d))),
        sfo: match raw {
            None => "none",
            Some(d) if psf_parse(&d).is_some() => "binary",
            Some(_) => "text",
        }
        .into(),
        name,
    })
}

fn slots_in(dir: &Path, side: Side, user: &str, icons: bool) -> Vec<Slot> {
    let mut v: Vec<Slot> = fs::read_dir(dir)
        .map(|it| it.flatten().filter_map(|e| read_slot(&e.path(), side, user, icons)).collect())
        .unwrap_or_default();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

fn slot_names(dir: &Path) -> Vec<String> {
    slots_in(dir, Side::Port, "", false).into_iter().map(|s| s.name).collect()
}

/// RPCS3's PS3 users (dev_hdd0/home/<8 digits>).
pub fn rpcs3_users(loc: &Locations) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(&loc.rpcs3_home)
        .map(|it| {
            it.flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| valid_user(n))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

pub fn list_slots(loc: &Locations, side: Side, icons: bool) -> Vec<Slot> {
    match side {
        Side::Port => slots_in(&loc.port_savedata(), Side::Port, PORT_USER, icons),
        Side::Rpcs3 => rpcs3_users(loc)
            .iter()
            .flat_map(|u| slots_in(&loc.rpcs3_savedata(u), Side::Rpcs3, u, icons))
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// Backups
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct BackupMeta {
    /// The folder name under save-backups/.
    pub id: String,
    pub created: u64,
    /// "manual", "before-play", "before-restore", "before-import",
    /// "before-export", "before-zip-import", "before-delete".
    pub reason: String,
    pub source: Side,
    pub user: String,
    pub slots: Vec<String>,
    /// The port's trophies folder is in it too.
    pub trophies: bool,
    pub size: u64,
    /// Title and progress of each slot when it was taken (slot -> subtitle).
    pub labels: BTreeMap<String, String>,
}

/// Back up slots of one side to `<backups>/<UTC stamp>-<side>-<reason>/`
/// (`savedata/<slot>/...`, `trophies/...`, `backup.json`). `slots: None`
/// takes every slot of the game there. Nothing to copy: `Ok(None)`.
pub fn create_backup(
    loc: &Locations,
    side: Side,
    user: &str,
    slots: Option<&[String]>,
    trophies: bool,
    reason: &str,
    now: u64,
) -> Result<Option<BackupMeta>, String> {
    let dir = savedata_dir(loc, side, user)?;
    let names: Vec<String> = match slots {
        Some(s) => {
            for n in s {
                check_slot(n)?;
            }
            s.iter().filter(|n| dir.join(n).is_dir()).cloned().collect()
        }
        None => slot_names(&dir),
    };
    let tro = trophies && side == Side::Port && !walk(&loc.port_trophies()).is_empty();
    if names.is_empty() && !tro {
        return Ok(None);
    }
    io(fs::create_dir_all(&loc.backups), &loc.backups)?;
    let base = format!("{}-{}-{}", stamp(now), side.label(), reason);
    let mut id = base.clone();
    let mut k = 2;
    while loc.backups.join(&id).exists() {
        id = format!("{base}-{k}");
        k += 1;
    }
    let tmp = loc.backups.join(format!(".{id}.partial"));
    let _ = fs::remove_dir_all(&tmp);
    let result = (|| -> Result<BackupMeta, String> {
        let mut labels = BTreeMap::new();
        for n in &names {
            copy_tree(&dir.join(n), &tmp.join("savedata").join(n), &[])?;
            let v = read_sfo(&dir.join(n).join(SFO)).unwrap_or_default();
            let label = v.get("SUB_TITLE").filter(|s| !s.is_empty()).or_else(|| v.get("TITLE")).cloned().unwrap_or_default();
            labels.insert(n.clone(), label);
        }
        if tro {
            copy_tree(&loc.port_trophies(), &tmp.join("trophies"), &[])?;
        }
        let meta = BackupMeta {
            id: id.clone(),
            created: now,
            reason: reason.into(),
            source: side,
            user: if side == Side::Port { PORT_USER.into() } else { user.into() },
            slots: names.clone(),
            trophies: tro,
            size: tree_size(&tmp),
            labels,
        };
        let json = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
        io(fs::write(tmp.join(MANIFEST), json), &tmp)?;
        io(fs::rename(&tmp, loc.backups.join(&id)), &tmp)?;
        Ok(meta)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&tmp);
    }
    result.map(Some)
}

pub fn list_backups(loc: &Locations) -> Vec<BackupMeta> {
    let mut v: Vec<BackupMeta> = fs::read_dir(&loc.backups)
        .map(|it| {
            it.flatten()
                .filter_map(|e| {
                    let id = e.file_name().to_string_lossy().into_owned();
                    if !valid_id(&id) {
                        return None;
                    }
                    let mut m: BackupMeta = serde_json::from_str(&fs::read_to_string(e.path().join(MANIFEST)).ok()?).ok()?;
                    m.id = id;
                    Some(m)
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.id.cmp(&a.id)));
    v
}

fn backup_dir(loc: &Locations, id: &str) -> Result<(PathBuf, BackupMeta), String> {
    if !valid_id(id) {
        return Err(format!("{id:?}: not a backup"));
    }
    let dir = loc.backups.join(id);
    let mut m: BackupMeta = serde_json::from_str(&io(fs::read_to_string(dir.join(MANIFEST)), &dir)?)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    m.id = id.into();
    for s in &m.slots {
        check_slot(s)?;
    }
    Ok((dir, m))
}

pub fn delete_backup(loc: &Locations, id: &str) -> Result<(), String> {
    let (dir, _) = backup_dir(loc, id)?;
    io(fs::remove_dir_all(&dir), &dir)
}

/// Keep the newest `keep` backups made for `reason`; returns how many went.
pub fn prune_backups(loc: &Locations, reason: &str, keep: usize) -> Result<usize, String> {
    let old: Vec<BackupMeta> = list_backups(loc).into_iter().filter(|b| b.reason == reason).skip(keep).collect();
    for b in &old {
        delete_backup(loc, &b.id)?;
    }
    Ok(old.len())
}

/// Before "Play": every slot of the port and its trophies, unless nothing
/// changed since the last such backup; then only the newest `keep` stay.
/// `keep == 0` turns it off.
pub fn backup_before_play(loc: &Locations, keep: u32, now: u64) -> Result<Option<BackupMeta>, String> {
    if keep == 0 {
        return Ok(None);
    }
    let dir = loc.port_savedata();
    let mut current = Vec::new();
    for n in slot_names(&dir) {
        digest(&dir.join(&n), &format!("savedata/{n}"), &mut current);
    }
    digest(&loc.port_trophies(), "trophies", &mut current);
    current.sort();
    if let Some(last) = list_backups(loc).into_iter().find(|b| b.reason == "before-play") {
        let bdir = loc.backups.join(&last.id);
        let mut prev = Vec::new();
        digest(&bdir.join("savedata"), "savedata", &mut prev);
        digest(&bdir.join("trophies"), "trophies", &mut prev);
        prev.sort();
        if prev == current {
            return Ok(None);
        }
    }
    let made = create_backup(loc, Side::Port, PORT_USER, None, true, "before-play", now)?;
    prune_backups(loc, "before-play", keep as usize)?;
    Ok(made)
}

#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// The backup made of what was there before (none if nothing was).
    pub backup: Option<BackupMeta>,
    /// The slots written.
    pub slots: Vec<String>,
}

/// Put a backup back where it came from (all of it, or one slot). The slots
/// it replaces are backed up first.
pub fn restore_backup(loc: &Locations, id: &str, slot: Option<&str>, now: u64) -> Result<Report, String> {
    let (bdir, m) = backup_dir(loc, id)?;
    let slots: Vec<String> = match slot {
        Some(s) if m.slots.iter().any(|x| x == s) => vec![s.to_string()],
        Some(s) => return Err(format!("{s}: not in backup {id}")),
        None => m.slots.clone(),
    };
    let trophies = slot.is_none() && m.trophies && m.source == Side::Port && bdir.join("trophies").is_dir();
    let dest = savedata_dir(loc, m.source, &m.user)?;
    if m.source == Side::Rpcs3 && !loc.rpcs3_home.join(&m.user).is_dir() {
        return Err(format!("RPCS3 user {} not found", m.user));
    }
    let backup = create_backup(loc, m.source, &m.user, Some(&slots), trophies, "before-restore", now)?;
    for s in &slots {
        replace_dir(&dest.join(s), |tmp| copy_tree(&bdir.join("savedata").join(s), tmp, &[]))?;
    }
    if trophies {
        replace_dir(&loc.port_trophies(), |tmp| copy_tree(&bdir.join("trophies"), tmp, &[]))?;
    }
    Ok(Report { backup, slots })
}

// ---------------------------------------------------------------------------
// Crossing sides
// ---------------------------------------------------------------------------

/// Fill `tmp` with the slot in `src` the way the port expects it: the game's
/// files as they are, PARAM.SFO as the port's text (converted from RPCS3's
/// PSF when needed) and PARAM.PFD naming the secure files (RPCS3's `*<file>`
/// keys, else the source's own text PFD, else the port's previous one, else
/// every file that is not an icon/picture/sound).
fn install_port_slot(src: &Path, tmp: &Path, previous_pfd: Option<Vec<String>>) -> Result<(), String> {
    copy_tree(src, tmp, &[SFO, PFD])?;
    let files: Vec<String> = walk(tmp).into_iter().map(|(r, _)| r).collect();
    let sfo = fs::read(src.join(SFO)).ok();
    let psf = sfo.as_deref().and_then(psf_parse);
    let from_psf: Option<Vec<String>> = psf.as_ref().and_then(|es| {
        let flagged: Vec<&PsfEntry> = es.iter().filter(|e| e.key.starts_with('*')).collect();
        (!flagged.is_empty()).then(|| {
            flagged.iter().filter(|e| e.as_int() == Some(1)).map(|e| e.key[1..].to_string()).collect()
        })
    });
    match (&sfo, &psf) {
        (Some(_), Some(es)) => {
            let v = es.iter().map(|e| (e.key.clone(), e.as_string())).collect();
            io(fs::write(tmp.join(SFO), port_sfo_text(&v)), tmp)?;
            set_mtime(&tmp.join(SFO), mtime(&src.join(SFO)));
        }
        (Some(_), None) => copy_file(&src.join(SFO), &tmp.join(SFO))?,
        (None, _) => {}
    }
    let secure = from_psf
        .or_else(|| fs::read(src.join(PFD)).ok().as_deref().and_then(text_pfd))
        .or(previous_pfd)
        .unwrap_or_else(|| files.iter().filter(|f| !CONTENT_FILES.contains(&f.as_str())).cloned().collect());
    let secure: Vec<&String> = secure.iter().filter(|n| files.contains(n)).collect();
    if !secure.is_empty() {
        let text: String = secure.iter().map(|n| format!("{n}\n")).collect();
        io(fs::write(tmp.join(PFD), text), tmp)?;
    }
    Ok(())
}

/// Fill `tmp` with the port's slot `src` the way RPCS3 expects it: the
/// game's files as they are; RPCS3's own PARAM.SFO when the slot was already
/// there (its titles updated), else a new one with the keys RPCS3 writes;
/// no port PARAM.PFD (RPCS3 keeps that in the SFO's `*<file>` keys).
fn install_rpcs3_slot(
    src: &Path,
    tmp: &Path,
    slot: &str,
    existing_sfo: Option<Vec<u8>>,
    existing_pfd: Option<Vec<u8>>,
) -> Result<(), String> {
    copy_tree(src, tmp, &[SFO, PFD])?;
    let raw = fs::read(src.join(SFO)).unwrap_or_default();
    let v = sfo_values(&raw);
    let secure: Vec<String> = match psf_parse(&raw) {
        Some(es) => es.iter().filter(|e| e.key.starts_with('*') && e.as_int() == Some(1)).map(|e| e.key[1..].to_string()).collect(),
        None => fs::read(src.join(PFD)).ok().as_deref().and_then(text_pfd).unwrap_or_default(),
    };
    let files: Vec<(String, bool)> = walk(tmp)
        .into_iter()
        .map(|(r, _)| {
            let s = secure.contains(&r);
            (r, s)
        })
        .collect();
    let sfo = match existing_sfo.as_deref().and_then(psf_parse) {
        Some(es) => rpcs3_sfo_update(es, &v, &files),
        None => match psf_parse(&raw) {
            Some(es) => psf_write(&es), // a slot that came from RPCS3 as it was
            None => rpcs3_sfo_new(slot, &v, &files),
        },
    };
    io(fs::write(tmp.join(SFO), sfo), tmp)?;
    set_mtime(&tmp.join(SFO), mtime(&src.join(SFO)));
    if let Some(p) = existing_pfd {
        io(fs::write(tmp.join(PFD), p), tmp)?;
    }
    Ok(())
}

fn previous_port_pfd(dest: &Path) -> Option<Vec<String>> {
    fs::read(dest.join(PFD)).ok().as_deref().and_then(text_pfd)
}

/// Copy an RPCS3 slot into the port's savedata (the port's copy of the slot
/// is backed up first).
pub fn import_from_rpcs3(loc: &Locations, user: &str, slot: &str, now: u64) -> Result<Report, String> {
    check_slot(slot)?;
    let src = savedata_dir(loc, Side::Rpcs3, user)?.join(slot);
    if !src.join(SFO).is_file() {
        return Err(format!("{}: no save there", src.display()));
    }
    let dest = loc.port_savedata().join(slot);
    let backup = create_backup(loc, Side::Port, PORT_USER, Some(&[slot.to_string()]), false, "before-import", now)?;
    let prev = previous_port_pfd(&dest);
    replace_dir(&dest, |tmp| install_port_slot(&src, tmp, prev))?;
    Ok(Report { backup, slots: vec![slot.into()] })
}

/// Copy the port's slot into an RPCS3 user's savedata (RPCS3's copy of the
/// slot is backed up first, and its PARAM.SFO kept).
pub fn export_to_rpcs3(loc: &Locations, slot: &str, user: &str, now: u64) -> Result<Report, String> {
    check_slot(slot)?;
    let src = loc.port_savedata().join(slot);
    if !src.join(SFO).is_file() {
        return Err(format!("{}: no save there", src.display()));
    }
    let dir = savedata_dir(loc, Side::Rpcs3, user)?;
    if !loc.rpcs3_home.join(user).is_dir() {
        return Err(format!("RPCS3 user {user} not found"));
    }
    let dest = dir.join(slot);
    let backup = create_backup(loc, Side::Rpcs3, user, Some(&[slot.to_string()]), false, "before-export", now)?;
    let (sfo, pfd) = (fs::read(dest.join(SFO)).ok(), fs::read(dest.join(PFD)).ok());
    replace_dir(&dest, |tmp| install_rpcs3_slot(&src, tmp, slot, sfo, pfd))?;
    Ok(Report { backup, slots: vec![slot.into()] })
}

/// Remove a slot, after backing it up (no backup, no delete).
pub fn delete_slot(loc: &Locations, side: Side, user: &str, slot: &str, now: u64) -> Result<BackupMeta, String> {
    check_slot(slot)?;
    let dest = savedata_dir(loc, side, user)?.join(slot);
    if !dest.is_dir() {
        return Err(format!("{}: no save there", dest.display()));
    }
    let b = create_backup(loc, side, user, Some(&[slot.to_string()]), false, "before-delete", now)?
        .ok_or_else(|| format!("{slot}: nothing to back up"))?;
    io(fs::remove_dir_all(&dest), &dest)?;
    Ok(b)
}

// ---------------------------------------------------------------------------
// .zip
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
struct ZipManifest {
    format: u32,
    game: String,
    created: u64,
    slots: Vec<String>,
}

/// The port's slots (all, or the ones named) in a .zip: `<slot>/<file>` plus
/// a small manifest. Returns the number of slots.
pub fn export_zip(loc: &Locations, slots: Option<&[String]>, dest: &Path, now: u64) -> Result<usize, String> {
    let dir = loc.port_savedata();
    let names: Vec<String> = match slots {
        Some(s) => {
            for n in s {
                check_slot(n)?;
                if !dir.join(n).is_dir() {
                    return Err(format!("{n}: no save there"));
                }
            }
            s.to_vec()
        }
        None => slot_names(&dir),
    };
    if names.is_empty() {
        return Err("no save to export".into());
    }
    let mut entries = Vec::new();
    let manifest = ZipManifest { format: 1, game: "inFAMOUS 2".into(), created: now, slots: names.clone() };
    entries.push(zipfile::Entry {
        name: ZIP_MANIFEST.into(),
        data: serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
        mtime: now,
    });
    for n in &names {
        for (rel, p) in walk(&dir.join(n)) {
            entries.push(zipfile::Entry { name: format!("{n}/{rel}"), data: io(fs::read(&p), &p)?, mtime: mtime(&p) });
        }
    }
    zipfile::write(dest, &entries)?;
    Ok(names.len())
}

/// The game's slots in a .zip (this app's, or any archive with the slot
/// folders somewhere inside, e.g. a zipped RPCS3 savedata folder), copied
/// into the port's savedata. The slots it replaces are backed up first.
pub fn import_zip(loc: &Locations, zip: &Path, now: u64) -> Result<Report, String> {
    let lim = zipfile::Limits { entries: 4096, file: 64 << 20, total: 256 << 20 };
    let entries = zipfile::read(zip, &lim)?;
    // slot -> [(path inside the slot, data, mtime)]
    let mut by_slot: BTreeMap<String, Vec<(Vec<String>, &zipfile::Entry)>> = BTreeMap::new();
    for e in &entries {
        let parts: Vec<&str> = e.name.split('/').filter(|p| !p.is_empty()).collect();
        if parts.first() == Some(&"__MACOSX") || parts.last().is_some_and(|n| *n == ".DS_Store" || n.starts_with("._")) {
            continue;
        }
        let Some(i) = parts.iter().position(|p| valid_slot(p)) else { continue };
        let rest: Vec<String> = parts[i + 1..].iter().map(|s| s.to_string()).collect();
        if rest.is_empty() || !rest.iter().all(|p| safe_component(p)) {
            continue;
        }
        by_slot.entry(parts[i].to_string()).or_default().push((rest, e));
    }
    by_slot.retain(|_, files| files.iter().any(|(r, _)| r.len() == 1 && r[0] == SFO));
    if by_slot.is_empty() {
        return Err(format!("{}: no inFAMOUS 2 save in this file", zip.display()));
    }
    let stage = loc.backups.join(format!(".zip-import-{now}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&stage);
    let result = (|| -> Result<Report, String> {
        for (slot, files) in &by_slot {
            for (rest, e) in files {
                let mut p = stage.join(slot);
                for c in rest {
                    p.push(c);
                }
                if let Some(d) = p.parent() {
                    io(fs::create_dir_all(d), d)?;
                }
                io(fs::write(&p, &e.data), &p)?;
                set_mtime(&p, e.mtime);
            }
        }
        let slots: Vec<String> = by_slot.keys().cloned().collect();
        let backup = create_backup(loc, Side::Port, PORT_USER, Some(&slots), false, "before-zip-import", now)?;
        let dir = loc.port_savedata();
        for s in &slots {
            let dest = dir.join(s);
            let prev = previous_port_pfd(&dest);
            replace_dir(&dest, |tmp| install_port_slot(&stage.join(s), tmp, prev))?;
        }
        Ok(Report { backup, slots })
    })();
    let _ = fs::remove_dir_all(&stage);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn setup(tag: &str) -> (Tmp, Locations) {
        let root = std::env::temp_dir().join(format!("i2l-saves-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let loc = Locations {
            port_root: root.join("gamedata"),
            rpcs3_home: root.join("rpcs3/dev_hdd0/home"),
            backups: root.join("save-backups"),
        };
        (Tmp(root), loc)
    }

    const SLOT: &str = "BCUS98125_INFAMOUS2_0";
    const DETAIL_PT: &str = "Completo: 1%\nPoderes e Atualizações: 13\n";

    /// A slot as the port writes it.
    fn port_slot(loc: &Locations, name: &str, state: &[u8]) -> PathBuf {
        let d = loc.port_savedata().join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(
            d.join(SFO),
            format!("TITLE=inFAMOUS 2\nSUB_TITLE=(Salvamento automático) 3. Impulso para Frente\nDETAIL={DETAIL_PT}\nATTRIBUTE=0\nLIST_PARAM=\n"),
        )
        .unwrap();
        fs::write(d.join("ICON0.PNG"), b"\x89PNG icon").unwrap();
        fs::write(d.join("STATE"), state).unwrap();
        fs::write(d.join(PFD), "STATE\n").unwrap();
        d
    }

    /// A slot as RPCS3 writes it (the same key set as a real one).
    fn rpcs3_slot(loc: &Locations, user: &str, name: &str, state: &[u8]) -> PathBuf {
        let d = loc.rpcs3_savedata(user).join(name);
        fs::create_dir_all(&d).unwrap();
        let mut v = BTreeMap::new();
        v.insert("TITLE".to_string(), "inFAMOUS 2".to_string());
        v.insert("SUB_TITLE".to_string(), "(Autosave) 3. Forward Momentum".to_string());
        v.insert("DETAIL".to_string(), "Completion: 1%\nPowers and Upgrades: 17\n".to_string());
        let files = vec![("ICON0.PNG".to_string(), false), ("STATE".to_string(), true)];
        fs::write(d.join(SFO), rpcs3_sfo_new(name, &v, &files)).unwrap();
        fs::write(d.join("ICON0.PNG"), b"\x89PNG icon").unwrap();
        fs::write(d.join("STATE"), state).unwrap();
        d
    }

    #[test]
    fn psf_layout_matches_rpcs3() {
        let mut v = BTreeMap::new();
        v.insert("TITLE".to_string(), "inFAMOUS 2".to_string());
        let files = vec![("ICON0.PNG".to_string(), false), ("STATE".to_string(), true)];
        let d = rpcs3_sfo_new(SLOT, &v, &files);
        // RPCS3's own save SFO for this game: 14 keys, key table at 244,
        // data at 396, 2804 bytes in all
        assert_eq!(d.len(), 2804);
        assert_eq!(u32::from_le_bytes(d[8..12].try_into().unwrap()), 244);
        assert_eq!(u32::from_le_bytes(d[12..16].try_into().unwrap()), 396);
        let es = psf_parse(&d).unwrap();
        assert_eq!(es.len(), 14);
        assert_eq!(es[0].key, "*ICON0.PNG");
        assert_eq!(psf_write(&es), d); // parse/write round trip
        let m = sfo_values(&d);
        assert_eq!(m["*STATE"], "1");
        assert_eq!(m["RPCS3_BLIST"], "ICON0.PNG/STATE");
        assert_eq!(m["SAVEDATA_DIRECTORY"], SLOT);
        assert_eq!(m["CATEGORY"], "SD");
    }

    #[test]
    fn text_sfo_keeps_multiline_detail() {
        let (_t, loc) = setup("text");
        let d = port_slot(&loc, SLOT, b"S");
        let m = read_sfo(&d.join(SFO)).unwrap();
        assert_eq!(m["DETAIL"], DETAIL_PT);
        assert_eq!(m["SUB_TITLE"], "(Salvamento automático) 3. Impulso para Frente");
        assert_eq!(m["LIST_PARAM"], "");
        // what the port writes, back again
        let mut back = String::new();
        back.push_str(&port_sfo_text(&m));
        assert_eq!(back, fs::read_to_string(d.join(SFO)).unwrap());
        // strings are cut on a character boundary
        let mut e = PsfEntry::string("SUB_TITLE", "ação", 4);
        assert_eq!(e.data, b"a\xc3\xa7\0");
        e.set_str("abc");
        assert_eq!(e.data, b"abc\0");
    }

    #[test]
    fn names_are_checked() {
        assert!(valid_slot(SLOT));
        assert!(valid_slot("NPUA80638-SAVE"));
        assert!(!valid_slot("BCUS98119_INFAMOUS_0")); // the first game
        assert!(!valid_slot("BCUS98125"));
        assert!(!valid_slot("BCUS98125_../x"));
        assert!(!valid_slot(".BCUS98125_INFAMOUS2_0.incoming"));
        assert!(valid_user("00000001") && !valid_user("0000001") && !valid_user("../../.."));
        assert!(!valid_id("../x") && !valid_id(".x.partial") && valid_id("20261017-120000-port-manual"));
    }

    #[test]
    fn lists_both_sides() {
        let (_t, loc) = setup("list");
        port_slot(&loc, SLOT, &[1u8; 32]);
        rpcs3_slot(&loc, "00000001", SLOT, &[1u8; 32]);
        rpcs3_slot(&loc, "00000002", "BCUS98125_INFAMOUS2_1", &[2u8; 32]);
        fs::create_dir_all(loc.rpcs3_savedata("00000001").join("BCUS98119_INFAMOUS_0")).unwrap(); // other game
        fs::create_dir_all(loc.rpcs3_home.join("not-a-user/savedata").join(SLOT)).unwrap();
        let p = list_slots(&loc, Side::Port, true);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].sfo, "text");
        assert_eq!(p[0].files, 4);
        assert!(p[0].icon.as_deref().unwrap().starts_with("data:image/png;base64,"));
        let r = list_slots(&loc, Side::Rpcs3, false);
        assert_eq!(r.iter().map(|s| (s.user.as_str(), s.name.as_str())).collect::<Vec<_>>(),
            vec![("00000001", SLOT), ("00000002", "BCUS98125_INFAMOUS2_1")]);
        assert_eq!(r[0].sfo, "binary");
        assert_eq!(r[0].subtitle, "(Autosave) 3. Forward Momentum");
        assert_eq!(r[0].state_hash, p[0].state_hash); // the same progress
        assert_ne!(r[1].state_hash, p[0].state_hash);
        assert_eq!(rpcs3_users(&loc), vec!["00000001", "00000002"]);
    }

    #[test]
    fn backup_and_restore() {
        let (_t, loc) = setup("backup");
        let d = port_slot(&loc, SLOT, b"first");
        fs::create_dir_all(loc.port_trophies().join("NPWR01234_00")).unwrap();
        fs::write(loc.port_trophies().join("NPWR01234_00/TROPUSR.DAT"), b"t1").unwrap();
        let old = UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        fs::OpenOptions::new().write(true).open(d.join("STATE")).unwrap().set_modified(old).unwrap();

        let b = create_backup(&loc, Side::Port, PORT_USER, None, true, "manual", 1_792_245_722).unwrap().unwrap();
        assert_eq!(b.id, "20261017-140202-port-manual");
        assert_eq!(b.slots, vec![SLOT]);
        assert!(b.trophies);
        assert_eq!(b.labels[SLOT], "(Salvamento automático) 3. Impulso para Frente");
        let bd = loc.backups.join(&b.id);
        assert_eq!(fs::read(bd.join("savedata").join(SLOT).join("STATE")).unwrap(), b"first");
        assert_eq!(mtime(&bd.join("savedata").join(SLOT).join("STATE")), 1_700_000_000); // date kept
        // same second: a second folder, not an overwrite
        let b2 = create_backup(&loc, Side::Port, PORT_USER, None, true, "manual", 1_792_245_722).unwrap().unwrap();
        assert_eq!(b2.id, "20261017-140202-port-manual-2");

        // the game moves on, then the backup comes back
        fs::write(d.join("STATE"), b"second").unwrap();
        fs::write(d.join("EXTRA"), b"x").unwrap();
        fs::write(loc.port_trophies().join("NPWR01234_00/TROPUSR.DAT"), b"t2").unwrap();
        let r = restore_backup(&loc, &b.id, None, 1_792_245_800).unwrap();
        assert_eq!(fs::read(d.join("STATE")).unwrap(), b"first");
        assert!(!d.join("EXTRA").exists()); // the slot is the backup's, whole
        assert_eq!(fs::read(loc.port_trophies().join("NPWR01234_00/TROPUSR.DAT")).unwrap(), b"t1");
        // ... and what it replaced was backed up first
        let safety = r.backup.unwrap();
        assert_eq!(safety.reason, "before-restore");
        let sd = loc.backups.join(&safety.id);
        assert_eq!(fs::read(sd.join("savedata").join(SLOT).join("STATE")).unwrap(), b"second");
        assert_eq!(fs::read(sd.join("trophies/NPWR01234_00/TROPUSR.DAT")).unwrap(), b"t2");
        assert_eq!(list_backups(&loc).len(), 3);
        assert!(list_backups(&loc)[0].id == safety.id); // newest first
        // no temporary folders left behind
        assert!(fs::read_dir(loc.port_savedata()).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with('.')));
        assert!(fs::read_dir(&loc.backups).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with('.')));

        // one slot of a backup, and names that are not in it
        assert!(restore_backup(&loc, &b.id, Some("BCUS98125_INFAMOUS2_9"), 1).is_err());
        assert!(restore_backup(&loc, "../gamedata", None, 1).is_err());
        delete_backup(&loc, &b2.id).unwrap();
        assert_eq!(list_backups(&loc).len(), 2);
    }

    #[test]
    fn backups_before_play_are_deduplicated_and_pruned() {
        let (_t, loc) = setup("play");
        let d = port_slot(&loc, SLOT, b"s0");
        assert!(backup_before_play(&loc, 0, 1_000).unwrap().is_none()); // off
        assert!(backup_before_play(&loc, 2, 1_000).unwrap().is_some());
        assert!(backup_before_play(&loc, 2, 1_100).unwrap().is_none()); // nothing changed
        for (i, t) in [1_200u64, 1_300, 1_400].iter().enumerate() {
            fs::write(d.join("STATE"), format!("s{}", i + 1)).unwrap();
            assert!(backup_before_play(&loc, 2, *t).unwrap().is_some());
        }
        create_backup(&loc, Side::Port, PORT_USER, None, true, "manual", 900).unwrap();
        let all = list_backups(&loc);
        let play: Vec<&BackupMeta> = all.iter().filter(|b| b.reason == "before-play").collect();
        assert_eq!(play.len(), 2); // only the newest N
        assert_eq!(play[0].created, 1_400);
        assert_eq!(play[1].created, 1_300);
        assert_eq!(all.len(), 3); // other backups are never pruned
        // no saves at all: nothing to back up
        let (_t2, empty) = setup("play-empty");
        assert!(backup_before_play(&empty, 5, 1).unwrap().is_none());
    }

    #[test]
    fn import_from_rpcs3_converts_the_metadata() {
        let (_t, loc) = setup("import");
        let state: Vec<u8> = (0..32768u32).map(|i| (i * 7) as u8).collect();
        rpcs3_slot(&loc, "00000001", SLOT, &state);
        let port = port_slot(&loc, SLOT, b"older port save");
        let r = import_from_rpcs3(&loc, "00000001", SLOT, 2_000).unwrap();
        assert_eq!(fs::read(port.join("STATE")).unwrap(), state);
        // the port's text SFO, read back the way cellSaveData reads it
        let sfo = fs::read_to_string(port.join(SFO)).unwrap();
        assert!(sfo.starts_with("TITLE=inFAMOUS 2\nSUB_TITLE=(Autosave) 3. Forward Momentum\nDETAIL=Completion: 1%\n"), "{sfo}");
        assert!(sfo.ends_with("ATTRIBUTE=0\nLIST_PARAM=\n"));
        assert_eq!(fs::read_to_string(port.join(PFD)).unwrap(), "STATE\n"); // from *STATE=1
        // the port's previous save is in the backup
        let b = r.backup.unwrap();
        assert_eq!(b.reason, "before-import");
        assert_eq!(fs::read(loc.backups.join(&b.id).join("savedata").join(SLOT).join("STATE")).unwrap(), b"older port save");
        // the RPCS3 side was only read
        assert!(psf_parse(&fs::read(loc.rpcs3_savedata("00000001").join(SLOT).join(SFO)).unwrap()).is_some());
        // a slot new to the port: nothing to back up
        rpcs3_slot(&loc, "00000001", "BCUS98125_INFAMOUS2_1", b"x");
        let r = import_from_rpcs3(&loc, "00000001", "BCUS98125_INFAMOUS2_1", 2_001).unwrap();
        assert!(r.backup.is_none());
        assert!(import_from_rpcs3(&loc, "00000001", "BCUS98125_INFAMOUS2_7", 2_002).is_err());
        assert!(import_from_rpcs3(&loc, "../x", SLOT, 2_003).is_err());
    }

    #[test]
    fn export_to_rpcs3_keeps_rpcs3s_sfo() {
        let (_t, loc) = setup("export");
        port_slot(&loc, SLOT, b"port progress");
        let rp = rpcs3_slot(&loc, "00000001", SLOT, b"rpcs3 progress");
        fs::write(rp.join("LEFTOVER"), b"x").unwrap();
        let before = psf_parse(&fs::read(rp.join(SFO)).unwrap()).unwrap();
        let r = export_to_rpcs3(&loc, SLOT, "00000001", 3_000).unwrap();
        assert_eq!(fs::read(rp.join("STATE")).unwrap(), b"port progress");
        assert!(!rp.join(PFD).exists()); // the port's PFD stays on the port's side
        assert!(!rp.join("LEFTOVER").exists());
        let after = psf_parse(&fs::read(rp.join(SFO)).unwrap()).unwrap();
        assert_eq!(after.len(), before.len()); // the full SFO, same keys
        let m = sfo_values(&fs::read(rp.join(SFO)).unwrap());
        assert_eq!(m["SUB_TITLE"], "(Salvamento automático) 3. Impulso para Frente"); // follows the save
        assert_eq!(m["DETAIL"], DETAIL_PT);
        assert_eq!(m["ACCOUNT_ID"], "0000000000000000");
        assert_eq!(m["RPCS3_BLIST"], "ICON0.PNG/STATE");
        // RPCS3's previous copy is in the backup
        let b = r.backup.unwrap();
        assert_eq!((b.source, b.user.as_str(), b.reason.as_str()), (Side::Rpcs3, "00000001", "before-export"));
        let bd = loc.backups.join(&b.id).join("savedata").join(SLOT);
        assert_eq!(fs::read(bd.join("STATE")).unwrap(), b"rpcs3 progress");
        assert!(bd.join("LEFTOVER").exists());
        // and it restores to RPCS3, not to the port
        restore_backup(&loc, &b.id, None, 3_001).unwrap();
        assert_eq!(fs::read(rp.join("STATE")).unwrap(), b"rpcs3 progress");
        assert_eq!(fs::read(loc.port_savedata().join(SLOT).join("STATE")).unwrap(), b"port progress");

        // a slot RPCS3 does not have yet gets a new, complete SFO
        port_slot(&loc, "BCUS98125_INFAMOUS2_1", b"p1");
        export_to_rpcs3(&loc, "BCUS98125_INFAMOUS2_1", "00000001", 3_002).unwrap();
        let d = fs::read(loc.rpcs3_savedata("00000001").join("BCUS98125_INFAMOUS2_1").join(SFO)).unwrap();
        let m = sfo_values(&d);
        assert_eq!(psf_parse(&d).unwrap().len(), 14);
        assert_eq!(m["SAVEDATA_DIRECTORY"], "BCUS98125_INFAMOUS2_1");
        assert_eq!(m["*STATE"], "1");
        assert_eq!(m["*ICON0.PNG"], "0");
        // no such RPCS3 user
        assert!(export_to_rpcs3(&loc, SLOT, "00000009", 3_003).is_err());
    }

    #[test]
    fn round_trip_port_rpcs3_port() {
        let (_t, loc) = setup("roundtrip");
        fs::create_dir_all(loc.rpcs3_home.join("00000001")).unwrap();
        let p = port_slot(&loc, SLOT, b"progress");
        let original_sfo = fs::read_to_string(p.join(SFO)).unwrap();
        export_to_rpcs3(&loc, SLOT, "00000001", 10).unwrap();
        fs::remove_dir_all(&p).unwrap();
        import_from_rpcs3(&loc, "00000001", SLOT, 11).unwrap();
        assert_eq!(fs::read_to_string(p.join(SFO)).unwrap(), original_sfo);
        assert_eq!(fs::read_to_string(p.join(PFD)).unwrap(), "STATE\n");
        assert_eq!(fs::read(p.join("STATE")).unwrap(), b"progress");
    }

    #[test]
    fn delete_backs_up_first() {
        let (_t, loc) = setup("delete");
        let p = port_slot(&loc, SLOT, b"keep me");
        let b = delete_slot(&loc, Side::Port, PORT_USER, SLOT, 50).unwrap();
        assert!(!p.exists());
        assert_eq!(fs::read(loc.backups.join(&b.id).join("savedata").join(SLOT).join("STATE")).unwrap(), b"keep me");
        assert!(delete_slot(&loc, Side::Port, PORT_USER, SLOT, 51).is_err()); // already gone
    }

    #[test]
    fn zip_export_and_import() {
        let (_t, loc) = setup("zip");
        port_slot(&loc, SLOT, b"zip progress");
        port_slot(&loc, "BCUS98125_INFAMOUS2_1", b"other");
        let z = loc.backups.parent().unwrap().join("saves.zip");
        assert_eq!(export_zip(&loc, Some(&[SLOT.to_string()]), &z, 100).unwrap(), 1);
        assert_eq!(export_zip(&loc, None, &z, 100).unwrap(), 2);
        // another machine: empty port, the archive comes in
        let (_t2, other) = setup("zip-other");
        let r = import_zip(&other, &z, 200).unwrap();
        assert_eq!(r.slots, vec![SLOT.to_string(), "BCUS98125_INFAMOUS2_1".to_string()]);
        assert!(r.backup.is_none());
        let d = other.port_savedata().join(SLOT);
        assert_eq!(fs::read(d.join("STATE")).unwrap(), b"zip progress");
        assert_eq!(fs::read_to_string(d.join(PFD)).unwrap(), "STATE\n");
        // again: now the slots there are backed up first
        fs::write(d.join("STATE"), b"changed").unwrap();
        let r = import_zip(&other, &z, 300).unwrap();
        assert_eq!(r.backup.unwrap().slots.len(), 2);
        assert_eq!(fs::read(d.join("STATE")).unwrap(), b"zip progress");

        // a zipped RPCS3 savedata folder (binary SFO, other games, Finder's
        // __MACOSX, a path that tries to climb out) works too
        let (_t3, third) = setup("zip-rpcs3");
        let rp = rpcs3_slot(&third, "00000001", SLOT, b"from rpcs3");
        let sfo = fs::read(rp.join(SFO)).unwrap();
        let e = |n: &str, d: &[u8]| zipfile::Entry { name: n.into(), data: d.to_vec(), mtime: 1_700_000_000 };
        let z2 = third.backups.parent().unwrap().join("rpcs3.zip");
        zipfile::write(&z2, &[
            e(&format!("savedata/{SLOT}/PARAM.SFO"), &sfo),
            e(&format!("savedata/{SLOT}/STATE"), b"from rpcs3"),
            e(&format!("savedata/{SLOT}/ICON0.PNG"), b"png"),
            e(&format!("__MACOSX/savedata/{SLOT}/._STATE"), b"junk"),
            e(&format!("savedata/{SLOT}/../../evil"), b"no"),
            e("savedata/BCUS98119_INFAMOUS_0/PARAM.SFO", b"first game"),
        ]).unwrap();
        let r = import_zip(&third, &z2, 400).unwrap();
        assert_eq!(r.slots, vec![SLOT.to_string()]);
        let d = third.port_savedata().join(SLOT);
        assert_eq!(fs::read(d.join("STATE")).unwrap(), b"from rpcs3");
        assert!(fs::read_to_string(d.join(SFO)).unwrap().starts_with("TITLE=inFAMOUS 2\n"));
        assert_eq!(mtime(&d.join("STATE")), 1_700_000_000);
        assert!(!third.port_root.join("evil").exists() && !d.join("evil").exists());
        assert!(!third.port_savedata().join("BCUS98119_INFAMOUS_0").exists());
        // nothing of the game in it
        let z3 = third.backups.parent().unwrap().join("none.zip");
        zipfile::write(&z3, &[e("readme.txt", b"hi")]).unwrap();
        assert!(import_zip(&third, &z3, 500).unwrap_err().contains("no inFAMOUS 2 save"));
    }
}
