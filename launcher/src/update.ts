// Updates (src-tauri/src/update.rs), shown in Advanced > Updates and, when
// there is news, as a bar on Home: whether the installed
// game was built from other port sources than this app carries (rebuild
// offered), and whether a newer app release is published on GitHub (checked
// once per session, again on "Check again").
import { invoke } from "@tauri-apps/api/core";
import { t } from "./i18n";

interface UpdateInfo {
  appVersion: string;
  sourcesCommit: string | null;
  installedCommit: string | null;
  rebuildSuggested: boolean;
  checkedOnline: boolean;
  latestVersion: string | null;
  latestUrl: string | null;
  newerRelease: boolean;
  checkError: string | null;
}

const $ = (id: string) => document.getElementById(id) as HTMLElement;
const show = (el: HTMLElement, on: boolean) => el.classList.toggle("hidden", !on);

let info: UpdateInfo | null = null;
let checkedOnce = false;
let busy = false;
let hooks: { canRebuild: () => boolean; rebuild: () => void } = { canRebuild: () => false, rebuild: () => {} };

function render() {
  const box = $("update-info");
  box.replaceChildren();
  const home = $("home-update");
  if (!info) {
    // Advanced > Updates keeps its card; it says what is going on meanwhile
    const d = document.createElement("div");
    d.className = "muted";
    d.textContent = t(busy ? "update.checking" : "update.unknown");
    box.appendChild(d);
    show($("update-rebuild"), false);
    show($("update-open"), false);
    ($("update-check") as HTMLButtonElement).disabled = busy;
    show(home, false);
    return;
  }
  const lines: string[] = [];
  const news: string[] = []; // what deserves a line on Home
  if (info.rebuildSuggested) {
    const l = t("update.rebuildNeeded", { built: info.installedCommit ?? "?", now: info.sourcesCommit ?? "?" });
    lines.push(l);
    news.push(l);
  }
  if (info.newerRelease) {
    const l = t("update.newRelease", { v: info.latestVersion ?? "", cur: info.appVersion });
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
  const canRebuild = hooks.canRebuild();
  for (const id of ["update-rebuild", "home-update-rebuild"]) {
    const b = $(id) as HTMLButtonElement;
    show(b, info.rebuildSuggested);
    b.disabled = !canRebuild;
  }
  for (const id of ["update-open", "home-update-open"]) show($(id), info.newerRelease && !!info.latestUrl);
  ($("update-check") as HTMLButtonElement).disabled = busy;
  $("home-update-text").textContent = news.join(" ");
  show(home, news.length > 0);
  const v = $("app-version");
  if (v) v.textContent = `${t("update.appVersion")} ${info.appVersion}`;
}

/** Local state on every Home refresh; the online check only the first time
 * (or when forced). */
export async function refreshUpdates(forceOnline = false) {
  if (busy) return;
  const online = forceOnline || !checkedOnce;
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
      r.newerRelease = info.newerRelease;
      r.checkError = info.checkError;
    }
    info = r;
  } catch {
    // an older backend without the command: no card
  } finally {
    busy = false;
  }
  render();
}

export function initUpdates(h: { canRebuild: () => boolean; rebuild: () => void }) {
  hooks = h;
  $("update-check").addEventListener("click", () => refreshUpdates(true));
  for (const id of ["update-rebuild", "home-update-rebuild"]) $(id).addEventListener("click", () => hooks.rebuild());
  for (const id of ["update-open", "home-update-open"])
    $(id).addEventListener("click", () => {
      if (info?.latestUrl) invoke("open_url", { url: info.latestUrl });
    });
}

export function renderUpdates() {
  render();
}
