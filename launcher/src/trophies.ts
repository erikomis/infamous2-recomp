// Game page: the game's trophies (names and icons from the player's own
// TROPHY.TRP, unlock times from the port's gamedata), read by the backend.
import { invoke } from "@tauri-apps/api/core";
import { Key, lang, t } from "./i18n";

interface Trophy {
  id: number;
  grade: string;
  name: string;
  detail: string;
  hidden: boolean;
  unlockedAt: number;
  icon: string;
}
interface TrophyList {
  commId: string;
  title: string;
  trophies: Trophy[];
}

let last: TrophyList | null = null;
let error = "";

const GRADE: Record<string, Key> = { P: "trophies.platinum", G: "trophies.gold", S: "trophies.silver", B: "trophies.bronze" };

function el<K extends keyof HTMLElementTagNameMap>(tag: K, cls?: string, text?: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined) e.textContent = text;
  return e;
}

export function renderTrophies() {
  const list = document.getElementById("trophy-list");
  const sum = document.getElementById("trophy-summary");
  if (!list || !sum) return;
  list.replaceChildren();
  if (!last) {
    sum.textContent = error ? t("trophies.none") : "";
    return;
  }
  const got = last.trophies.filter((x) => x.unlockedAt > 0).length;
  sum.textContent = t("trophies.summary", { got, total: last.trophies.length });
  const sorted = [...last.trophies].sort((a, b) => (b.unlockedAt > 0 ? 1 : 0) - (a.unlockedAt > 0 ? 1 : 0) || a.id - b.id);
  for (const x of sorted) {
    const row = el("div", "trophy" + (x.unlockedAt > 0 ? " got" : ""));
    const hide = x.hidden && x.unlockedAt === 0;
    const img = el("img");
    img.alt = "";
    if (x.icon) img.src = x.icon;
    row.append(img);
    const txt = el("div", "what");
    txt.append(el("b", "", hide ? t("trophies.hidden") : x.name));
    txt.append(el("span", "small muted", hide ? "" : x.detail));
    row.append(txt);
    const meta = el("div", "meta small");
    meta.append(el("span", "grade grade-" + x.grade, GRADE[x.grade] ? t(GRADE[x.grade]) : x.grade));
    if (x.unlockedAt > 0) meta.append(el("span", "muted", new Date(x.unlockedAt * 1000).toLocaleDateString(lang() === "pt-BR" ? "pt-BR" : "en-US")));
    row.append(meta);
    list.append(row);
  }
}

export async function refreshTrophies() {
  try {
    last = await invoke<TrophyList>("list_trophies");
    error = "";
  } catch (e) {
    last = null;
    error = String(e);
  }
  renderTrophies();
}

export function initTrophies() {
  document.getElementById("trophy-refresh")?.addEventListener("click", () => void refreshTrophies());
  void refreshTrophies();
}
