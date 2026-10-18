import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ask, open } from "@tauri-apps/plugin-dialog";
import { GAME_LANGS, Key, Lang, lang, setLang, t, translateDom } from "./i18n";
import { initControls, renderControls, showControls } from "./controls";
import { RebuildProgress } from "./progress";

// ---- types shared with src-tauri/src/lib.rs (camelCase) ----------------------
interface Settings {
  uiLang: string;
  installDir: string;
  gameLang: string;
  gpuLight: boolean;
  gpuSsao: boolean;
  skipOcclusion: boolean;
  gammaOverride: boolean;
  gamma: number;
  mouse: boolean;
  mouseSens: number;
  mouseInvertY: boolean;
  mouseInvertX: boolean;
  mouseDeadzone: number;
  mouseCapture: boolean;
  windowMode: string;
  windowSize: string;
  gameIcon: boolean;
  display: string;
  vsync: boolean;
  audioVolume: number;
  audioDevice: string;
  rumble: boolean;
  rumbleStrength: number;
  extraEnv: string;
  gamePaths: string[];
  rapPath: string;
}
interface InstallInfo {
  layout: "install" | "dev";
  title: string;
  appVer: string;
  runSh: string;
  executable: string;
  builtAt: string;
  portCommit: string;
}
interface SaveInfo {
  title: string;
  subtitle: string;
  detail: string;
  modified: number;
  folder: string;
}
interface Status {
  installDir: string;
  install: InstallInfo | null;
  installError: string | null;
  icon: string | null;
  background: string | null;
  lastSave: SaveInfo | null;
  gameRunning: boolean;
  rebuildRunning: boolean;
  rebuildScript: string | null;
  dataRoot: string;
  defaultInstallDir: string;
  hostSupported: boolean;
  os: string;
}
interface CheckItem {
  level: "ok" | "warn" | "fail";
  name: string;
  message: string;
}
interface Prereqs {
  os: string;
  arch: string;
  supported: boolean;
  clt: boolean;
  homebrew: string | null;
  script: string | null;
  checks: CheckItem[];
  brewMissing: string[];
  rpcs3Missing: boolean;
  firmwareMissing: boolean;
  ok: boolean;
  error: string | null;
}
interface LicenceSearch {
  gameDir: string | null;
  title: string | null;
  appVer: string | null;
  contentId: string | null;
  raps: string[];
  searched: string[];
}

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const show = (el: HTMLElement, on: boolean) => el.classList.toggle("hidden", !on);

let settings: Settings;
let status: Status | null = null;
let gameRunning = false;
let rebuildRunning = false;
let lastDry = false;
const progress = new RebuildProgress();

// ---- navigation ----------------------------------------------------------------
function showView(name: string) {
  document.querySelectorAll<HTMLElement>(".view").forEach((v) => v.classList.toggle("active", v.id === `view-${name}`));
  document.querySelectorAll<HTMLElement>(".nav").forEach((b) => b.classList.toggle("active", b.dataset.view === name));
  if (name === "home") refreshStatus();
  if (name === "settings") {
    updatePreview();
    refreshOutputs();
  }
  if (name === "controls") showControls();
}
document.querySelectorAll<HTMLElement>(".nav").forEach((b) => b.addEventListener("click", () => showView(b.dataset.view!)));

// ---- settings persistence --------------------------------------------------------
let saveTimer = 0;
function persist() {
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(async () => {
    await invoke("save_settings", { settings });
    for (const s of [$("saved"), $("controls-saved")]) {
      show(s, true);
      window.setTimeout(() => show(s, false), 1200);
    }
    updatePreview();
  }, 250);
}

async function updatePreview() {
  try {
    $("preview").textContent = await invoke<string>("launch_preview", { settings });
  } catch (e) {
    $("preview").textContent = String(e);
  }
}

