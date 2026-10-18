//! Debug mode ("Report a problem"): a game session started in debug mode
//! writes what `infamous2-lle/tools/play_dump.sh` writes (the texture log,
//! every pipeline's shaders, and on request one frame's render targets and
//! pass log) into its own folder, `<data>/reports/<UTC time>/`, with the
//! game's problem logs turned on (`DEBUG_ENV`) and a session index,
//! `events.tsv`: the start, a tick every 2 s with the log's size, each
//! capture with the frame on screen, the exit (and the macOS crash report,
//! when there is one, as `crash.ips`). A capture touches the trigger file the
//! Metal backend watches. `infamous2-lle/tools/log_triage.py` reads the
//! folder and writes `triage.md` / `triage.json`; the folder is packed into
//! a .zip to send (surfaces as PNG, text with the home folder's path replaced
//! by `~`).

use crate::zipfile;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The file whose presence at a present makes the backend dump that frame
/// (PS3RECOMP_METAL_DUMP_TRIGGER; the backend removes it).
pub const TRIGGER: &str = ".dump-now";
/// The game's output in debug mode.
pub const LOG: &str = "game.log";
/// One line per capture the player asked for.
pub const CAPTURES: &str = "captures.txt";
pub const SYSTEM: &str = "system.txt";
pub const NOTES: &str = "notes.txt";
/// The session index: one tab-separated line per event (utc, unix ms, the
/// log's size then, event, detail), for log_triage.py to put times on lines.
pub const EVENTS: &str = "events.tsv";
/// log_triage.py's output.
pub const TRIAGE_MD: &str = "triage.md";
pub const TRIAGE_JSON: &str = "triage.json";
/// The macOS crash report of a game that crashed, copied from
/// ~/Library/Logs/DiagnosticReports.
pub const CRASH: &str = "crash.ips";

/// What debug mode adds to the dumps: lines in the log when something goes
/// wrong, none of which changes what the game draws. Each is cheap: a
/// counter or clock read per frame, or a check on a path that only runs when
/// it fails, with its own cap (the toolkit and infamous2-lle sources).
pub const DEBUG_ENV: &[(&str, &str)] = &[
    // per 10 s: frames that drew, world vs light frames, and where a
    // present's time went (in-flight wait, nextDrawable, encode, commit)
    ("PS3RECOMP_ENG_FPSLOG", "1"),
    // the Edge geometry job's slot allocator, checked after every runtime
    // call: names the call when the rare Edge SPU fault starts (edge_watch.c)
    ("PS3_EDGE_WATCH", "1"),
    // every syscall that fails, once per (number, error), at most 64
    ("LV2_ERRDBG", "1"),
    // every 5 s: mixed blocks vs real time, silent share, frames dropped
    ("AUDIO_RATE", "1"),
    // draws the engine drops, and why (6 per reason)
    ("PS3RECOMP_ENG_DROPLOG", "1"),
    // every 60 s the guest threads and where each one waits: the last dump
    // before a hang says what it hangs on
    ("PS3_THREAD_DUMP", "60"),
];

