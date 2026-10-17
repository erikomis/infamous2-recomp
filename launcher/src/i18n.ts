// UI strings. pt-BR is the default; English is the fallback for missing keys.

export type Lang = "pt-BR" | "en";

const en = {
  "nav.home": "Home",
  "nav.install": "Install",
  "nav.settings": "Settings",
  "app.lang": "App language",

  "home.notInstalled": "The game is not installed yet",
  "home.notInstalledHint":
    "The app ships no game files. Point it at your own copy (RPCS3 folder or PKG) and your .rap, and it rebuilds the game on this computer.",
  "home.goInstall": "Install from my copy",
  "home.play": "Play",
  "home.stop": "Stop",
  "home.running": "Running",
  "home.version": "Version",
  "home.builtAt": "Built",
  "home.devTree": "development checkout",
  "home.lastSave": "Last save",
  "home.noSave": "No save yet",
  "home.openSaves": "Open saves folder",
  "home.openLog": "Open game log",
  "home.exited": "The game closed (code {code}).",
  "home.started": "Starting… log: {log}",

  "install.title": "Install / rebuild",
  "install.lead":
    "Decrypts your copy, translates the PS3 code (PPU, SPU, jobs) and compiles it here. The first run takes several minutes; later runs only redo what changed.",
  "install.game": "Game copy",
  "install.gameHint": "RPCS3's NPUA80638 folder, or the PKG files (base game first, then updates).",
  "install.pickFolder": "Folder…",
  "install.pickPkg": "PKG…",
  "install.detect": "Find in RPCS3",
  "install.detected": "Found in RPCS3.",
  "install.notDetected": "RPCS3 install of NPUA80638 not found.",
  "install.rap": "License (.rap)",
  "install.pick": "Choose…",
  "install.out": "Install folder",
  "install.rapKey": "RAP key (optional, DLC only)",
  "install.rapKeyHint":
    "Only to unlock the bonus DLC. Not stored; passed to the script for this run. The base game does not need it.",
  "install.force": "Redo every step",
  "install.check": "Check",
  "install.start": "Install",
  "install.cancel": "Cancel",
  "install.copyLog": "Copy log",
  "install.checks": "Requirements",
  "install.steps": "Steps",
  "install.log": "Log",
  "install.eta": "about {min} min left",
  "install.done": "Done. The game is ready to play.",
  "install.dryOk": "Everything needed is in place. Nothing was changed.",
  "install.failed": "Stopped with an error ({code}). See the log.",
  "install.cancelled": "Cancelled.",
  "install.missing": "Some requirements are missing (see above).",
  "install.noScript": "rebuild.py was not found. Set INFAMOUS2_REPO to the repository folder.",
  "install.needInputs": "Choose the game copy and the .rap first.",

  "state.run": "pending",
  "state.up-to-date": "up to date",
  "state.skip": "skipped",
  "state.running": "running",
  "state.done": "done",
  "state.error": "error",

  "step.toolkit": "Toolkit (ps3recomp + patches)",
  "step.venv": "Python environment",
  "step.game": "Import the game copy",
  "step.decrypt": "Decrypt (RPCS3)",
  "step.port": "Port sources",
  "step.firmware": "Translate the firmware",
  "step.ppu": "Translate the PPU code",
  "step.spu": "Translate the SPU programs",
  "step.jobs": "Translate the SPU jobs",
  "step.dlc": "DLC license (optional)",
  "step.compile": "Compile",
  "step.finish": "Finish",

  "settings.title": "Settings",
  "settings.lead": "Applied the next time the game starts. Saved automatically.",
  "settings.game": "Game",
  "settings.gameLang": "Dub, subtitles and menus",
  "settings.gameLangHint": "Only languages your copy ships. The game reads it at startup.",
  "settings.graphics": "Graphics",
  "settings.gpuLight": "Light pass on the GPU",
  "settings.gpuLightHint": "Faster. Off runs it on the emulated SPUs, like the console.",
  "settings.gpuSsao": "Ambient occlusion (SSAO) on the GPU",
  "settings.gpuSsaoHint": "Faster (needs the light pass on the GPU).",
  "settings.skipOcc": "Skip occlusion-volume passes",
  "settings.skipOccHint": "Avoids facades (and Cole) turning black at dusk; loses only soft contact shadows.",
  "settings.gamma": "Override brightness (gamma)",
  "settings.gammaHint": "Off: the game's own brightness option is used, as on the console.",
  "settings.display": "Display mode",
  "settings.windowed": "Window",
  "settings.displayHint": "A dedicated fullscreen mode is not supported yet; use the window's green button for macOS fullscreen.",
  "settings.mouse": "Mouse",
  "settings.mouseOn": "Mouse moves the camera",
  "settings.mouseSens": "Sensitivity",
  "settings.mouseInvert": "Invert vertical axis",
  "settings.mouseDeadzone": "Dead zone",
  "settings.advanced": "Advanced",
  "settings.installDir": "Install folder",
  "settings.installDirHint": "Folder with install.json, or the root of a development checkout.",
  "settings.extraEnv": "Extra environment variables",
  "settings.extraEnvHint": "KEY=VALUE, one per line (diagnostics). '#' comments a line.",
  "settings.preview": "Equivalent command",
  "settings.reset": "Restore defaults",
  "settings.saved": "Saved",
};