function applyLang(l: Lang) {
  setLang(l);
  translateDom();
  const sel = $<HTMLSelectElement>("game-lang");
  sel.innerHTML = "";
  for (const g of GAME_LANGS) {
    const o = document.createElement("option");
    o.value = g.code;
    o.textContent = `${lang() === "en" ? g.en : g.pt} (${g.code})`;
    sel.appendChild(o);
  }
  sel.value = settings.gameLang;
  renderHome();
  renderProgress();
  renderPrereqs();
  renderGame(lastSearch);
  renderControls();
  renderOutputs();
}

// ---- screens and audio outputs (PS3_DISPLAY, PS3_AUDIO_DEVICE) ------------------
interface Outputs {
  screens: string[];
  audioOutputs: string[];
}
let outputs: Outputs = { screens: [], audioOutputs: [] };

async function refreshOutputs() {
  try {
    outputs = await invoke<Outputs>("list_outputs");
  } catch {
    outputs = { screens: [], audioOutputs: [] };
  }
  renderOutputs();
}

/** Fill a select; a saved value the system no longer lists stays selectable. */
function fillSelect(id: string, opts: { value: string; label: string }[], current: string, missing: Key) {
  if (current && !opts.some((o) => o.value === current)) opts.push({ value: current, label: t(missing, { name: current }) });
  const sel = $<HTMLSelectElement>(id);
  sel.innerHTML = "";
  for (const o of opts) {
    const el = document.createElement("option");
    el.value = o.value;
    el.textContent = o.label;
    sel.appendChild(el);
  }
  sel.value = current;
}

function renderOutputs() {
  // A screen goes by its name, or by its number when two share one (the
  // game's PS3_DISPLAY counts from 1 in the system's order).
  const seen = new Map<string, number>();
  for (const n of outputs.screens) seen.set(n, (seen.get(n) ?? 0) + 1);
  const screens = [{ value: "", label: t("settings.screenMain") }];
  outputs.screens.forEach((n, i) => {
    const dup = (seen.get(n) ?? 0) > 1;
    screens.push({ value: dup ? String(i + 1) : n, label: dup ? `${n} (${i + 1})` : n });
  });
  fillSelect("screen", screens, settings.display, "settings.screenMissing");
  const devices = [{ value: "", label: t("settings.audioDefault") }, ...outputs.audioOutputs.map((n) => ({ value: n, label: n }))];
  fillSelect("audio-device", devices, settings.audioDevice, "settings.audioMissing");
}

