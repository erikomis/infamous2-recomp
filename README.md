# inFAMOUS 2 — recompilação estática para PC (macOS)

Port nativo do **inFAMOUS 2** (PS3, versão digital `NPUA80638`) para
**macOS/arm64**, feito com recompilação estática: o código PowerPC e SPU do
jogo é traduzido para C/C++ e compilado como um executável nativo, com
gráficos em Metal. Não é um emulador.

Construído sobre o [ps3recomp](https://github.com/sp00nznet/ps3recomp) e,
principalmente, sobre o **fork do inFAMOUS 1** do
[NoahBPeterson](https://github.com/NoahBPeterson/ps3recomp), que trouxe o
firmware em LLE e as correções que fizeram o primeiro jogo rodar no Mac. Veja
[CREDITS.md](CREDITS.md).

> **Este repositório não contém o jogo, o firmware nem nada traduzido a partir
> deles.** Cada pessoa gera tudo localmente a partir da própria cópia legítima.

## Estado

| | |
| --- | --- |
| Boot, firmware em LLE, SPURS, áudio Wwise (jobs SPU) | ✅ |
| Aviso de saúde, vídeos Bink, telas de loading | ✅ |
| Menu principal 3D, menu de pausa, opções | ✅ |
| Jogo novo → vídeo de abertura → cutscene no motor do jogo | ✅ |
| Fim da cutscene → tutorial do primeiro combate | ✅ |
| Gameplay controlável | ✅ por script: tutorial, sequência da Besta e navio cargueiro (~16 fps); teclado/controle com janela ainda por testar |
| Áudio (mixagem), DLC, todos os imports do firmware | ✅ |
| T-pose do Cole no fundo do menu de opções | 🐞 conhecido |

Detalhes e próximos passos em [ROADMAP.md](ROADMAP.md).

## Estrutura

| Pasta | O que é |
| ----- | ------- |
| [`infamous2-lle/`](infamous2-lle/README.md) | **Port atual.** Fork do inFAMOUS 1 + firmware em LLE. |
| [`infamous2/`](infamous2/README.md) | Primeira tentativa, contra o ps3recomp upstream v0.14.0 com SPURS em HLE. Para antes do primeiro frame; fica como referência. Suas pastas locais `game/`, `fw/` e `vfs/` também são usadas pelo port atual. |
| `tools/setup_toolkit.sh` | Clona o fork no commit usado e aplica os patches deste repositório. |

## O que você precisa

- Mac com Apple Silicon (testado num M4 Pro, macOS 27).
- Uma cópia legítima do inFAMOUS 2 instalada no [RPCS3](https://rpcs3.net),
  com a licença (`.rap`) e o firmware do PS3 instalados nele.
- Xcode Command Line Tools e Homebrew:

```sh
brew install cmake ninja sdl2 glslang spirv-cross spirv-tools python@3.12
python3.12 -m venv .venv
```

## Como montar

```sh
git clone https://github.com/erikomis/infamous2-recomp.git
cd infamous2-recomp
tools/setup_toolkit.sh                        # toolkit em ./ps3recomp-fork
.venv/bin/pip install -r ps3recomp-fork/requirements.txt
```

Depois siga a receita do port em
[`infamous2-lle/README.md`](infamous2-lle/README.md#rebuilding):

1. Descriptografar o EBOOT e o firmware com o RPCS3.
2. Traduzir o firmware, o PPU e os programas SPU.
3. Clonar os dados do jogo para `infamous2/vfs/`.
4. Compilar.

A tradução leva poucos minutos e a compilação ~80 s num M4 Pro.

## Como jogar

```sh
cd infamous2-lle
./run.sh
```

Teclado (enquanto o controle físico não é testado):

| Tecla | PS3 |
| ----- | --- |
| Setas | D-pad |
| X / espaço | ✕ |
| Return | START |
| W A S D | analógico esquerdo |
| I J K L | analógico direito |

Para testes sem janela: `tools/capture.sh <nome> <segundos>` salva um PNG do
frame a cada 10 s, `PAD_SCRIPT="95:0x4000"` aperta ✕ aos 95 s e
`PAD_STICK="lx,ly,rx,ry,de,até;..."` segura os analógicos num intervalo
(0–255, 128 = centro). Foi assim que o tutorial foi jogado: `PAD_SCRIPT`
com ✕ aos 95 e 110 s para o menu, o analógico direito em cada direção por
10 s a partir dos 470 s, depois o esquerdo, e R1 (`0x0800`), L1 (`0x0400`),
R2 (`0x0200`) e ✕ a partir dos 655 s.

## Correções feitas aqui

No toolkit (em `infamous2-lle/patches/`, aplicadas pelo `setup_toolkit.sh`):

- **Lifter:** jump tables perdidas atrás de um epílogo com tail-call.
- **cellGcm:** o FIFO pulava comandos quando o jogo "estacionava" a GPU antes de religar o bloco; o menu congelava.
- **SPU:** o interpretador não voltava ao código traduzido dentro dos módulos de job WWS; o menu rodava a ~0,4 fps.
- **Fragment shaders:** os opcodes de pack/unpack do NV40 (PK4UB, UP4UB, PKG, UPG e os irmãos de 16 bits) não existiam no decompilador e eram pulados; o jogo os usa como conversão sRGB↔linear nos materiais de personagem, e sem eles o couro cabeludo do Cole saía verde e a pele com tons errados. Implementados seguindo o RPCS3.
- **Upstream:** PRs abertos aplicados — #297, #275, #274, #278, #293, #303; #266 e #302 portados à mão (créditos em [CREDITS.md](CREDITS.md)).

No port: extrator dos 27 módulos de job embutidos no EBOOT, registro do
policy module `Wws_Job`, PARAM.SFO no boot e scripts de execução e captura.

## Licença

Código deste repositório: MIT ([LICENSE](LICENSE)). O toolkit e o fork são
MIT dos seus autores. inFAMOUS 2 © Sony Computer Entertainment America LLC,
desenvolvido pela Sucker Punch Productions; projeto sem afiliação com a Sony.