/// The variables play_dump.sh sets, pointed at `dir`, and `DEBUG_ENV`. Two
/// differences from play_dump.sh: no automatic surface dump at the first
/// world frame (only the captures dump: SURF_DUMP_MIN above any frame's
/// record count), and the FPS line in the log once per second.
pub fn env(dir: &Path) -> Vec<(String, String)> {
    let p = |s: &str| dir.join(s).to_string_lossy().into_owned();
    [
        ("PS3RECOMP_ENG_TEXLOG", "1".to_string()),
        ("PS3RECOMP_ENG_SHADER_DUMP", p("shaders")),
        ("PS3RECOMP_METAL_SURF_DUMP", p("surf")),
        ("PS3RECOMP_METAL_SURF_DUMP_EVERY", "100000000".into()),
        ("PS3RECOMP_METAL_SURF_DUMP_MIN", "2000000000".into()),
        ("PS3RECOMP_METAL_PASS_LOG", "2".into()),
        ("PS3RECOMP_METAL_PASS_LOG_MIN", "1".into()),
        ("PS3RECOMP_METAL_PASS_LOG_EVERY", "100000000".into()),
        ("PS3RECOMP_METAL_PASS_LOG_FLUSH", "1".into()),
        ("PS3RECOMP_METAL_DUMP_TRIGGER", p(TRIGGER)),
        ("PS3RECOMP_METAL_FRAME_DUMP", p("frame.ppm")),
        ("PS3RECOMP_METAL_FPS", "1".into()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .chain(DEBUG_ENV.iter().map(|(k, v)| (k.to_string(), v.to_string())))
    .collect()
}

/// Append one line to the session index: `utc`, `unix_ms`, the log's size
/// now, the event and its detail (`key=value` words).
pub fn event(dir: &Path, unix_ms: u64, utc: &str, kind: &str, detail: &str) -> Result<(), String> {
    let path = dir.join(EVENTS);
    let new = !path.exists();
    let size = fs::metadata(dir.join(LOG)).map(|m| m.len()).unwrap_or(0);
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut line = String::new();
    if new {
        line.push_str("# utc\tunix_ms\tlog_bytes\tevent\tdetail\n");
    }
    line.push_str(&format!("{utc}\t{unix_ms}\t{size}\t{kind}\t{}\n", detail.replace(['\t', '\n'], " ")));
    f.write_all(line.as_bytes()).map_err(|e| e.to_string())
}

/// The present count of the last whole-frame dump the game logged ("captured
/// frame N", every 120 presents): the frame on screen, to the nearest 120.
pub fn last_frame(dir: &Path) -> Option<u64> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = fs::File::open(dir.join(LOG)).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(512 * 1024);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    let key = "] captured frame ";
    let i = text.rfind(key)? + key.len();
    text[i..].split(' ').next()?.parse().ok()
}

/// A new, empty report folder `<reports>/<UTC time>` (with a suffix when two
/// start in the same second), with `surf/`, `shaders/` and `shots/`.
pub fn create(reports: &Path, id: &str) -> Result<PathBuf, String> {
    fs::create_dir_all(reports).map_err(|e| format!("{}: {e}", reports.display()))?;
    let mut dir = reports.join(id);
    let mut n = 1;
    while dir.exists() {
        n += 1;
        dir = reports.join(format!("{id}-{n}"));
    }
    for sub in ["surf", "shaders", "shots"] {
        fs::create_dir_all(dir.join(sub)).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    Ok(dir)
}

/// Ask the running game for this frame: touch the trigger, keep the latest
/// whole-frame image (written every 120 presents) as `shots/shot<n>.ppm`,
/// and note the capture in captures.txt and the session index (with the
/// frame on screen). Returns the capture's number.
pub fn capture(dir: &Path, when: &str, unix_ms: u64) -> Result<u32, String> {
    let n = count_captures(dir) + 1;
    fs::write(dir.join(TRIGGER), b"").map_err(|e| format!("{}: {e}", dir.display()))?;
    let frame = dir.join("frame.ppm");
    let shot = if fs::copy(&frame, dir.join("shots").join(format!("shot{n}.ppm"))).is_ok() {
        format!("shots/shot{n}.png")
    } else {
        "-".into()
    };
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(CAPTURES))
        .map_err(|e| e.to_string())?;
    let frame = last_frame(dir);
    let fr = frame.map(|v| format!(", frame ~{v}")).unwrap_or_default();
    writeln!(f, "capture {n} at {when} UTC (last whole frame: {shot}{fr})").map_err(|e| e.to_string())?;
    let fr = frame.map(|v| format!(" frame={v}")).unwrap_or_default();
    event(dir, unix_ms, when, "capture", &format!("n={n}{fr} shot={shot}"))?;
    Ok(n)
}

fn count_captures(dir: &Path) -> u32 {
    fs::read_to_string(dir.join(CAPTURES))
        .map(|s| s.lines().filter(|l| l.starts_with("capture ")).count() as u32)
        .unwrap_or(0)
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub dir: String,
    pub id: String,
    /// The game started with this report is still running.
    pub live: bool,
    /// Started in debug mode (it has a session index); older diagnostic
    /// reports have none.
    pub debug: bool,
    pub captures: u32,
    /// A capture was asked for and the game has not taken it yet (no present since).
    pub pending: bool,
    pub surfaces: u32,
    pub shaders: u32,
    pub bytes: u64,
    pub log_bytes: u64,
    /// Seconds from the start to the exit (or the last tick).
    pub duration: Option<f64>,
    /// How it ended: "code=0", "signal=11", ... and whether Stop was pressed.
    pub exit: Option<String>,
    pub stopped: bool,
    pub crash_report: bool,
    /// The .zip made from it, when there is one.
    pub zip: Option<String>,
    pub zip_bytes: u64,
    /// log_triage.py's summary, when it has run.
    pub triage: Option<Triage>,
}

/// The part of triage.json the launcher shows.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Triage {
    pub errors: u64,
    pub warnings: u64,
    pub expected: u64,
    pub captures: u64,
    /// The log changed after the triage ran (the game was still running).
    pub stale: bool,
    pub md: String,
    pub top: Vec<Issue>,
    /// Problem counts by category (errors + warnings), the non-zero ones.
    pub categories: Vec<(String, u64)>,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub category: String,
    pub severity: String,
    pub title: String,
    pub count: u64,
    pub first: Option<f64>,
    pub last: Option<f64>,
}