function bindSettings() {
  const bindCheck = (id: string, key: keyof Settings) => {
    const el = $<HTMLInputElement>(id);
    el.checked = settings[key] as boolean;
    el.addEventListener("change", () => {
      (settings[key] as boolean) = el.checked;
      syncDependent();
      persist();
    });
  };
  const bindRange = (id: string, key: keyof Settings, fmt: (v: number) => string) => {
    const el = $<HTMLInputElement>(id);
    const out = $(`${id}-out`);
    el.value = String(settings[key]);
    out.textContent = fmt(Number(el.value));
    el.addEventListener("input", () => {
      (settings[key] as number) = Number(el.value);
      out.textContent = fmt(Number(el.value));
      persist();
    });
  };
  bindCheck("gpu-light", "gpuLight");
  bindCheck("gpu-ssao", "gpuSsao");
  bindCheck("skip-occ", "skipOcclusion");
  bindCheck("gamma-on", "gammaOverride");
  bindCheck("mouse-on", "mouse");
  bindCheck("mouse-invert", "mouseInvertY");
  bindCheck("mouse-invert-x", "mouseInvertX");
  bindCheck("mouse-capture", "mouseCapture");
  bindCheck("game-icon", "gameIcon");
  bindCheck("vsync", "vsync");
  bindCheck("rumble-on", "rumble");
  for (const [id, key] of [["screen", "display"], ["audio-device", "audioDevice"]] as const) {
    const el = $<HTMLSelectElement>(id);
    el.addEventListener("change", () => {
      settings[key] = el.value;
      persist();
    });
  }
  const bindSelect = (id: string, key: "windowMode" | "windowSize") => {
    const el = $<HTMLSelectElement>(id);
    el.value = settings[key];
    if (el.value !== settings[key]) el.selectedIndex = 0; // a value the list does not offer
    el.addEventListener("change", () => {
      settings[key] = el.value;
      persist();
    });
  };
  bindSelect("display", "windowMode");
  bindSelect("window-size", "windowSize");
  bindRange("gamma", "gamma", (v) => v.toFixed(2));
  bindRange("mouse-sens", "mouseSens", (v) => `${v.toFixed(2)}×`);
  bindRange("mouse-dz", "mouseDeadzone", (v) => String(Math.round(v)));
  bindRange("audio-volume", "audioVolume", (v) => `${Math.round(v)}%`);
  bindRange("rumble-strength", "rumbleStrength", (v) => `${Math.round(v)}%`);
  $("rumble-test").addEventListener("click", testRumble);

  const gl = $<HTMLSelectElement>("game-lang");
  gl.addEventListener("change", () => {
    settings.gameLang = gl.value;
    persist();
  });
  const env = $<HTMLTextAreaElement>("extra-env");
  env.value = settings.extraEnv;
  env.addEventListener("input", () => {
    settings.extraEnv = env.value;
    persist();
  });
  const dir = $<HTMLInputElement>("install-dir");
  dir.value = settings.installDir;
  dir.addEventListener("change", () => {
    settings.installDir = dir.value.trim();
    $<HTMLInputElement>("out-dir").value = settings.installDir;
    persist();
  });
  $("pick-install-dir").addEventListener("click", async () => {
    const p = await open({ directory: true, defaultPath: settings.installDir });
    if (typeof p === "string") {
      dir.value = p;
      dir.dispatchEvent(new Event("change"));
    }
  });
  const ui = $<HTMLSelectElement>("ui-lang");
  ui.value = settings.uiLang;
  ui.addEventListener("change", () => {
    settings.uiLang = ui.value;
    applyLang(ui.value as Lang);
    persist();
  });
  $("reset").addEventListener("click", async () => {
    const keep = { uiLang: settings.uiLang, installDir: settings.installDir, gamePaths: settings.gamePaths, rapPath: settings.rapPath };
    Object.assign(settings, {
      gameLang: "pt",
      gpuLight: true,
      gpuSsao: true,
      skipOcclusion: true,
      gammaOverride: false,
      gamma: 1.0,
      mouse: true,
      mouseSens: 1.0,
      mouseInvertY: false,
      mouseInvertX: false,
      mouseDeadzone: 30,
      mouseCapture: true,
      windowMode: "windowed",
      windowSize: "",
      gameIcon: false,
      display: "",
      vsync: true,
      audioVolume: 100,
      audioDevice: "",
      rumble: true,
      rumbleStrength: 100,
      extraEnv: "",
    }, keep);
    await invoke("save_settings", { settings });
    location.reload();
  });
  syncDependent();
}

/** The GPU SSAO needs the GPU light pass; gamma/mouse sliders follow their toggles. */
function syncDependent() {
  const ssao = $<HTMLInputElement>("gpu-ssao");
  ssao.disabled = !settings.gpuLight;
  $<HTMLInputElement>("gamma").disabled = !settings.gammaOverride;
  for (const id of ["mouse-sens", "mouse-invert", "mouse-invert-x", "mouse-dz", "mouse-capture"]) $<HTMLInputElement>(id).disabled = !settings.mouse;
  $<HTMLInputElement>("rumble-strength").disabled = !settings.rumble;
  $<HTMLButtonElement>("rumble-test").disabled = !settings.rumble;
}

/** A half-second rumble at the chosen strength through the Gamepad API, so
 * the player feels what the game will do (the game itself goes through SDL;
 * above 100% only the weak values change, so the test tops out at full). */
