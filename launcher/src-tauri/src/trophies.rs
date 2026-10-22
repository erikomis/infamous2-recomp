//! The game's trophy list for the Game page: names, descriptions, grades and
//! icons from the player's own `TROPHY.TRP` (`TROPDIR/<commId>_00` in the
//! install's VFS; nothing ships with the app), unlock times from the port's
//! `<gamedata>/trophies/<commId>.json` (the toolkit's sceNpTrophy).

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Trophy {
    pub id: u32,
    /// "P", "G", "S" or "B".
    pub grade: String,
    pub name: String,
    pub detail: String,
    pub hidden: bool,
    /// Seconds since the epoch; 0 while locked.
    pub unlocked_at: u64,
    /// data: URL of TROPnnn.PNG, or "".
    pub icon: String,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrophyList {
    pub comm_id: String,
    pub title: String,
    pub trophies: Vec<Trophy>,
}

/// The entries of a TRP archive: (name, offset, size).
pub fn trp_entries(d: &[u8]) -> Vec<(String, usize, usize)> {
    let be32 = |o: usize| u32::from_be_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]) as usize;
    let be64 = |o: usize| u64::from_be_bytes(d[o..o + 8].try_into().unwrap()) as usize;
    let mut out = Vec::new();
    if d.len() < 0x40 || d[0..4] != [0xDC, 0xA2, 0x4D, 0x00] {
        return out;
    }
    let (count, esz) = (be32(16), be32(20));
    if esz < 48 {
        return out;
    }
    for i in 0..count.min(4096) {
        let o = 0x40 + i * esz;
        if o + 48 > d.len() {
            break;
        }
        let name: String = d[o..o + 32].iter().take_while(|&&c| c != 0).map(|&c| c as char).collect();
        let (off, sz) = (be64(o + 32), be64(o + 40));
        if off.checked_add(sz).is_some_and(|e| e <= d.len()) {
            out.push((name, off, sz));
        }
    }
    out
}

fn entry<'a>(d: &'a [u8], entries: &[(String, usize, usize)], name: &str) -> Option<&'a [u8]> {
    entries.iter().find(|e| e.0.eq_ignore_ascii_case(name)).map(|e| &d[e.1..e.1 + e.2])
}

/// TROP_<id>.SFM for a game-language code (PS3 system language ids).
pub fn lang_file(lang: &str) -> &'static str {
    match lang.get(..2).unwrap_or("") {
        "ja" => "TROP_00.SFM",
        "fr" => "TROP_02.SFM",
        "es" => "TROP_03.SFM",
        "de" => "TROP_04.SFM",
        "it" => "TROP_05.SFM",
        "nl" => "TROP_06.SFM",
        "pt" => "TROP_07.SFM",
        "ru" => "TROP_08.SFM",
        "ko" => "TROP_09.SFM",
        "zh" => "TROP_11.SFM",
        "fi" => "TROP_12.SFM",
        "sv" => "TROP_13.SFM",
        "da" => "TROP_14.SFM",
        "no" => "TROP_15.SFM",
        "pl" => "TROP_16.SFM",
        _ => "TROP.SFM",
    }
}

fn tag<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let a = s.find(&open)? + open.len();
    let b = s[a..].find(&close)? + a;
    Some(&s[a..b])
}

fn attr<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("{name}=\"");
    let a = s.find(&key)? + key.len();
    let b = s[a..].find('"')? + a;
    Some(&s[a..b])
}

fn unescape(s: &str) -> String {
    s.replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

/// The trophies of an SFM (XML) file, in file order, without unlock state.
pub fn parse_sfm(xml: &str) -> (String, Vec<Trophy>) {
    let title = tag(xml, "title-name").map(unescape).unwrap_or_default();
    let mut list = Vec::new();
    for chunk in xml.split("<trophy ").skip(1) {
        let head = &chunk[..chunk.find('>').unwrap_or(chunk.len())];
        let Some(id) = attr(head, "id").and_then(|v| v.parse::<u32>().ok()) else { continue };
        let body = &chunk[..chunk.find("</trophy>").unwrap_or(chunk.len())];
        list.push(Trophy {
            id,
            grade: attr(head, "ttype").unwrap_or("").to_string(),
            name: tag(body, "name").map(unescape).unwrap_or_default(),
            detail: tag(body, "detail").map(unescape).unwrap_or_default(),
            hidden: attr(head, "hidden") == Some("yes"),
            unlocked_at: 0,
            icon: String::new(),
        });
    }
    (title, list)
}

/// `{ "trophies": [ { "id": N, "timestamp": T }, ... ] }` -> (id, time).
pub fn parse_unlocked(json: &str) -> Vec<(u32, u64)> {
    let mut out = Vec::new();
    for item in json.split('{').skip(2) {
        let num = |key: &str| -> Option<u64> {
            let k = format!("\"{key}\"");
            let a = item.find(&k)? + k.len();
            let rest = item[a..].trim_start().strip_prefix(':')?.trim_start();
            let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
            rest[..end].parse().ok()
        };
        if let (Some(id), Some(t)) = (num("id"), num("timestamp")) {
            out.push((id as u32, t));
        }
    }
    out
}

/// TROPDIR/<commId>_00/TROPHY.TRP under the install's VFS.
fn find_trp(vfs: &Path) -> Option<(PathBuf, String)> {
    let mut dirs = vec![vfs.join("PS3_GAME/TROPDIR")];
    if let Ok(rd) = fs::read_dir(vfs.join("dev_hdd0/game")) {
        dirs.extend(rd.flatten().map(|e| e.path().join("TROPDIR")));
    }
    for d in dirs {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path().join("TROPHY.TRP");
            let name = e.file_name().to_string_lossy().into_owned();
            if p.is_file() {
                let comm = name.trim_end_matches("_00").to_string();
                return Some((p, comm));
            }
        }
    }
    None
}