/// Read triage.json (only what the launcher shows; unknown fields ignored).
pub fn read_triage(dir: &Path) -> Option<Triage> {
    let path = dir.join(TRIAGE_JSON);
    let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).ok()?).ok()?;
    let sm = &v["summary"];
    let n = |x: &serde_json::Value| x.as_u64().unwrap_or(0);
    let top = v["top"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|g| Issue {
            category: g["category"].as_str().unwrap_or("").into(),
            severity: g["severity"].as_str().unwrap_or("").into(),
            title: g["title"].as_str().unwrap_or("").into(),
            count: n(&g["count"]),
            first: g["first"].as_f64(),
            last: g["last"].as_f64(),
        })
        .collect();
    let categories = sm["categories"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, c)| (k.clone(), n(&c["errors"]) + n(&c["warnings"])))
        .filter(|(_, c)| *c > 0)
        .collect::<Vec<_>>();
    let mut categories = categories;
    categories.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mtime = |p: PathBuf| fs::metadata(p).and_then(|m| m.modified()).ok();
    let stale = match (mtime(dir.join(LOG)), mtime(path)) {
        (Some(log), Some(t)) => log > t,
        _ => false,
    };
    Some(Triage {
        errors: n(&sm["errors"]),
        warnings: n(&sm["warnings"]),
        expected: n(&sm["expected"]),
        captures: n(&sm["captures"]),
        stale,
        md: dir.join(TRIAGE_MD).to_string_lossy().into_owned(),
        top,
        categories,
    })
}

/// The session index's start, last line and exit, for the list.
struct Ending {
    duration: Option<f64>,
    exit: Option<String>,
    stopped: bool,
}

fn ending(dir: &Path) -> Option<Ending> {
    let text = fs::read_to_string(dir.join(EVENTS)).ok()?;
    let (mut start, mut last, mut exit, mut stopped) = (None, None, None, false);
    for l in text.lines().filter(|l| !l.starts_with('#')) {
        let p: Vec<&str> = l.split('\t').collect();
        let Some(ms) = p.get(1).and_then(|v| v.parse::<u64>().ok()) else { continue };
        match p.get(3).copied() {
            Some("start") => start = Some(ms),
            Some("stop") => stopped = true,
            Some("exit") => exit = p.get(4).map(|d| d.to_string()),
            _ => {}
        }
        last = Some(ms);
    }
    Some(Ending {
        duration: start.zip(last).map(|(a, b)| b.saturating_sub(a) as f64 / 1000.0),
        exit,
        stopped,
    })
}

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(it) = fs::read_dir(&d) else { continue };
        for e in it.flatten() {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(p),
                Ok(t) if t.is_file() => out.push(p),
                _ => {}
            }
        }
    }
    out.sort();
    out
}

pub fn zip_path(dir: &Path) -> PathBuf {
    let id = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    dir.with_file_name(format!("infamous2-report-{id}.zip"))
}

