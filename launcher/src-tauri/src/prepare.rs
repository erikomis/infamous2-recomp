//! The launcher without its window, for programs that start it
//! (RecompLauncher, see `recomplauncher.json` at the repository's top and
//! the launcher README, "RecompLauncher"):
//!
//! - `--prepare --game <copy> [--rap <file.rap>]`: what Advanced > Install /
//!   rebuild does, for a player who has nothing yet or an install to bring up
//!   to date: the copy checked (RPCS3's NPUA80638 folder, or RPCS3's data
//!   folder that holds it; APP_VER 01.03) and its licence found (RPCS3's
//!   exdata, next to the copy, Downloads/Desktop/Documents), both remembered
//!   as the window's Install screen remembers them; Apple's Command Line
//!   Tools; the build tools rebuild.py --check-json finds missing, with
//!   Homebrew; then the window's own rebuild.py command into the install
//!   folder. It prints `progress <0..1> <text>` lines, the rebuild's output
//!   as plain lines (also into <data>/logs/setup.log), and exits with one of
//!   the codes in `Exit`, 0 only when the game's build is there afterwards.
//! - `--play` with `RECOMPLAUNCHER=1`: the game started at once (the same
//!   Play as the window's button: the settings, the save backup,
//!   logs/game.log), no window; the launcher's process becomes the game's
//!   (exec of run.sh, which execs the game), so its exit code and its
//!   lifetime are the game's. `--game <copy>` is remembered when it is good.
//!
//! Both decide in `run()` before any window or Tauri event loop exists. The
//! rebuild itself is the window's (rebuild_command, prereqs, brew_command in
//! lib.rs); only where its output goes differs.

use crate::*;
use std::io::Write;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// The headless prepare's and `--play`'s exit codes (as recomplauncher.json's
/// exit_codes says them to the player).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// Prepare: the game is built. Play: the game was quit normally.
    Ok,
    /// No copy given, not found, not NPUA80638, not APP_VER 01.03, a disc
    /// copy or PKGs (not read here), or incomplete.
    Copy,
    /// Something the player has to do first: Apple's Command Line Tools
    /// (their installer is opened), Homebrew, RPCS3 or its firmware, disk
    /// space, or not a Mac.
    NeedsPlayer,
    /// A download the build makes failed (the toolkit, Python packages,
    /// Homebrew's formulae): the connection.
    Sources,
    /// The rebuild failed (setup.log has why).
    Build,
    /// Another prepare, a rebuild, or the game itself is running.
    Running,
    /// Play: the game is not built yet.
    NotBuilt,
    /// The launcher failed on its own (its settings, starting a program).
    Failed,
    /// The copy's licence (.rap) was not found.
    Licence,
}

impl Exit {
    #[cfg(test)]
    pub const ALL: [Exit; 9] =
        [Exit::Ok, Exit::Copy, Exit::NeedsPlayer, Exit::Sources, Exit::Build, Exit::Running, Exit::NotBuilt, Exit::Failed, Exit::Licence];

    pub fn code(self) -> i32 {
        match self {
            Exit::Ok => 0,
            Exit::Copy => 40,
            Exit::NeedsPlayer => 41,
            Exit::Sources => 42,
            Exit::Build => 43,
            Exit::Running => 44,
            Exit::NotBuilt => 45,
            Exit::Failed => 46,
            Exit::Licence => 47,
        }
    }

    /// What the player is told (recomplauncher.json's exit_codes; "ok" for
    /// a normal end).
    pub fn says(self) -> &'static str {
        match self {
            Exit::Ok => "ok",
            Exit::Copy => "Your copy isn't one this port builds from: it needs your own inFAMOUS 2 from the PlayStation Store (NPUA80638) with its 1.03 update, as RPCS3 installed it (the NPUA80638 folder in dev_hdd0/game). Version 01.00: install the 1.03 update in RPCS3 first (File > Install Packages/Raps/Edats, the update's .pkg). The disc release doesn't work yet. Pick it again on the game's page.",
            Exit::NeedsPlayer => "Your Mac needs something first: Apple's Command Line Tools (their installer opened: finish it), Homebrew (brew.sh) for the build tools, RPCS3 with the PS3 firmware installed (it decrypts the game once), or more free disk space. The setup's last lines say which; then set the game up again.",
            Exit::Sources => "Couldn't download what the build needs (the toolkit from GitHub, its Python packages or the build tools). Check the internet connection and set the game up again.",
            Exit::Build => "Building the game failed. Set it up again: it continues where it stopped. setup.log in the logs folder says why.",
            Exit::Running => "inFAMOUS 2 is already running, or another setup or rebuild of it is. Close it or wait for it, then try again.",
            Exit::NotBuilt => "The game isn't built yet. Set it up again on the game's page (the first time takes 10 to 30 minutes).",
            Exit::Failed => "The inFAMOUS 2 launcher couldn't do it. Open the launcher itself (Advanced > Install / rebuild) to see why.",
            Exit::Licence => "Your licence for the game (its .rap file) wasn't found. RPCS3 keeps it in dev_hdd0/home/<user>/exdata once the game is installed there; or put the .rap next to the game's folder. Then set the game up again.",
        }
    }
}

// ---------------------------------------------------------------------------
// The command line
// ---------------------------------------------------------------------------

pub const PREPARE_ARG: &str = "--prepare";

#[derive(Debug, PartialEq)]
pub enum Mode {
    /// `--prepare [--game <copy>] [--rap <file>]`: the headless install.
    Prepare { game: Option<PathBuf>, rap: Option<PathBuf> },
    /// `--play [--game <copy>]` started by RecompLauncher: the game, no window.
    PlayDirect { game: Option<PathBuf> },
    /// The window (with `--play`: Play once it is up).
    Window { game: Option<PathBuf> },
}

/// `<flag> <path>` or `<flag>=<path>` (the first one; an empty value is none).
pub fn path_arg(args: &[String], flag: &str) -> Option<PathBuf> {
    let eq = format!("{flag}=");
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        if let Some(v) = a.strip_prefix(&eq) {
            return Some(PathBuf::from(v)).filter(|p| !p.as_os_str().is_empty());
        }
        if a == flag {
            return it.next().filter(|v| !v.is_empty()).map(PathBuf::from);
        }
    }
    None
}

/// What the command line (argv, program first) asks for; `recomplauncher`:
/// RECOMPLAUNCHER=1 is set (under_recomplauncher).
pub fn mode(args: &[String], recomplauncher: bool) -> Mode {
    let game = path_arg(args, "--game");
    if args.iter().skip(1).any(|a| a == PREPARE_ARG) {
        Mode::Prepare { game, rap: path_arg(args, "--rap") }
    } else if recomplauncher && play_shortcut::requested(args.iter().cloned()) {
        Mode::PlayDirect { game }
    } else {
        Mode::Window { game }
    }
}

/// RecompLauncher sets RECOMPLAUNCHER=1 for every game it starts.
pub fn under_recomplauncher() -> bool {
    std::env::var("RECOMPLAUNCHER").is_ok_and(|v| v.trim() == "1")
}

// ---------------------------------------------------------------------------
// The player's copy and licence
// ---------------------------------------------------------------------------

const TESTED_APP_VER: &str = "01.03";

/// The player's copy, as rebuild.py takes it.
#[derive(Debug, PartialEq)]
pub struct GameCopy {
    /// The NPUA80638 folder (PARAM.SFO, USRDIR/EBOOT.BIN).
    pub dir: PathBuf,
    pub app_ver: String,
}

/// Another APP_VER is refused, as rebuild.py's checks do (INFAMOUS2_ANY_VERSION
/// turns that into a warning there, for port development, and here too).
fn any_version() -> bool {
    std::env::var_os("INFAMOUS2_ANY_VERSION").is_some_and(|v| !v.is_empty())
}