pub fn list(vfs: &Path, gamedata: &Path, lang: &str, icons: bool) -> Result<TrophyList, String> {
    let (trp, comm_id) = find_trp(vfs).ok_or("no TROPHY.TRP in the installed game")?;
    let d = fs::read(&trp).map_err(|e| format!("{}: {e}", trp.display()))?;
    let entries = trp_entries(&d);
    let sfm = entry(&d, &entries, lang_file(lang))
        .or_else(|| entry(&d, &entries, "TROP.SFM"))
        .ok_or("TROPHY.TRP has no TROP.SFM")?;
    let (title, mut trophies) = parse_sfm(&String::from_utf8_lossy(sfm));
    let unlocked = fs::read_to_string(gamedata.join("trophies").join(format!("{comm_id}.json")))
        .map(|s| parse_unlocked(&s))
        .unwrap_or_default();
    for t in &mut trophies {
        if let Some(u) = unlocked.iter().find(|u| u.0 == t.id) {
            t.unlocked_at = u.1;
        }
        if icons {
            if let Some(png) = entry(&d, &entries, &format!("TROP{:03}.PNG", t.id)) {
                t.icon = format!("data:image/png;base64,{}", crate::base64(png));
            }
        }
    }
    Ok(TrophyList { comm_id, title, trophies })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trp(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut d = vec![0u8; 0x40 + files.len() * 64];
        d[0..4].copy_from_slice(&[0xDC, 0xA2, 0x4D, 0x00]);
        d[16..20].copy_from_slice(&(files.len() as u32).to_be_bytes());
        d[20..24].copy_from_slice(&64u32.to_be_bytes());
        for (i, (name, data)) in files.iter().enumerate() {
            let o = 0x40 + i * 64;
            d[o..o + name.len()].copy_from_slice(name.as_bytes());
            let off = d.len() as u64;
            d[o + 32..o + 40].copy_from_slice(&off.to_be_bytes());
            d[o + 40..o + 48].copy_from_slice(&(data.len() as u64).to_be_bytes());
            d.extend_from_slice(data);
        }
        d
    }

    #[test]
    fn trp_sfm_and_unlocks() {
        let sfm = br#"<trophyconf><title-name>Game &amp; Co</title-name>
<trophy id="000" hidden="no" ttype="P" pid="000"><name>All</name><detail>Everything</detail></trophy>
<trophy id="028" hidden="yes" ttype="B" pid="000"><name>Watch That First Step</name><detail>Do &quot;it&quot;</detail></trophy>
</trophyconf>"#;
        let d = trp(&[("TROP.SFM", sfm), ("TROP028.PNG", b"png")]);
        let e = trp_entries(&d);
        assert_eq!(e.len(), 2);
        let (title, t) = parse_sfm(&String::from_utf8_lossy(entry(&d, &e, "TROP.SFM").unwrap()));
        assert_eq!(title, "Game & Co");
        assert_eq!(t.len(), 2);
        assert_eq!((t[1].id, t[1].grade.as_str(), t[1].hidden), (28, "B", true));
        assert_eq!(t[1].detail, "Do \"it\"");
        let u = parse_unlocked("{\n  \"trophies\": [\n    { \"id\": 28, \"timestamp\": 1792039153 }\n  ]\n}");
        assert_eq!(u, vec![(28, 1792039153)]);
        assert_eq!(lang_file("pt"), "TROP_07.SFM");
        assert_eq!(lang_file("en"), "TROP.SFM");
        assert!(trp_entries(b"not a trp").is_empty());
    }
}

#[cfg(test)]
mod real_copy {
    /// `INFAMOUS2_VFS=<vfs> INFAMOUS2_GAMEDATA=<gamedata> cargo test -- --ignored real_copy`
    #[test]
    #[ignore]
    fn real_copy() {
        let (Ok(vfs), Ok(gd)) = (std::env::var("INFAMOUS2_VFS"), std::env::var("INFAMOUS2_GAMEDATA")) else { return };
        let l = super::list(std::path::Path::new(&vfs), std::path::Path::new(&gd), "pt", true).unwrap();
        let got: Vec<_> = l.trophies.iter().filter(|t| t.unlocked_at > 0).map(|t| (t.id, t.name.clone())).collect();
        println!("{} {} trophies={} got={:?} icons={}", l.comm_id, l.title, l.trophies.len(), got,
                 l.trophies.iter().filter(|t| !t.icon.is_empty()).count());
    }
}
