// UI strings. pt-BR is the default; English is the fallback for missing keys.

export type Lang = "pt-BR" | "en";

const en = {
  "nav.home": "Home",
  "nav.install": "Install",
  "nav.settings": "Settings",
  "app.lang": "App language",

  "home.notInstalled": "The game is not installed yet",
  "home.notInstalledHint":
    "The app ships no game files. Pick your own copy (RPCS3's game folder) and it rebuilds the game on this computer.",
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

  "unsupported.title": "This system cannot build or play the game yet",
  "unsupported.body":
    "The port runs only on macOS (Apple Silicon, Metal) for now. Windows (Direct3D 12) and Linux (Vulkan) are planned; this app already installs here so it is ready when they land. Settings can be edited meanwhile.",
  "unsupported.follow": "Follow the project",

  "prereq.title": "What this computer needs",
  "prereq.lead":
    "Free tools that turn your copy into a native build. The app can install what is missing (with your OK); RPCS3 and the PS3 firmware you get from their official sites.",
  "prereq.checking": "Checking…",
  "prereq.error": "Could not run the checks: {e}",
  "prereq.allOk": "all set",
  "prereq.missingN": "{n} missing",
  "prereq.fix": "Install what is missing",
  "prereq.fixBrew": "Install with Homebrew: {pkgs}",
  "prereq.recheck": "Check again",
  "prereq.installClt": "Install the Command Line Tools",
  "prereq.askClt":
    "macOS will open Apple's installer for the Xcode Command Line Tools (compiler, git, Python; about 1 GB, no Apple ID needed). Continue?",
  "prereq.cltStarted": "Follow Apple's installer window. This screen updates by itself when it finishes.",
  "prereq.askBrew": "This runs \"brew install {pkgs}\" in the background. It can take a few minutes. Continue?",
  "prereq.brewRunning": "Installing with Homebrew…",
  "prereq.brewDone": "Installed.",
  "prereq.brewFailed": "Homebrew stopped with an error ({code}); see the output above.",
  "prereq.noBrew":
    "The build tools (CMake, Ninja, SDL2, glslang, SPIRV-Cross, Python) come from Homebrew, which is not installed. Install it with this command in Terminal, then click Check again:",
  "prereq.openBrew": "Open brew.sh",
  "prereq.copyCmd": "Copy the command",
  "prereq.getRpcs3": "Download RPCS3",
  "prereq.howFirmware": "How to install the firmware",
  "prereq.hint.rpcs3":
    "RPCS3 (free PS3 emulator) is used once, to decrypt your copy with your licence. Install it in Applications. It is not bundled with this app.",
  "prereq.hint.firmware":
    "Download the PS3 system software (PS3UPDAT.PUP) from playstation.com and install it in RPCS3: File > Install Firmware.",
  "prereq.hint.clt": "Apple's free compiler package.",
  "prereq.hint.disk": "Free some space on this disk, or choose another install folder.",
  "prereq.name.host": "System",
  "prereq.name.xcode-clt": "Xcode Command Line Tools",
  "prereq.name.python": "Python 3.10+",
  "prereq.name.cmake": "CMake",
  "prereq.name.ninja": "Ninja",
  "prereq.name.compiler": "C/C++ compiler",
  "prereq.name.git": "git",
  "prereq.name.sdl2": "SDL2",
  "prereq.name.glslang": "glslang",
  "prereq.name.spirv-cross": "SPIRV-Cross",
  "prereq.name.rpcs3": "RPCS3",
  "prereq.name.firmware": "PS3 firmware (in RPCS3)",
  "prereq.name.disk": "Disk space",
  "prereq.name.patch": "Toolkit patch",
  "prereq.name.toolkit": "Toolkit",

  "game.title": "Your copy of the game",
  "game.lead":
    "Pick the game's folder: RPCS3's NPUA80638 folder (or RPCS3's data folder). The licence (.rap) is found on its own when it is in RPCS3, next to the copy or in Downloads.",
  "game.pick": "Choose the game folder…",
  "game.more": "More options (PKG files, licence, install folder, DLC)",
  "game.pkgs": "{n} PKG file(s)",
  "game.notGame": "No inFAMOUS 2 (NPUA80638) in this folder",
  "game.rapFound": "Licence: {rap}",
  "game.rapMissing": "Licence {cid}.rap not found: choose it under More options.",

  "run.title": "Install",
  "run.lead": "Builds the game on this computer. It takes a while the first time; the app keeps working meanwhile.",
  "install.rapHint": "Your licence file. Filled in automatically when it is found.",
  "install.askMissing": "Some requirements are still missing, so the install will probably stop early. Start anyway?",
};

export type Key = keyof typeof en;