/// Whether `p` is the player's NPUA80638 at version 01.03: RPCS3's
/// dev_hdd0/game/NPUA80638 folder, or a folder that holds it (RPCS3's data
/// folder, its dev_hdd0 or game folder), with PARAM.SFO saying NPUA80638 and
/// USRDIR/EBOOT.BIN there. Reads only. The error is said to the player.
pub fn check_copy(p: &Path) -> Result<GameCopy, String> {
    if !p.exists() {
        return Err(format!("{} isn't there: pick your inFAMOUS 2 folder (NPUA80638, in RPCS3's dev_hdd0/game) again.", p.display()));
    }
    if p.is_file() {
        let pkg = p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pkg"));
        return Err(if pkg {
            format!("{} is a PKG: install it in RPCS3 with the 1.03 update's PKG (File > Install Packages/Raps/Edats), then pick its NPUA80638 folder.", p.display())
        } else {
            format!("{} is a file: this port reads the PlayStation Store release as RPCS3 installed it, the NPUA80638 folder (disc images don't work yet).", p.display())
        });
    }
    let Some(dir) = resolve_game_dir(p) else {
        if p.join("PS3_GAME/PARAM.SFO").is_file() {
            return Err(format!("{} is a disc copy (PS3_GAME): this port reads the PlayStation Store release NPUA80638 only so far.", p.display()));
        }
        return Err(format!("{} has no PARAM.SFO: pick RPCS3's dev_hdd0/game/NPUA80638 folder (or RPCS3's data folder that holds it).", p.display()));
    };
    let sfo = read_sfo(&dir.join("PARAM.SFO")).ok_or("Its PARAM.SFO can't be read: the copy may be incomplete.")?;
    let id = sfo.get("TITLE_ID").map(|v| v.trim().to_string()).unwrap_or_default();
    if id != TITLE_ID {
        return Err(if id.is_empty() {
            "That copy's PARAM.SFO has no TITLE_ID: this port needs inFAMOUS 2 from the PlayStation Store, NPUA80638.".into()
        } else if id == "BCUS98125" {
            "That is the disc release (BCUS98125): this port reads the PlayStation Store release NPUA80638 only so far.".into()
        } else {
            format!("That copy is {id}, not inFAMOUS 2 from the PlayStation Store (NPUA80638).")
        });
    }
    if !dir.join("USRDIR/EBOOT.BIN").is_file() {
        return Err(format!("{} has no USRDIR/EBOOT.BIN: the copy is incomplete.", dir.display()));
    }
    let app_ver = sfo.get("APP_VER").map(|v| v.trim().to_string()).unwrap_or_default();
    if app_ver != TESTED_APP_VER && !any_version() {
        return Err(format!(
            "That copy is version {}: the port needs the game with its 1.03 update. Install the inFAMOUS 2 1.03 update in RPCS3 (File > Install Packages/Raps/Edats, the update's .pkg), then set it up again.",
            if app_ver.is_empty() { "?" } else { &app_ver }
        ));
    }
    Ok(GameCopy { dir, app_ver })
}

/// A .rap is 16 bytes.
fn is_rap(p: &Path) -> bool {
    fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() == 16)
}

/// The licence for `copy`: the one given (`--rap`), else the .rap named
/// after the copy's content id where it usually is (search_licence, as the
/// Install screen looks), else the one the launcher remembers, else any
/// NPUA80638 licence found. Err: the given one is not a .rap.
pub fn pick_licence(copy: &Path, given: Option<&Path>, remembered: &str) -> Result<Option<PathBuf>, String> {
    if let Some(r) = given {
        return if is_rap(r) { Ok(Some(r.to_path_buf())) } else { Err(format!("{}: not a licence (a .rap is 16 bytes)", r.display())) };
    }
    let found = search_licence(&[copy.to_string_lossy().into_owned()]);
    let want = found.content_id.as_ref().map(|c| format!("{}.rap", c.to_lowercase()));
    let exact = found.raps.iter().map(PathBuf::from).find(|p| {
        let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase());
        want.is_some() && name == want
    });
    let remembered = Some(PathBuf::from(remembered.trim())).filter(|p| !p.as_os_str().is_empty() && is_rap(p));
    Ok(exact.or(remembered).or_else(|| found.raps.first().map(PathBuf::from)))
}

/// Remember the copy (and its licence) as the Install screen does, so the
/// window and a later Play see the same.
fn remember_copy(copy: &GameCopy, rap: Option<&Path>) -> Result<(), String> {
    let mut st = load_settings_file();
    st.game_paths = vec![copy.dir.to_string_lossy().into_owned()];
    if let Some(r) = rap {
        st.rap_path = r.to_string_lossy().into_owned();
    }
    write_settings(&st)
}

/// `--game` with the window: when it is a good copy, remember it (and its
/// licence, when one is found) before the window opens; otherwise say why
/// on stderr. The window shows the rest as usual.
pub fn window_game(game: Option<&Path>) {
    let Some(g) = game else { return };
    match check_copy(g) {
        Ok(copy) => {
            let rap = pick_licence(g, None, &load_settings_file().rap_path).ok().flatten();
            match remember_copy(&copy, rap.as_deref()) {
                Ok(()) => eprintln!("[launcher] --game: the copy is {}", copy.dir.display()),
                Err(e) => eprintln!("[launcher] --game: {e}"),
            }
        }
        Err(e) => eprintln!("[launcher] --game {}: {e}", g.display()),
    }
}

// ---------------------------------------------------------------------------
// Progress
// ---------------------------------------------------------------------------

/// `progress <fraction> <text>` (RecompLauncher's prepare contract): the
/// fraction from 0 to 1 with three decimals, the text on one line.
pub fn progress_line(f: f64, text: &str) -> String {
    let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("progress {:.3} {text}", f.clamp(0.0, 1.0))
}

