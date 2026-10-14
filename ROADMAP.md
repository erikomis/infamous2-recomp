# Roadmap

Estado do port nativo do inFAMOUS 2 (`NPUA80638`) em macOS/arm64 e o que vem
a seguir. `[x]` = feito e verificado; `[ ]` = a fazer.

## Situação atual

Menu → jogo novo → vídeo de abertura → cutscene no motor do jogo nas docas de
Empire City (Cole, Zeke, Lucy Kuo), com menu de pausa e opções funcionando.
Personagens com iluminação e cor de pele corretas. Média de ~17 fps numa
execução de 330 s que inclui o boot. Ainda não chegou ao gameplay controlável.

## Fase 1 — Boot e pipeline ✅

- [x] EBOOT descriptografado com o RPCS3 (`--decrypt`), usando a licença do próprio jogador.
- [x] PPU traduzido para C++: 32.426 funções.
- [x] Programas SPU do EBOOT: descompressão zlib, Bink, policy module `Wws_Job`.
- [x] 27 módulos de job WWS (`C0DEC0DE`) extraídos do EBOOT e traduzidos.
- [x] Jobs SPURS de áudio do Wwise capturados em execução e traduzidos.
- [x] Firmware em LLE, traduzido do `dev_flash` do jogador: liblv2, libsysmodule, libsre e o kernel SPU do SPURS.
- [x] Metal com os shaders do próprio jogo (glslang + SPIRV-Cross).
- [x] Dados do jogo isolados num clone copy-on-write; o RPCS3 nunca é escrito.

## Fase 2 — Do menu à cutscene ✅

- [x] Boot pelo `module_start` da liblv2, como no console.
- [x] FIOS, `.psarc`, descompressão em SPU.
- [x] Aviso de saúde, vídeo Bink, tela de loading.
- [x] Menu principal 3D.
- [x] Jogo novo, vídeo de abertura, cutscene no motor do jogo.
- [x] Menu de pausa e tela de opções.
- [x] Iluminação dos personagens nas cutscenes (comparação de profundidade das sombras).
- [x] Cor dos materiais de personagem: o decompilador pulava os opcodes de pack/unpack (PK4UB, UP4UB, PKG, UPG) que o jogo usa como conversão sRGB↔linear; era a mancha verde no couro cabeludo do Cole e tons de pele errados.

## Correções no toolkit ✅

Aplicadas pelo `tools/setup_toolkit.sh` a partir de
`infamous2-lle/patches/ps3recomp-fork-770660a-with-upstream-prs.patch`.

Feitas neste port:

- [x] Lifter PPU: jump tables perdidas atrás de um epílogo com tail-call.
- [x] cellGcm: o FIFO pulava comandos quando o jogo "estacionava" a GPU antes de religar o bloco; o menu congelava.
- [x] SPU: o interpretador não voltava ao código traduzido dentro dos módulos de job WWS; o menu rodava a ~0,4 fps.
- [x] Fragment shaders: opcodes de pack/unpack do NV40 (PK4UB, UP4UB, PKG, UPG, PK4B, UP4B, PK2H, UP2H, PK2US, UP2US).
- [x] `sys_lwcond_t` com layout errado (FIOS recusava toda espera). Só no upstream; o fork já tinha.

PRs abertos do upstream aplicados:

- [x] #297, #275, #274, #278, #293, #303.
- [x] #266 e #302, portados à mão para o motor do fork.
- [x] #268 já estava no fork.

## Ferramentas e infraestrutura ✅

- [x] `tools/setup_toolkit.sh`: clona o fork do inFAMOUS 1 no commit usado e aplica o patch; testado num checkout limpo.
- [x] Execução sem janela com tempo limite (`boot.sh`) e captura de um frame a cada 10 s (`capture.sh`).
- [x] Entrada de controle por script (`PAD_SCRIPT`) para testar menus sem controle.
- [x] Diagnóstico do renderizador: pass log por material, limiares configuráveis, dump de shaders e render targets, e análise com `passlog.py`, `psfeatures.py`, `surfstats.py` e `greenshare.py`.
- [x] Extrator dos 27 módulos de job do EBOOT (`extract_eboot_jobmods.py`) e tradução dos jobs SPURS capturados (`lift_spu_jobs.sh`).
- [x] Repositório no GitHub com README, créditos, licença e `.gitignore` que bloqueia dados do jogo e do firmware.