async function testRumble() {
  const msg = $("rumble-test-msg");
  msg.textContent = "";
  const pads = (navigator.getGamepads ? Array.from(navigator.getGamepads()) : []).filter((p): p is Gamepad => !!p);
  if (!pads.length) {
    msg.textContent = t("settings.rumbleTestNoPad");
    return;
  }
  type Actuator = { playEffect?: (type: string, params: Record<string, number>) => Promise<unknown> };
  const act = (pads[0] as unknown as { vibrationActuator?: Actuator }).vibrationActuator;
  if (!act?.playEffect) {
    msg.textContent = t("settings.rumbleTestNoSupport");
    return;
  }
  const k = Math.min(1, settings.rumbleStrength / 100);
  try {
    await act.playEffect("dual-rumble", { duration: 500, startDelay: 0, strongMagnitude: k, weakMagnitude: k });
  } catch {
    msg.textContent = t("settings.rumbleTestNoSupport");
  }
}

// ---- home ---------------------------------------------------------------------
async function refreshStatus() {
  status = await invoke<Status>("get_status");
  gameRunning = status.gameRunning;
  rebuildRunning = status.rebuildRunning || rebuildRunning;
  renderHome();
}

function renderHome() {
  if (!status) return;
  const inst = status.install;
  show($("installed-actions"), !!inst);
  show($("not-installed"), !inst);
  const cover = $<HTMLImageElement>("cover");
  if (status.icon) cover.src = status.icon;
  show(cover, !!status.icon);
  $("hero").style.backgroundImage = status.background ? `url(${status.background})` : "";
  $("game-title").textContent = inst?.title || "inFAMOUS 2";
  const meta: string[] = [];
  if (inst) {
    if (inst.appVer) meta.push(`${t("home.version")} ${inst.appVer}`);
    if (inst.builtAt) meta.push(`${t("home.builtAt")} ${new Date(inst.builtAt).toLocaleString(lang())}`);
    if (inst.layout === "dev") meta.push(t("home.devTree"));
    if (inst.portCommit) meta.push(`port ${inst.portCommit}`);
  }
  $("game-meta").textContent = meta.join(" · ");
  const play = $<HTMLButtonElement>("play");
  play.textContent = t(gameRunning ? "home.stop" : "home.play");
  play.classList.toggle("danger", gameRunning);
  play.disabled = rebuildRunning || !status.hostSupported;
  show($("run-state"), gameRunning);
  show($("home-unsupported"), !status.hostSupported);
  show($("go-install"), status.hostSupported);

  const s = status.lastSave;
  const box = $("save-info");
  box.innerHTML = "";
  if (!s) {
    box.textContent = t("home.noSave");
  } else {
    const b = document.createElement("strong");
    b.textContent = s.subtitle || s.title;
    const d = document.createElement("div");
    d.textContent = s.detail.split("\n").join(" · ");
    const w = document.createElement("div");
    w.className = "small";
    w.textContent = s.modified ? new Date(s.modified * 1000).toLocaleString(lang()) : "";
    box.append(b, d, w);
  }
}

$("play").addEventListener("click", async () => {
  const msg = $("home-msg");
  try {
    if (gameRunning) {
      await invoke("stop_game");
      return;
    }
    const log = await invoke<string>("play");
    gameRunning = true;
    msg.textContent = t("home.started", { log });
  } catch (e) {
    msg.textContent = String(e);
  }
  renderHome();
});
$("go-install").addEventListener("click", () => showView("install"));
$("open-saves").addEventListener("click", async () => {
  const p = await invoke<Record<string, string>>("log_paths");
  invoke("reveal", { path: status?.lastSave?.folder ?? p.saves });
});
$("open-log").addEventListener("click", async () => {
  const p = await invoke<Record<string, string>>("log_paths");
  invoke("reveal", { path: p.game });
});

// ---- install: 1. prerequisites ---------------------------------------------------
const URL_RPCS3 = "https://rpcs3.net/download";
const URL_FIRMWARE = "https://rpcs3.net/quickstart";
let prereqs: Prereqs | null = null;
let setupRunning = false;
let cltPoll = 0;