export type Key = keyof typeof en;

const ptBR: Partial<Record<Key, string>> = {
  "nav.home": "Início",
  "nav.install": "Instalar",
  "nav.settings": "Configurações",
  "app.lang": "Idioma do app",

  "home.notInstalled": "O jogo ainda não está instalado",
  "home.notInstalledHint":
    "O app não traz nenhum arquivo do jogo. Aponte para a sua cópia (pasta do RPCS3 ou PKG) e o seu .rap, e ele recompila o jogo neste computador.",
  "home.goInstall": "Instalar a partir da minha cópia",
  "home.play": "Jogar",
  "home.stop": "Parar",
  "home.running": "Em execução",
  "home.version": "Versão",
  "home.builtAt": "Compilado em",
  "home.devTree": "cópia de desenvolvimento",
  "home.lastSave": "Último save",
  "home.noSave": "Nenhum save ainda",
  "home.openSaves": "Abrir pasta de saves",
  "home.openLog": "Abrir log do jogo",
  "home.exited": "O jogo fechou (código {code}).",
  "home.started": "Iniciando… log: {log}",

  "install.title": "Instalar / recompilar",
  "install.lead":
    "Descriptografa a sua cópia, traduz o código do PS3 (PPU, SPU, jobs) e compila aqui. A primeira vez leva vários minutos; depois só refaz o que mudou.",
  "install.game": "Cópia do jogo",
  "install.gameHint": "A pasta NPUA80638 do RPCS3, ou os arquivos PKG (jogo base primeiro, depois as atualizações).",
  "install.pickFolder": "Pasta…",
  "install.pickPkg": "PKG…",
  "install.detect": "Procurar no RPCS3",
  "install.detected": "Encontrado no RPCS3.",
  "install.notDetected": "Instalação do NPUA80638 no RPCS3 não encontrada.",
  "install.rap": "Licença (.rap)",
  "install.pick": "Escolher…",
  "install.out": "Pasta de instalação",
  "install.rapKey": "Chave RAP (opcional, só para o DLC)",
  "install.rapKeyHint":
    "Só para liberar o DLC bônus. Não é salva; vai para o script só nesta execução. O jogo base não precisa dela.",
  "install.force": "Refazer todas as etapas",
  "install.check": "Verificar",
  "install.start": "Instalar",
  "install.cancel": "Cancelar",
  "install.copyLog": "Copiar log",
  "install.checks": "Requisitos",
  "install.steps": "Etapas",
  "install.log": "Log",
  "install.eta": "faltam cerca de {min} min",
  "install.done": "Pronto. O jogo está pronto para jogar.",
  "install.dryOk": "Está tudo no lugar. Nada foi alterado.",
  "install.failed": "Parou com erro ({code}). Veja o log.",
  "install.cancelled": "Cancelado.",
  "install.missing": "Faltam requisitos (veja acima).",
  "install.noScript": "rebuild.py não encontrado. Defina INFAMOUS2_REPO com a pasta do repositório.",
  "install.needInputs": "Escolha primeiro a cópia do jogo e o .rap.",

  "state.run": "pendente",
  "state.up-to-date": "em dia",
  "state.skip": "pulada",
  "state.running": "executando",
  "state.done": "concluída",
  "state.error": "erro",

  "step.toolkit": "Toolkit (ps3recomp + patches)",
  "step.venv": "Ambiente Python",
  "step.game": "Importar a cópia do jogo",
  "step.decrypt": "Descriptografar (RPCS3)",
  "step.port": "Fontes do port",
  "step.firmware": "Traduzir o firmware",
  "step.ppu": "Traduzir o código PPU",
  "step.spu": "Traduzir os programas SPU",
  "step.jobs": "Traduzir os jobs SPU",
  "step.dlc": "Licença do DLC (opcional)",
  "step.compile": "Compilar",
  "step.finish": "Finalizar",

  "settings.title": "Configurações",
  "settings.lead": "Valem na próxima vez que o jogo abrir. Salvas automaticamente.",
  "settings.game": "Jogo",
  "settings.gameLang": "Dublagem, legendas e menus",
  "settings.gameLangHint": "Só os idiomas que a sua cópia traz. O jogo lê na inicialização.",
  "settings.graphics": "Gráficos",
  "settings.gpuLight": "Iluminação na GPU",
  "settings.gpuLightHint": "Mais rápido. Desligado, roda nas SPUs emuladas, como no console.",
  "settings.gpuSsao": "Oclusão de ambiente (SSAO) na GPU",
  "settings.gpuSsaoHint": "Mais rápido (precisa da iluminação na GPU).",
  "settings.skipOcc": "Pular os passes de volume de oclusão",
  "settings.skipOccHint": "Evita fachadas (e o Cole) pretas ao entardecer; perde só a sombra suave de contato.",
  "settings.gamma": "Forçar brilho (gama)",
  "settings.gammaHint": "Desligado: vale a opção de brilho do próprio jogo, como no console.",
  "settings.display": "Modo de exibição",
  "settings.windowed": "Janela",
  "settings.displayHint": "Tela cheia dedicada ainda não é suportada; use o botão verde da janela para a tela cheia do macOS.",
  "settings.mouse": "Mouse",
  "settings.mouseOn": "Mouse move a câmera",
  "settings.mouseSens": "Sensibilidade",
  "settings.mouseInvert": "Inverter eixo vertical",
  "settings.mouseDeadzone": "Zona morta",
  "settings.advanced": "Avançado",
  "settings.installDir": "Pasta de instalação",
  "settings.installDirHint": "Pasta com o install.json, ou a raiz de uma cópia de desenvolvimento.",
  "settings.extraEnv": "Variáveis de ambiente extras",
  "settings.extraEnvHint": "CHAVE=VALOR, uma por linha (diagnóstico). '#' comenta a linha.",
  "settings.preview": "Comando equivalente",
  "settings.reset": "Restaurar padrões",
  "settings.saved": "Salvo",
};