## Fase 3 — Gameplay

- [ ] Passar da cutscene das docas para a primeira missão e corrigir o que quebrar.
- [ ] Cole em T-pose no fundo do menu de opções (animação; o inFamous 1 no fork tem o mesmo sintoma).
- [ ] Trecho SPU que ainda roda interpretado (`LS 0x6C64`, ~22 mil instruções por vez): achar a imagem dona e traduzir esse ponto de entrada.
- [ ] Áudio real: até agora todos os testes usaram `SDL_AUDIODRIVER=dummy`.
- [ ] 3 imports sem implementação: `0x2E0A170D`, `0xA7FD2F5B`, `0x9B882495`.
- [ ] DLC `unlock_review.edat`: derivar a chave RIF do `.rap` do jogador.

## Fase 4 — Controles

- [x] Teclado como controle (já existe no toolkit): setas = D-pad, X/espaço = ✕, Return = START, WASD = analógico esquerdo, IJKL = analógico direito.
- [ ] Testar o teclado com janela (`./run.sh`) do menu ao gameplay.
- [ ] Mouse para a câmera: mapear o movimento para o analógico direito, com sensibilidade e inversão configuráveis; botões do mouse para R1/R2 (raios e granadas).
- [ ] Captura do cursor na janela e atalho para soltá-lo.
- [ ] Mapeamento configurável (arquivo de configuração em vez de teclas fixas no código).
- [ ] Controle físico via SDL2 (DualShock/DualSense/Xbox) testado, incluindo vibração.
- [ ] Prompts de botão: opção de trocar os ícones de PS3 pelos de teclado.

## Fase 5 — Gráficos e resolução

- [ ] Resolução interna acima de 720p: renderizar os render targets em escala (1080p, 1440p, 4K), com ajuste dos passes de pós-processamento que dependem do tamanho.
- [ ] Upscaling:
  - [ ] MetalFX (macOS, equivalente da Apple ao DLSS) — caminho principal neste Mac.
  - [ ] AMD FSR (espacial, portável).
  - [ ] NVIDIA DLSS e Intel XeSS: só fazem sentido numa build Windows com GPU NVIDIA/Intel.
- [ ] Tela cheia, janela redimensionável e alternância por atalho.
- [ ] Widescreen/ultrawide (21:9): FOV e HUD corretos.
- [ ] Taxa de quadros: desbloquear de 30 fps onde a lógica do jogo permitir.
- [ ] Ganhar do upstream os PRs de renderização que hoje não aplicam no fork: #304 (sRGB), #305 (gamma de textura), #307 (snapshot de render target). Exige terminar o merge do motor de desenho (`rsx_draw_engine.c`).

## Fase 6 — Desempenho e qualidade

- [ ] Medir fps no gameplay; perfilar PPU e SPU.
- [ ] Reduzir o que ainda passa pelo interpretador SPU.
- [ ] `libgcm_sys` e `libfs` em LLE (o RPCS3 roda os dois assim).
- [ ] Saves: conferir criação, carregamento e local dos arquivos.
- [x] Montagem do toolkit por script (`tools/setup_toolkit.sh`).
- [ ] Um único script para o resto do rebuild do zero: descriptografar, traduzir e compilar (hoje a receita está no README do port).

## Fase 7 — Outras plataformas

- [ ] Linux (Vulkan, já existe no toolkit).
- [ ] Windows (D3D12, o backend mais maduro do toolkit).

## Contribuir de volta

- [ ] Enviar as correções deste port para o fork do inFamous 1 (NoahBPeterson, upstream PR #213).
- [ ] Enviar a correção de jump table do lifter para o upstream (sp00nznet).
- [ ] Enviar os opcodes de pack/unpack do decompilador para o fork e para o upstream.
- [ ] Pedir para listar o port no README do ps3recomp.

---

O repositório nunca terá dados do jogo, do firmware ou nada traduzido a
partir deles (`game/`, `fw/`, `vfs/`, `recompiled/`, `lle/`, `lle_spu/`,
`spu/`, `prx/`). Cada pessoa gera isso localmente a partir da própria cópia.