/** Human name of a rebuild.py check. */
function checkLabel(name: string): string {
  const k = `prereq.name.${name}` as Key;
  const v = t(k);
  return v === k ? name : v;
}

async function refreshPrereqs() {
  $("prereq-msg").textContent = t("prereq.checking");
  prereqs = await invoke<Prereqs>("check_prereqs");
  $("prereq-msg").textContent = prereqs.error && prereqs.supported ? t("prereq.error", { e: prereqs.error }) : "";
  renderPrereqs();
  // the Command Line Tools install in Apple's own window; stop polling once they are in
  if (cltPoll && prereqs.clt) {
    window.clearInterval(cltPoll);
    cltPoll = 0;
  }
}

function prereqAction(c: CheckItem): HTMLButtonElement | null {
  if (c.level !== "fail" || !prereqs) return null;
  const b = document.createElement("button");
  if (c.name === "xcode-clt") {
    b.textContent = t("prereq.installClt");
    b.addEventListener("click", installClt);
  } else if (c.name === "rpcs3") {
    b.textContent = t("prereq.getRpcs3");
    b.addEventListener("click", () => invoke("open_url", { url: URL_RPCS3 }));
  } else if (c.name === "firmware") {
    b.textContent = t("prereq.howFirmware");
    b.addEventListener("click", () => invoke("open_url", { url: URL_FIRMWARE }));
  } else {
    return null;
  }
  return b;
}

function renderPrereqs() {
  const p = prereqs;
  const card = $("card-prereqs");
  const list = $("prereqs");
  list.innerHTML = "";
  const supported = status?.hostSupported ?? p?.supported ?? true;
  show($("install-unsupported"), !supported);
  for (const id of ["card-prereqs", "card-game", "card-run"]) $(id).classList.toggle("disabled", !supported);
  if (!p || !p.supported) {
    $("prereq-summary").textContent = "";
    show($("prereq-fix"), false);
    show($("prereq-brew-missing"), false);
    return;
  }
  const fails = p.checks.filter((c) => c.level === "fail");
  const hints: Record<string, Key> = {
    rpcs3: "prereq.hint.rpcs3",
    firmware: "prereq.hint.firmware",
    "xcode-clt": "prereq.hint.clt",
    disk: "prereq.hint.disk",
  };
  // the failed ones first, then the rest
  for (const c of [...fails, ...p.checks.filter((c) => c.level !== "fail")]) {
    const li = document.createElement("li");
    li.className = `check-${c.level}`;
    li.innerHTML = `<span class="ico"></span><span class="what"><b></b><span class="tip"></span><span class="msg"></span></span>`;
    li.querySelector(".ico")!.textContent = ICON[c.level] ?? "·";
    li.querySelector("b")!.textContent = checkLabel(c.name);
    li.querySelector(".tip")!.textContent = c.level === "fail" && hints[c.name] ? t(hints[c.name]) : "";
    li.querySelector(".msg")!.textContent = c.message;
    const act = prereqAction(c);
    if (act) li.appendChild(act);
    list.appendChild(li);
  }
  const sum = $("prereq-summary");
  sum.className = "pill " + (p.ok ? "" : "bad");
  sum.textContent = p.ok ? t("prereq.allOk") : t("prereq.missingN", { n: fails.length || 1 });
  card.classList.toggle("done", p.ok);
  const brewable = p.brewMissing.length > 0;
  show($("prereq-fix"), !p.clt || (brewable && !!p.homebrew));
  $<HTMLButtonElement>("prereq-fix").disabled = setupRunning;
  $("prereq-fix").textContent = p.homebrew
    ? t("prereq.fixBrew", { pkgs: p.brewMissing.join(" ") })
    : t("prereq.installClt");
  show($("prereq-brew-missing"), brewable && !p.homebrew);
  show($("setup-cancel"), setupRunning);
}

