// The Saves screen: the game's slots in this port and in RPCS3 side by side,
// backups, restore, copies between the two and .zip files. The work is done
// in src-tauri/src/saves.rs; every command there refuses while the game runs.
import { invoke } from "@tauri-apps/api/core";
import { ask, open, save } from "@tauri-apps/plugin-dialog";
import { Key, lang, t } from "./i18n";

type Side = "port" | "rpcs3";
interface Slot {
  side: Side;
  user: string;
  name: string;
  path: string;
  title: string;
  subtitle: string;
  detail: string;
  modified: number;
  size: number;
  files: number;
  icon: string | null;
  stateHash: string | null;
  sfo: "text" | "binary" | "none";
}
interface BackupMeta {
  id: string;
  created: number;
  reason: string;
  source: Side;
  user: string;
  slots: string[];
  trophies: boolean;
  size: number;
  labels: Record<string, string>;
}
interface Overview {
  port: Slot[];
  rpcs3: Slot[];
  rpcs3Users: string[];
  backups: BackupMeta[];
  portDir: string;
  rpcs3Dir: string;
  backupsDir: string;
  gameRunning: boolean;
  rpcs3Running: boolean;
}
interface Report {
  backup: BackupMeta | null;
  slots: string[];
}

/** What the screen needs from main.ts (the settings object lives there). */
export interface SavesHost {
  keep(): number;
  setKeep(n: number): void;
}

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const el = <K extends keyof HTMLElementTagNameMap>(tag: K, cls = "", text = "") => {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text) e.textContent = text;
  return e;
};

let host: SavesHost;
let ov: Overview | null = null;
let user = "";
let working = false;

const date = (secs: number) => (secs ? new Date(secs * 1000).toLocaleString(lang()) : "");
const kb = (n: number) => (n < 1024 * 1024 ? `${Math.max(1, Math.round(n / 1024))} KB` : `${(n / 1024 / 1024).toFixed(1)} MB`);
const reveal = (path: string) => invoke("reveal", { path });

function message(text: string, ok: boolean) {
  const m = $("saves-msg");
  m.textContent = text;
  m.className = `small ${ok ? "ok" : "bad"}`;
}

async function confirm(text: string, danger = false): Promise<boolean> {
  return ask(text, { title: t("saves.confirm"), kind: danger ? "warning" : "info" });
}

function safetyNote(r: Report): string {
  return r.backup ? t("saves.doneSafety", { id: r.backup.id }) : "";
}

/** Run one operation: buttons off meanwhile, the result (or error) shown, the lists refreshed. */
async function run(op: () => Promise<string>) {
  if (working) return;
  working = true;
  render();
  try {
    message(await op(), true);
  } catch (e) {
    message(String(e), false);
  }
  working = false;
  await refresh();
}

export async function refresh() {
  try {
    ov = await invoke<Overview>("saves_overview");
  } catch (e) {
    message(String(e), false);
  }
  if (ov && !ov.rpcs3Users.includes(user)) {
    user = ov.rpcs3Users.includes("00000001") ? "00000001" : ov.rpcs3Users[0] ?? "";
  }
  render();
}

function compare(s: Slot, others: Slot[], otherName: string): { text: string; cls: string } {
  const o = others.find((x) => x.name === s.name);
  if (!o) return { text: t("saves.only"), cls: "pill warn" };
  if (s.stateHash && s.stateHash === o.stateHash) return { text: t("saves.same", { other: otherName }), cls: "pill" };
  return s.modified >= o.modified
    ? { text: t("saves.newer", { other: otherName }), cls: "pill accent" }
    : { text: t("saves.older", { other: otherName }), cls: "pill muted-pill" };
}

function button(label: Key, onClick: () => void, cls = ""): HTMLButtonElement {
  const b = el("button", cls, t(label));
  b.disabled = working || !!ov?.gameRunning;
  b.addEventListener("click", onClick);
  return b;
}