const ptBR: Partial<Record<Key, string>> = {
  "nav.home": "Início",
  "nav.install": "Instalar",
  "nav.settings": "Configurações",
  "app.lang": "Idioma do app",

  "home.notInstalled": "O jogo ainda não está instalado",
  "home.notInstalledHint":
    "O app não traz nenhum arquivo do jogo. Escolha a sua cópia (a pasta do jogo no RPCS3) e ele recompila o jogo neste computador.",
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

  "unsupported.title": "Este sistema ainda não compila nem roda o jogo",
  "unsupported.body":
    "Por enquanto o port roda só no macOS (Apple Silicon, Metal). Windows (Direct3D 12) e Linux (Vulkan) estão nos planos; o app já instala aqui para estar pronto quando chegarem. Dá para editar as configurações enquanto isso.",
  "unsupported.follow": "Acompanhar o projeto",

  "prereq.title": "O que este computador precisa",
  "prereq.lead":
    "Ferramentas gratuitas que transformam a sua cópia num build nativo. O app instala o que faltar (com a sua permissão); o RPCS3 e o firmware do PS3 vêm dos sites oficiais.",
  "prereq.checking": "Verificando…",
  "prereq.error": "Não foi possível verificar: {e}",
  "prereq.allOk": "tudo pronto",
  "prereq.missingN": "{n} faltando",
  "prereq.fix": "Instalar o que falta",
  "prereq.fixBrew": "Instalar com o Homebrew: {pkgs}",
  "prereq.recheck": "Verificar de novo",
  "prereq.installClt": "Instalar as Command Line Tools",
  "prereq.askClt":
    "O macOS vai abrir o instalador da Apple das Xcode Command Line Tools (compilador, git, Python; cerca de 1 GB, sem Apple ID). Continuar?",
  "prereq.cltStarted": "Siga a janela do instalador da Apple. Esta tela se atualiza sozinha quando terminar.",
  "prereq.askBrew": "Isto roda \"brew install {pkgs}\" em segundo plano. Pode levar alguns minutos. Continuar?",
  "prereq.brewRunning": "Instalando com o Homebrew…",
  "prereq.brewDone": "Instalado.",
  "prereq.brewFailed": "O Homebrew parou com erro ({code}); veja a saída acima.",
  "prereq.noBrew":
    "As ferramentas de build (CMake, Ninja, SDL2, glslang, SPIRV-Cross, Python) vêm do Homebrew, que não está instalado. Instale com este comando no Terminal e depois clique em Verificar de novo:",
  "prereq.openBrew": "Abrir brew.sh",
  "prereq.copyCmd": "Copiar o comando",
  "prereq.getRpcs3": "Baixar o RPCS3",
  "prereq.howFirmware": "Como instalar o firmware",
  "prereq.hint.rpcs3":
    "O RPCS3 (emulador de PS3 gratuito) é usado uma vez, para descriptografar a sua cópia com a sua licença. Instale em Aplicativos. Ele não vem junto com este app.",
  "prereq.hint.firmware":
    "Baixe o software do sistema do PS3 (PS3UPDAT.PUP) em playstation.com e instale no RPCS3: File > Install Firmware.",
  "prereq.hint.clt": "Pacote de compilador gratuito da Apple.",
  "prereq.hint.disk": "Libere espaço neste disco ou escolha outra pasta de instalação.",
  "prereq.name.host": "Sistema",
  "prereq.name.compiler": "Compilador C/C++",
  "prereq.name.firmware": "Firmware do PS3 (no RPCS3)",
  "prereq.name.disk": "Espaço em disco",
  "prereq.name.patch": "Patch do toolkit",

  "game.title": "A sua cópia do jogo",
  "game.lead":
    "Escolha a pasta do jogo: a pasta NPUA80638 do RPCS3 (ou a pasta de dados do RPCS3). A licença (.rap) é encontrada sozinha quando está no RPCS3, junto da cópia ou em Downloads.",
  "game.pick": "Escolher a pasta do jogo…",
  "game.more": "Mais opções (arquivos PKG, licença, pasta de instalação, DLC)",
  "game.pkgs": "{n} arquivo(s) PKG",
  "game.notGame": "Nenhum inFAMOUS 2 (NPUA80638) nesta pasta",
  "game.rapFound": "Licença: {rap}",
  "game.rapMissing": "Licença {cid}.rap não encontrada: escolha em Mais opções.",

  "run.title": "Instalar",
  "run.lead": "Compila o jogo neste computador. Na primeira vez demora; o computador continua utilizável enquanto isso.",
  "install.rapHint": "O seu arquivo de licença. Preenchido sozinho quando é encontrado.",
  "install.askMissing": "Ainda faltam requisitos, então a instalação provavelmente vai parar no começo. Começar mesmo assim?",
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