async function installClt() {
  const yes = await ask(t("prereq.askClt"), { title: t("prereq.installClt"), kind: "info" });
  if (!yes) return;
  try {
    await invoke("install_clt");
    $("prereq-msg").textContent = t("prereq.cltStarted");
    if (!cltPoll) cltPoll = window.setInterval(refreshPrereqs, 15000);
  } catch (e) {
    $("prereq-msg").textContent = String(e);
  }
}

$("prereq-fix").addEventListener("click", async () => {
  if (!prereqs) return;
  if (!prereqs.clt) return installClt();
  if (!prereqs.homebrew || !prereqs.brewMissing.length) return;
  const pkgs = prereqs.brewMissing;
  const yes = await ask(t("prereq.askBrew", { pkgs: pkgs.join(" ") }), { title: t("prereq.fix"), kind: "info" });
  if (!yes) return;
  const pre = $("setup-log");
  pre.textContent = "";
  show(pre, true);
  try {
    await invoke("install_brew", { packages: pkgs });
    setupRunning = true;
    $("prereq-msg").textContent = t("prereq.brewRunning");
  } catch (e) {
    $("prereq-msg").textContent = String(e);
  }
  renderPrereqs();
});
$("prereq-recheck").addEventListener("click", refreshPrereqs);
$("setup-cancel").addEventListener("click", () => invoke("cancel_setup"));
$("copy-brew").addEventListener("click", () =>
  navigator.clipboard.writeText(document.querySelector("#prereq-brew-missing code")!.textContent ?? ""));
document.querySelectorAll<HTMLElement>("[data-url]").forEach((b) =>
  b.addEventListener("click", () => invoke("open_url", { url: b.dataset.url })));

listen<string>("setup-line", (ev) => {
  const pre = $("setup-log");
  pre.textContent += ev.payload + "\n";
  pre.scrollTop = pre.scrollHeight;
});
listen<number>("setup-exit", (ev) => {
  setupRunning = false;
  $("prereq-msg").textContent = ev.payload === 0 ? t("prereq.brewDone") : t("prereq.brewFailed", { code: ev.payload });
  refreshPrereqs();
});

// ---- install: 2. the game copy and the licence ------------------------------------
const gamePaths = $<HTMLTextAreaElement>("game-paths");
const rapPath = $<HTMLInputElement>("rap-path");
const outDir = $<HTMLInputElement>("out-dir");

function readInstallForm() {
  settings.gamePaths = gamePaths.value.split("\n").map((s) => s.trim()).filter(Boolean);
  settings.rapPath = rapPath.value.trim();
  const out = outDir.value.trim();
  if (out) settings.installDir = out;
  $<HTMLInputElement>("install-dir").value = settings.installDir;
  persist();
}
[gamePaths, rapPath, outDir].forEach((el) => el.addEventListener("change", () => {
  readInstallForm();
  if (el === gamePaths) lookForLicence(false);
  else renderGame(lastSearch);
}));

let lastSearch: LicenceSearch | null = null;

function renderGame(r: LicenceSearch | null) {
  const box = $("game-summary");
  const have = settings.gamePaths.length > 0;
  show(box, have);
  if (!have) return;
  const isPkg = settings.gamePaths.every((p) => p.toLowerCase().endsWith(".pkg"));
  const gameOk = isPkg || !!r?.gameDir;
  $("game-ico").textContent = gameOk ? "✓" : "✕";
  $("game-ico").className = `ico ${gameOk ? "ok-ico" : "bad-ico"}`;
  $("game-name").textContent = gameOk
    ? isPkg
      ? t("game.pkgs", { n: settings.gamePaths.length })
      : `${r?.title || "inFAMOUS 2"}${r?.appVer ? ` · ${t("home.version")} ${r.appVer}` : ""}`
    : t("game.notGame");
  $("game-where").textContent = r?.gameDir ?? settings.gamePaths.join(", ");
  const rapOk = !!settings.rapPath;
  $("rap-ico").textContent = rapOk ? "✓" : "✕";
  $("rap-ico").className = `ico ${rapOk ? "ok-ico" : "bad-ico"}`;
  $("rap-state").textContent = rapOk ? t("game.rapFound", { rap: settings.rapPath }) : t("game.rapMissing", { cid: r?.contentId ?? "NPUA80638" });
  if (!rapOk) ($("more-options") as HTMLDetailsElement).open = true;
}