/// A terminal's colour and bold escapes (\x1b[...m) taken out.
pub fn strip_esc(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut it = line.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\x1b' && it.peek() == Some(&'[') {
            it.next();
            for c in it.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// A child's output line as it is printed: without escapes or rebuild.py's
/// "LOG " prefix, and indented when it would read as a progress line itself
/// (rebuild.py's own PROGRESS lines among them).
pub fn log_line(raw: &str) -> String {
    let l = strip_esc(raw);
    let l = l.strip_prefix("LOG ").map(str::to_string).unwrap_or(l);
    if l.get(..8).is_some_and(|p| p.eq_ignore_ascii_case("progress")) {
        format!("  {l}")
    } else {
        l
    }
}

/// Where everything prepare prints also goes: <data>/logs/setup.log.
struct Out {
    log: Mutex<Option<fs::File>>,
}

impl Out {
    fn open() -> Out {
        let logs = data_root().join("logs");
        let f = fs::create_dir_all(&logs).ok().and_then(|_| fs::File::create(logs.join("setup.log")).ok());
        Out { log: Mutex::new(f) }
    }

    /// One line on stdout (a closed stdout is ignored: the program that
    /// started us may have stopped reading) and in setup.log.
    fn say(&self, line: &str) {
        let mut o = std::io::stdout().lock();
        let _ = writeln!(o, "{line}");
        let _ = o.flush();
        if let Some(f) = self.log.lock().unwrap().as_mut() {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// The progress lines printed: the fraction never goes back, and a line is
/// printed only when the text changes or the fraction moves by half a
/// percent.
#[derive(Default)]
pub struct Reporter {
    last: f64,
    printed: f64,
    text: String,
    any: bool,
}

impl Reporter {
    pub fn update(&mut self, f: f64, text: &str) -> Option<String> {
        let f = f.clamp(0.0, 1.0).max(self.last);
        self.last = f;
        if self.any && text == self.text && f < self.printed + 0.005 && !(f >= 1.0 && self.printed < 1.0) {
            return None;
        }
        self.any = true;
        self.printed = f;
        self.text = text.to_string();
        Some(progress_line(f, text))
    }

    /// The same, always printed (a step's end or a failure's reason).
    pub fn force(&mut self, f: f64, text: &str) -> String {
        self.any = false;
        self.update(f, text).unwrap_or_default()
    }

    #[cfg(test)]
    pub fn last(&self) -> f64 {
        self.last
    }
}

/// A step's rough share of a first rebuild (src/progress.ts: the same ids
/// and weights; the PPU lift and the compile dominate).
fn weight(id: &str) -> f64 {
    match id {
        "toolkit" | "venv" | "jobs" => 3.0,
        "game" | "decrypt" | "firmware" => 2.0,
        "pup" | "spu" => 1.0,
        "port" | "dlc" => 0.2,
        "ppu" => 30.0,
        "compile" => 50.0,
        "finish" => 0.1,
        _ => 1.0,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum StepState {
    Pending,
    Running,
    Done,
    Error,
}

#[derive(Clone, Debug)]
struct Step {
    id: String,
    state: StepState,
    start: f64,
    sub: f64,
}

/// The rebuild's progress from rebuild.py's machine-readable lines (its
/// docstring; src/progress.ts reads them the same way): PLAN lists the steps
/// (up-to-date ones count as done), PROGRESS starts one, SUBPROGRESS moves it
/// along (the compile), STEP-DONE and STEP-SKIP end it, ERROR fails it.
/// Times are in seconds from any start.
#[derive(Default)]
pub struct StepTracker {
    steps: Vec<Step>,
    /// The step started last: its text stays under the bar until the next
    /// one starts.
    last: Option<String>,
}

impl StepTracker {
    fn step(&mut self, id: &str) -> Option<&mut Step> {
        self.steps.iter_mut().find(|s| s.id == id)
    }

    pub fn feed(&mut self, line: &str, t: f64) {
        let mut w = line.split_whitespace();
        let (Some(kind), Some(a)) = (w.next(), w.next()) else { return };
        match kind {
            "PLAN" => {
                // PLAN i/n id state title
                let (Some(id), Some(state)) = (w.next(), w.next()) else { return };
                if self.step(id).is_none() {
                    let state = if state == "run" { StepState::Pending } else { StepState::Done };
                    self.steps.push(Step { id: id.into(), state, start: t, sub: 0.0 });
                }
            }
            "PROGRESS" => {
                if let Some(s) = self.step(a) {
                    if s.state != StepState::Running {
                        *s = Step { id: s.id.clone(), state: StepState::Running, start: t, sub: 0.0 };
                    }
                    self.last = Some(a.to_string());
                }
            }
            "SUBPROGRESS" => {
                let frac = w.next().and_then(|f| f.split_once('/')).and_then(|(d, n)| Some((d.parse::<f64>().ok()?, n.parse::<f64>().ok()?)));
                if let (Some(s), Some((d, n))) = (self.step(a), frac) {
                    if n > 0.0 {
                        s.sub = (d / n).clamp(0.0, 1.0);
                    }
                }
            }
            "STEP-DONE" | "STEP-SKIP" => {
                if let Some(s) = self.step(a) {
                    s.state = StepState::Done;
                }
            }
            "ERROR" => {
                if let Some(s) = self.step(a) {
                    s.state = StepState::Error;
                }
            }
            _ => {}
        }
    }

    /// The run ended well: every step counts as done.
    pub fn finish(&mut self) {
        for s in &mut self.steps {
            s.state = StepState::Done;
        }
    }

    /// The share of the rebuild done at `t`: a running step counts as far
    /// as it says, or by its time (half done after two minutes, at most 90 %).
    pub fn fraction(&self, t: f64) -> f64 {
        let total: f64 = self.steps.iter().map(|s| weight(&s.id)).sum();
        if total == 0.0 {
            return 0.0;
        }
        let mut done = 0.0;
        for s in &self.steps {
            let w = weight(&s.id);
            match s.state {
                StepState::Done => done += w,
                StepState::Running => {
                    let el = (t - s.start).max(0.0);
                    done += w * s.sub.max((el / (el + 120.0)).min(0.9));
                }
                _ => {}
            }
        }
        done / total
    }

    /// The step running, or the one that failed, else the one started last.
    pub fn stage(&self) -> Option<&str> {
        self.steps
            .iter()
            .find(|s| matches!(s.state, StepState::Running | StepState::Error))
            .map(|s| s.id.as_str())
            .or(self.last.as_deref())
    }
}

/// A step's text under the progress bar.
pub fn stage_text(id: Option<&str>) -> &'static str {
    match id {
        Some("toolkit") => "Setting up the toolkit (ps3recomp)",
        Some("venv") => "Setting up Python for the translators",
        Some("game") => "Copying your game's files",
        Some("pup") => "Taking the firmware out of PS3UPDAT.PUP",
        Some("decrypt") => "Decrypting EBOOT.BIN and the firmware with your licence",
        Some("port") => "Copying the port's sources",
        Some("firmware") => "Translating the firmware",
        Some("ppu") => "Translating the game's code (the longest step)",
        Some("spu") => "Translating the SPU programs",
        Some("jobs") => "Translating the SPU job modules",
        Some("dlc") => "The DLC licence key",
        Some("compile") => "Compiling the game (the other long step)",
        Some("finish") => "Finishing the install",
        _ => "Checking what the build needs",
    }
}

/// The share of the bar before the rebuild starts (the copy, the licence,
/// the tools) and where the rebuild's share ends.
const INSTALL_FROM: f64 = 0.06;
const INSTALL_TO: f64 = 0.99;

pub fn overall(rebuild_fraction: f64) -> f64 {
    INSTALL_FROM + (INSTALL_TO - INSTALL_FROM) * rebuild_fraction.clamp(0.0, 1.0)
}

/// What the end of a download failure looks like (git, pip, curl, Homebrew).
const NETWORK: &[&str] = &[
    "could not resolve host",
    "failed to connect",
    "unable to access",
    "connection timed out",
    "network is unreachable",
    "temporary failure in name resolution",
    "nodename nor servname",
    "newconnectionerror",
    "connection broken",
    "no matching distribution found",
    "could not download",
];

fn looks_offline(tail: &[String]) -> bool {
    let text = tail.join("\n").to_ascii_lowercase();
    NETWORK.iter().any(|n| text.contains(n))
}

/// Why rebuild.py failed, from its exit status (2: a prerequisite, 1: a
/// step), the checks that failed (names), the step it stopped at and its
/// last lines.
pub fn classify(code: i32, failed_checks: &[String], stage: Option<&str>, tail: &[String]) -> Exit {
    let failed = |n: &str| failed_checks.iter().any(|c| c == n);
    if code == 2 {
        return if failed("game") {
            Exit::Copy
        } else if failed("rap") {
            Exit::Licence
        } else if failed("patch") || failed("toolkit") || failed("out") || failed_checks.is_empty() {
            Exit::Failed
        } else {
            Exit::NeedsPlayer
        };
    }
    let text = tail.join("\n").to_ascii_lowercase();
    if matches!(stage, Some("toolkit" | "venv")) && looks_offline(tail) {
        Exit::Sources
    } else if stage == Some("game") && (text.contains("app_ver") || text.contains("not npua80638")) {
        Exit::Copy
    } else {
        Exit::Build
    }
}

// ---------------------------------------------------------------------------
// One at a time
// ---------------------------------------------------------------------------

/// One headless prepare at a time: an exclusive lock on <data>/prepare.lock,
/// held while the returned file is open (the system drops it when the
/// process ends, however it ends). None: another one holds it.
fn take_lock() -> Result<Option<fs::File>, String> {
    let data = data_root();
    fs::create_dir_all(&data).map_err(|e| format!("{}: {e}", data.display()))?;
    let p = data.join("prepare.lock");
    let f = fs::OpenOptions::new().create(true).truncate(false).write(true).open(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    match f.try_lock() {
        Ok(()) => Ok(Some(f)),
        Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(e)) => Err(format!("{}: {e}", p.display())),
    }
}

/// A headless prepare holds the lock right now (only looked at).
fn prepare_running() -> bool {
    matches!(take_lock(), Ok(None))
}

/// The processes' `ps` field (`comm`: each executable's path, spaces kept;
/// `args`: the whole command line), one per process. Only looked at.
fn process_lines(field: &str) -> Vec<String> {
    if !cfg!(unix) {
        return Vec::new();
    }
    Command::new("/bin/ps")
        .args(["-axo", &format!("{field}=")])
        .stderr(Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim().to_string()).collect())
        .unwrap_or_default()
}

/// This game's build, from its executable's path: the bundle's binary as
/// run.sh execs it, or build/Infamous2Recomp from before the bundle, in an
/// install or a development checkout. Not the name alone: the other ports'
/// games on this Mac are *Recomp too, and a command that only mentions the
/// path is not the game.
pub fn is_game_exe(exe: &str) -> bool {
    let e = exe.trim();
    e.ends_with("/infamous2-lle/build/Infamous2Recomp") || e.ends_with("/infamous2-lle/build/Infamous2Recomp.app/Contents/MacOS/Infamous2Recomp")
}

/// A rebuild.py of this port doing work (the window's Install / rebuild, a
/// terminal, or a prepare whose own process was stopped): a Python whose
/// script is .../infamous2-lle/tools/rebuild.py (after options such as -u),
/// not a command that only mentions it; its quick --check-json, --dry-run
/// and --list-steps runs are not.
pub fn is_rebuild_line(l: &str) -> bool {
    let Some(i) = l.find("infamous2-lle/tools/rebuild.py").filter(|&i| i == 0 || matches!(l.as_bytes()[i - 1], b'/' | b' ')) else { return false };
    if ["--check-json", "--dry-run", "--list-steps"].iter().any(|a| l[i..].split_whitespace().any(|w| w == *a)) {
        return false;
    }
    let mut words = l[..i].split(' ');
    let program = words.next().unwrap_or("");
    if !program.rsplit('/').next().unwrap_or("").to_ascii_lowercase().starts_with("python") {
        return false;
    }
    for w in words.filter(|w| !w.is_empty()) {
        if !w.starts_with('-') {
            break; // the script's path
        }
        if w == "-c" || w == "-m" {
            return false;
        }
    }
    true
}

fn game_running() -> bool {
    process_lines("comm").iter().any(|e| is_game_exe(e))
}

fn rebuild_running() -> bool {
    process_lines("args").iter().any(|l| is_rebuild_line(l))
}

/// The app's resources folder (Contents/Resources of the .app the launcher
/// runs from), where the bundled port's rebuild.py is; none in a development
/// build (rebuild_script_in looks next to the sources then).
fn resource_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let res = exe.parent()?.parent()?.join("Resources");
    res.is_dir().then_some(res)
}

// ---------------------------------------------------------------------------
// --prepare
// ---------------------------------------------------------------------------

/// `--prepare`'s process: run_prepare, then on a failure what its exit code
/// means to the player as the last line. Returns the exit code.
pub fn prepare_main(game: Option<&Path>, rap: Option<&Path>) -> i32 {
    let out = Out::open();
    let e = run_prepare(&out, game, rap);
    if e != Exit::Ok {
        out.say(&format!("Exit code {}: {}", e.code(), e.says()));
    }
    e.code()
}

/// A child's stdout and stderr as lines, in the order they come (stderr's
/// with rebuild.py's "LOG " in front, as the window shows them); the
/// channel closes once both have ended.
fn child_lines(child: &mut std::process::Child) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    if let Some(o) = child.stdout.take() {
        let tx = tx.clone();
        thread::spawn(move || {
            for l in BufReader::new(o).lines().map_while(Result::ok) {
                let _ = tx.send(l);
            }
        });
    }
    if let Some(e) = child.stderr.take() {
        thread::spawn(move || {
            for l in BufReader::new(e).lines().map_while(Result::ok) {
                let _ = tx.send(format!("LOG {l}"));
            }
        });
    }
    rx
}

/// `brew install` of what the checks found missing, its output printed.
fn brew_install(out: &Out, rep: &mut Reporter, brew: &Path, formulae: &[String]) -> Result<(), Exit> {
    if let Some(l) = rep.update(0.03, "Installing the build tools with Homebrew") {
        out.say(&l);
    }
    out.say(&format!("$ brew install {}", formulae.join(" ")));
    let mut child = brew_command(brew, formulae).spawn().map_err(|e| {
        out.say(&rep.force(rep.last, &format!("Homebrew wouldn't start: {e}")));
        Exit::Failed
    })?;
    let mut tail = Vec::new();
    for l in child_lines(&mut child) {
        let l = log_line(&l);
        out.say(&l);
        tail.push(l);
        if tail.len() > 200 {
            tail.remove(0);
        }
    }
    let ok = child.wait().is_ok_and(|s| s.success());
    if ok {
        return Ok(());
    }
    let why = format!("Homebrew couldn't install {}: run brew install {} in Terminal to see why.", formulae.join(", "), formulae.join(" "));
    out.say(&rep.force(rep.last, &why));
    Err(if looks_offline(&tail) { Exit::Sources } else { Exit::NeedsPlayer })
}

/// The machine's prerequisites (rebuild.py --check-json), with what
/// Homebrew provides installed first. Err: what the player has to do.
fn build_tools(out: &Out, rep: &mut Reporter, script: &Path, decrypt: &DecryptOptions) -> Result<(), Exit> {
    if let Some(l) = rep.update(0.02, "Checking the build tools") {
        out.say(&l);
    }
    let mut p = prereqs(Some(script.to_path_buf()), decrypt);
    if !p.brew_missing.is_empty() {
        match find_brew() {
            Some(brew) => {
                brew_install(out, rep, &brew, &p.brew_missing)?;
                p = prereqs(Some(script.to_path_buf()), decrypt);
            }
            None => {
                out.say(&rep.force(rep.last, &format!("The build tools ({}) come from Homebrew: install it from brew.sh, then set the game up again.", p.brew_missing.join(", "))));
                return Err(Exit::NeedsPlayer);
            }
        }
    }
    if let Some(e) = &p.error {
        out.say(&rep.force(rep.last, &format!("The build tools couldn't be checked: {e}")));
        return Err(Exit::Failed);
    }
    let fails: Vec<&CheckItem> = p.checks.iter().filter(|c| c.level == "fail").collect();
    if fails.is_empty() {
        return Ok(());
    }
    for c in &fails {
        out.say(&format!("missing: {}: {}", c.name, c.message));
    }
    let why = if p.rpcs3_missing {
        "RPCS3 decrypts the game once: install it from rpcs3.net (with the PS3 firmware: File > Install Firmware), then set the game up again.".to_string()
    } else if p.firmware_missing {
        "The PS3 firmware isn't in RPCS3: install it there (File > Install Firmware, Sony's PS3UPDAT.PUP), then set the game up again.".to_string()
    } else {
        fails.iter().map(|c| format!("{}: {}", c.name, c.message)).collect::<Vec<_>>().join("; ")
    };
    out.say(&rep.force(rep.last, &why));
    Err(if fails.iter().any(|c| c.name == "patch") { Exit::Failed } else { Exit::NeedsPlayer })
}

/// `--prepare --game <copy>`: see the module's comment. Prints progress and
/// log lines on stdout; never opens a window.
fn run_prepare(out: &Out, game: Option<&Path>, rap_arg: Option<&Path>) -> Exit {
    let mut rep = Reporter::default();
    let fail = |rep: &mut Reporter, why: &str| out.say(&rep.force(rep.last, why));
    out.say(&rep.force(0.0, "Checking your copy of the game"));
    if !cfg!(target_os = "macos") || !host_supported() {
        fail(&mut rep, "This port builds and runs on a Mac (Apple silicon) only.");
        return Exit::NeedsPlayer;
    }
    let Some(game) = game else {
        fail(&mut rep, "No copy of the game given: --game <your inFAMOUS 2 NPUA80638 folder>.");
        return Exit::Copy;
    };
    // read only so far: nothing is written before the copy is known good
    let copy = match check_copy(game) {
        Ok(c) => c,
        Err(e) => {
            fail(&mut rep, &e);
            return Exit::Copy;
        }
    };
    out.say(&format!("Your copy: {} ({TITLE_ID}, version {})", copy.dir.display(), copy.app_ver));
    let st = load_settings_file();
    let rap = match pick_licence(game, rap_arg, &st.rap_path) {
        Ok(Some(r)) => r,
        Ok(None) => {
            fail(&mut rep, "Your licence (the .rap for NPUA80638) wasn't found in RPCS3's exdata, next to the copy, or in Downloads, Desktop or Documents.");
            return Exit::Licence;
        }
        Err(e) => {
            fail(&mut rep, &e);
            return Exit::Licence;
        }
    };
    out.say(&format!("Your licence: {}", rap.display()));

    let _lock = match take_lock() {
        Ok(Some(f)) => f,
        Ok(None) => {
            fail(&mut rep, "Another setup of inFAMOUS 2 is already running.");
            return Exit::Running;
        }
        Err(e) => {
            fail(&mut rep, &format!("Couldn't write the launcher's folder: {e}"));
            return Exit::Failed;
        }
    };
    if game_running() {
        fail(&mut rep, "inFAMOUS 2 is running: close it, then set it up again.");
        return Exit::Running;
    }
    if rebuild_running() {
        fail(&mut rep, "A rebuild of inFAMOUS 2 is running (the launcher's Install / rebuild, or a terminal): wait for it, then try again.");
        return Exit::Running;
    }
    let install = Some(PathBuf::from(st.install_dir.trim())).filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(default_install_dir);
    if install.join(".git").exists() {
        fail(&mut rep, &format!("The launcher's install folder is a development checkout ({}): the setup builds only into an install folder. Pick one in the launcher (Advanced > Install / rebuild).", install.display()));
        return Exit::Failed;
    }
    if let Err(e) = remember_copy(&copy, Some(&rap)) {
        fail(&mut rep, &format!("Couldn't remember the copy in the launcher's settings: {e}"));
        return Exit::Failed;
    }

    // Apple's Command Line Tools: their installer needs the player
    out.say(&rep.force(0.01, "Checking Apple's Command Line Tools"));
    if !clt_installed() {
        let opened = install_clt().is_ok();
        fail(&mut rep, if opened {
            "Apple's Command Line Tools are needed: finish their installer (it just opened), then set the game up again."
        } else {
            "Apple's Command Line Tools are needed: run xcode-select --install in Terminal, then set the game up again."
        });
        return Exit::NeedsPlayer;
    }
    let Some(script) = rebuild_script_in(resource_dir()) else {
        fail(&mut rep, "rebuild.py isn't in the launcher (the app may be damaged): download it again.");
        return Exit::Failed;
    };
    let decrypt = DecryptOptions { keys: st.keys_path.clone(), firmware: st.firmware_path.clone(), decrypt_with: st.decrypt_with.clone() };
    if let Err(e) = build_tools(out, &mut rep, &script, &decrypt) {
        return e;
    }
    let Some(python) = find_python() else {
        fail(&mut rep, "No Python 3 to run the rebuild: install the Command Line Tools (xcode-select --install).");
        return Exit::NeedsPlayer;
    };

    // the window's Install / rebuild, with its output here
    let req = RebuildRequest {
        games: vec![copy.dir.to_string_lossy().into_owned()],
        rap: rap.to_string_lossy().into_owned(),
        out: install.to_string_lossy().into_owned(),
        dry_run: false,
        force: false,
        rap_key: None,
        decrypt,
    };
    let mut cmd = match rebuild_command(&python, &script, &req) {
        Ok(c) => c,
        Err(e) => {
            fail(&mut rep, &e);
            return Exit::Failed;
        }
    };
    out.say(&rep.force(INSTALL_FROM, stage_text(None)));
    out.say(&format!("$ {} -u {} --game {} --rap {} --out {}", python.display(), script.display(), req.games[0], req.rap, req.out));
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            fail(&mut rep, &format!("Couldn't start the rebuild: {}: {e}", python.display()));
            return Exit::Failed;
        }
    };
    let lines = child_lines(&mut child);
    let t0 = Instant::now();
    let mut steps = StepTracker::default();
    let mut tail: Vec<String> = Vec::new();
    let mut failed_checks: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    loop {
        // a tick every 5 s moves the bar inside a long step (the lift)
        let line = match lines.recv_timeout(Duration::from_secs(5)) {
            Ok(l) => Some(l),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let t = t0.elapsed().as_secs_f64();
        if let Some(raw) = line {
            let raw = strip_esc(&raw);
            steps.feed(&raw, t);
            let mut w = raw.split_whitespace();
            match (w.next(), w.next(), w.next()) {
                (Some("CHECK"), Some("fail"), Some(name)) => failed_checks.push(name.to_string()),
                (Some("ERROR"), Some(_), Some(_)) => errors.push(raw.splitn(3, ' ').nth(2).unwrap_or_default().to_string()),
                _ => {}
            }
            let shown = log_line(&raw);
            out.say(&shown);
            tail.push(shown);
            if tail.len() > 400 {
                tail.remove(0);
            }
        }
        if let Some(l) = rep.update(overall(steps.fraction(t)), stage_text(steps.stage())) {
            out.say(&l);
        }
    }
    let code = child.wait().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
    if code != 0 {
        let stage = steps.stage().map(str::to_string);
        let exit = classify(code, &failed_checks, stage.as_deref(), &tail);
        let why = errors.last().cloned().unwrap_or_else(|| format!("{} failed (code {code}).", stage_text(stage.as_deref())));
        fail(&mut rep, &why);
        out.say(&format!("The setup's log: {}", data_root().join("logs/setup.log").display()));
        return exit;
    }
    steps.finish();
    let mut st = load_settings_file();
    st.install_dir = install.to_string_lossy().into_owned();
    if let Err(e) = built_install(&st) {
        fail(&mut rep, &format!("The rebuild ended, but the game isn't there: {e}"));
        return Exit::Build;
    }
    if let Err(e) = write_settings(&st) {
        fail(&mut rep, &format!("Couldn't remember the install in the launcher's settings: {e}"));
        return Exit::Failed;
    }
    out.say(&rep.force(1.0, "Ready to play"));
    Exit::Ok
}

// ---------------------------------------------------------------------------
// --play under RecompLauncher
// ---------------------------------------------------------------------------

/// `--play` under RecompLauncher: the copy given with `--game` remembered
/// when it is good (Play itself reads the install's own copy of the game, so
/// a copy moved since is only reported), then the window's Play (the
/// settings, the save backup, logs/game.log), and the launcher's process
/// replaced by run.sh's, which execs the game. Returns an exit code only
/// when the game could not start.
pub fn play_direct(game: Option<&Path>) -> i32 {
    let refuse = |e: Exit, why: &str| {
        eprintln!("[launcher] --play: {why}");
        e.code()
    };
    if let Err(e) = require_supported_host() {
        return refuse(Exit::NeedsPlayer, &e);
    }
    if let Some(g) = game {
        match check_copy(g) {
            Ok(copy) => {
                let rap = pick_licence(g, None, &load_settings_file().rap_path).ok().flatten();
                if let Err(e) = remember_copy(&copy, rap.as_deref()) {
                    eprintln!("[launcher] --play: {e}");
                }
            }
            Err(e) => eprintln!("[launcher] --play: --game {}: {e}", g.display()),
        }
    }
    if game_running() {
        return refuse(Exit::Running, "the game is already running");
    }
    if prepare_running() || rebuild_running() {
        return refuse(Exit::Running, "a setup or a rebuild of the game is running");
    }
    let s = load_settings_file();
    let inst = match built_install(&s) {
        Ok(i) => i,
        Err(e) => return refuse(Exit::NotBuilt, &e),
    };
    let log_path = match game_log_file(None) {
        Ok(p) => p,
        Err(e) => return refuse(Exit::Failed, &e),
    };
    let (out, err) = match fs::File::create(&log_path).and_then(|f| f.try_clone().map(|g| (f, g))) {
        Ok(f) => f,
        Err(e) => return refuse(Exit::Failed, &format!("{}: {e}", log_path.display())),
    };
    if let (_, Some(e)) = backup_saves(&s) {
        eprintln!("[launcher] --play: the save backup failed: {e}");
    }
    let (mut cmd, _) = game_command(&s, &inst, None);
    cmd.stdout(out).stderr(err);
    eprintln!("[launcher] --play: starting the game, log {}", log_path.display());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let e = cmd.exec();
        refuse(Exit::Failed, &format!("{}: {e}", inst.run_sh))
    }
    #[cfg(not(unix))]
    {
        match cmd.status() {
            Ok(st) => st.code().unwrap_or(Exit::Failed.code()),
            Err(e) => refuse(Exit::Failed, &format!("{}: {e}", inst.run_sh)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("i2l-prep-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn args(v: &[&str]) -> Vec<String> {
        std::iter::once("launcher").chain(v.iter().copied()).map(String::from).collect()
    }

    fn profile() -> serde_json::Value {
        serde_json::from_str(include_str!("../../../recomplauncher.json")).unwrap()
    }

    #[test]
    fn command_line() {
        let g = |p: &str| Some(PathBuf::from(p));
        assert_eq!(mode(&args(&["--prepare", "--game", "/x/NPUA80638"]), false), Mode::Prepare { game: g("/x/NPUA80638"), rap: None });
        assert_eq!(mode(&args(&["--game=/x/rpcs3", "--prepare", "--rap", "/r/a.rap"]), true), Mode::Prepare { game: g("/x/rpcs3"), rap: g("/r/a.rap") });
        assert_eq!(mode(&args(&["--prepare"]), false), Mode::Prepare { game: None, rap: None });
        assert_eq!(mode(&args(&["--prepare", "--game"]), false), Mode::Prepare { game: None, rap: None });
        assert_eq!(mode(&args(&["--prepare", "--game", "", "--rap="]), false), Mode::Prepare { game: None, rap: None });
        // --play opens the window, unless RecompLauncher started it
        assert_eq!(mode(&args(&["--play"]), false), Mode::Window { game: None });
        assert_eq!(mode(&args(&["--play", "--game", "/a b/NPUA80638"]), true), Mode::PlayDirect { game: g("/a b/NPUA80638") });
        assert_eq!(mode(&args(&["--play", "--exit-with-game"]), false), Mode::Window { game: None });
        assert_eq!(mode(&args(&["--game", "/x"]), true), Mode::Window { game: g("/x") });
        assert_eq!(mode(&args(&[]), true), Mode::Window { game: None });
        // the program's own name is never an argument
        assert_eq!(mode(&["--prepare".to_string()], false), Mode::Window { game: None });
    }

    #[test]
    fn exit_codes_are_distinct_and_in_the_profile() {
        let codes: std::collections::BTreeSet<i32> = Exit::ALL.iter().map(|e| e.code()).collect();
        assert_eq!(codes.len(), Exit::ALL.len());
        assert_eq!(Exit::Ok.code(), 0);
        // rebuild.py's own statuses (1, 2) are never passed on as they are
        assert!(!codes.contains(&1) && !codes.contains(&2));
        // recomplauncher.json says each one to the player, word for word
        let p = profile();
        let said = p["exit_codes"].as_object().unwrap();
        assert_eq!(said.len(), Exit::ALL.len());
        for e in Exit::ALL {
            assert_eq!(said[&e.code().to_string()].as_str(), Some(e.says()), "{e:?}");
        }
    }

    #[test]
    fn failures_are_told_apart() {
        let none: Vec<String> = Vec::new();
        let c = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // exit 2: a prerequisite, by the checks that failed
        assert_eq!(classify(2, &c(&["game"]), None, &none), Exit::Copy);
        assert_eq!(classify(2, &c(&["disk", "rap"]), None, &none), Exit::Licence);
        assert_eq!(classify(2, &c(&["rpcs3", "firmware"]), None, &none), Exit::NeedsPlayer);
        assert_eq!(classify(2, &c(&["disk"]), None, &none), Exit::NeedsPlayer);
        assert_eq!(classify(2, &c(&["patch"]), None, &none), Exit::Failed);
        assert_eq!(classify(2, &none, None, &c(&["usage: rebuild.py [-h]"])), Exit::Failed);
        // exit 1: a step
        assert_eq!(
            classify(1, &none, Some("toolkit"), &c(&["fatal: unable to access 'https://github.com/x/ps3recomp.git/': Could not resolve host: github.com", "ERROR toolkit setup_toolkit.sh exited with status 128"])),
            Exit::Sources
        );
        assert_eq!(
            classify(1, &none, Some("venv"), &c(&["WARNING: Retrying (Retry(total=4)) after connection broken by 'NewConnectionError'", "ERROR: No matching distribution found for pycryptodome"])),
            Exit::Sources
        );
        assert_eq!(classify(1, &none, Some("toolkit"), &c(&["error: patch failed: CMakeLists.txt:12"])), Exit::Build);
        assert_eq!(
            classify(1, &none, Some("game"), &c(&["ERROR game the imported copy is APP_VER 01.00; the port needs the game with its 1.03 update"])),
            Exit::Copy
        );
        // the decrypt step's failure names nothing of the connection: the build's
        assert_eq!(classify(1, &none, Some("decrypt"), &c(&["ERROR decrypt rpcs3 exited with status 1"])), Exit::Build);
        assert_eq!(classify(1, &none, Some("compile"), &c(&["ninja: build stopped: subcommand failed."])), Exit::Build);
        assert_eq!(classify(-1, &none, Some("ppu"), &none), Exit::Build);
    }

    /// RecompLauncher edits launcher.json with the profile's settings: every
    /// key is one of Settings' own, every value it writes reads back as that
    /// value (a value serde refused would reset the whole file to the
    /// defaults), and the defaults are the launcher's.
    #[test]
    fn profile_settings_are_the_launchers() {
        let p = profile();
        let ours = serde_json::to_value(Settings::default()).unwrap();
        // numbers as numbers (an f32 setting comes back as 0.05000000074...)
        let same = |a: &serde_json::Value, b: &serde_json::Value| match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() < 1e-6,
            _ => a == b,
        };
        let json = |v: &serde_json::Value| -> serde_json::Value { serde_json::from_str(v.as_str().unwrap()).unwrap() };
        assert_eq!(p["config"][0]["format"], "json");
        assert!(p["config"][0]["file"][0]["path"].as_str().unwrap().ends_with("/infamous2-recomp/launcher.json"));
        let groups: Vec<&str> = p["groups"].as_array().unwrap().iter().map(|g| g["id"].as_str().unwrap()).collect();
        for s in p["settings"].as_array().unwrap() {
            let id = s["id"].as_str().unwrap();
            assert!(groups.contains(&s["group"].as_str().unwrap()), "{id}: its group is not in groups");
            for (k, v) in s["defaults"].as_object().unwrap() {
                assert!(same(&ours[k], &json(v)), "{id}: default of {k} is {} here, {} in the profile", ours[k], json(v));
            }
            for (k, v) in s.get("when").and_then(|w| w.as_object()).into_iter().flatten() {
                assert!(ours.get(k).is_some(), "{id}: when {k}");
                let _ = json(v);
            }
            let mut writes: Vec<(String, serde_json::Value)> = Vec::new();
            for c in s["choices"].as_array().into_iter().flatten() {
                for (k, v) in c["set"].as_object().unwrap() {
                    writes.push((k.clone(), json(v)));
                }
            }
            if let Some(sl) = s.get("slider") {
                let (min, max, step) = (sl["min"].as_f64().unwrap(), sl["max"].as_f64().unwrap(), sl["step"].as_f64().unwrap());
                let key = sl["key"].as_str().unwrap().to_string();
                let mut x = min;
                while x <= max + 1e-9 {
                    // as the slider writes it ("%d" or "%.2f")
                    let text = if sl["format"] == "%d" { format!("{}", x.round() as i64) } else { format!("{x:.2}") };
                    writes.push((key.clone(), serde_json::from_str(&text).unwrap()));
                    x += step;
                }
            }
            assert!(!writes.is_empty(), "{id}");
            for (k, v) in writes {
                assert!(ours.get(&k).is_some(), "{id}: {k} is not a launcher setting");
                let mut file = ours.clone();
                file[&k] = v.clone();
                let back = serde_json::to_value(serde_json::from_value::<Settings>(file).unwrap_or_else(|e| panic!("{id}: {k}={v}: {e}"))).unwrap();
                assert!(same(&back[&k], &v), "{id}: {k}={v} reads back as {}", back[&k]);
            }
        }
        // a launcher.json RecompLauncher made from nothing: only its keys,
        // the rest the defaults
        let s: Settings = serde_json::from_str(r#"{"windowMode":"borderless","aniso":16,"sharpen":0.35,"gameLang":"en"}"#).unwrap();
        assert_eq!((s.window_mode.as_str(), s.aniso, s.game_lang.as_str(), s.vsync), ("borderless", 16, "en", true));
        assert!((s.sharpen - 0.35).abs() < 1e-6);
        // and every language offered is one the game has files for
        for c in p["settings"].as_array().unwrap().iter().find(|s| s["id"] == "game_lang").unwrap()["choices"].as_array().unwrap() {
            let code = json(&c["set"]["gameLang"]);
            assert!(lang_name(code.as_str().unwrap()).is_some(), "{code}");
        }
    }

    #[test]
    fn profile_points_at_this_app() {
        let p = profile();
        let start = p["start"]["program"][0].as_str().unwrap();
        assert_eq!(start, p["prepare"]["program"][0].as_str().unwrap());
        // the .app is tauri.conf.json's productName, its program Cargo's name
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(start, format!("{}.app/Contents/MacOS/{}", conf["productName"].as_str().unwrap(), env!("CARGO_PKG_NAME")));
        assert_eq!(p["start"]["args"], serde_json::json!(["--play", "--game", "{game}"]));
        assert_eq!(p["prepare"]["args"], serde_json::json!(["--prepare", "--game", "{game}"]));
        assert!(matches!(mode(&args(&["--prepare", "--game", "/g"]), true), Mode::Prepare { .. }));
        assert!(matches!(mode(&args(&["--play", "--game", "/g"]), true), Mode::PlayDirect { .. }));
        assert_eq!(p["game"]["check"]["ids"], serde_json::json!([TITLE_ID]));
    }

    #[test]
    fn progress_lines() {
        assert_eq!(progress_line(0.25, "Translating  the\ngame's code"), "progress 0.250 Translating the game's code");
        assert_eq!(progress_line(1.7, "x"), "progress 1.000 x");
        assert_eq!(progress_line(-1.0, "x"), "progress 0.000 x");
        // RecompLauncher reads "progress <n> <text>": a child's line must not pass for one
        assert_eq!(log_line("progress 0.5 fake"), "  progress 0.5 fake");
        assert_eq!(log_line("PROGRESS ppu 8/13 translating (a few minutes)"), "  PROGRESS ppu 8/13 translating (a few minutes)");
        assert_eq!(log_line("LOG [12/900] Building C object"), "[12/900] Building C object");
        assert_eq!(log_line("LOG progress 0.9 x"), "  progress 0.9 x");
        assert_eq!(log_line("\x1b[1m==> cmake\x1b[0m"), "==> cmake");

        let mut r = Reporter::default();
        assert_eq!(r.update(0.1, "a").as_deref(), Some("progress 0.100 a"));
        assert_eq!(r.update(0.102, "a"), None, "under half a percent");
        assert_eq!(r.update(0.05, "b").as_deref(), Some("progress 0.102 b"), "never back");
        assert_eq!(r.update(0.2, "b").as_deref(), Some("progress 0.200 b"));
        assert_eq!(r.force(0.0, "failed"), "progress 0.200 failed");
        assert_eq!(r.update(1.0, "failed").as_deref(), Some("progress 1.000 failed"));
    }

    /// rebuild.py's output on a first rebuild, step by step (its protocol).
    const REBUILD: &[&str] = &[
        "CHECK ok host macOS 26.1 arm64",
        "CHECK ok game NPUA80638 \"inFAMOUS 2\" APP_VER 01.03 at /r/dev_hdd0/game/NPUA80638",
        "PLAN 1/12 toolkit run Toolkit (ps3recomp fork + patches)",
        "PLAN 2/12 venv run Python environment for the translators",
        "PLAN 3/12 game run Import the game copy",
        "PLAN 4/12 decrypt run Decrypt EBOOT and firmware",
        "PLAN 5/12 port run Port sources",
        "PLAN 6/12 firmware run Translate the firmware (liblv2, libsysmodule, libsre)",
        "PLAN 7/12 ppu run Translate the PPU code",
        "PLAN 8/12 spu run Translate the SPU programs",
        "PLAN 9/12 jobs run Translate the SPU job modules",
        "PLAN 10/12 dlc run DLC licence key (optional)",
        "PLAN 11/12 compile run Compile",
        "PLAN 12/12 finish run Write install.json",
        "PROGRESS toolkit 1/12 Toolkit (ps3recomp fork + patches)",
        "LOG $ tools/setup_toolkit.sh --managed",
        "STEP-DONE toolkit 1/12 40",
        "PROGRESS venv 2/12 Python environment for the translators",
        "STEP-DONE venv 2/12 30",
        "PROGRESS game 3/12 Import the game copy",
        "PROGRESS game 3/12 cloning /r/dev_hdd0/game/NPUA80638",
        "STEP-DONE game 3/12 2",
        "PROGRESS decrypt 4/12 Decrypt EBOOT and firmware",
        "PROGRESS decrypt 4/12 EBOOT.BIN",
        "STEP-DONE decrypt 4/12 10",
        "PROGRESS port 5/12 Port sources",
        "STEP-DONE port 5/12 0",
        "PROGRESS firmware 6/12 Translate the firmware (liblv2, libsysmodule, libsre)",
        "STEP-DONE firmware 6/12 20",
        "PROGRESS ppu 7/12 Translate the PPU code",
        "PROGRESS ppu 7/12 translating (a few minutes)",
        "STEP-DONE ppu 7/12 300",
        "PROGRESS spu 8/12 Translate the SPU programs",
        "STEP-DONE spu 8/12 10",
        "PROGRESS jobs 9/12 Translate the SPU job modules",
        "STEP-DONE jobs 9/12 30",
        "STEP-SKIP dlc 10/12 up to date",
        "PROGRESS compile 11/12 Compile",
        "SUBPROGRESS compile 300/1200",
        "LOG [300/1200] Building C object",
        "SUBPROGRESS compile 1200/1200",
        "STEP-DONE compile 11/12 90",
        "PROGRESS finish 12/12 Write install.json",
        "STEP-DONE finish 12/12 0",
        "DONE /i/install.json",
    ];

    #[test]
    fn rebuild_steps_move_the_bar_forward() {
        let mut steps = StepTracker::default();
        let mut rep = Reporter::default();
        let mut last = -1.0;
        let mut stages: Vec<String> = Vec::new();
        for (i, l) in REBUILD.iter().enumerate() {
            let t = i as f64 * 600.0;
            steps.feed(l, t);
            for dt in [0.0, 60.0, 599.0] {
                let f = overall(steps.fraction(t + dt));
                assert!((INSTALL_FROM..=INSTALL_TO).contains(&f), "{l}: {f}");
                rep.update(f, stage_text(steps.stage()));
                assert!(rep.last() >= last, "{l}: {} < {last}", rep.last());
                last = rep.last();
            }
            if let Some(s) = steps.stage() {
                if stages.last().map(String::as_str) != Some(s) {
                    stages.push(s.to_string());
                }
            }
        }
        assert_eq!(stages, ["toolkit", "venv", "game", "decrypt", "port", "firmware", "ppu", "spu", "jobs", "compile", "finish"]);
        assert!((steps.fraction(1e9) - 1.0).abs() < 1e-9, "every step done: {}", steps.fraction(1e9));
        assert!((overall(1.0) - INSTALL_TO).abs() < 1e-9 && overall(0.0) == INSTALL_FROM);

        // a rerun with nothing changed: every step up to date from the plan on
        let mut s2 = StepTracker::default();
        for l in ["PLAN 1/2 toolkit up-to-date Toolkit", "PLAN 2/2 compile up-to-date Compile"] {
            s2.feed(l, 0.0);
        }
        assert!((s2.fraction(0.0) - 1.0).abs() < 1e-9);
        // the compile's SUBPROGRESS moves it, time moves the lift
        let mut s3 = StepTracker::default();
        for l in ["PLAN 1/2 ppu run PPU", "PLAN 2/2 compile run Compile", "PROGRESS ppu 1/2 PPU"] {
            s3.feed(l, 0.0);
        }
        let early = s3.fraction(1.0);
        assert!(s3.fraction(600.0) > early && s3.fraction(1e9) <= 0.9 * 30.0 / 80.0 + 1e-9);
        s3.feed("STEP-DONE ppu 1/2 600", 600.0);
        s3.feed("PROGRESS compile 2/2 Compile", 600.0);
        s3.feed("SUBPROGRESS compile 600/1200", 601.0);
        assert!(s3.fraction(601.0) >= (30.0 + 25.0) / 80.0 - 1e-9);
        // a failure keeps the step it stopped at
        s3.feed("ERROR compile cmake exited with status 1", 700.0);
        assert_eq!(s3.stage(), Some("compile"));
        // lines that only look like steps change nothing
        let mut s4 = StepTracker::default();
        for l in ["PLAN", "PROGRESS", "LOG PROGRESS ppu 1/2 x", "progress 0.5 ppu", "SUBPROGRESS compile x/y"] {
            s4.feed(l, 0.0);
        }
        assert_eq!(s4.stage(), None);
        assert_eq!(s4.fraction(0.0), 0.0);
    }

    fn sfo(id: &str, ver: &str) -> Vec<u8> {
        saves::psf_write(&[
            saves::PsfEntry { key: "APP_VER".into(), fmt: saves::PSF_UTF8, data: format!("{ver}\0").into_bytes(), max_len: 8 },
            saves::PsfEntry { key: "TITLE".into(), fmt: saves::PSF_UTF8, data: b"inFAMOUS 2\0".to_vec(), max_len: 128 },
            saves::PsfEntry { key: "TITLE_ID".into(), fmt: saves::PSF_UTF8, data: format!("{id}\0").into_bytes(), max_len: 16 },
        ])
    }

    #[test]
    fn copies() {
        let d = tmp("copy");
        let rpcs3 = d.join("rpcs3");
        let game = rpcs3.join("dev_hdd0/game/NPUA80638");
        assert!(check_copy(&game).unwrap_err().contains("isn't there"));
        fs::create_dir_all(game.join("USRDIR")).unwrap();
        assert!(check_copy(&game).unwrap_err().contains("no PARAM.SFO"));
        fs::write(game.join("PARAM.SFO"), sfo("NPUA80638", "01.03")).unwrap();
        assert!(check_copy(&game).unwrap_err().contains("USRDIR/EBOOT.BIN"));
        fs::write(game.join("USRDIR/EBOOT.BIN"), b"SCE\0").unwrap();
        let good = GameCopy { dir: game.clone(), app_ver: "01.03".into() };
        assert_eq!(check_copy(&game).unwrap(), good);
        // RPCS3's data folder, its dev_hdd0 and its game folder lead to it too
        for p in [rpcs3.clone(), rpcs3.join("dev_hdd0"), rpcs3.join("dev_hdd0/game")] {
            assert_eq!(check_copy(&p).unwrap(), good, "{}", p.display());
        }
        // the base game without its update: refused, with how to fix it
        fs::write(game.join("PARAM.SFO"), sfo("NPUA80638", "01.00")).unwrap();
        let e = check_copy(&game).unwrap_err();
        assert!(e.contains("version 01.00") && e.contains("1.03 update"), "{e}");
        // another game, the disc release
        fs::write(game.join("PARAM.SFO"), sfo("NPUA80070", "01.03")).unwrap();
        assert!(check_copy(&game).unwrap_err().contains("NPUA80070"));
        let disc = d.join("BCUS98125");
        fs::create_dir_all(disc.join("PS3_GAME/USRDIR")).unwrap();
        fs::write(disc.join("PS3_GAME/PARAM.SFO"), sfo("BCUS98125", "01.00")).unwrap();
        assert!(check_copy(&disc).unwrap_err().contains("disc copy"));
        assert!(check_copy(&disc.join("PS3_GAME")).unwrap_err().contains("disc release"));
        // files: a PKG, a disc image
        let pkg = d.join("UP9000-NPUA80638_00-INFAMOUS2.pkg");
        fs::write(&pkg, b"\x7fPKG").unwrap();
        assert!(check_copy(&pkg).unwrap_err().contains("install it in RPCS3"));
        let iso = d.join("inFAMOUS 2.iso");
        fs::write(&iso, b"x").unwrap();
        assert!(check_copy(&iso).unwrap_err().contains("disc images don't work yet"));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn licences() {
        let d = tmp("rap");
        let game = d.join("dev_hdd0/game/NPUA80638");
        let exdata = d.join("dev_hdd0/home/00000001/exdata");
        fs::create_dir_all(game.join("USRDIR")).unwrap();
        fs::create_dir_all(&exdata).unwrap();
        fs::write(game.join("PARAM.SFO"), sfo("NPUA80638", "01.03")).unwrap();
        let cid = "UP9000-NPUA80638_00-TESTTESTTEST0002";
        let mut eboot = vec![0u8; 0x200];
        eboot[0x80..0x84].copy_from_slice(b"NPD\0");
        eboot[0x90..0x90 + cid.len()].copy_from_slice(cid.as_bytes());
        fs::write(game.join("USRDIR/EBOOT.BIN"), &eboot).unwrap();
        let other = exdata.join("EP0000-NPUA80638_00-ZZZZOTHER0000001.rap");
        fs::write(&other, [0u8; 16]).unwrap();
        let remembered = d.join("kept.rap");
        fs::write(&remembered, [2u8; 16]).unwrap();
        let none = "";
        // no exact one: the remembered licence, else any NPUA80638 one
        assert_eq!(pick_licence(&game, None, &remembered.to_string_lossy()).unwrap(), Some(remembered.clone()));
        let any = pick_licence(&game, None, none).unwrap().unwrap();
        assert!(any.to_string_lossy().contains("NPUA80638") && any.extension().is_some_and(|e| e == "rap"), "{}", any.display());
        // the copy's own content id wins over both
        let exact = exdata.join(format!("{cid}.rap"));
        fs::write(&exact, [1u8; 16]).unwrap();
        assert_eq!(pick_licence(&game, None, &remembered.to_string_lossy()).unwrap(), Some(exact.clone()));
        assert_eq!(pick_licence(&d, None, none).unwrap(), Some(exact.clone()), "from RPCS3's data folder too");
        // --rap wins, and must be a .rap
        assert_eq!(pick_licence(&game, Some(&remembered), none).unwrap(), Some(remembered.clone()));
        let bad = d.join("bad.rap");
        fs::write(&bad, [0u8; 3]).unwrap();
        assert!(pick_licence(&game, Some(&bad), none).is_err());
        // a remembered path that is gone is passed over
        assert_eq!(pick_licence(&game, None, "/nonexistent/x.rap").unwrap(), Some(exact));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn running_checks_match_only_this_game() {
        let install = "/Users/p/Library/Application Support/infamous2-recomp/install/infamous2-lle/build/Infamous2Recomp.app/Contents/MacOS/Infamous2Recomp";
        assert!(is_game_exe(install));
        assert!(is_game_exe("/r/recomp-infamous-2/infamous2-lle/build/Infamous2Recomp"));
        for other in [
            "/Users/p/Desktop/infamous1-recomp/infamous-lle/build/Infamous1Recomp.app/Contents/MacOS/Infamous1Recomp",
            "/Users/p/Desktop/resistance2-recomp/build/BCUS98120/ResistanceRecomp",
            "/Applications/infamous2-recomp.app/Contents/MacOS/infamous2-launcher",
            "/bin/zsh",
            "Infamous2Recomp",
        ] {
            assert!(!is_game_exe(other), "{other}");
        }
        assert!(is_rebuild_line("/opt/homebrew/bin/python3.12 -u /A/infamous2-recomp.app/Contents/Resources/port/infamous2-lle/tools/rebuild.py --game /g --nice 10"));
        assert!(is_rebuild_line("/Library/Developer/CommandLineTools/Library/Frameworks/Python3.framework/Versions/3.9/Resources/Python.app/Contents/MacOS/Python -u /Users/p/Library/Application Support/x/infamous2-lle/tools/rebuild.py --game /g"));
        assert!(is_rebuild_line("python3 ./infamous2-lle/tools/rebuild.py --out /o"));
        assert!(is_rebuild_line("python3 infamous2-lle/tools/rebuild.py --out /o"));
        assert!(!is_rebuild_line("python3 /x/notinfamous2-lle/tools/rebuild.py --out /o"));
        // its quick checks, other ports, commands that only mention it
        assert!(!is_rebuild_line("/opt/homebrew/bin/python3.12 /a/infamous2-lle/tools/rebuild.py --check-json"));
        assert!(!is_rebuild_line("python3 /x/infamous1-lle/tools/rebuild.py --game /g"));
        assert!(!is_rebuild_line("/bin/zsh -c cat /a/infamous2-lle/tools/rebuild.py --game && echo"));
        assert!(!is_rebuild_line("vim /a/infamous2-lle/tools/rebuild.py"));
        assert!(!is_rebuild_line("python3 -c open('/a/infamous2-lle/tools/rebuild.py')"));
        // the live check runs (ps is there) and this test is neither
        let _ = (game_running(), rebuild_running());
        assert!(!process_lines("comm").is_empty());
    }
}
