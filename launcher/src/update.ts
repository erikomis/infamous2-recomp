// Home screen "Updates" card (src-tauri/src/update.rs): whether the installed
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
  const card = $("update-card");
  if (!info) {
    show(card, false);
    return;
  }
  show(card, true);
  const lines: string[] = [];
  if (info.rebuildSuggested)
    lines.push(t("update.rebuildNeeded", { built: info.installedCommit ?? "?", now: info.sourcesCommit ?? "?" }));
  if (info.newerRelease) lines.push(t("update.newRelease", { v: info.latestVersion ?? "", cur: info.appVersion }));
  else if (info.checkError) lines.push(t("update.checkFailed", { e: info.checkError }));
  else if (info.checkedOnline) lines.push(t("update.upToDate", { cur: info.appVersion }));
  if (!lines.length) lines.push(t("update.notChecked", { cur: info.appVersion }));
  const box = $("update-info");
  box.replaceChildren();
  for (const l of lines) {
    const d = document.createElement("div");
    d.textContent = l;
    box.appendChild(d);
  }
  const rebuild = $("update-rebuild") as HTMLButtonElement;
  show(rebuild, info.rebuildSuggested);
  rebuild.disabled = !hooks.canRebuild();
  show($("update-open"), info.newerRelease && !!info.latestUrl);
  ($("update-check") as HTMLButtonElement).disabled = busy;
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
  $("update-rebuild").addEventListener("click", () => hooks.rebuild());
  $("update-open").addEventListener("click", () => {
    if (info?.latestUrl) invoke("open_url", { url: info.latestUrl });
  });
}

export function renderUpdates() {
  render();
}
