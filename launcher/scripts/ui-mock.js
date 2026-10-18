// A fake Tauri backend so the built frontend can be looked at in a plain
// browser (screenshots in docs/screenshots). Injected before the page loads:
//
//   npm run build && npx vite preview --port 4173
//   # then, e.g. with Playwright: page.addInitScript({ path: "scripts/ui-mock.js" })
//
// It answers the commands the window calls with an installed game, a save and
// a log, and no cover art (the art only ever comes from the player's copy).
// window.__UI_MOCK__ = { notInstalled: true } or { running: true }, set by an
// earlier init script, shows those states instead.
(() => {
  const flags = window.__UI_MOCK__ || {};
  const settings = {
    uiLang: "pt-BR", installDir: "/Users/player/Library/Application Support/infamous2-recomp/install",
    gameLang: "pt", gpuLight: true, gpuSsao: true, skipOcclusion: true, gammaOverride: false, gamma: 1.0,
    aniso: 8, aa: "fxaa", sharpen: 0.35, upscale: "metalfx", mouse: true, mouseSens: 1.25, mouseInvertY: false,
    mouseInvertX: false, mouseDeadzone: 30, mouseCapture: true, windowMode: "windowed", windowSize: "1920x1080",
    gameIcon: false, display: "", vsync: true, audioVolume: 85, audioDevice: "", rumble: true, rumbleStrength: 100,
    showFps: false, extraEnv: "", gamePaths: ["/Users/player/Library/Application Support/rpcs3/dev_hdd0/game/NPUA80638"],
    rapPath: "/Users/player/Library/Application Support/rpcs3/dev_hdd0/home/00000001/exdata/UP9000-NPUA80638_00-XXXXXXXXXXXXXXXX.rap",
    keysPath: "", firmwarePath: "", decryptWith: "auto", saveBackupsKeep: 10,
  };
  const now = Math.floor(Date.now() / 1000);
  const install = {
    layout: "install", title: "inFAMOUS 2", appVer: "01.03", runSh: "/x/run.sh", executable: "/x/Infamous2Recomp",
    builtAt: new Date((now - 86400) * 1000).toISOString(), portCommit: "89216bb",
  };
  const save = (side, user, name, sub, detail, mod) => ({
    side, user, name, path: `/saves/${name}`, title: "inFAMOUS 2", subtitle: sub, detail, modified: mod,
    size: 412000, files: 4, icon: null, stateHash: "abc", sfo: side === "port" ? "text" : "binary",
  });
  const log = [
    "[run.sh] PS3_LANG=pt PS3_UPSCALE=metalfx PS3_AA=fxaa",
    "[boot] Infamous2Recomp starting (port 89216bb)",
    "[vfs] mounted /dev_hdd0, /dev_flash",
    "[rsx] Metal device: Apple M4 Pro",
    "[rsx] MetalFX spatial scaler 1280x720 -> 1920x1080",
    "[spurs] 5 SPU threads, light pass on the GPU, SSAO on the GPU",
    "[cellSaveData] loaded NPUA80638-AUTO-",
    "[warn] cellGame: stub cellGameGetParamInt(id=5)",
    "[RSX FPS] display 30.0 game 30.0 frame 33.3 ms",
    "[RSX FPS] display 30.0 game 29.9 frame 33.5 ms",
    "[error] cellAudio: underrun on port 0 (recovered)",
    "[RSX FPS] display 30.0 game 30.0 frame 33.2 ms",
  ].join("\n") + "\n";
  const answers = {
    get_settings: () => settings,
    save_settings: () => null,
    get_status: () => ({
      installDir: settings.installDir, install: flags.notInstalled ? null : install, installError: null, icon: null, background: null,
      lastSave: { title: "inFAMOUS 2", subtitle: "(Auto) 7. Sangue no gelo", detail: "Progresso: 34%\nKarma: Herói", modified: now - 3600 * 5, folder: "/saves/x" },
      gameRunning: !!flags.running, rebuildRunning: false, rebuildScript: "/x/rebuild.py", dataRoot: "/x", defaultInstallDir: settings.installDir,
      hostSupported: true, os: "macos",
    }),
    check_updates: () => ({
      appVersion: "0.1.0", sourcesCommit: "89216bb", installedCommit: "89216bb", rebuildSuggested: false,
      checkedOnline: true, latestVersion: "0.1.0", latestUrl: null, newerRelease: false, checkError: null,
    }),
    list_outputs: () => ({ screens: ["Tela Retina integrada", "DELL U2720Q"], audioOutputs: ["Alto-falantes (MacBook Pro)", "AirPods Pro"] }),
    get_input_map: () => ({ path: "/Users/player/Library/Application Support/infamous2-recomp/input.toml", exists: true, bindings: { keyboard: {}, mouseButtons: {}, controller: {} } }),
    save_input_map: () => "/x/input.toml",
    report_info: () => (flags.running
      ? { dir: "/x/reports/20261018T130000Z", id: "20261018T130000Z", live: true, captures: 2, pending: false, surfaces: 412, shaders: 96, bytes: 512000000, zip: null, zipBytes: 0 }
      : null),
    check_prereqs: () => ({
      os: "macos", arch: "aarch64", supported: true, clt: true, homebrew: "/opt/homebrew/bin/brew", script: "/x/rebuild.py",
      checks: ["host", "xcode-clt", "python", "cmake", "ninja", "sdl2", "glslang", "spirv-cross", "rpcs3", "firmware", "disk"].map((n) => ({ level: "ok", name: n, message: "ok" })),
      brewMissing: [], rpcs3Missing: false, firmwareMissing: false, ok: true, error: null,
    }),
    detect_rpcs3: () => ({ gameDir: settings.gamePaths[0], title: "inFAMOUS 2", appVer: "01.03", contentId: "UP9000-NPUA80638_00", raps: [settings.rapPath], searched: [] }),
    find_licence: () => answers.detect_rpcs3(),
    saves_overview: () => ({
      port: [save("port", "00000001", "NPUA80638-AUTO-", "(Auto) 7. Sangue no gelo", "Progresso: 34%", now - 3600 * 5)],
      rpcs3: [save("rpcs3", "00000001", "NPUA80638-AUTO-", "(Auto) 5. A chegada", "Progresso: 21%", now - 86400 * 9)],
      rpcs3Users: ["00000001"],
      backups: [{ id: "20261018T101500Z-port-before-play", created: now - 3600 * 6, reason: "before-play", source: "port", user: "00000001", slots: ["NPUA80638-AUTO-"], trophies: true, size: 523000, labels: { "NPUA80638-AUTO-": "(Auto) 7. Sangue no gelo" } }],
      portDir: "/x/savedata", rpcs3Dir: "/x/rpcs3/home", backupsDir: "/x/save-backups", gameRunning: false, rpcs3Running: false,
    }),
    launch_preview: () => "INFAMOUS2_VFS=/x/vfs PS3_LANG=pt PS3_ANISO=8 PS3_AA=fxaa PS3_SHARPEN=0.35 PS3_UPSCALE=metalfx PS3_WINDOW_SIZE=1920x1080 PS3_AUDIO_VOLUME=85 /x/run.sh",
    read_game_log: () => ({ path: "/Users/player/Library/Application Support/infamous2-recomp/logs/game.log", exists: true, size: log.length, modified: now - 120, text: log, truncated: false }),
    log_paths: () => ({ game: "/x/game.log", saves: "/x/gamedata" }),
  };
  let cb = 0;
  window.__TAURI_INTERNALS__ = {
    invoke: async (cmd, args) => {
      if (cmd.startsWith("plugin:event|")) return cmd.endsWith("listen") ? ++cb : null;
      if (cmd.startsWith("plugin:dialog|")) return null;
      if (cmd === "save_settings") Object.assign(settings, args.settings);
      const f = answers[cmd];
      if (!f) {
        console.warn("ui-mock: no answer for", cmd, args);
        return null;
      }
      return f(args);
    },
    transformCallback: () => ++cb,
    unregisterCallback: () => {},
    convertFileSrc: (p) => p,
    metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
})();
