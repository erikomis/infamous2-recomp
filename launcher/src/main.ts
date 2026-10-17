import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { GAME_LANGS, Key, Lang, lang, setLang, t, translateDom } from "./i18n";
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
  mouseDeadzone: number;
  mouseCapture: boolean;
  windowMode: string;
  windowSize: string;
  gameIcon: boolean;
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
  if (name === "settings") updatePreview();
}
document.querySelectorAll<HTMLElement>(".nav").forEach((b) => b.addEventListener("click", () => showView(b.dataset.view!)));

// ---- settings persistence --------------------------------------------------------
let saveTimer = 0;
function persist() {
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(async () => {
    await invoke("save_settings", { settings });
    const s = $("saved");
    show(s, true);
    window.setTimeout(() => show(s, false), 1200);
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
  bindCheck("mouse-capture", "mouseCapture");
  bindCheck("game-icon", "gameIcon");
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
      mouseDeadzone: 30,
      mouseCapture: true,
      windowMode: "windowed",
      windowSize: "",
      gameIcon: false,
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
  for (const id of ["mouse-sens", "mouse-invert", "mouse-dz", "mouse-capture"]) $<HTMLInputElement>(id).disabled = !settings.mouse;
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
  play.disabled = rebuildRunning;
  show($("run-state"), gameRunning);

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

// ---- install ------------------------------------------------------------------
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
[gamePaths, rapPath, outDir].forEach((el) => el.addEventListener("change", readInstallForm));

$("pick-game-dir").addEventListener("click", async () => {
  const p = await open({ directory: true });
  if (typeof p === "string") {
    gamePaths.value = p;
    readInstallForm();
  }
});
$("pick-game-pkg").addEventListener("click", async () => {
  const p = await open({ multiple: true, filters: [{ name: "PKG", extensions: ["pkg", "PKG"] }] });
  if (Array.isArray(p) && p.length) {
    gamePaths.value = p.join("\n");
    readInstallForm();
  }
});
$("pick-rap").addEventListener("click", async () => {
  const p = await open({ filters: [{ name: "RAP", extensions: ["rap", "RAP"] }] });
  if (typeof p === "string") {
    rapPath.value = p;
    readInstallForm();
  }
});
$("pick-out").addEventListener("click", async () => {
  const p = await open({ directory: true, defaultPath: outDir.value || undefined });
  if (typeof p === "string") {
    outDir.value = p;
    readInstallForm();
  }
});
$("detect").addEventListener("click", async () => {
  const d = await invoke<{ gameDir: string | null; raps: string[] }>("detect_rpcs3");
  if (d.gameDir) gamePaths.value = d.gameDir;
  if (d.raps.length) rapPath.value = d.raps[0];
  $("install-msg").textContent = t(d.gameDir ? "install.detected" : "install.notDetected");
  readInstallForm();
});

async function startRebuild(dryRun: boolean) {
  readInstallForm();
  if (!status?.rebuildScript) {
    $("install-msg").textContent = t("install.noScript");
    return;
  }
  if (!settings.gamePaths.length || !settings.rapPath) {
    $("install-msg").textContent = t("install.needInputs");
    return;
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
  gamePaths.value = settings.gamePaths.join("\n");
  rapPath.value = settings.rapPath;
  outDir.value = settings.installDir;
  applyLang(settings.uiLang as Lang);
  await refreshStatus();
  if (!status?.install) showView("install");
}
main();