/** Look for the .rap that goes with the picked copy; fill it in when found.
 * `replace`: a new copy was picked, so a licence chosen for the old one goes. */
async function lookForLicence(replace: boolean) {
  if (!settings.gamePaths.length) return renderGame(null);
  const r = await invoke<LicenceSearch>("find_licence", { paths: settings.gamePaths });
  lastSearch = r;
  if (r.raps.length && (replace || !settings.rapPath)) {
    rapPath.value = r.raps[0];
  } else if (replace && !r.raps.length) {
    rapPath.value = "";
  }
  readInstallForm();
  renderGame(r);
}

$("pick-game-dir").addEventListener("click", async () => {
  const p = await open({ directory: true, title: t("game.pick") });
  if (typeof p === "string") {
    gamePaths.value = p;
    readInstallForm();
    await lookForLicence(true);
  }
});
$("pick-game-pkg").addEventListener("click", async () => {
  const p = await open({ multiple: true, filters: [{ name: "PKG", extensions: ["pkg", "PKG"] }] });
  if (Array.isArray(p) && p.length) {
    gamePaths.value = p.join("\n");
    readInstallForm();
    await lookForLicence(true);
  }
});
$("pick-rap").addEventListener("click", async () => {
  const p = await open({ filters: [{ name: "RAP", extensions: ["rap", "RAP"] }] });
  if (typeof p === "string") {
    rapPath.value = p;
    readInstallForm();
    renderGame(lastSearch);
  }
});
$("pick-out").addEventListener("click", async () => {
  const p = await open({ directory: true, defaultPath: outDir.value || undefined });
  if (typeof p === "string") {
    outDir.value = p;
    readInstallForm();
  }
});

/** RPCS3's own install, when the player installed the game there. */
async function detectRpcs3(quiet: boolean) {
  const d = await invoke<LicenceSearch>("detect_rpcs3");
  if (d.gameDir) {
    gamePaths.value = d.gameDir;
    if (d.raps.length) rapPath.value = d.raps[0];
    readInstallForm();
    lastSearch = d;
    renderGame(d);
  }
  if (!quiet || d.gameDir) $("install-msg-top").textContent = t(d.gameDir ? "install.detected" : "install.notDetected");
}
$("detect").addEventListener("click", () => detectRpcs3(false));

// ---- install: 3. the rebuild ------------------------------------------------------
async function startRebuild(dryRun: boolean) {
  readInstallForm();
  if (status && !status.hostSupported) {
    $("install-msg").textContent = t("unsupported.title");
    return;
  }
  if (!status?.rebuildScript) {
    $("install-msg").textContent = t("install.noScript");
    return;
  }
  if (!settings.gamePaths.length || !settings.rapPath) {
    $("install-msg").textContent = t("install.needInputs");
    return;
  }
  if (!dryRun && prereqs && !prereqs.ok) {
    const go = await ask(t("install.askMissing"), { title: t("install.start"), kind: "warning" });
    if (!go) return;
  }
  progress.reset();
  lastDry = dryRun;
  $("log").textContent = "";
  $("install-msg").textContent = "";
  try {
    await invoke("start_rebuild", {
      req: {
        games: settings.gamePaths,
        rap: settings.rapPath,
        out: settings.installDir,
        dryRun,
        force: $<HTMLInputElement>("force").checked,
        rapKey: $<HTMLInputElement>("rap-key").value || null,
      },
    });
    rebuildRunning = true;
  } catch (e) {
    $("install-msg").textContent = String(e);
  }
  renderProgress();
}
$("dry-run").addEventListener("click", () => startRebuild(true));
$("start").addEventListener("click", () => startRebuild(false));
$("cancel").addEventListener("click", () => {
  progress.cancelled = true;
  invoke("cancel_rebuild");
});
$("copy-log").addEventListener("click", () => navigator.clipboard.writeText(progress.log.join("\n")));