let current: Lang = "pt-BR";

export function setLang(l: Lang) {
  current = l === "en" ? "en" : "pt-BR";
  document.documentElement.lang = current;
}

export function lang(): Lang {
  return current;
}

export function t(key: Key, vars: Record<string, string | number> = {}): string {
  const s = (current === "pt-BR" ? ptBR[key] : undefined) ?? en[key] ?? key;
  return s.replace(/\{(\w+)\}/g, (_, k) => String(vars[k] ?? ""));
}

/** Fill every element carrying data-i18n="key". */
export function translateDom(root: ParentNode = document) {
  root.querySelectorAll<HTMLElement>("[data-i18n]").forEach((el) => {
    el.textContent = t(el.dataset.i18n as Key);
  });
}

/** Game languages (PS3_LANG). "pt" is the one that selects the Brazilian dub
 * on this Americas release; see infamous2-lle/run.sh. */
export const GAME_LANGS: { code: string; pt: string; en: string }[] = [
  { code: "pt", pt: "Português (Brasil)", en: "Portuguese (Brazil)" },
  { code: "en", pt: "Inglês (EUA)", en: "English (US)" },
  { code: "en-gb", pt: "Inglês (Reino Unido)", en: "English (UK)" },
  { code: "es", pt: "Espanhol", en: "Spanish" },
  { code: "fr", pt: "Francês", en: "French" },
  { code: "de", pt: "Alemão", en: "German" },
  { code: "it", pt: "Italiano", en: "Italian" },
  { code: "nl", pt: "Holandês", en: "Dutch" },
  { code: "pl", pt: "Polonês", en: "Polish" },
  { code: "ru", pt: "Russo", en: "Russian" },
  { code: "tr", pt: "Turco", en: "Turkish" },
  { code: "ja", pt: "Japonês", en: "Japanese" },
  { code: "ko", pt: "Coreano (legendas)", en: "Korean (subtitles)" },
  { code: "zh-t", pt: "Chinês tradicional (legendas)", en: "Chinese, traditional (subtitles)" },
];