pub fn info(dir: &Path, live: bool) -> Info {
    let all = files(dir);
    let count = |sub: &str, ext: &str| {
        all.iter()
            .filter(|p| p.parent().and_then(Path::file_name).is_some_and(|n| n == sub))
            .filter(|p| p.to_string_lossy().ends_with(ext))
            .count() as u32
    };
    let zip = zip_path(dir);
    let zip_bytes = fs::metadata(&zip).map(|m| m.len()).ok();
    let end = ending(dir);
    Info {
        dir: dir.to_string_lossy().into_owned(),
        id: dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        live,
        debug: end.is_some(),
        captures: count_captures(dir),
        pending: dir.join(TRIGGER).exists(),
        surfaces: count("surf", ".ppm"),
        shaders: count("shaders", "_ps.hlsl"),
        bytes: all.iter().filter_map(|p| fs::metadata(p).ok()).map(|m| m.len()).sum(),
        log_bytes: fs::metadata(dir.join(LOG)).map(|m| m.len()).unwrap_or(0),
        duration: end.as_ref().and_then(|e| e.duration),
        exit: end.as_ref().and_then(|e| e.exit.clone()),
        stopped: end.as_ref().is_some_and(|e| e.stopped),
        crash_report: dir.join(CRASH).is_file(),
        zip: zip_bytes.map(|_| zip.to_string_lossy().into_owned()),
        zip_bytes: zip_bytes.unwrap_or(0),
        triage: read_triage(dir),
    }
}

/// Every report folder under `reports`, newest first (at most `max`).
pub fn list(reports: &Path, max: usize) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(reports)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(|c: char| c.is_ascii_digit())))
        .collect();
    v.sort();
    v.reverse();
    v.truncate(max);
    v
}

/// A report folder by its id, refusing anything that is not a plain name
/// under `reports`.
pub fn by_id(reports: &Path, id: &str) -> Result<PathBuf, String> {
    let ok = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let dir = reports.join(id);
    if ok && dir.is_dir() {
        Ok(dir)
    } else {
        Err(format!("no report {id:?}"))
    }
}

/// The newest macOS crash report of `exe` written at or after `since`
/// (seconds since the epoch) in `reports_dir` (~/Library/Logs/DiagnosticReports).
pub fn find_crash_report(reports_dir: &Path, exe: &str, since: u64) -> Option<PathBuf> {
    let mut best: Option<(u64, PathBuf)> = None;
    for e in fs::read_dir(reports_dir).ok()?.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.starts_with(exe) || !(name.ends_with(".ips") || name.ends_with(".crash")) {
            continue;
        }
        let Some(t) = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
        else {
            continue;
        };
        if t >= since && best.as_ref().map_or(true, |(b, _)| t > *b) {
            best = Some((t, e.path()));
        }
    }
    best.map(|(_, p)| p)
}

/// The newest report folder under `reports` (their names sort by time).
pub fn latest(reports: &Path) -> Option<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(reports)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(|c: char| c.is_ascii_digit())))
        .collect();
    v.sort();
    v.pop()
}

