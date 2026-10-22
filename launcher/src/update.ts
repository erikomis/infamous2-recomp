// Updates (src-tauri/src/update.rs), shown in Advanced > Updates and, when
// there is news, as a bar on Home: whether the installed game was built from
// other port sources than this app carries (rebuild offered), and whether a
// newer app version is published (the release manifest, latest.json; checked
// once per session when "Check for updates automatically" is on, and on
// "Check again"). A newer version can be downloaded and installed in place
// (macOS app), then "Restart to finish" relaunches into it.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Key, t } from "./i18n";
import { human } from "./ui";

interface UpdateInfo {
  appVersion: string;
  sourcesCommit: string | null;
  installedCommit: string | null;
  rebuildSuggested: boolean;
  disabled: boolean;
  checkedOnline: boolean;
  latestVersion: string | null;
  latestUrl: string | null;
  latestNotes: string | null;
  latestDate: string | null;
  newerRelease: boolean;
  checkError: string | null;
  installBlocker: string | null;
  installedPending: string | null;
}

interface Progress {
  downloaded: number;
  total: number | null;
  phase: "download" | "install";
}

interface Hooks {
  canRebuild: () => boolean;
  rebuild: () => void;
  /** The game or a rebuild is running (no install, no restart then). */
  busy: () => "game" | "rebuild" | null;
  /** "Check for updates automatically" (launcher.json autoUpdateCheck). */
  auto: () => boolean;
  setAuto: (on: boolean) => void;
}

const $ = (id: string) => document.getElementById(id) as HTMLElement;
const show = (el: HTMLElement, on: boolean) => el.classList.toggle("hidden", !on);
const all = (sel: string) => Array.from(document.querySelectorAll<HTMLElement>(sel));

let info: UpdateInfo | null = null;
let checkedOnce = false;
let busy = false;
/** The install: idle, running (with its last progress), done, or failed. */
let install: { state: "idle" | "running" | "done" | "failed"; progress?: Progress; version?: string; error?: string } = {
  state: "idle",
};
let hooks: Hooks = {
  canRebuild: () => false,
  rebuild: () => {},
  busy: () => null,
  auto: () => true,
  setAuto: () => {},
};

const BLOCKERS: Record<string, Key> = {
  game: "update.blocked.game",
  rebuild: "update.blocked.rebuild",
  dmg: "update.blocked.dmg",
  translocated: "update.blocked.translocated",
  dev: "update.blocked.dev",
  platform: "update.blocked.platform",
};

/** "blocked:<why>" errors from the backend, in words. */
function explain(e: unknown): string {
  const s = String(e);
  const m = /^blocked:(\w+)$/.exec(s);
  return m && BLOCKERS[m[1]] ? t(BLOCKERS[m[1]]) : s;
}

function progressText(): string {
  const p = install.progress;
  const v = install.version ?? info?.latestVersion ?? "";
  if (!p || p.phase === "install") return t(p ? "update.installing" : "update.starting", { v });
  if (p.total) return t("update.downloading", { v, done: human(p.downloaded), total: human(p.total) });
  return t("update.downloadingNoTotal", { v, done: human(p.downloaded) });
}

function renderProgress() {
  const running = install.state === "running";
  for (const box of all(".update-progress")) show(box, running);
  if (!running) return;
  const p = install.progress;
  const frac = p?.phase === "download" && p.total ? Math.min(1, p.downloaded / p.total) : null;
  for (const bar of all(".update-bar")) {
    bar.classList.toggle("indeterminate", frac === null);
    bar.style.width = frac === null ? "" : `${(frac * 100).toFixed(1)}%`;
  }
  for (const el of all(".update-progress-text")) el.textContent = progressText();
}