function slotCard(s: Slot, others: Slot[]): HTMLElement {
  const card = el("div", "slot");
  if (s.icon) {
    const img = el("img", "slot-icon");
    img.src = s.icon;
    img.alt = "";
    card.appendChild(img);
  }
  const body = el("div", "slot-body");
  body.appendChild(el("strong", "", s.subtitle || s.title || s.name));
  const detail = s.detail.split("\n").filter(Boolean).join(" · ");
  if (detail) body.appendChild(el("div", "", detail));
  const meta = el("div", "small muted");
  meta.textContent = [date(s.modified), kb(s.size), t("saves.files", { n: s.files }), s.name].join(" · ");
  body.appendChild(meta);
  const cmp = compare(s, others, t(s.side === "port" ? "saves.rpcs3" : "saves.otherPort"));
  const pill = el("span", cmp.cls, cmp.text);
  body.appendChild(pill);

  const acts = el("div", "row slot-actions");
  if (s.side === "port") {
    if (ov?.rpcs3Users.length) acts.appendChild(button("saves.toRpcs3", () => exportToRpcs3(s)));
    acts.appendChild(button("saves.backup", () => backup("port", s.user, s.name)));
    acts.appendChild(button("saves.zip", () => exportZip([s.name])));
    acts.appendChild(button("saves.delete", () => deleteSlot(s), "link danger-link"));
  } else {
    acts.appendChild(button("saves.toPort", () => importFromRpcs3(s)));
    acts.appendChild(button("saves.backup", () => backup("rpcs3", s.user, s.name)));
  }
  const show = el("button", "link", t("saves.show"));
  show.addEventListener("click", () => reveal(s.path));
  acts.appendChild(show);
  body.appendChild(acts);
  card.appendChild(body);
  return card;
}

function backupItem(b: BackupMeta): HTMLElement {
  const li = el("li", "backup");
  const what = el("div", "what");
  const reason = t(`saves.reason.${b.reason}` as Key);
  const source = b.source === "port" ? t("saves.port") : `${t("saves.rpcs3")} · ${t("saves.user")} ${b.user}`;
  what.appendChild(el("b", "", `${date(b.created)} · ${reason.startsWith("saves.") ? b.reason : reason}`));
  const labels = b.slots.map((s) => b.labels?.[s] || s);
  const extra = [source, kb(b.size)];
  if (b.trophies) extra.push(t("saves.withTrophies"));
  what.appendChild(el("span", "msg", `${labels.join(", ")} — ${extra.join(" · ")}`));
  li.appendChild(what);
  const acts = el("div", "row");
  if (b.slots.length || b.trophies) acts.appendChild(button("saves.restore", () => restore(b)));
  const show = el("button", "link", t("saves.show"));
  show.addEventListener("click", () => reveal(`${ov?.backupsDir}/${b.id}`));
  acts.appendChild(show);
  acts.appendChild(button("saves.delete", () => deleteBackup(b), "link danger-link"));
  li.appendChild(acts);
  return li;
}

export function render() {
  if (!ov || !host) return;
  const busy = ov.gameRunning;
  $("saves-busy").classList.toggle("hidden", !busy);
  $("saves-rpcs3-open").classList.toggle("hidden", !ov.rpcs3Running);
  for (const id of ["saves-backup-all", "saves-export-zip", "saves-import-zip"]) $<HTMLButtonElement>(id).disabled = busy || working;
  $<HTMLButtonElement>("saves-export-zip").disabled ||= !ov.port.length;

  const rp = ov.rpcs3.filter((s) => s.user === user);
  const port = $("saves-port");
  port.innerHTML = "";
  if (!ov.port.length) port.appendChild(el("p", "muted", t("saves.none")));
  for (const s of ov.port) port.appendChild(slotCard(s, rp));

  const sel = $<HTMLSelectElement>("saves-user");
  sel.innerHTML = "";
  for (const u of ov.rpcs3Users) {
    const o = el("option", "", u);
    o.value = u;
    sel.appendChild(o);
  }
  sel.value = user;
  $("saves-user-wrap").classList.toggle("hidden", ov.rpcs3Users.length < 2);
  const r = $("saves-rpcs3");
  r.innerHTML = "";
  if (!ov.rpcs3Users.length) r.appendChild(el("p", "muted", t("saves.noRpcs3", { dir: ov.rpcs3Dir })));
  else if (!rp.length) r.appendChild(el("p", "muted", t("saves.none")));
  for (const s of rp) r.appendChild(slotCard(s, ov.port));

  const list = $("saves-backups");
  list.innerHTML = "";
  if (!ov.backups.length) list.appendChild(el("li", "muted", t("saves.noBackups")));
  for (const b of ov.backups) list.appendChild(backupItem(b));
  $<HTMLInputElement>("saves-keep").value = String(host.keep());
}

