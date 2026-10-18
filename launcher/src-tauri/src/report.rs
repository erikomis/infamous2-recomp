//! "Report a problem": a game session started in diagnostic mode writes what
//! `infamous2-lle/tools/play_dump.sh` writes (the texture log, every
//! pipeline's shaders, and on request one frame's render targets and pass
//! log) into its own folder, `<data>/reports/<UTC time>/`; a capture touches
//! the trigger file the Metal backend watches, and the folder is then packed
//! into a .zip to send (surfaces as PNG, text with the home folder's path
//! replaced by `~`).

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
/// The game's output in diagnostic mode.
pub const LOG: &str = "game.log";
/// One line per capture the player asked for.
pub const CAPTURES: &str = "captures.txt";
pub const SYSTEM: &str = "system.txt";
pub const NOTES: &str = "notes.txt";

/// The variables play_dump.sh sets, pointed at `dir`. Two differences: no
/// automatic surface dump at the first world frame (only the captures dump:
/// SURF_DUMP_MIN above any frame's record count), and the FPS line in the
/// log once per second.
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
    .collect()
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
/// and note the capture. Returns the capture's number.
pub fn capture(dir: &Path, when: &str) -> Result<u32, String> {
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
    writeln!(f, "capture {n} at {when} UTC (last whole frame: {shot})").map_err(|e| e.to_string())?;
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
    pub captures: u32,
    /// A capture was asked for and the game has not taken it yet (no present since).
    pub pending: bool,
    pub surfaces: u32,
    pub shaders: u32,
    pub bytes: u64,
    /// The .zip made from it, when there is one.
    pub zip: Option<String>,
    pub zip_bytes: u64,
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
    Info {
        dir: dir.to_string_lossy().into_owned(),
        id: dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        live,
        captures: count_captures(dir),
        pending: dir.join(TRIGGER).exists(),
        surfaces: count("surf", ".ppm"),
        shaders: count("shaders", "_ps.hlsl"),
        bytes: all.iter().filter_map(|p| fs::metadata(p).ok()).map(|m| m.len()).sum(),
        zip: zip_bytes.map(|_| zip.to_string_lossy().into_owned()),
        zip_bytes: zip_bytes.unwrap_or(0),
    }
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
    [".log", ".txt"].iter().any(|e| name.ends_with(e))
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
        assert_eq!(capture(&dir, "2026-10-17 12:00:01").unwrap(), 1);
        assert!(dir.join(TRIGGER).exists());
        assert!(dir.join("shots/shot1.ppm").is_file());
        // the game takes the capture
        fs::remove_file(dir.join(TRIGGER)).unwrap();
        fs::write(dir.join("surf/f12_h1.ppm"), ppm(3, 2).0).unwrap();
        fs::write(dir.join("surf/f12_h2.ppm"), b"P6\n9 9\n255\n").unwrap(); // half written
        fs::write(dir.join("shaders/p1_vs.hlsl"), "vs").unwrap();
        fs::write(dir.join("shaders/p1_ps.hlsl"), "ps").unwrap();
        fs::write(dir.join("shaders/manifest.txt"), "p=1 fp=0 vp=0\n").unwrap();
        assert_eq!(capture(&dir, "2026-10-17 12:00:09").unwrap(), 2);
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
        for want in ["game.log", "captures.txt", "frame.png", "shots/shot1.png", "shots/shot2.png", "surf/f12_h1.png", "surf/f12_h2.ppm", "shaders/p1_ps.hlsl", "shaders/manifest.txt"] {
            assert!(names.contains(&want), "{want} missing from {names:?}");
        }
        assert!(!names.iter().any(|n| n.contains(TRIGGER) || n.ends_with(".zip")));
        let log = got.iter().find(|e| e.name == "game.log").unwrap();
        assert_eq!(String::from_utf8_lossy(&log.data), "[rsx] vfs ~/Library/x\nline 2\n");
        assert_eq!(info(&dir, false).zip.as_deref(), Some(zip.to_string_lossy().as_ref()));
        fs::remove_dir_all(&root).unwrap();
    }
}