/// Binary PPM (P6, maxval 255) -> PNG (RGB, 8 bit, "Sub" filter on every
/// row); None when it is not one (or was cut short while being written).
pub fn ppm_to_png(d: &[u8]) -> Option<Vec<u8>> {
    if d.get(..2)? != b"P6" {
        return None;
    }
    let mut i = 2;
    let mut nums = [0usize; 3];
    for n in nums.iter_mut() {
        loop {
            while d.get(i)?.is_ascii_whitespace() {
                i += 1;
            }
            if *d.get(i)? == b'#' {
                while *d.get(i)? != b'\n' {
                    i += 1;
                }
            } else {
                break;
            }
        }
        let start = i;
        while d.get(i)?.is_ascii_digit() {
            i += 1;
        }
        *n = std::str::from_utf8(&d[start..i]).ok()?.parse().ok()?;
    }
    let [w, h, max] = nums;
    if max != 255 || w == 0 || h == 0 || w > 16384 || h > 16384 || !d.get(i)?.is_ascii_whitespace() {
        return None;
    }
    let px = d.get(i + 1..i + 1 + w * h * 3)?;
    let stride = w * 3;
    let mut raw = Vec::with_capacity((stride + 1) * h);
    for row in px.chunks_exact(stride) {
        raw.push(1u8); // Sub: each byte minus the same channel one pixel left
        raw.extend_from_slice(&row[..3]);
        raw.extend(row[3..].iter().zip(row.iter()).map(|(a, b)| a.wrapping_sub(*b)));
    }
    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
    z.write_all(&raw).ok()?;
    let idat = z.finish().ok()?;
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8 bit, RGB, deflate, adaptive filters, no interlace
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    for (kind, body) in [(&b"IHDR"[..], &ihdr[..]), (b"IDAT", &idat), (b"IEND", &[])] {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        let crc = crc32fast::hash(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    Some(out)
}

/// Text files whose paths go through `redact` in the .zip.
fn is_text(name: &str) -> bool {
    [".log", ".txt", ".md", ".json", ".tsv", ".ips"].iter().any(|e| name.ends_with(e))
}

/// Pack `dir` into `dest`: every file but the trigger and earlier archives,
/// PPM images as PNG, the log and other text with `home` shown as `~`.
pub fn pack(dir: &Path, dest: &Path, home: &str) -> Result<u64, String> {
    let mut entries = Vec::new();
    for p in files(dir) {
        let rel = p.strip_prefix(dir).map_err(|e| e.to_string())?;
        let mut name = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
        if name == TRIGGER || name.ends_with(".zip") || name.ends_with(".partial") {
            continue;
        }
        let mtime = fs::metadata(&p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let Ok(mut data) = fs::read(&p) else { continue };
        if name.ends_with(".ppm") {
            if let Some(png) = ppm_to_png(&data) {
                data = png;
                name.truncate(name.len() - 4);
                name.push_str(".png");
            }
        } else if is_text(&name) && home.len() > 1 {
            data = String::from_utf8_lossy(&data).replace(home, "~").into_bytes();
        }
        entries.push(zipfile::Entry { name, data, mtime });
    }
    if entries.is_empty() {
        return Err(format!("{}: nothing to pack", dir.display()));
    }
    zipfile::write(dest, &entries)?;
    fs::metadata(dest).map(|m| m.len()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    fn ppm(w: usize, h: usize) -> (Vec<u8>, Vec<u8>) {
        let px: Vec<u8> = (0..w * h * 3).map(|i| (i * 7 % 251) as u8).collect();
        let mut d = format!("P6\n# a comment\n{w} {h}\n255\n").into_bytes();
        d.extend_from_slice(&px);
        (d, px)
    }

    #[test]
    fn ppm_becomes_a_png_with_the_same_pixels() {
        let (d, px) = ppm(5, 3);
        let png = ppm_to_png(&d).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 5);
        assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 3);
        let ihdr_crc = crc32fast::hash(&png[12..29]);
        assert_eq!(u32::from_be_bytes(png[29..33].try_into().unwrap()), ihdr_crc);
        let len = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
        assert_eq!(&png[37..41], b"IDAT");
        let mut raw = Vec::new();
        ZlibDecoder::new(&png[41..41 + len]).read_to_end(&mut raw).unwrap();
        // undo the Sub filter
        let mut back = Vec::new();
        for row in raw.chunks_exact(1 + 15) {
            assert_eq!(row[0], 1);
            let mut r = row[1..].to_vec();
            for i in 3..r.len() {
                r[i] = r[i].wrapping_add(r[i - 3]);
            }
            back.extend(r);
        }
        assert_eq!(back, px);
        assert!(png.ends_with(&[0xAE, 0x42, 0x60, 0x82])); // IEND's CRC
    }

    #[test]
    fn short_or_foreign_images_are_left_alone() {
        let (d, _) = ppm(4, 4);
        assert!(ppm_to_png(&d[..d.len() - 1]).is_none()); // cut short
        assert!(ppm_to_png(b"P5\n1 1\n255\n\0").is_none());
        assert!(ppm_to_png(b"P6\n1 1\n65535\n\0\0\0\0\0\0").is_none());
        assert!(ppm_to_png(b"").is_none());
    }

    #[test]
    fn env_points_every_dump_at_the_report() {
        let env: std::collections::BTreeMap<_, _> = env(Path::new("/r/x")).into_iter().collect();
        assert_eq!(env["PS3RECOMP_METAL_DUMP_TRIGGER"], "/r/x/.dump-now");
        assert_eq!(env["PS3RECOMP_METAL_SURF_DUMP"], "/r/x/surf");
        assert_eq!(env["PS3RECOMP_ENG_SHADER_DUMP"], "/r/x/shaders");
        assert_eq!(env["PS3RECOMP_METAL_FRAME_DUMP"], "/r/x/frame.ppm");
        assert_eq!(env["PS3RECOMP_METAL_PASS_LOG"], "2");
        assert_eq!(env["PS3RECOMP_METAL_FPS"], "1");
        // atoi() in the backend: it must fit an int
        assert!(env["PS3RECOMP_METAL_SURF_DUMP_MIN"].parse::<i32>().is_ok());
        // debug mode's problem logs
        for k in ["PS3RECOMP_ENG_FPSLOG", "PS3_EDGE_WATCH", "LV2_ERRDBG", "AUDIO_RATE", "PS3RECOMP_ENG_DROPLOG", "PS3_THREAD_DUMP"] {
            assert!(env.contains_key(k), "{k}");
        }
        // nothing that changes what is drawn (the edge watch only reports at 1; 2 repairs)
        assert_eq!(env["PS3_EDGE_WATCH"], "1");
    }

    #[test]
    fn session_index_triage_and_list() {
        let root = std::env::temp_dir().join(format!("i2l-session-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = create(&root, "20261018-120000").unwrap();
        assert!(last_frame(&dir).is_none());
        fs::write(dir.join(LOG), "[boot] x\n[rsx engine/metal] captured frame 120 to /f.ppm t=1.000\n").unwrap();
        event(&dir, 1_000, "2026-10-18 12:00:00", "start", "").unwrap();
        let size = fs::metadata(dir.join(LOG)).unwrap().len();
        fs::OpenOptions::new().append(true).open(dir.join(LOG)).unwrap()
            .write_all(b"[rsx engine/metal] captured frame 240 to /f.ppm t=5.000\n[WAIT] x\n").unwrap();
        assert_eq!(last_frame(&dir), Some(240));
        assert_eq!(capture(&dir, "2026-10-18 12:00:04", 4_000).unwrap(), 1);
        event(&dir, 9_500, "2026-10-18 12:00:09", "stop", "").unwrap();
        event(&dir, 10_000, "2026-10-18 12:00:10", "exit", "signal=15").unwrap();
        let ev = fs::read_to_string(dir.join(EVENTS)).unwrap();
        let lines: Vec<&str> = ev.lines().collect();
        assert!(lines[0].starts_with("# utc\tunix_ms"));
        assert_eq!(lines[1], format!("2026-10-18 12:00:00\t1000\t{size}\tstart\t"));
        assert!(lines[2].ends_with("\tcapture\tn=1 frame=240 shot=-"), "{}", lines[2]);
        assert!(fs::read_to_string(dir.join(CAPTURES)).unwrap().contains("frame ~240"));
        let i = info(&dir, false);
        assert!(i.debug && i.stopped && !i.crash_report && i.triage.is_none());
        assert_eq!(i.exit.as_deref(), Some("signal=15"));
        assert_eq!(i.duration, Some(9.0));

        fs::write(
            dir.join(TRIAGE_JSON),
            r#"{"schema":1,"summary":{"errors":2,"warnings":5,"expected":9,"captures":1,
               "categories":{"stall":{"errors":0,"warnings":3},"fs":{"errors":0,"warnings":0},"shader":{"errors":2,"warnings":2}}},
               "top":[{"category":"shader","severity":"error","title":"pipeline x FAILED","count":2,"first":1.5,"last":null}]}"#,
        )
        .unwrap();
        let t = read_triage(&dir).unwrap();
        assert_eq!((t.errors, t.warnings, t.expected, t.captures), (2, 5, 9, 1));
        assert_eq!(t.categories, vec![("shader".to_string(), 4), ("stall".to_string(), 3)]);
        assert_eq!(t.top[0], Issue { category: "shader".into(), severity: "error".into(), title: "pipeline x FAILED".into(), count: 2, first: Some(1.5), last: None });

        let older = create(&root, "20261017-090000").unwrap();
        fs::create_dir_all(root.join("not-a-report")).unwrap();
        assert_eq!(list(&root, 10), vec![dir.clone(), older.clone()]);
        assert_eq!(list(&root, 1), vec![dir.clone()]);
        assert_eq!(by_id(&root, "20261017-090000").unwrap(), older);
        for bad in ["", "..", "../x", "a/b", "20261099-000000"] {
            assert!(by_id(&root, bad).is_err(), "{bad}");
        }

        let diag = root.join("DiagnosticReports");
        fs::create_dir_all(&diag).unwrap();
        fs::write(diag.join("Infamous2Recomp-2026-10-18-120010.ips"), "{}").unwrap();
        fs::write(diag.join("Other-2026-10-18-120010.ips"), "{}").unwrap();
        assert_eq!(find_crash_report(&diag, "Infamous2Recomp", 0).unwrap(), diag.join("Infamous2Recomp-2026-10-18-120010.ips"));
        assert!(find_crash_report(&diag, "Infamous2Recomp", u64::MAX / 2).is_none());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn capture_info_and_zip() {
        let root = std::env::temp_dir().join(format!("i2l-report-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = create(&root, "20261017-120000").unwrap();
        assert_eq!(create(&root, "20261017-120000").unwrap(), root.join("20261017-120000-2"));
        let home = "/Users/someone";
        fs::write(dir.join(LOG), format!("[rsx] vfs {home}/Library/x\nline 2\n")).unwrap();
        fs::write(dir.join("frame.ppm"), ppm(2, 2).0).unwrap();
        assert_eq!(capture(&dir, "2026-10-17 12:00:01", 1).unwrap(), 1);
        assert!(dir.join(TRIGGER).exists());
        assert!(dir.join("shots/shot1.ppm").is_file());
        // the game takes the capture
        fs::remove_file(dir.join(TRIGGER)).unwrap();
        fs::write(dir.join("surf/f12_h1.ppm"), ppm(3, 2).0).unwrap();
        fs::write(dir.join("surf/f12_h2.ppm"), b"P6\n9 9\n255\n").unwrap(); // half written
        fs::write(dir.join("shaders/p1_vs.hlsl"), "vs").unwrap();
        fs::write(dir.join("shaders/p1_ps.hlsl"), "ps").unwrap();
        fs::write(dir.join("shaders/manifest.txt"), "p=1 fp=0 vp=0\n").unwrap();
        assert_eq!(capture(&dir, "2026-10-17 12:00:09", 9).unwrap(), 2);
        let i = info(&dir, true);
        assert_eq!((i.captures, i.surfaces, i.shaders, i.pending), (2, 2, 1, true));
        assert!(i.zip.is_none());
        assert_eq!(latest(&root).unwrap(), root.join("20261017-120000-2"));

        let zip = zip_path(&dir);
        assert_eq!(zip, root.join("infamous2-report-20261017-120000.zip"));
        pack(&dir, &zip, home).unwrap();
        let lim = zipfile::Limits { entries: 100, file: 1 << 24, total: 1 << 26 };
        let got = zipfile::read(&zip, &lim).unwrap();
        let names: Vec<&str> = got.iter().map(|e| e.name.as_str()).collect();
        for want in ["game.log", "captures.txt", "events.tsv", "frame.png", "shots/shot1.png", "shots/shot2.png", "surf/f12_h1.png", "surf/f12_h2.ppm", "shaders/p1_ps.hlsl", "shaders/manifest.txt"] {
            assert!(names.contains(&want), "{want} missing from {names:?}");
        }
        assert!(!names.iter().any(|n| n.contains(TRIGGER) || n.ends_with(".zip")));
        let log = got.iter().find(|e| e.name == "game.log").unwrap();
        assert_eq!(String::from_utf8_lossy(&log.data), "[rsx] vfs ~/Library/x\nline 2\n");
        assert_eq!(info(&dir, false).zip.as_deref(), Some(zip.to_string_lossy().as_ref()));
        fs::remove_dir_all(&root).unwrap();
    }
}