// ---- operations ---------------------------------------------------------------
function backup(side: Side, u: string, slot: string | null) {
  return run(async () => {
    const b = await invoke<BackupMeta | null>("saves_backup", { side, user: u, slot });
    return b ? t("saves.doneBackup", { id: b.id }) : t("saves.doneNothing");
  });
}

async function exportToRpcs3(s: Slot) {
  let text = t("saves.askToRpcs3", { slot: s.subtitle || s.name, user });
  if (ov?.rpcs3Running) text += `\n\n${t("saves.rpcs3Open")}`;
  if (!(await confirm(text, true))) return;
  return run(async () => {
    const r = await invoke<Report>("saves_export_rpcs3", { slot: s.name, user });
    return t("saves.doneCopied", { slots: r.slots.join(", ") }) + safetyNote(r);
  });
}

async function importFromRpcs3(s: Slot) {
  if (!(await confirm(t("saves.askToPort", { slot: s.subtitle || s.name, user: s.user }), true))) return;
  return run(async () => {
    const r = await invoke<Report>("saves_import_rpcs3", { user: s.user, slot: s.name });
    return t("saves.doneCopied", { slots: r.slots.join(", ") }) + safetyNote(r);
  });
}

async function deleteSlot(s: Slot) {
  const side = s.side === "port" ? t("saves.port") : t("saves.rpcs3");
  if (!(await confirm(t("saves.askDelete", { slot: s.subtitle || s.name, side }), true))) return;
  return run(async () => {
    const b = await invoke<BackupMeta>("saves_delete_slot", { side: s.side, user: s.user, slot: s.name });
    return t("saves.doneDeleted", { id: b.id });
  });
}

async function restore(b: BackupMeta) {
  const slots = b.slots.map((s) => b.labels?.[s] || s).join(", ");
  if (!(await confirm(t("saves.askRestore", { date: date(b.created), slots }), true))) return;
  return run(async () => {
    const r = await invoke<Report>("saves_restore", { id: b.id, slot: null });
    return t("saves.doneRestored", { slots: r.slots.join(", ") }) + safetyNote(r);
  });
}

async function deleteBackup(b: BackupMeta) {
  if (!(await confirm(t("saves.askDeleteBackup", { date: date(b.created) }), true))) return;
  return run(async () => {
    await invoke("saves_delete_backup", { id: b.id });
    return t("saves.doneDeletedBackup");
  });
}

async function exportZip(slots: string[] | null) {
  const day = new Date().toISOString().slice(0, 10);
  const dest = await save({
    defaultPath: `infamous2-saves-${day}.zip`,
    filters: [{ name: "ZIP", extensions: ["zip"] }],
  });
  if (!dest) return;
  return run(async () => {
    const file = await invoke<string>("saves_export_zip", { slots, dest });
    return t("saves.doneExported", { file });
  });
}

async function importZip() {
  const file = await open({ filters: [{ name: "ZIP", extensions: ["zip", "ZIP"] }] });
  if (typeof file !== "string") return;
  if (!(await confirm(t("saves.askImportZip", { file }), true))) return;
  return run(async () => {
    const r = await invoke<Report>("saves_import_zip", { path: file });
    return t("saves.doneCopied", { slots: r.slots.join(", ") }) + safetyNote(r);
  });
}

export function initSaves(h: SavesHost) {
  host = h;
  $("saves-backup-all").addEventListener("click", () => backup("port", "00000001", null));
  $("saves-export-zip").addEventListener("click", () => exportZip(null));
  $("saves-import-zip").addEventListener("click", importZip);
  $("saves-refresh").addEventListener("click", refresh);
  $("saves-open-port").addEventListener("click", () => ov && reveal(ov.portDir));
  $("saves-open-rpcs3").addEventListener("click", () => ov && reveal(user ? `${ov.rpcs3Dir}/${user}/savedata` : ov.rpcs3Dir));
  $("saves-open-backups").addEventListener("click", () => ov && reveal(ov.backupsDir));
  $<HTMLSelectElement>("saves-user").addEventListener("change", (e) => {
    user = (e.target as HTMLSelectElement).value;
    render();
  });
  const keep = $<HTMLInputElement>("saves-keep");
  keep.addEventListener("change", () => {
    const n = Math.max(0, Math.min(100, Math.round(Number(keep.value) || 0)));
    keep.value = String(n);
    host.setKeep(n);
  });
}
