import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ask, open } from "@tauri-apps/plugin-dialog";
import { GAME_LANGS, Key, Lang, lang, setLang, t, translateDom } from "./i18n";
import { initControls, renderControls, showControls } from "./controls";
import { RebuildProgress } from "./progress";
import { initSaves, refresh as refreshSaves, render as renderSaves } from "./saves";
import { initUpdates, refreshUpdates, renderUpdates } from "./update";
import { initLog, renderLog, showLog } from "./gamelog";
import { flashSaved, human } from "./ui";
import { initTrophies, refreshTrophies, renderTrophies } from "./trophies";

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
  aniso: number;
  aa: string;
  sharpen: number;
  patches: string[];
  vibrance: number;
  contrast: number;
  upscale: string;
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
  showFps: boolean;
  extraEnv: string;
  gamePaths: string[];
  rapPath: string;
  keysPath: string;
  firmwarePath: string;
  decryptWith: string;
  saveBackupsKeep: number;
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
  /** The .app the launcher runs from (macOS), and whether it has the custom Finder icon. */
  appBundle: string | null;
  finderIcon: boolean;
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
/** A debug session's folder (src-tauri/src/report.rs). */
interface ReportInfo {
  dir: string;
  id: string;
  live: boolean;
  debug: boolean;
  captures: number;
  pending: boolean;
  surfaces: number;
  shaders: number;
  bytes: number;
  logBytes: number;
  duration: number | null;
  exit: string | null;
  stopped: boolean;
  crashReport: boolean;
  zip: string | null;
  zipBytes: number;
  triage: Triage | null;
}
/** What the launcher shows of log_triage.py's triage.json. */
interface Triage {
  errors: number;
  warnings: number;
  expected: number;
  captures: number;
  stale: boolean;
  md: string;
  top: { category: string; severity: string; title: string; count: number; first: number | null; last: number | null }[];
  categories: [string, number][];
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
// Pages: home, graphics, performance, game, controls, advanced; Advanced has
// tabs (install, updates, log, diagnostics = debug mode, env). Older names still work.
const LEGACY: Record<string, [string, string?]> = {
  install: ["advanced", "install"],
  settings: ["graphics"],
  saves: ["game"],
};
let advancedTab = "install";

function showView(view: string, tab?: string) {
  const [name, legacyTab] = LEGACY[view] ?? [view];
  tab ??= legacyTab;
  document.querySelectorAll<HTMLElement>(".view").forEach((v) => v.classList.toggle("active", v.id === `view-${name}`));
  document.querySelectorAll<HTMLElement>(".nav").forEach((b) => b.classList.toggle("active", b.dataset.view === name));
  document.querySelector("main")!.scrollTop = 0;
  if (name === "home") refreshStatus();
  if (name === "graphics") refreshOutputs();
  if (name === "game") {
    refreshOutputs();
    refreshSaves();
  }
  if (name === "controls") showControls();
  if (name === "advanced") showTab(tab ?? advancedTab);
}

function showTab(tab: string) {
  advancedTab = tab;
  document.querySelectorAll<HTMLElement>(".tab").forEach((b) => b.classList.toggle("active", b.dataset.tab === tab));
  document.querySelectorAll<HTMLElement>(".tab-panel").forEach((p) => p.classList.toggle("active", p.id === `tab-${tab}`));
  if (tab === "env") updatePreview();
  if (tab === "log") showLog();
  if (tab === "updates") renderUpdates();
  if (tab === "diagnostics") refreshReport();
}

document.querySelectorAll<HTMLElement>(".nav").forEach((b) => b.addEventListener("click", () => showView(b.dataset.view!)));
document.querySelectorAll<HTMLElement>(".tab").forEach((b) => b.addEventListener("click", () => showTab(b.dataset.tab!)));
// in-page shortcuts: data-go="page" or "page:tab"
document.querySelectorAll<HTMLElement>("[data-go]").forEach((b) =>
  b.addEventListener("click", () => {
    const [view, tab] = b.dataset.go!.split(":");
    showView(view, tab);
  }));

// ---- settings persistence --------------------------------------------------------
let saveTimer = 0;
function persist() {
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(async () => {
    await invoke("save_settings", { settings });
    flashSaved();
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
  if (status) renderFinderIcon();
  renderProgress();
  renderPrereqs();
  renderGame(lastSearch);
  renderControls();
  renderSaves();
  renderTrophies();
  renderOutputs();
  renderReport();
  renderLog();
  renderUpdates();
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

/** A setting can have more than one control (Home's quick settings repeat
 * a few of Graphics and Performance): after every change each control is set
 * again from `settings`. */
const syncers: (() => void)[] = [];
function syncAll() {
  for (const f of syncers) f();
  syncDependent();
}

function bindSettings() {
  const bindCheck = (id: string, key: keyof Settings) => {
    const el = $<HTMLInputElement>(id);
    const sync = () => (el.checked = settings[key] as boolean);
    syncers.push(sync);
    sync();
    el.addEventListener("change", () => {
      (settings[key] as boolean) = el.checked;
      syncAll();
      persist();
    });
  };
  const bindRange = (id: string, key: keyof Settings, fmt: (v: number) => string) => {
    const el = $<HTMLInputElement>(id);
    const out = $(`${id}-out`);
    const sync = () => {
      el.value = String(settings[key]);
      out.textContent = fmt(Number(el.value));
    };
    syncers.push(sync);
    sync();
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
  bindCheck("q-vsync", "vsync");
  bindCheck("rumble-on", "rumble");
  bindCheck("show-fps", "showFps");
  bindCheck("q-fps", "showFps");
  for (const [id, key] of [["screen", "display"], ["audio-device", "audioDevice"]] as const) {
    const el = $<HTMLSelectElement>(id);
    el.addEventListener("change", () => {
      settings[key] = el.value;
      persist();
    });
  }
  bindSelect("display", "windowMode");
  bindSelect("q-display", "windowMode");
  bindSelect("window-size", "windowSize");
  bindRange("gamma", "gamma", (v) => v.toFixed(2));
  bindGraphics(bindRange);
  bindRange("mouse-sens", "mouseSens", (v) => `${v.toFixed(2)}×`);
  bindRange("mouse-dz", "mouseDeadzone", (v) => String(Math.round(v)));
  bindRange("audio-volume", "audioVolume", (v) => `${Math.round(v)}%`);
  bindRange("rumble-strength", "rumbleStrength", (v) => `${Math.round(v)}%`);
  $("rumble-test").addEventListener("click", testRumble);
  $("finder-icon").addEventListener("click", toggleFinderIcon);

  for (const el of Array.from(document.querySelectorAll<HTMLInputElement>("input[data-patch]"))) {
    const name = el.dataset.patch!;
    const sync = () => { el.checked = (settings.patches ?? []).includes(name); };
    syncers.push(sync);
    sync();
    el.addEventListener("change", () => {
      const set = new Set(settings.patches ?? []);
      if (el.checked) set.add(name); else set.delete(name);
      settings.patches = Array.from(set);
      persist();
    });
  }
  const gl = $<HTMLSelectElement>("game-lang");
  gl.addEventListener("change", () => {
    settings.gameLang = gl.value;
    persist();
    void refreshTrophies();
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
    const keep = {
      uiLang: settings.uiLang,
      installDir: settings.installDir,
      gamePaths: settings.gamePaths,
      rapPath: settings.rapPath,
      keysPath: settings.keysPath,
      firmwarePath: settings.firmwarePath,
      decryptWith: settings.decryptWith,
    };
    Object.assign(settings, {
      gameLang: "pt",
      patches: [],
      gpuLight: true,
      gpuSsao: true,
      skipOcclusion: true,
      gammaOverride: false,
      gamma: 1.0,
      aniso: 0,
      aa: "",
      sharpen: 0,
      vibrance: 0,
      contrast: 1,
      upscale: "",
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
      showFps: false,
      extraEnv: "",
    }, keep);
    await invoke("save_settings", { settings });
    location.reload();
  });
  syncDependent();
}

/** A fixed list (window mode and size, anisotropic, AA, upscaling). */
function bindSelect(id: string, key: "windowMode" | "windowSize" | "aniso" | "aa" | "upscale") {
  const el = $<HTMLSelectElement>(id);
  const sync = () => {
    el.value = String(settings[key]);
    if (el.value !== String(settings[key])) el.selectedIndex = 0; // a value the list does not offer
  };
  syncers.push(sync);
  sync();
  el.addEventListener("change", () => {
    if (key === "aniso") settings.aniso = Number(el.value);
    else settings[key] = el.value;
    syncAll();
    persist();
  });
}

/** Anisotropic filtering, AA, sharpening and upscaling (PS3_ANISO, PS3_AA,
 * PS3_SHARPEN, PS3_UPSCALE); Play passes only what differs from the default. */
function bindGraphics(bindRange: (id: string, key: keyof Settings, fmt: (v: number) => string) => void) {
  bindSelect("aniso", "aniso");
  bindSelect("aa", "aa");
  bindSelect("upscale", "upscale");
  bindSelect("q-upscale", "upscale");
  bindRange("sharpen", "sharpen", (v) => (v > 0 ? `${Math.round(v * 100)}%` : t("settings.off")));
  bindRange("vibrance", "vibrance", (v) => (Math.abs(v) >= 0.005 ? `${v > 0 ? "+" : ""}${Math.round(v * 100)}%` : t("settings.off")));
  bindRange("contrast", "contrast", (v) => (Math.abs(v - 1) >= 0.005 ? `${Math.round(v * 100)}%` : t("settings.normal")));
  bindPreset();
}

/** Ready-made image-quality profiles: they only set the four image options
 * above (the GPU light/SSAO cost nothing visually and stay as they are). The
 * list shows the profile the current values match, or "custom". */
const PRESETS: Record<string, Pick<Settings, "aniso" | "aa" | "sharpen" | "upscale">> = {
  performance: { aniso: 0, aa: "", sharpen: 0, upscale: "" },
  balanced: { aniso: 4, aa: "fxaa", sharpen: 0.2, upscale: "" },
  quality: { aniso: 16, aa: "smaa", sharpen: 0.35, upscale: "metalfx" },
};

function currentPreset(): string {
  for (const [name, p] of Object.entries(PRESETS)) {
    if (settings.aniso === p.aniso && settings.aa === p.aa && settings.upscale === p.upscale &&
        Math.abs(settings.sharpen - p.sharpen) < 0.005) return name;
  }
  return "custom";
}

function bindPreset() {
  const el = $<HTMLSelectElement>("preset");
  syncers.push(() => { el.value = currentPreset(); });
  el.value = currentPreset();
  $("sharpen").addEventListener("input", () => { el.value = currentPreset(); });
  el.addEventListener("change", () => {
    const p = PRESETS[el.value];
    if (!p) return;
    Object.assign(settings, p);
    syncAll();
    persist();
  });
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

/** "Use as the app's Finder icon": the square icon made from the copy's
 * ICON0.PNG on the installed .app (macOS; not in a development build). */
function renderFinderIcon(msg?: string) {
  const can = !!status?.appBundle && status.os === "macos" && (!!status.icon || status.finderIcon);
  show($("finder-icon-row"), can);
  $("finder-icon").textContent = t(status?.finderIcon ? "settings.finderIconUnset" : "settings.finderIconSet");
  $("finder-icon-msg").textContent = msg ?? t("settings.finderIconHint");
}

async function toggleFinderIcon() {
  if (!status) return;
  const btn = $<HTMLButtonElement>("finder-icon");
  btn.disabled = true;
  try {
    status.finderIcon = await invoke<boolean>("finder_icon", { set: !status.finderIcon });
    renderFinderIcon(t("settings.finderIconDone"));
  } catch (e) {
    renderFinderIcon(t("settings.finderIconFailed", { e: String(e) }));
  } finally {
    btn.disabled = false;
  }
}

// ---- home ---------------------------------------------------------------------
async function refreshStatus() {
  status = await invoke<Status>("get_status");
  gameRunning = status.gameRunning;
  rebuildRunning = status.rebuildRunning || rebuildRunning;
  renderHome();
  renderFinderIcon();
  refreshUpdates();
}

const TITLE_ID = "NPUA80638";

function renderHome() {
  if (!status) return;
  const inst = status.install;
  show($("installed-actions"), !!inst);
  show($("not-installed"), !inst);
  // the art comes from the player's own copy (ICON0.PNG, PIC1.PNG); without
  // it the hero is a plain styled title
  const cover = $<HTMLImageElement>("cover");
  if (status.icon) cover.src = status.icon;
  show(cover, !!status.icon);
  $("hero-art").style.backgroundImage = status.background ? `url(${status.background})` : "";
  $("hero").classList.toggle("no-art", !status.background);
  $("game-title").textContent = inst?.title || "inFAMOUS 2";
  $("game-id").textContent = inst?.appVer ? `${TITLE_ID} · ${t("home.version")} ${inst.appVer}` : inst ? TITLE_ID : "";
  const meta: string[] = [];
  if (inst) {
    if (inst.builtAt) meta.push(`${t("home.builtAt")} ${new Date(inst.builtAt).toLocaleString(lang())}`);
    if (inst.layout === "dev") meta.push(t("home.devTree"));
    if (inst.portCommit) meta.push(`port ${inst.portCommit}`);
  }
  $("game-meta").textContent = meta.join(" · ");
  const play = $<HTMLButtonElement>("play");
  $("play-label").textContent = t(gameRunning ? "home.stop" : "home.play");
  play.classList.toggle("stop", gameRunning);
  play.disabled = rebuildRunning || !status.hostSupported;
  show($("run-state"), gameRunning);
  renderStatusList();
  renderReport();
  show($("home-unsupported"), !status.hostSupported);
  show($("go-install"), status.hostSupported);
  renderUpdates();

  const s = status.lastSave;
  const box = $("save-info");
  box.innerHTML = "";
  if (!s) {
    box.textContent = t("home.noSave");
  } else {
    const b = document.createElement("strong");
    b.textContent = s.subtitle || s.title;
    const d = document.createElement("div");
    d.textContent = s.detail.split("\n").filter(Boolean).join(" · ");
    const w = document.createElement("div");
    w.className = "small";
    w.textContent = s.modified ? new Date(s.modified * 1000).toLocaleString(lang()) : "";
    box.append(b, d, w);
  }
}

/** Home's Status card. */
function renderStatusList() {
  if (!status) return;
  const inst = status.install;
  const rows: [Key, string, string?][] = [];
  const live = !!report?.live && gameRunning;
  rows.push([
    "home.st.state",
    rebuildRunning
      ? t("home.st.rebuilding")
      : gameRunning
        ? t(live ? "home.st.diag" : "home.st.running")
        : inst
          ? t(inst.layout === "dev" ? "home.st.dev" : "home.st.installed")
          : t("home.st.notInstalled"),
    inst || gameRunning ? "ok-text" : "warn-text",
  ]);
  if (inst?.appVer) rows.push(["home.st.version", `${TITLE_ID} ${inst.appVer}`]);
  if (inst?.builtAt) rows.push(["home.st.built", new Date(inst.builtAt).toLocaleString(lang())]);
  if (inst?.portCommit) rows.push(["home.st.port", inst.portCommit]);
  rows.push(["home.st.folder", status.installDir.replace(/^\/Users\/[^/]+/, "~")]);
  const dl = $("status-list");
  dl.replaceChildren();
  for (const [k, v, cls] of rows) {
    const dt = document.createElement("dt");
    dt.textContent = t(k);
    const dd = document.createElement("dd");
    dd.textContent = v;
    if (cls) dd.className = cls;
    dl.append(dt, dd);
  }
}

async function startGame(diagnostic: boolean) {
  const msg = $("home-msg");
  try {
    const r = await invoke<{ log: string; backup: string | null; backupError: string | null; report: string | null }>("play", { diagnostic });
    gameRunning = true;
    msg.textContent =
      (r.report ? t("home.startedDiag", { dir: r.report }) : t("home.started", { log: r.log })) +
      (r.backup ? t("home.backedUp", { id: r.backup }) : "") +
      (r.backupError ? t("home.backupFailed", { e: r.backupError }) : "");
    $("report-msg").textContent = "";
  } catch (e) {
    msg.textContent = String(e);
  }
  renderHome();
  refreshReport();
}

$("play").addEventListener("click", async () => {
  if (gameRunning) {
    await invoke("stop_game").catch((e) => ($("home-msg").textContent = String(e)));
    return;
  }
  await startGame(false);
});
$("go-install").addEventListener("click", () => showView("advanced", "install"));
$("go-saves").addEventListener("click", () => {
  showView("game");
  $("saves-section").scrollIntoView({ behavior: "smooth", block: "start" });
});
$("open-saves").addEventListener("click", async () => {
  const p = await invoke<Record<string, string>>("log_paths");
  invoke("reveal", { path: status?.lastSave?.folder ?? p.saves });
});
$("open-log").addEventListener("click", async () => {
  const p = await invoke<Record<string, string>>("log_paths");
  invoke("reveal", { path: p.game });
});

// ---- debug mode (report a problem, src-tauri/src/report.rs) ---------------------------
let report: ReportInfo | null = null;
let reportPoll = 0;
let sessions: ReportInfo[] = [];
/** Sessions with an action running: id -> "zip" | "triage". */
const busy = new Map<string, string>();
const URL_ISSUE = "https://github.com/erikomis/infamous2-recomp/issues/new";

async function refreshReport() {
  try {
    report = await invoke<ReportInfo | null>("report_info");
  } catch {
    report = null;
  }
  renderReport();
  // follow the dumps while the debug game runs
  const live = !!report?.live && gameRunning;
  if (live && !reportPoll) reportPoll = window.setInterval(refreshReport, 2000);
  if (!live && reportPoll) {
    window.clearInterval(reportPoll);
    reportPoll = 0;
  }
  if (advancedTab === "diagnostics") refreshSessions();
}

function renderReport() {
  const r = report;
  const live = !!r?.live && gameRunning;
  $("report-hint").textContent = t(live ? "report.hintLive" : gameRunning ? "report.hintNotDiag" : "report.hintIdle");
  show($("report-play"), !gameRunning);
  $<HTMLButtonElement>("report-play").disabled = rebuildRunning || !status?.hostSupported || !status?.install;
  show($("report-capture"), live);
  show($("hero-capture"), live);
  renderStatusList();
  $("report-state").textContent = r && live
    ? t("report.state", { id: r.id, captures: r.captures, surfaces: r.surfaces, shaders: r.shaders, size: human(r.bytes) }) +
      (r.pending ? t("report.pending") : "")
    : "";
}

$("report-play").addEventListener("click", () => startGame(true));
async function captureFrame() {
  let text: string;
  try {
    report = await invoke<ReportInfo>("report_capture");
    text = t("report.captured", { n: report.captures });
  } catch (e) {
    text = String(e);
  }
  $("report-msg").textContent = text;
  $("home-msg").textContent = text;
  renderReport();
}
$("report-capture").addEventListener("click", captureFrame);
$("hero-capture").addEventListener("click", captureFrame);
$("report-issue").addEventListener("click", () => invoke("open_url", { url: URL_ISSUE }));
$("report-refresh").addEventListener("click", () => refreshSessions());
listen("report-changed", () => refreshSessions());

async function refreshSessions() {
  try {
    sessions = await invoke<ReportInfo[]>("report_list");
  } catch (e) {
    $("report-msg").textContent = String(e);
  }
  renderSessions();
}

/** "20261018-120000" (UTC) -> the local date and time. */
function sessionTime(id: string): string {
  const m = id.match(/^(\d{4})(\d{2})(\d{2})-(\d{2})(\d{2})(\d{2})/);
  if (!m) return id;
  const d = new Date(Date.UTC(+m[1], +m[2] - 1, +m[3], +m[4], +m[5], +m[6]));
  return d.toLocaleString(lang(), { dateStyle: "medium", timeStyle: "short" });
}

/** Seconds -> m:ss or h:mm:ss. */
function clock(secs: number | null): string {
  if (secs == null) return "-";
  const s = Math.round(secs);
  const h = Math.floor(s / 3600);
  const mm = String(Math.floor((s % 3600) / 60)).padStart(h ? 2 : 1, "0");
  const ss = String(s % 60).padStart(2, "0");
  return h ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls = "", text = ""): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text) e.textContent = text;
  return e;
}

function sessionEnd(r: ReportInfo): [string, string] {
  if (r.live && gameRunning) return [t("report.live"), "accent"];
  if (!r.debug) return [t("report.oldReport"), "muted-pill"];
  if (r.stopped) return [t("report.stopped"), "muted-pill"];
  if (r.exit && r.exit !== "code=0") return [t("report.crashed", { exit: r.exit }), "bad"];
  return [t("report.ended"), "muted-pill"];
}

function renderSessions() {
  const ul = $("report-sessions");
  ul.replaceChildren();
  show($("report-none"), sessions.length === 0);
  for (const r of sessions) {
    const li = el("li", "session");
    const head = el("div", "row session-head");
    head.append(el("strong", "", sessionTime(r.id)));
    if (r.duration != null) head.append(el("span", "small muted", clock(r.duration)));
    const [end, cls] = sessionEnd(r);
    head.append(el("span", `pill ${cls}`, end));
    if (r.crashReport) head.append(el("span", "pill bad", t("report.crashReport")));
    li.append(head);
    li.append(el("div", "small muted", t("report.rowMeta", { captures: r.captures, log: human(r.logBytes), size: human(r.bytes) })));

    const tr = r.triage;
    const sum = el("div", "session-triage");
    if (!tr) {
      sum.append(el("span", "small muted", t("report.triageNone")));
    } else {
      const counts = el("div", "row");
      counts.append(el("span", tr.errors ? "pill bad" : "pill muted-pill", t("report.triageCounts", tr as unknown as Record<string, number>)));
      for (const [c, n] of tr.categories) counts.append(el("span", "pill warn", `${t(`cat.${c}` as Key)} ${n}`));
      if (tr.stale) counts.append(el("span", "small warn-text", t("report.stale")));
      sum.append(counts);
      if (!tr.top.length) sum.append(el("p", "small muted", t("report.clean")));
      else {
        const ol = el("ol", "issues");
        for (const g of tr.top.slice(0, 8)) {
          const item = el("li", `sev-${g.severity}`);
          item.append(el("span", "pill muted-pill", t(`cat.${g.category}` as Key)), " ");
          item.append(el("span", "issue-title", g.title));
          item.append(el("span", "small muted", ` ×${g.count} · ${t("report.at", { first: clock(g.first), last: clock(g.last) })}`));
          ol.append(item);
        }
        sum.append(ol);
      }
    }
    li.append(sum);

    const act = el("div", "row");
    const work = busy.get(r.id);
    const analyse = el("button", "", t(work === "triage" ? "report.analysing" : "report.analyse"));
    analyse.disabled = !!work;
    analyse.addEventListener("click", () => sessionAction(r, "triage"));
    const zip = el("button", "", t("report.zip") + (work === "zip" ? "…" : ""));
    zip.disabled = !!work || (r.live && gameRunning);
    zip.addEventListener("click", () => sessionAction(r, "zip"));
    const open = el("button", "link", t("report.open"));
    open.addEventListener("click", () => invoke("reveal", { path: r.dir }));
    act.append(analyse, zip, open);
    if (tr) {
      const md = el("button", "link", t("report.openTriage"));
      md.addEventListener("click", () => invoke("reveal", { path: tr.md }));
      act.append(md);
    }
    if (r.zip) {
      const z = el("button", "link", t("report.showZip"));
      z.addEventListener("click", () => invoke("reveal", { path: r.zip! }));
      act.append(z);
    }
    li.append(act);
    ul.append(li);
  }
}

async function sessionAction(r: ReportInfo, what: "triage" | "zip") {
  const msg = $("report-msg");
  busy.set(r.id, what);
  msg.textContent = t(what === "zip" ? "report.zipping" : "report.analysing");
  renderSessions();
  try {
    const got =
      what === "zip"
        ? await invoke<ReportInfo>("report_zip", { id: r.id, notes: $<HTMLTextAreaElement>("report-notes").value })
        : await invoke<ReportInfo>("report_triage", { id: r.id });
    sessions = sessions.map((s) => (s.id === got.id ? got : s));
    msg.textContent = what === "zip" ? t("report.zipped", { file: got.zip ?? "", size: human(got.zipBytes) }) : "";
    if (what === "zip" && got.zip) invoke("reveal", { path: got.zip });
  } catch (e) {
    msg.textContent = String(e);
  }
  busy.delete(r.id);
  renderSessions();
}

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

/** The decryption the Install screen is set to (rebuild.py --keys/--firmware/--decrypt-with). */
function decryptOpts() {
  return { keys: settings.keysPath ?? "", firmware: settings.firmwarePath ?? "", decryptWith: settings.decryptWith || "auto" };
}

let prereqSeq = 0;
async function refreshPrereqs() {
  $("prereq-msg").textContent = t("prereq.checking");
  const seq = ++prereqSeq;
  const p = await invoke<Prereqs>("check_prereqs", { decrypt: decryptOpts() });
  if (seq !== prereqSeq) return; // a newer check (the decryption changed meanwhile) wins
  prereqs = p;
  $("prereq-msg").textContent = prereqs.error && prereqs.supported ? t("prereq.error", { e: prereqs.error }) : "";
  renderPrereqs();
  // the Command Line Tools install in Apple's own window; stop polling once they are in
  if (cltPoll && prereqs.clt) {
    window.clearInterval(cltPoll);
    cltPoll = 0;
  }
}

/** Open "More options" at the decryption fields. */
function showDecryptOptions(focus: string) {
  if (!$("tab-install").classList.contains("active") || !$("view-advanced").classList.contains("active")) showView("advanced", "install");
  ($("more-options") as HTMLDetailsElement).open = true;
  const el = $(focus);
  el.scrollIntoView({ behavior: "smooth", block: "center" });
  el.focus();
}

function prereqActions(c: CheckItem): HTMLButtonElement[] {
  if (c.level !== "fail" || !prereqs) return [];
  const button = (label: Key, run: () => unknown) => {
    const b = document.createElement("button");
    b.textContent = t(label);
    b.addEventListener("click", () => run());
    return b;
  };
  switch (c.name) {
    case "xcode-clt":
      return [button("prereq.installClt", installClt)];
    case "rpcs3":
      return [
        button("prereq.getRpcs3", () => invoke("open_url", { url: URL_RPCS3 })),
        button("prereq.withoutRpcs3", () => showDecryptOptions("decrypt-with")),
      ];
    case "firmware":
      return settings.firmwarePath
        ? [button("prereq.chooseFirmware", () => showDecryptOptions("firmware-path"))]
        : [
            button("prereq.howFirmware", () => invoke("open_url", { url: URL_FIRMWARE })),
            button("prereq.chooseFirmware", () => showDecryptOptions("firmware-path")),
          ];
    case "keys":
      return [button("prereq.chooseKeys", () => showDecryptOptions("keys-path"))];
    default:
      return [];
  }
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
    keys: "prereq.hint.keys",
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
    li.append(...prereqActions(c));
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
const keysPath = $<HTMLInputElement>("keys-path");
const firmwarePath = $<HTMLInputElement>("firmware-path");
const decryptWith = $<HTMLSelectElement>("decrypt-with");

function readInstallForm() {
  settings.gamePaths = gamePaths.value.split("\n").map((s) => s.trim()).filter(Boolean);
  settings.rapPath = rapPath.value.trim();
  settings.keysPath = keysPath.value.trim();
  settings.firmwarePath = firmwarePath.value.trim();
  settings.decryptWith = decryptWith.value || "auto";
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
// Decryption: what the requirements need changes with it, so check again.
let recheckTimer = 0;
function decryptChanged() {
  readInstallForm();
  window.clearTimeout(recheckTimer);
  recheckTimer = window.setTimeout(refreshPrereqs, 300);
}
[keysPath, firmwarePath, decryptWith].forEach((el) => el.addEventListener("change", decryptChanged));
$("pick-keys").addEventListener("click", async () => {
  const p = await open({ title: t("install.keys"), defaultPath: keysPath.value || undefined });
  if (typeof p === "string") {
    keysPath.value = p;
    decryptChanged();
  }
});
$("pick-fw-dir").addEventListener("click", async () => {
  const p = await open({ directory: true, title: t("install.firmware") });
  if (typeof p === "string") {
    firmwarePath.value = p;
    decryptChanged();
  }
});
$("pick-fw-pup").addEventListener("click", async () => {
  const p = await open({ title: t("install.firmware"), filters: [{ name: "PS3UPDAT.PUP", extensions: ["PUP", "pup"] }] });
  if (typeof p === "string") {
    firmwarePath.value = p;
    decryptChanged();
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
        decrypt: decryptOpts(),
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
  refreshReport();
  if ($("tab-log").classList.contains("active")) showLog();
});

// ---- start ---------------------------------------------------------------------
async function main() {
  settings = await invoke<Settings>("get_settings");
  setLang(settings.uiLang as Lang);
  bindSettings();
  initControls();
  initLog();
  initTrophies();
  initSaves({
    keep: () => settings.saveBackupsKeep ?? 10,
    setKeep: (n) => {
      settings.saveBackupsKeep = n;
      persist();
    },
  });
  initUpdates({
    canRebuild: () => !rebuildRunning && !gameRunning && !!status?.hostSupported && !!status?.rebuildScript,
    rebuild: () => {
      showView("advanced", "install");
      startRebuild(false);
    },
  });
  refreshOutputs();
  gamePaths.value = settings.gamePaths.join("\n");
  rapPath.value = settings.rapPath;
  outDir.value = settings.installDir;
  keysPath.value = settings.keysPath ?? "";
  firmwarePath.value = settings.firmwarePath ?? "";
  decryptWith.value = settings.decryptWith || "auto";
  if (!decryptWith.value) decryptWith.value = "auto"; // a value the list does not offer
  applyLang(settings.uiLang as Lang);
  await refreshStatus();
  refreshReport();
  renderPrereqs();
  if (!status?.install) showView("advanced", "install");
  if (status?.hostSupported) {
    // "only pick the game folder": RPCS3's copy and licence are found on their own
    if (!settings.gamePaths.length) await detectRpcs3(true);
    else await lookForLicence(false);
    await refreshPrereqs();
  }
}
main();