function render() {
  const box = $("update-info");
  box.replaceChildren();
  const home = $("home-update");
  ($("update-auto") as HTMLInputElement).checked = hooks.auto();
  show($("update-auto-row"), !info?.disabled);
  if (!info) {
    // Advanced > Updates keeps its card; it says what is going on meanwhile
    const d = document.createElement("div");
    d.className = "muted";
    d.textContent = t(busy ? "update.checking" : "update.unknown");
    box.appendChild(d);
    for (const id of ["update-rebuild", "update-open", "update-install", "update-restart"]) show($(id), false);
    ($("update-check") as HTMLButtonElement).disabled = busy;
    show(home, false);
    renderProgress();
    return;
  }
  const lines: string[] = [];
  const news: string[] = []; // what deserves a line on Home
  const pending = install.state === "done" ? install.version : info.installedPending;
  if (info.rebuildSuggested && !pending) {
    const l = t("update.rebuildNeeded", { built: info.installedCommit ?? "?", now: info.sourcesCommit ?? "?" });
    lines.push(l);
    news.push(l);
  }
  const blocker = info.installBlocker;
  const busyNow = hooks.busy();
  if (pending) {
    const l = t("update.installed", { v: pending });
    lines.push(l);
    news.push(l);
  } else if (info.disabled) {
    lines.push(t("update.disabled", { cur: info.appVersion }));
  } else if (info.newerRelease) {
    let l = t("update.newRelease", { v: info.latestVersion ?? "", cur: info.appVersion });
    if (install.state === "failed") l += " " + t("update.installFailed", { e: install.error ?? "" });
    else if (install.state !== "running" && blocker) l += " " + t(BLOCKERS[blocker] ?? "update.blocked.platform");
    else if (install.state !== "running" && busyNow) l += " " + t(BLOCKERS[busyNow]);
    lines.push(l);
    news.push(l);
  } else if (info.checkError) lines.push(t("update.checkFailed", { e: info.checkError }));
  else if (info.checkedOnline) lines.push(t("update.upToDate", { cur: info.appVersion }));
  if (!lines.length) lines.push(t("update.notChecked", { cur: info.appVersion }));
  for (const l of lines) {
    const d = document.createElement("div");
    d.textContent = l;
    box.appendChild(d);
  }
  if (info.newerRelease && info.latestNotes && !pending) {
    const notes = document.createElement("div");
    notes.className = "update-notes";
    notes.textContent = info.latestNotes.length > 1500 ? info.latestNotes.slice(0, 1500) + "…" : info.latestNotes;
    box.appendChild(notes);
  }
  const canRebuild = hooks.canRebuild() && install.state !== "running";
  for (const id of ["update-rebuild", "home-update-rebuild"]) {
    const b = $(id) as HTMLButtonElement;
    show(b, info.rebuildSuggested && !pending);
    b.disabled = !canRebuild;
  }
  const offerInstall = info.newerRelease && !info.disabled && !pending && !blocker;
  for (const id of ["update-install", "home-update-install"]) {
    const b = $(id) as HTMLButtonElement;
    show(b, offerInstall);
    b.disabled = install.state === "running" || !!busyNow;
  }
  for (const id of ["update-restart", "home-update-restart"]) {
    const b = $(id) as HTMLButtonElement;
    show(b, !!pending);
    b.disabled = !!busyNow;
  }
  // the release page: in Advanced for any newer version, on Home only when
  // it cannot be installed from here
  for (const id of ["update-open", "home-update-open"])
    show(
      $(id),
      info.newerRelease && !!info.latestUrl && !pending && (id === "update-open" || !!blocker || install.state === "failed"),
    );
  ($("update-check") as HTMLButtonElement).disabled = busy || install.state === "running";
  $("home-update-text").textContent = news.join(" ");
  show(home, news.length > 0 || install.state === "running");
  const v = $("app-version");
  if (v) v.textContent = `${t("update.appVersion")} ${info.appVersion}`;
  renderProgress();
}

/** Local state on every Home refresh; the online check the first time (when
 * the automatic check is on) or when forced. */
export async function refreshUpdates(forceOnline = false) {
  if (busy) return;
  const online = forceOnline || (!checkedOnce && hooks.auto());
  busy = online;
  render();
  try {
    const r = await invoke<UpdateInfo>("check_updates", { online });
    if (online) checkedOnce = true;
    // keep the last online answer when this was a local-only refresh
    if (!online && info?.checkedOnline) {
      r.checkedOnline = info.checkedOnline;
      r.latestVersion = info.latestVersion;
      r.latestUrl = info.latestUrl;
      r.latestNotes = info.latestNotes;
      r.latestDate = info.latestDate;
      r.newerRelease = info.newerRelease;
      r.checkError = info.checkError;
      r.installBlocker = r.installBlocker ?? info.installBlocker;
    }
    info = r;
  } catch {
    // an older backend without the command: no card
  } finally {
    busy = false;
  }
  render();
}

async function startInstall() {
  if (install.state === "running" || !info?.newerRelease) return;
  install = { state: "running", version: info.latestVersion ?? undefined };
  render();
  try {
    const v = await invoke<string>("install_update");
    install = { state: "done", version: v };
  } catch (e) {
    install = { state: "failed", error: explain(e) };
  }
  render();
}

async function restart() {
  try {
    await invoke("restart_app");
  } catch (e) {
    $("home-update-text").textContent = explain(e);
  }
}

export function initUpdates(h: Hooks) {
  hooks = h;
  $("update-check").addEventListener("click", () => refreshUpdates(true));
  for (const id of ["update-rebuild", "home-update-rebuild"]) $(id).addEventListener("click", () => hooks.rebuild());
  for (const id of ["update-install", "home-update-install"]) $(id).addEventListener("click", startInstall);
  for (const id of ["update-restart", "home-update-restart"]) $(id).addEventListener("click", restart);
  for (const id of ["update-open", "home-update-open"])
    $(id).addEventListener("click", () => {
      if (info?.latestUrl) invoke("open_url", { url: info.latestUrl });
    });
  const auto = $("update-auto") as HTMLInputElement;
  auto.addEventListener("change", () => {
    hooks.setAuto(auto.checked);
    if (auto.checked && !checkedOnce) refreshUpdates(true);
  });
  listen<Progress>("update-progress", (ev) => {
    if (install.state !== "running") return;
    install.progress = ev.payload;
    renderProgress();
  });
}

export function renderUpdates() {
  render();
}

/** An app update is being installed (Play and the rebuild wait for it). */
export function updateRunning(): boolean {
  return install.state === "running";
}
