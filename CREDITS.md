# Créditos

Este port só existe por causa do trabalho abaixo. Obrigado a todos.

## Toolkit e base

- **[ps3recomp](https://github.com/sp00nznet/ps3recomp)** — Ned Heller
  ([@sp00nznet](https://github.com/sp00nznet)) e
  [contribuidores](https://github.com/sp00nznet/ps3recomp/blob/master/CONTRIBUTORS.md).
  Runtime, HLE, lifters PPU/SPU e o template de projeto (`main.cpp`,
  `stubs.cpp`, `CMakeLists.txt` e `config.toml` dos ports partem dele). MIT.
- **Fork do inFAMOUS 1 — [NoahBPeterson/ps3recomp](https://github.com/NoahBPeterson/ps3recomp)**
  ([@NoahBPeterson](https://github.com/NoahBPeterson)), branch
  `infamous/rebase-v0.13.0`, upstream
  [PR #213](https://github.com/sp00nznet/ps3recomp/pull/213). O port atual
  (`infamous2-lle/`) roda sobre esse fork: boot com o firmware em LLE
  (liblv2, libsysmodule, libsre), as correções de PPU/SPU/lv2 e as suítes de
  conformidade que fizeram o inFAMOUS 1 rodar no macOS. Sem esse trabalho o
  inFAMOUS 2 não teria passado do primeiro frame.

## Pull requests do upstream aplicados

Diffs originais em `infamous2-lle/patches/upstream-prs/`.

- [@undecuple](https://github.com/undecuple) —
  [#297](https://github.com/sp00nznet/ps3recomp/pull/297) (lifter: `sc` não encerra função),
  [#275](https://github.com/sp00nznet/ps3recomp/pull/275) (flip no FIFO),
  [#274](https://github.com/sp00nznet/ps3recomp/pull/274) (klicensee do EDAT),
  [#278](https://github.com/sp00nznet/ps3recomp/pull/278) (escrita predicada no fragment program),
  [#268](https://github.com/sp00nznet/ps3recomp/pull/268) (`lqa`/`stqa` no micro-interpretador; já estava no fork).
- [@tekulvw](https://github.com/tekulvw) —
  [#266](https://github.com/sp00nznet/ps3recomp/pull/266) (retomar código SPU traduzido após o interpretador),
  [#302](https://github.com/sp00nznet/ps3recomp/pull/302) (comparação de profundidade / sombras),
  [#304](https://github.com/sp00nznet/ps3recomp/pull/304) (sRGB na escrita do framebuffer),
  [#305](https://github.com/sp00nznet/ps3recomp/pull/305) (gamma de textura por canal),
  [#293](https://github.com/sp00nznet/ps3recomp/pull/293) (clears no Metal),
  [#303](https://github.com/sp00nznet/ps3recomp/pull/303) (clamp das cores de entrada).

## Ports usados como referência

- [sp00nznet/tornado](https://github.com/sp00nznet/tornado) — receita de
  captura e tradução dos jobs SPURS do Wwise (`tools/lift_spu_jobs.sh`).
- [sp00nznet/youdontknowjack](https://github.com/sp00nznet/youdontknowjack) —
  padrão de carga da libsre traduzida (`infamous2/prx_init.cpp`).
- O caminho de módulos de job WWS do LittleBigPlanet no toolkit
  (`tools/lift_jobmods.py`), usado aqui para os 27 módulos do inFAMOUS 2.

## Ferramentas

- [RPCS3](https://github.com/RPCS3/rpcs3) — descriptografia do EBOOT e do
  firmware do próprio jogador (`rpcs3 --decrypt`), e referência de
  comportamento do toolkit.
- [SDL2](https://www.libsdl.org/), [glslang](https://github.com/KhronosGroup/glslang),
  [SPIRV-Cross](https://github.com/KhronosGroup/SPIRV-Cross),
  [SPIRV-Tools](https://github.com/KhronosGroup/SPIRV-Tools),
  [Capstone](https://www.capstone-engine.org/), CMake, Ninja.

## Jogo

inFAMOUS 2 © 2011 Sony Computer Entertainment America LLC, desenvolvido pela
Sucker Punch Productions. Este projeto não é afiliado nem endossado pela Sony
ou pela Sucker Punch, e não contém nenhum dado do jogo ou do firmware. Para
usar, é preciso ter uma cópia legítima do jogo e um PS3/firmware próprios.