const ICON: Record<string, string> = { ok: "✓", warn: "!", fail: "✕" };

function renderProgress() {
  show($("cancel"), rebuildRunning);
  $<HTMLButtonElement>("start").disabled = rebuildRunning;
  $<HTMLButtonElement>("dry-run").disabled = rebuildRunning;

  const checks = $("checks");
  checks.innerHTML = "";
  for (const c of progress.checks) {
    const li = document.createElement("li");
    li.className = `check-${c.level}`;
    li.innerHTML = `<span class="ico"></span><b></b> <span class="msg"></span>`;
    li.querySelector(".ico")!.textContent = ICON[c.level] ?? "·";
    li.querySelector("b")!.textContent = c.name;
    li.querySelector(".msg")!.textContent = c.message;
    checks.appendChild(li);
  }
  const steps = $("steps");
  steps.innerHTML = "";
  for (const s of progress.steps) {
    const li = document.createElement("li");
    li.className = `step-${s.state}`;
    const name = t(`step.${s.id}` as Key) || s.title;
    li.innerHTML = `<span class="name"></span><span class="state"></span>`;
    li.querySelector(".name")!.textContent = name;
    li.querySelector(".state")!.textContent =
      (s.state === "running" && s.message ? `${s.message} · ` : "") + t(`state.${s.state}` as Key);
    steps.appendChild(li);
  }
  const frac = progress.fraction();
  $("bar").style.width = `${(frac * 100).toFixed(1)}%`;
  const eta = progress.etaMinutes();
  $("progress-text").textContent = rebuildRunning && !lastDry
    ? `${Math.floor(frac * 100)}%` + (eta !== null ? ` · ${t("install.eta", { min: Math.max(1, Math.round(eta)) })}` : "")
    : "";
}

let pendingLog: string[] = [];
let frame = 0;
function flushLog() {
  frame = 0;
  const pre = $("log");
  const atBottom = pre.scrollTop + pre.clientHeight >= pre.scrollHeight - 30;
  pre.textContent = progress.log.slice(-2000).join("\n");
  if (atBottom) pre.scrollTop = pre.scrollHeight;
  pendingLog = [];
  renderProgress();
}

listen<string>("rebuild-line", (ev) => {
  progress.feed(ev.payload);
  pendingLog.push(ev.payload);
  if (!frame) frame = requestAnimationFrame(flushLog);
});
listen<number>("rebuild-exit", (ev) => {
  rebuildRunning = false;
  const code = ev.payload;
  const msg = $("install-msg");
  if (progress.cancelled) msg.textContent = t("install.cancelled");
  else if (code === 0) msg.textContent = t(lastDry ? "install.dryOk" : "install.done");
  else if (code === 2) msg.textContent = t("install.missing");
  else msg.textContent = t("install.failed", { code: progress.error ?? code });
  msg.className = code === 0 ? "ok" : "bad";
  if (code === 0 && !lastDry) progress.finish();
  flushLog();
  refreshStatus();
});
listen<number>("game-exit", (ev) => {
  gameRunning = false;
  $("home-msg").textContent = t("home.exited", { code: ev.payload });
  renderHome();
});

// ---- start ---------------------------------------------------------------------
async function main() {
  settings = await invoke<Settings>("get_settings");
  setLang(settings.uiLang as Lang);
  bindSettings();
  initControls();
  refreshOutputs();
  gamePaths.value = settings.gamePaths.join("\n");
  rapPath.value = settings.rapPath;
  outDir.value = settings.installDir;
  applyLang(settings.uiLang as Lang);
  await refreshStatus();
  renderPrereqs();
  if (!status?.install) showView("install");
  if (status?.hostSupported) {
    // "only pick the game folder": RPCS3's copy and licence are found on their own
    if (!settings.gamePaths.length) await detectRpcs3(true);
    else await lookForLicence(false);
    await refreshPrereqs();
  }
}
main();
