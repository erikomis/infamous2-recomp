// Parser for the machine-readable lines of infamous2-lle/tools/rebuild.py
// (see the protocol in its docstring) and the overall progress estimate.

export type StepState = "run" | "up-to-date" | "skip" | "running" | "done" | "error";

export interface Step {
  id: string;
  title: string;
  state: StepState;
  message: string;
  sub: number; // 0..1 within the step (the compile reports it)
}

export interface Check {
  level: "ok" | "warn" | "fail";
  name: string;
  message: string;
}

// Rough share of the total time of a first rebuild on an M4 Pro: the PPU
// lift and the compile dominate.
const WEIGHT: Record<string, number> = {
  toolkit: 3,
  venv: 3,
  game: 2,
  decrypt: 2,
  port: 0.2,
  firmware: 2,
  ppu: 30,
  spu: 1,
  jobs: 3,
  dlc: 0.2,
  compile: 50,
  finish: 0.1,
};

export class RebuildProgress {
  steps: Step[] = [];
  checks: Check[] = [];
  log: string[] = [];
  error: string | null = null;
  done: string | null = null;
  cancelled = false;
  private started = 0;
  private startFrac = 0;

  reset() {
    this.steps = [];
    this.checks = [];
    this.log = [];
    this.error = null;
    this.done = null;
    this.cancelled = false;
    this.started = 0;
    this.startFrac = 0;
  }

  private step(id: string): Step | undefined {
    return this.steps.find((s) => s.id === id);
  }

  feed(line: string) {
    const sp = line.indexOf(" ");
    const kind = sp < 0 ? line : line.slice(0, sp);
    const rest = sp < 0 ? "" : line.slice(sp + 1);
    const words = rest.split(" ");
    switch (kind) {
      case "LOG":
        this.log.push(rest);
        return;
      case "PLAN": {
        // PLAN i/n id state title...
        const [, id, state, ...title] = words;
        if (!this.step(id)) this.steps.push({ id, title: title.join(" "), state: state as StepState, message: "", sub: 0 });
        break;
      }
      case "CHECK": {
        const [level, name, ...msg] = words;
        this.checks.push({ level: level as Check["level"], name, message: msg.join(" ") });
        break;
      }
      case "PROGRESS": {
        const [id, , ...msg] = words;
        const s = this.step(id);
        if (s) {
          if (s.state !== "running") {
            s.state = "running";
            s.message = "";
          } else {
            s.message = msg.join(" ");
          }
          if (!this.started) {
            this.started = Date.now();
            this.startFrac = this.fraction();
          }
        }
        break;
      }
      case "SUBPROGRESS": {
        const [id, frac] = words;
        const [a, b] = frac.split("/").map(Number);
        const s = this.step(id);
        if (s && b > 0) s.sub = Math.min(1, a / b);
        break;
      }
      case "STEP-DONE": {
        const s = this.step(words[0]);
        if (s) {
          s.state = "done";
          s.sub = 1;
          s.message = "";
        }
        break;
      }
      case "STEP-SKIP": {
        const s = this.step(words[0]);
        if (s) s.state = words.slice(2).join(" ") === "not requested" ? "skip" : "up-to-date";
        break;
      }
      case "ERROR": {
        const [id, ...msg] = words;
        this.error = `${id}: ${msg.join(" ")}`;
        const s = this.step(id);
        if (s) s.state = "error";
        break;
      }
      case "DONE":
        this.done = rest;
        break;
    }
    this.log.push(line);
  }

  /** Called when the script exited with 0 after a real run. */
  finish() {
    for (const s of this.steps) if (s.state === "run" || s.state === "running") s.state = "done";
  }

  fraction(): number {
    let total = 0;
    let got = 0;
    for (const s of this.steps) {
      const w = WEIGHT[s.id] ?? 1;
      total += w;
      if (s.state === "done" || s.state === "up-to-date" || s.state === "skip") got += w;
      else if (s.state === "running") got += w * s.sub;
    }
    return total ? got / total : 0;
  }

  /** Minutes left, extrapolated from this run's own pace; null until there is one. */
  etaMinutes(): number | null {
    if (!this.started) return null;
    const f = this.fraction();
    const elapsed = (Date.now() - this.started) / 60000;
    const doneHere = f - this.startFrac;
    if (doneHere < 0.02 || elapsed < 0.2) return null;
    return (elapsed / doneHere) * (1 - f);
  }
}
