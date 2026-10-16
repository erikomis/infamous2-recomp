# Roadmap

Estado do port nativo do inFAMOUS 2 (`NPUA80638`) em macOS/arm64 e o que vem
a seguir. `[x]` = feito e verificado; `[ ]` = a fazer. Itens marcados
"(novo)" entraram na revisão de 2026-10-15; "Atualização" é o que mudou num item
que já existia.

## Situação atual (2026-10-15)

O jogo roda do boot ao início de New Marais e já é jogado com janela e
DualSense: o save atual é o autosave "3. Forward Momentum" (1% de conclusão,
13 poderes), gravado e carregado pelo próprio jogo. Menu, vídeos, cutscenes no
motor, prólogo em Empire City (a Besta, o cargueiro) e as primeiras missões em
New Marais funcionam, com som.

Em 2026-10-15/16 os erros gráficos grandes foram corrigidos (objetos brancos estourados, NPCs pretos, cubos/painéis pretos em volta de props, rio preto, normal maps sem efeito): ver "Gráficos" em Feito. O que separa isso de "100% jogável" está logo abaixo: o fps (7–16, o jogo foi
feito para 30), um travamento aleatório numa cutscene do prólogo, erros
gráficos que restam em New Marais e o fato de que ninguém passou ainda de 1% do
jogo.

---

## O que falta para ficar 100% jogável

### Bloqueantes

- [ ] Fps em New Marais (7–16): perfilar por thread para achar o gargalo (a sessão do `tools/profile_run.sh`).
  - Perfil de New Marais (2026-10-16, `profile_run.sh newmarais`, amostras aos 250 e 400 s, agregadas por módulo com o `sample_agg.py` da sessão): os 5 SPUs do jogo ficam ~96% ocupados e são o **kernel SPURS real do próprio jogo** (imagem 100, grupo `CellSpursKernelGroup`, 5 SPUs; todos os workloads com contenção máxima 5). O tempo deles: código de job traduzido ~55% (iluminação `datamod_0087f880/0087ec80` ~23%, `jobmod_c6e7383dc013` 11%, geometria Edge `d0d15ec83992` 9%), runtime ~35% (despachante por passo `spu_task_launch_check`/`spu_check_stack_reset`/`spu_indirect_branch_slow`/`spu_drain_call` ~13%, `spu_ls_write128` 4%, `spu_overlay_note_get` 4%, `mfc_do_transfer` 2%, helpers de float não inlinados `cflts/cfltu/fi/sf_sum` ~8%). A thread do RSX/frame clock usa 0,7 núcleo a ~14 fps = ~50 ms de CPU por quadro (teto de ~20 fps sozinha): `nv3089_blit` ~25% (cópia pixel a pixel, `vm_read32`+`vm_write32`, das superfícies 1280x720 para a XDR que a iluminação SPU lê), decodificação+hash de texturas ~15%, fetch/decodificação de vértices ~20%, encode Metal o resto. A thread principal do PPU usa 0,3 núcleo (espera as light tasks e o flip).
  - Feito a partir disso: cópia por linha no `nv3089_blit` (`NV3089_ROWCOPY`, padrão 1; `=0` volta à cópia por pixel que os diagnósticos `PPU_WWATCH`/`PPU_WVAL` enxergam): o blit sumiu do perfil da thread do RSX (era ~25% dela), fps igual no cais (o RSX não é o gargalo ali). Knobs experimentais, todos desligados por padrão: `PS3_HOT_QOS=1`, `GCM_FLIP_IMMEDIATE=1`, `GCM_SEMA_FASTPOLL=1` (A/B intercalado base/exp/base/exp, `profile_run.sh newmarais 480`: no cais 12–13 fps nos quatro, na caminhada 7–8 nos quatro; a diferença só apareceu na "rua", que depende de onde o Cole parou, então sem ganho comprovado); `SPURS_EXTRA_SPUS=2` (7 threads de SPU para o kernel SPURS real do jogo: grupo maior, threads clonadas com o número de SPU em r3, contenção máxima dos workloads 5→7 e os sinais de acordar da libsre repassados aos clones); `PS3_USLEEP_SPIN_US=<n>` (sleeps de até n µs viram spin com `sched_yield` em vez de `nanosleep`: o jogo faz `usleep(3)`/`usleep(10)` nos laços que disparam e esperam as light tasks, e no macOS `nanosleep(3 µs)` dorme ≥50 µs; na fase de caminhada os 5 SPUs ficam só 68% ocupados, a thread principal 52% nesse laço e a "SPU Light Task Kicker" 98% dormindo nele, ou seja, latência, não throughput; o ramo Windows já fazia spin abaixo de 1 ms). Resultado do `SPURS_EXTRA_SPUS=2` (ab4_x1, ab6_x2): os 2 SPUs clonados nunca recebem trabalho. A libsre escreve o `readyCount` de cada workload limitado ao seu nSpus (5), então o kernel só distribui para 5 SPUs; os clones acordam ~20 vezes/s e voltam a esperar (`rdch SPU_RdEventStat`). Fazer funcionar exigiria mexer no `readyCount`/`nSpus` do protocolo SPURS, não só na contenção máxima: parado. Efeito colateral curioso: com o knob ligado o jogo pula o menu e o quadrinho e entra no gameplay aos ~40 s em vez de ~115 s, o que muda o percurso da caminhada. Um ponto importante desses runs: na vista da marina (ab6_x2 aos 400 s) o jogo faz 5–6 fps com os 5 SPUs a 40% e o RSX a 41%, ou seja, nas cenas pesadas ninguém está saturado: é latência de sincronização (light tasks disparadas uma a uma com `usleep(3)`/`usleep(10)`), e por isso testei o `PS3_USLEEP_SPIN_US=100`: o run u1 travou aos 140 s (thread principal girando na espera das light tasks, kicker num semáforo, SPUs ociosos); o u2 rodou inteiro e deu o mesmo fps da base (cais 13,1 contra 13,3; caminhada 7,3 contra 7,7). Ou seja, a granularidade do `usleep` não é o que limita, e o spin ainda expõe um travamento: fica desligado. O que limita a caminhada a 7–8 fps com SPUs, RSX e PPU todos parcialmente ociosos continua em aberto (candidatos: o streaming do psarc + inflate Edge Zlib nos SPUs e as esperas de eventos entre PPU e SPU; medir com `PS3_POLLTOP` e o tempo das light tasks por quadro).
  - Atualização: o prólogo já foi perfilado (ver "Desempenho" em Feito): 12 fps nas cutscenes e no píer, com a thread principal do PPU ~80% parada, 59% num laço `usleep(10)` (`func_00350CE0`) esperando as "SPU light tasks" do quadro. Em New Marais o gargalo **não** é o SPU (traduzir os jobs relocáveis tirou 98% do interpretado e o fps não mexeu); falta o perfil de lá. O jogo trava em 30 fps, então esse é o alvo.
- [ ] Reduzir o que ainda passa pelo interpretador SPU: no prólogo já é só 3% do tempo de SPU (quase tudo no stub de LS 0x3F000); em New Marais os jobs relocáveis foram traduzidos (7a74368) e o interpretado caiu de 14,8 G para 0,2 G instruções por 8 min. Falta perfilar New Marais com o mesmo relatório.
- [ ] Cortar a sobrecarga do runtime SPU: tentado em 2026-10-15 (checagens por passo, `spu_overlay_note_get`, conversões NEON, LS inline, QoS das threads SPU). A sobrecarga medida caiu de 23–31% para 15–22%, mas o fps não mudou (12 no cutscene), a espera do PPU pelas light tasks ficou igual, e o Erik viu cutscenes lentas, áudio ruim e iluminação piscando/branca na janela logo depois. Revertido. O tempo das SPUs está no próprio código de iluminação traduzido (`jobmod_c6e7383dc013`, datamods `0087f880`/`0087ec80`: fm/fma/shufb), não na sobrecarga; os fallbacks de float são só 1,5–2,6%.
  - (novo) Próximo passo: olhar a qualidade do código que o lifter gera para esse job de iluminação (vetorização das operações de float e shuffle).
  - Tentado em 2026-10-15, com as ideias do port do Ben 10 (andrebrumdev/ben10-omniverse-recomp, `notes/`): `shufb` NEON sem `rev32` (6 operações em vez de 11), `spu__perm` sem `rev32` (família `rotqby`/`shlqby`) e `fceq`/`fcgt`/`fcmeq`/`fcmgt` em NEON, todos bit a bit iguais às referências. A/B intercalado com `profile_run.sh newmarais 480` (4 corridas): gameplay 12,9 e 14,7 fps (antes) contra 13,7 e 13,9 (depois), média ~13,8 nos dois; a função quente `datamod_0087f880 func_7320` caiu ~9% nas amostras. Sem ganho de fps; revertido. O código está em `infamous2-lle/patches/experiments/spu-neon-helpers.patch` (com os testes) para quando o gargalo de verdade sair. A variação entre duas corridas do mesmo binário (12,9 contra 14,7) é maior que o efeito: um A/B de fps aqui precisa de pelo menos 2 corridas por lado, intercaladas.
- [x] Travamento intermitente na cutscene — **resolvido** (5 de 5 execuções passam desde a re-tradução da task Edge Zlib, imagem 1; falta só confirmar numa execução com janela). Histórico (9 execuções em 14 travavam): na cena "cityfalls" (Cole, Kuo e Zeke olhando a cidade) o jogo continua renderizando, mas a cena nunca termina. Diagnóstico com `tools/watch_stall.sh` (amostra o processo quando a cena não acaba) e os contadores novos do toolkit (`SPU_INTERP_HIST`, `SPU_INTERP_ENTRY_LOG`, `SPU_IRQSTAT`): um dos seis SPUs fica preso no job de **geometria Edge** (`jobmod_d0d15ec83992`, strings "EDGE ASSERTION FAILURE", skinning, culling), rodando código traduzido num laço que só sai por orçamento de tempo ou quando o trabalho acaba; sem interrupções, sem DMA; o loop passa pelas rotinas atômicas da alocação do ring de saída. A thread principal fica em `cellSpursGetWorkloadInfo` + `usleep(1)` esperando o job. Duas entradas que faltavam noutro módulo (`ab654d30fd90`, LS 0x15280/0x155EC) foram re-traduzidas pelo caminho, e o decrementer do SPU tinha um overflow de 64 bits aos 231 s/462 s (corrigido). Descartado depois: o job chain do Edge (`SPU_ATOMICSTAT`), o áudio (`AUDIO_WAV` continua crescendo a 384 KB/s), as `lwcond` do FIOS (`LWCOND_TRACE`), filas de evento cheias e corrida de `lseek`+`read` entre threads (`FS_READ_ALL` com tid) — tudo normal na parada. O que acontece (visto com `PPU_WWATCH` na lista de grupos de atualização em 0xA8EE50 e lldb no processo vivo): no início da cena o jogo troca de "mundo": `func_002F4E20` libera o grupo 0 (`func_001BF4C4`) e zera a entrada; numa execução que passa, os assets do mundo novo entram no registro e o grupo 0 é recriado (`func_002F02FC`); numa que trava, a thread principal fica consultando o registro por hash (`func_00662EF0`) para sempre. A diferença nas leituras do `infamous1.psarc_s`: a execução boa relê `/jbi_intro_001#31.xpps` depois da troca; a travada não (acha que já tem o pré-carregamento feito segundos antes, que pertencia ao grupo liberado). É uma corrida do próprio jogo entre o pré-carregamento do stream da cutscene e a troca de mundo, que o console nunca perde porque o inflate em SPU (Edge Zlib) termina a tempo. Medido depois (play33, saudável): o pré-carregamento do #31 termina ~158 s antes da troca, então lentidão pura não é o mecanismo. O inflate é a task SPURS Edge Zlib (`spu/images/spu_0000_at_007FA100.elf`, imagem 1), e o log de entradas do interpretador mostra que ela roda em parte **interpretada** (entradas em LS 0x9C84 e 0x9DB8, dentro do código da task, que a descoberta automática de funções não via); uma task de inflate interrompida no meio pelo scheduler enquanto está no interpretador é a hipótese atual para dados descomprimidos inválidos de vez em quando. Em teste: imagem 1 re-traduzida com essas entradas; se não bastar, `SPU_LOCKSTEP=1` e a substituição do inflate por zlib do host (recomendada nos docs do ps3recomp).
  - Atualização 2026-10-16: com a imagem 1 re-traduzida, 5 execuções em 5 passaram pela cena (play35, play37 do c8/a0; play41–43 desta sessão, com `watch_stall.sh`, cada uma com a cena terminando normalmente). Antes eram ~2 travamentos em 3. Com a taxa antiga, 5 passagens seguidas por acaso teriam ~0,4% de chance: dá para tratar como resolvido, confirmando numa sessão com janela do Erik.
- [ ] Jogar daí em diante (a Besta, a chegada a New Marais) com teclado ou controle numa execução com janela; a entrada por script só vai até onde o roteiro alcança.
  - Atualização: com DualSense e janela o Erik passou do prólogo e chegou a New Marais (autosave "3. Forward Momentum", 1%). Com teclado ainda não.
- [ ] (novo) Jogar o jogo inteiro e anotar o que quebra: todas as missões da história, os dois caminhos de karma, os poderes novos (gelo/fogo), as cutscenes em quadrinhos (Bink), os chefes, a parte alagada e a de Gas Works, as missões secundárias e os dois finais. `tools/play_capture.sh` grava o que um bug gráfico precisa numa sessão com janela.

### Gráficos

- [x] Gráficos em New Marais (reconferidos nas capturas do Erik de 2026-10-15 17:36 e 23:5x): inimigos/NPCs pretos → espera do label do SPU (`run.sh`); janelas do barco do Zeke → FACING + `perspective_corr`; retângulos escuros translúcidos perto do Cole → volumes de oclusão (textura 3D + `_bx2`); tudo verificado limpo. Falta só reconferir os blocos de madeira do guindaste (saíam azul-turquesa) quando alguém passar por lá.
- [x] Objetos estourados em branco (barco no reboque, amplificador do Cole, caixote do píer, lixeira, guincho) e retângulos de brilho alinhados à tela perto dos personagens (capturas do Erik, 11:59). Causa, achada em 2026-10-15 à noite: o walker do FIFO não bloqueava nos ACQUIREs de semáforo (padrão do toolkit), então o compositor diferido (FP 0xF384F111AD5956BA) amostrava os buffers XDR 0x07000B80/0x07384B80 enquanto ainda tinham as cópias NV3089 das normais e da profundidade; o "SPU Light Task" escreve a luz ali alguns ms depois (tiles de 32x40 em volta de cada luz, zero no resto, dump no píer). Com as normais no lugar da luz, `tex5.a` (byte alto da profundidade, ≈1) ligava o reflexo do cubemap em tudo que tem brilho (branco estourado); com máscaras de MRT por alvo a luz saía na cor da normal (verde/magenta); os tiles já escritos pelo SPU eram os retângulos. Conserto: `run.sh` exporta `GCM_SEMA_ACQUIRE=1` e `GCM_SEMA_ACQUIRE_MS=200` (ACQUIRE bloqueante com limite, igual ao hardware; nenhum estourou do boot ao píer; fps igual dentro do ruído). O amplificador volta a ser metal escuro, as cintas das tábuas e o tambor ficam normais. Para olhar os buffers: `PS3RECOMP_ENG_MEMDUMP=47500B80:708000:238s:<arquivo>` (os dois, no píer). Também corrigido no caminho: a expansão `UNSIGNED_REMAP_BIASED` (bit 12 de SET_TEXTURE_ADDRESS) vale para toda leitura da textura, não só TEX com EXP_TEX (o compositor lê as normais assim). Os retângulos escuros translúcidos em volta de props (máquina de venda, caixotes) tinham outra causa, achada em 2026-10-16 de madrugada: o passe de volumes de oclusão (p113) consulta uma textura **3D 8×8×8** (FORMAT dimensão 3, CONTROL3 profundidade 8) com direção/ângulo de cone por célula, e o motor não tinha texturas 3D (subia a primeira fatia como 2D e o shader amostrava `.xy`). Texturas 3D implementadas de ponta a ponta (layout Morton 3D, upload por fatia, `MTLTextureType3D`, `Texture3D`/`.Sample(xyz)` no decompilador, chave de pipeline): o shader do p113 agora declara `Texture3D rsx_tex2` e a caixa da máquina de venda some. Ainda conferir com o Erik na praça ao entardecer (luzes de rua) e o reflexo do céu no rio.
- [x] **Cubos pretos em volta de props (mesas do café no terraço do EXPO, foto do Erik 22:19) e normal maps sem efeito (2026-10-16):** o decompilador só expandia (x·2−1) leituras de textura quando o registrador da unidade pedia `UNSIGNED_REMAP_BIASED`; mas o modificador `_bx2` (EXP_TEX, bit 21 do OPDEST) é da instrução, não da textura, e expande sempre. 213 dos ~615 fragment programs do jogo usam `TEX_bx2`: os normal maps tangentes (p181 `tex0`) e a LUT 8×8×8 das caixas de oclusão (p113, valores simétricos em torno de 128) chegavam sem sinal → normais erradas em todo material com normal map e direção do cone sempre no octante positivo → cobertura≈1 → caixa inteira preta. Agora `_bx2` expande as 4 lanes sempre (`rsx_fp_texture_bx2_units`). Também: o snapshot de profundidade entrega o **stencil** na lane B (o compositor lê as flags de material dali: bit 7 escala do ambiente, bit 5 desliga o SSAO; p113/p114 excluem personagens pelo bit 6), antes era 0. Ferramentas novas: `tools/play_dump.sh <nome>` (sessão na janela; Enter no terminal grava o quadro, todos os render targets, pass log e shaders — `PS3RECOMP_METAL_DUMP_TRIGGER=<arquivo>`), `PS3RECOMP_METAL_SURF_DUMP_AT=<s>`, analógico direito no `PAD_SCRIPT` (0x100000 cima, 0x200000 baixo, 0x400000/0x800000 lados), log `texture bind FAILED`, `DUMPTEX` para R16/R8, comentário `/* EXP_TEX unexpanded */` nos shaders dumpados.
- [ ] (novo) Retângulos **claros** translúcidos em volta de personagens/NPCs (fundo atrás deles mais claro, bordas retas) — **não apareceram** nos três quadros do Erik de 23:5x depois do conserto do `_bx2` (os passes de luz leem as normais do G-buffer, que mudaram com os normal maps certos); reconferir na praça ao entardecer com NPCs perto. Histórico: vêm dos passes de luz em volume (pular todos os FPs p116–p122/p303 os remove; só o p120 — luz em caixa com o mesmo VS dos volumes de oclusão — não basta). Esses passes desenham as faces traseiras do volume com LEQUAL e reconstroem a posição da profundidade (a simulação offline do p114, mesmo VS e mesma leitura de profundidade, dá alpha≈1 longe da caixa, então a entrada parece certa); descartados (2026-10-16): convenção do `position.w` (`PS3RECOMP_FRAGCOORD_W=w` não muda), rampa 1024×8 (é a LUT de especular, lida de volta), constantes por draw (staging por record), load das attachments de profundidade/stencil, stencil (p116/p120 rodam com teste desligado), cull/winding/compare (iguais ao RPCS3). O p118 (luz de personagem) com constantes + profundidade reais da cena "FOR RENT" simula atenuação ≈ 0 (as luzes capturadas estavam a ~110 m); pular p116, p117/118/303, p120, p122 ou p119+p121 **isoladamente** deixa caixas fracas, só pular todos limpa. Teste rápido na janela: `PS3RECOMP_ENG_SKIP_FP=d98fefc56d43594b,83d934dc18efcea3,db0f414789b4ad56,6d715ad65444fa49,c5e0d94ef8e23ba3,7f104f9b0f3b938f ./run.sh` (sem luzes-volume) e depois um hash por vez. Dumps por hash: `PS3RECOMP_ENG_FPK_FP=<hash> PS3RECOMP_ENG_FPK_AFTER=<s>` (constantes FP+VP) e `PS3RECOMP_ENG_DUMPTEX_FP=<hash>:<unidade>:<s>:<arquivo>` (textura lida de volta); simuladores `p118_sim.py`/`p120_sim.py`/`p114_sim.py` reproduzem a conta dos shaders a partir desses dumps. Ferramentas: `PS3RECOMP_ENG_SKIP_FP=<hash,..>` (pula draws por hash do FP, estável entre execuções; `manifest.txt` no `PS3RECOMP_ENG_SHADER_DUMP` lista p= → fp/vp), `PS3RECOMP_METAL_DBG_DUMPTEX=<p>:<unidade>:<seg>:<arquivo>` (lê de volta a textura que um pipeline amostra), `PS3RECOMP_ENG_TEXSTATE_PIPE=<p>` (registradores de textura de um pipeline, com dimensão/profundidade/cor de borda), `PS3RECOMP_ENG_FPK_AFTER`/`[vpk]` (constantes FP e VP de um draw numa cena), `PS3RECOMP_ENG_VPC_WATCH=<p>:<c>`, `TCONST_SLOT`/`TCONST_AFTER`, e o analógico esquerdo no `PAD_SCRIPT` (bits 0x10000 cima, 0x20000 baixo, 0x40000/0x80000 lados, `:<seg>` segura) — `175:0x10000:9,184:0x90000:8,192:0x10000:25,218:0x80000:3,221:0x10000:30` leva o Cole do píer à rua do Club Bar (não determinístico: ~1 em 3).
- [ ] (novo) Rio/água sem o reflexo do céu (capturas do Erik 23:5x: água cinza chapada sob um pôr do sol laranja). O passe da água (p149/p150) lê a cópia da cena em 0x00E3C000 com `perspective_corr`; falta ver se o termo de reflexo usa um cubemap/normal da água que chega sem sinal ou um render target que não resolvemos (`tools/play_dump.sh` + `manifest.txt` do shader da água).
  - Atualização 2026-10-16 (capturas w83–w92 com `RSX_FP_DEBUG`, `PS3RECOMP_ENG_FPK_FP` e dump de texturas): as entradas do shader da água (FP 2bc5c87f50037448) estão certas: constantes sãs (c2 = posição do olho em cm, c6=2/c7=−1 são a expansão 2w−1 que o próprio shader faz do mapa de ondas Y16_X16, lido sem expansão; c19=1), RT de ondulações uniforme em (128,128,0) e lido com `_bx2` (vira 0), tc0 = posição no mundo. O sintoma medido: as componentes xy da normal saturam perto de ±1 e o termo de Fresnel vai a 1, então o rio sai 100% reflexo (escuro, com os postes refletidos e sem céu). Suspeitos que restam: amplitude/lanes do mapa de ondas Y16_X16 (dumpar a unidade 0 como RG16 e ver o histograma; o crossbar 0xE4→0x56 põe .y/.w = X/Y) e se o RT de reflexo planar 256x256 (unidade 4, 0x0F6F0000) contém o céu. Cuidado com o `RSX_FP_DEBUG`: o índice n é contado antes da instrução n "tratada" e não bate com a contagem manual do HLSL; usar junto com `PS3RECOMP_ENG_SHADER_DUMP` (criar o diretório antes) para ver onde o `_rsx_dbg =` caiu.
  - Atualização 2026-10-16 (manhã, capturas w93–w102): fechado sem bug encontrado. Mapa de ondas Y16_X16 praticamente plano (X,Y = 0,5 ± 0,016: normal certa, a leitura anterior de "normal saturada" era um índice errado do `RSX_FP_DEBUG`; com o dump do shader junto o índice bate com a contagem dos blocos do HLSL, e a saída sobrepõe por blend aditivo, então "branco" não é confiável). O RT de reflexo planar 256x256 (0x0F6F0000) tem o céu em cima, horizonte a 66% e preto embaixo: é a vista de uma câmera abaixo da água olhando para cima **sem** espelhar o eixo vertical, e é assim que o jogo faz (determinantes das matrizes do passe de reflexo com o mesmo sinal da view-projection principal; viewport, clip, janela e cull iguais aos outros passes; registradores inteiros comparados com `PS3RECOMP_ENG_REGDUMP_PIPE`; dumps de textura confirmados de cabeça para cima com `PS3RECOMP_METAL_DBG_DUMPTEX`/`DUMPTEX_FP`). Com isso a água perto amostra o preto do RT (sem reflexo, domina a refração escura) e a água longe amostra a faixa do horizonte (névoa cinza), que é o cinza que se vê. Amostrar o reflexo com v invertido (`RSX_FP_FLIPV=2bc5c87f50037448:4`, knob de teste) dá reflexos do píer na água perto mas não tem base no hardware. Para ir além é preciso uma referência do original no mesmo ponto (RPCS3 com o save do cais, ou vídeo), porque a conta do shader com as entradas verificadas dá exatamente isto.
- [x] "SPU Light Task": roda a cada frame, consome as cópias XDR das normais/profundidade e escreve de volta a luz difusa (0x07000B80: RGB luz, A intensidade) e a especular (0x07384B80), em tiles de 32x40 só onde há luz; no menu não há luz nenhuma, por isso parecia não escrever. O RSX precisa esperar o label 137 antes do compositor (ver item acima).
- [ ] (novo) Menu principal: o contorno cinza-azulado do Cole e os destroços escuros não aparecem; há um borrão cinza estático sob a barra que não existe no original.

### Estabilidade

- [ ] Congelamento no gameplay (a imagem para de vez; ~1 execução em 5 em New Marais, e no píer diurno do `m2` aos ~40 s de jogo): o walker do FIFO para num JUMP para si mesmo (o "estacionamento" no fim de um bloco de comandos montado pelas SPUs, que o jogo depois troca por um JUMP para o bloco seguinte) e o watchdog "stuck-get" desiste dele depois de 8 passadas, poucos milissegundos, antes do prazo próprio do estacionamento (`GCM_PARK_GRACE_MS`), pulando para o `put` e perdendo os comandos do quadro. Corrigido: o watchdog não conta passadas paradas num estacionamento (o prazo de 50 ms decide). Validação em andamento (3 execuções a partir do save). Não é a causa do travamento da cena "cityfalls": a maioria das execuções travadas ali não tem nenhum stuck-get.
  - Atualização: as 3 execuções de validação rodaram sem congelar (ee6ed84). Falta confirmar em sessões longas com janela.
  - Atualização 2026-10-16: em 88 execuções headless de New Marais de 2026-10-15/16 (~4 min cada, píer e caminhada até a rua) nenhuma congelou; 4 tiveram um "FIFO resync (jump-park)" (o prazo de 50 ms do estacionamento venceu: um quadro perdido, não um travamento). Pode ser marcado como resolvido depois de uma sessão longa do Erik na janela.

### Saves

- [ ] Saves: conferir criação, carregamento e local dos arquivos.
  - Atualização: criação e carregamento funcionam (o autosave do Erik é gravado e carregado pelo próprio jogo). O local é o problema: o `/dev_hdd0` é `gamedata/` relativo ao diretório de onde o jogo foi aberto, então abrir de outra pasta é começar sem save. Precisa de um caminho fixo (por exemplo em `~/Library/Application Support`).

### Controles

- [ ] Testar o teclado com janela (`./run.sh`) do menu ao gameplay.
- [ ] Captura do cursor na janela e atalho para soltá-lo.
- [ ] Mapeamento configurável (arquivo de configuração em vez de teclas fixas no código).
- [ ] Controle físico via SDL2 (DualShock/DualSense/Xbox) testado, incluindo vibração.
  - Atualização: o DualSense funciona (botões e analógicos) no jogo com janela. Faltam vibração, Xbox e DualShock.
- [ ] Prompts de botão: opção de trocar os ícones de PS3 pelos de teclado.
- [ ] Portão do píer, "Hold L1 to aim": o script só tocava em L1; agora `PAD_SCRIPT` aceita `seg:máscara:duração` e segura. Em teste.
  - Atualização: `tools/play_prologue.sh` segura L1 aos 760 s, mas o roteiro nunca foi conferido até o fim. Jogando à mão o portão já foi passado.

### Rebuild

- [ ] Um único script para o resto do rebuild do zero: descriptografar, traduzir e compilar (hoje a receita está no README do port).

---

## Depois de jogável (melhorias)

### Gráficos e resolução

- [ ] Resolução interna acima de 720p: renderizar os render targets em escala (1080p, 1440p, 4K), com ajuste dos passes de pós-processamento que dependem do tamanho.
- [ ] Upscaling:
  - [ ] MetalFX (macOS, equivalente da Apple ao DLSS): upscaling espacial e temporal, e geração de quadros onde o macOS oferece. Caminho principal neste Mac.
  - [ ] AMD FSR 3.1: upscaling + geração de quadros, funciona em qualquer GPU (NVIDIA, AMD, Intel) e é o padrão nas builds Windows (D3D12/Vulkan) e Linux (Vulkan); no Mac só com um port próprio para Metal (o SDK oficial não cobre Metal), então lá o padrão é o MetalFX.
  - [ ] AMD FSR 4.x (incluindo o 4.1): por IA, melhor qualidade, mas só em GPUs AMD que o SDK suporta (RDNA 4 em diante), em Windows e onde o driver Linux oferecer; nas outras GPUs o app cai para o FSR 3.1.
  - [ ] NVIDIA DLSS (a versão mais nova disponível) e Intel XeSS: só fazem sentido numa build Windows com GPU NVIDIA/Intel.
  - O temporal (MetalFX temporal, FSR, DLSS) precisa de vetores de movimento e de jitter na câmera, que o jogo não gera: ou se derivam da profundidade + matrizes do quadro anterior, ou fica só o espacial.
- [ ] Tela cheia, janela redimensionável e alternância por atalho.
- [ ] Widescreen/ultrawide (21:9): FOV e HUD corretos.
- [ ] Taxa de quadros: desbloquear de 30 fps onde a lógica do jogo permitir.
- [ ] #307 (snapshot de render target) do upstream: exige terminar o merge do motor de desenho (`rsx_draw_engine.c`).

### App para instalar e jogar (launcher)

Um app para **macOS, Windows e Linux**, com a mesma interface nos três, que leva da cópia do jogo do jogador até o "Jogar", sem terminal: ele mesmo faz o recomp (rebuild) na máquina do jogador e depois abre o jogo. O app não traz código nem arquivos do jogo: o jogador fornece a própria cópia. Backend gráfico por sistema: Metal (macOS), D3D12 ou Vulkan (Windows), Vulkan (Linux). Decidido (2026-10-16): **launcher em Tauri** (núcleo em Rust que roda o rebuild e mostra progresso/log; interface web; instalador pequeno nos três sistemas) + mais tarde um **menu dentro do jogo em SDL + ImGui** (resolução, upscaling, fps sem sair do jogo). Os dois conversam por um `config.toml` que o launcher grava e o jogo lê ao abrir, substituindo as variáveis de ambiente de hoje. Ordem: (1) script único de rebuild, (2) o jogo lendo o `config.toml`, (3) o launcher, (4) o menu no jogo.

- [ ] **Instalação / rebuild:** escolher a cópia do jogo (pasta do jogo instalado no RPCS3, PKG ou disco) e o `.rap`; o app confere a versão (NPUA80638 01.00), descriptografa, traduz (PPU, SPU, jobs) e compila, com barra de progresso, tempo estimado e log. Depende do script único de rebuild (seção Rebuild), que precisa rodar nos três sistemas: Python dos tradutores, CMake + Clang (Xcode no Mac, Clang/MSVC no Windows, Clang/GCC no Linux). O app confere e instala o que faltar (ou traz um toolchain embutido) e mostra o espaço em disco necessário. Refazer só o que mudou quando o toolkit é atualizado.
- [ ] **Instalador nativo em cada sistema** (gerado pelo Tauri): Windows `.exe` (NSIS) ou `.msi` com atalho no Menu Iniciar/área de trabalho e desinstalação pelo sistema; macOS `.dmg` com o `.app` para a pasta Aplicativos; Linux `.AppImage`, `.deb` e Flatpak com ícone no menu. O executável do jogo gerado pelo recomp fica na pasta de dados do app; opção de criar um atalho que abre o jogo direto. Assinatura de código para não aparecer o aviso de app desconhecido: conta Apple Developer (US$ 99/ano + notarização) e certificado de assinatura no Windows; sem isso funciona, com o aviso na primeira abertura.
- [ ] **Ícones e identidade visual:** ícone original do app (sem logo nem arte da Sony, que são marca registrada), gerado em todos os formatos com `tauri icon` (Windows `.ico`, macOS `.icns`, PNGs do Linux); o mesmo ícone na janela e no Dock/barra de tarefas do próprio jogo (hoje a janela Metal não tem ícone); tela de abertura do launcher. A arte oficial vem da cópia do jogador na instalação: `PS3_GAME/ICON0.PNG` (ícone), `PIC1.PNG` (fundo) e o título de `PARAM.SFO`, mostrados na tela inicial, sem nada disso ir no instalador.
- [ ] **Tela inicial:** botão Jogar, capa do jogo, último save (missão e data), versão do port.
- [ ] **Configurações de tela:**
  - modo janela / tela cheia / tela cheia sem borda, monitor, sincronia vertical;
  - resolução interna (720p nativo, 1080p, 1440p, 4K) e proporção (16:9, 21:9 ultrawide, com FOV e HUD corretos);
  - limite de fps (30 original; acima quando a lógica do jogo permitir);
  - upscaling: MetalFX (Mac), FSR 3.1 (todas as GPUs, Windows/Linux), FSR 4.x/4.1 (GPUs AMD suportadas), DLSS (NVIDIA, Windows), XeSS (Windows/Linux), mostrando só os que a GPU do jogador suporta, com qualidade (desempenho/equilibrado/qualidade) e escala;
  - filtragem anisotrópica, anti-aliasing (FXAA/SMAA/TAA), nitidez;
  - brilho/gamma e HDR (EDR no Mac).
- [ ] **Controles:** remapear teclado, mouse e controle (DualSense/DualShock/Xbox) com a tela de teste dos botões; sensibilidade e inversão do mouse; vibração; ícones de botão PS3 ou teclado nos prompts.
- [ ] **Áudio:** volume geral e dispositivo de saída.
- [ ] **Saves:** listar, fazer backup, restaurar e importar/exportar (inclusive saves do RPCS3), para nunca perder o progresso.
- [ ] **Pacotes de texturas e mods:** carregar texturas substitutas (alta resolução, refeitas por IA) e patches, ligar/desligar cada um.
- [ ] **Desempenho e diagnóstico:** contador de fps/tempo de quadro na tela; "reportar problema" que salva o quadro atual com os dados de diagnóstico (`tools/play_dump.sh`) e o log num arquivo para mandar; perfis prontos (qualidade/equilibrado/desempenho).
- [ ] **Atualizações:** avisar quando o port tiver versão nova e refazer o rebuild sozinho.
- [ ] **Idioma** da interface do app (português e inglês).
- [ ] **Idioma do jogo (dublagem, legendas e menus)** escolhível no launcher e no menu dentro do jogo, entre os que a cópia do jogador traz (`cache/psarc/<idioma>.psarc` e `<idioma>_text.psarc`: português do Brasil, de Portugal, espanhol latino, inglês e outros). Grava `PS3_LANG` no `config.toml`. Feito (2026-10-16): o `run.sh` já abre em português do Brasil (`PS3_LANG=pt`, código 7: o jogo trata todo idioma do sistema acima de 16 como desconhecido e cai para o inglês, e nesta versão das Américas o "Português" 7 é o que escolhe os arquivos `brazilian*`; `func_0009D9DC`). Também corrigido no toolkit: os códigos 17 (inglês do Reino Unido) e 18 (português do Brasil) estavam trocados. O jogo carrega o idioma ao abrir, então trocar pede reiniciar; o app avisa e reinicia sozinho. Opcional: voz e legenda em idiomas diferentes (ex.: voz em inglês, legenda em português), redirecionando a abertura dos `.psarc` de voz ou de texto.
- [ ] **Versões do jogo:** hoje só a PSN (`NPUA80638`, PKG ou pasta do RPCS3 + `.rap` da compra do jogador). Para quem tem o disco: aceitar a .iso (`BCUS98125`) com a chave do disco (`.dkey`/`.ird`) e a atualização oficial, como os ports de Twisted Metal, All-Stars e Drakengard 3 (`decrypt_iso.py`, AES-CBC). Antes, comparar o EBOOT do disco atualizado com o da PSN: se for igual, é só a descriptografia; se não, um segundo perfil de recomp. O app identifica a versão do executável e avisa quando não é suportada.

### Desempenho

- [ ] `libgcm_sys` e `libfs` em LLE (o RPCS3 roda os dois assim).

### Outras plataformas

- [ ] Linux (Vulkan, já existe no toolkit), com o app e o rebuild rodando nativos.
- [ ] Windows (D3D12, o backend mais maduro do toolkit; Vulkan como alternativa), com o app e o rebuild rodando nativos.
- [ ] O que é só do macOS no port hoje e precisa de equivalente: a thread principal do AppKit em `main.cpp`, o caminho de vídeo VideoToolbox, o áudio CoreAudio, os shims Win32 em POSIX (no Windows somem).

### Fora do escopo por enquanto

- [ ] (novo) Missões de usuário (UGC): dependem da PSN, que não existe mais; só com um servidor da comunidade.

---

## Feito e finalizado

### Boot e pipeline

- [x] EBOOT descriptografado com o RPCS3 (`--decrypt`), usando a licença do próprio jogador.
- [x] PPU traduzido para C++: 32.426 funções.
- [x] Programas SPU do EBOOT: descompressão zlib, Bink, policy module `Wws_Job`.
- [x] 27 módulos de job WWS (`C0DEC0DE`) extraídos do EBOOT e traduzidos.
- [x] Jobs SPURS de áudio do Wwise capturados em execução e traduzidos.
- [x] Trecho SPU interpretado (`LS 0x6C64`): eram cinco módulos `DA7ABA5E` embutidos no EBOOT que o gerenciador de jobs carrega em LS 0x6C00 e ninguém traduzia. `tools/lift_datamods.py` os traduz e registra como regiões de código; zero trechos longos no menu e ~30 fps nesse trecho (eram ~17).
- [x] Firmware em LLE, traduzido do `dev_flash` do jogador: liblv2, libsysmodule, libsre e o kernel SPU do SPURS.
- [x] Metal com os shaders do próprio jogo (glslang + SPIRV-Cross).
- [x] Dados do jogo isolados num clone copy-on-write; o RPCS3 nunca é escrito.
- [x] Execução com janela no macOS: o guest roda fora da thread principal (a janela ficava cinza num deadlock com o Metal).

### Do menu ao gameplay

- [x] Boot pelo `module_start` da liblv2, como no console.
- [x] FIOS, `.psarc`, descompressão em SPU.
- [x] Aviso de saúde, vídeo Bink, tela de loading.
- [x] Menu principal 3D. Conferido contra o vídeo do título original (2026-10-14): é um vazio preto com o Cole de costas quase em silhueta, um raio, faíscas e a barra "START A NEW GAME"; o nosso é igual (as diferenças pequenas estão em "Gráficos", acima).
- [x] Jogo novo, vídeo de abertura, cutscene no motor do jogo.
- [x] Menu de pausa e tela de opções.
- [x] "Cole em T-pose no menu de opções": não é visível. Pausar a cutscene (START) mostra o menu de pausa com a cena congelada na pose correta ao fundo, e a tela de opções usa a arte estilizada da cidade, como no original; a figura em T-pose que eu tinha visto estava num render target intermediário que nunca vai para a tela.
- [x] Passar da cutscene das docas para a primeira missão: o tutorial do primeiro combate aparece sem intervenção.
- [x] Exercitar o gameplay: os prompts do tutorial respondem à entrada por script. Olhar com o analógico direito (a câmera sobe até a Besta), andar com o esquerdo ("Head towards the disturbance": Cole anda pelo Píer 13 até o portão, e sobe no cargueiro ao lado quando o script o leva lá), pular com ✕. ~16 fps sem janela; 19.080 frames em 900 s e 19.320 em 1200 s, sem travar.
- [x] Áudio: a mixagem produz som (146 s gravados com `AUDIO_WAV`, pico 0,61, primeiro som aos 6 s); ouvido pelo alto-falante com janela.
- [x] **Som picotado em New Marais / jobs SPU relocáveis (2026-10-15):** os ~30 blocos de código SPU que o kernel do SPURS copia do EBOOT (0x8E8C00–0x928150) para uma posição diferente da LS a cada execução rodavam inteiros no interpretador. Agora são traduzidos uma vez em modo relocável (`spu_lifter.py --relocatable`, `tools/lift_relocmods.py`, `spu_overlay_register_region_reloc`), e o corpo do `libsre` em 0x2A180 (política das cadeias de jobs, LS 0xA00) foi traduzido também. Medido no mesmo roteiro: instruções interpretadas 14,8 bi → 0,2 bi em 8 min; blocos de áudio com som ~35% → 100%. O fps **não** mudou (10–16 no pântano, 7–10 nas ruas): o gargalo de fps está em outro lugar (PPU, RSX ou Metal). `PS3_RELOCMODS=0` e `FW_SPU_INTERP=104` desligam cada parte para comparar.
- [x] Imports sem implementação: `cellGemGetMemorySize` e `cellCameraSetNotifyEventQueue2` implementados no toolkit; `cellFsGetDirectoryEntries` veio do PR #247 do upstream.
- [x] DLC `unlock_review.edat`: `tools/rap2rifkey.py` deriva a chave RIF do `.rap` do jogador e grava o sidecar que o runtime lê; o EDAT é decifrado.

### Gráficos

- [x] Iluminação dos personagens nas cutscenes (comparação de profundidade das sombras).
- [x] Cor dos materiais de personagem: o decompilador pulava os opcodes de pack/unpack (PK4UB, UP4UB, PKG, UPG) que o jogo usa como conversão sRGB↔linear; era a mancha verde no couro cabeludo do Cole e tons de pele errados.
- [x] Brilho e contraste: a imagem saía escura e lamacenta (personagens quase pretos nas cenas de contraluz, sombras esmagadas). O jogo pede ao RSX a conversão linear→sRGB na escrita do framebuffer (`SET_SHADER_PACKER`) e a decodificação sRGB→linear ao ler texturas (bits de gamma do `TEXTURE_ADDRESS`), e o motor do fork ignorava as duas. PRs #304 e #305 do upstream portados à mão para o motor do fork.
- [x] **Trepadeiras e plantas pretas, paredes roxas, rostos de NPC pretos (2026-10-15):** o decompilador de fragment programs devolvia zero para as entradas FACING (0xE), TC8 (0xC) e TC9 (0xD). Os materiais de dois lados fazem `normal × FACING` para virar a normal da face de trás; com zero, a normal virava NaN no G-buffer e o pixel ficava preto. Agora FACING é `SV_IsFrontFace ? +1 : −1` (o `ssa` do RPCS3), TC8 vem de `o[15]` e TC9 de `o[6]` do vertex program. Eram 116 dos 513 pixel shaders de New Marais (71 FACING, 45 TC8/9). Na mesma cena: trepadeiras verdes, paredes de madeira (não mais roxas). Teste em `test_shader_msl` (`[[front_facing]]`, `locn11/12`).
- [x] **Rio preto em New Marais (2026-10-15):** a água copia a cena já desenhada (alvo em VRAM 0x00E3C000) e a lê na posição da tela. O vertex program escreve `uv × w` em TC2 e o fragment program lê TC2 com o bit `perspective_corr` (bit 31 da palavra SRC2) ligado, que no RSX multiplica a entrada por 1/w. O decompilador ignorava esse bit: a água lia `uv × w`, muito fora da cópia, e saía preta. Agora a leitura vira `tcN * _rsx_inv_w(input.position)` (como o `* gl_FragCoord.w` do RPCS3); nos caminhos via glslang (Metal, Vulkan) `position.w` já é 1/w (`RSX_FRAGCOORD_W_IS_RCP`), no FXC (D3D12) o helper divide. 163 pixel shaders de New Marais usam o bit. Na mesma cena: o rio aparece ao lado do píer e atrás da Kuo. Testes em `test_fp_decompiler` e `test_shader_msl`. Achado com `PS3RECOMP_METAL_DBG_NOZ=<alvo>` (a água era desenhada, mas preta), `SKIP_PIPE` e `DBG_SHOWTEX` (a textura estava certa, as coordenadas não). Diagnósticos novos, desligados por padrão: `RSX_FP_NAN_COLOR=1` (NaN magenta, Inf ciano) e `PS3RECOMP_ENG_FPK_PIPE=<p>` (constantes do FP por draw).

### Desempenho

- [x] Medir fps no gameplay; perfilar PPU e SPU (`tools/profile_run.sh` + `tools/profile_report.py`, 2026-10-15, prólogo sem janela): menu ~30 fps; cutscene das docas e píer **11–13 fps**, estável em 12 (≈ 5 vblanks por quadro). A thread principal do PPU fica ~80% do tempo parada: 59% num laço `usleep(10)` (`func_00350CE0`) esperando zerar o contador de "SPU light tasks" pendentes, que a thread "SPU Light Task Kicker" vai submetendo. Os 5 SPUs de trabalho ficam a ~93%, e o tempo deles se divide em: código de job traduzido 54–70%, **helpers do runtime 23–31%** (`spu_ls_write128`, `spu_overlay_note_get`, `spu_task_launch_check`/`spu_check_stack_reset` a cada passo do drain, `mfc_do_transfer`, conversões de float), interpretador 3%, RSX/Metal 2–7%. Ou seja: o quadro é limitado pela vazão das SPUs, e um quarto a um terço dela vai para sobrecarga do runtime.

### Controles

- [x] Teclado como controle (já existe no toolkit, `cellPad.c`): analógico esquerdo W/A/S/D, direito I/J/K/L, D-pad nas setas; ✕ Espaço, ○ Q, □ E, △ R; L1 U, R1 O, L2 Y, R2 P, L3 G, R3 H; START Return, SELECT Tab. A tecla X não faz nada.
- [x] Mouse para a câmera (2026-10-14): com a janela em foco, o movimento vira o analógico direito, proporcional à velocidade; botão esquerdo = R1 (raio), direito = R2 (granada), do meio = L1 (mirar). Configuração por variável de ambiente: `PS3_MOUSE_SENS=<fator>` (padrão 1.0), `PS3_MOUSE_INVERT_Y=1`, `PS3_MOUSE_DEADZONE=<0-120>` (padrão 30), `PS3_MOUSE=0` desliga; `PS3_MOUSE_LOG=1` registra o que chega. Junto: teclado e mouse deixaram de ser desligados quando há um controle físico conectado; agora somam com ele (um DualSense pareado e parado fazia todas as teclas não responderem).

### Correções no toolkit

Aplicadas pelo `tools/setup_toolkit.sh` a partir de
`infamous2-lle/patches/ps3recomp-fork-770660a-with-upstream-prs.patch`.

Feitas neste port:

- [x] Lifter PPU: jump tables perdidas atrás de um epílogo com tail-call.
- [x] cellGcm: o FIFO pulava comandos quando o jogo "estacionava" a GPU antes de religar o bloco; o menu congelava.
- [x] SPU: o interpretador não voltava ao código traduzido dentro dos módulos de job WWS; o menu rodava a ~0,4 fps.
- [x] SPU: overflow de 64 bits do decrementer aos 231 s/462 s.
- [x] Fragment shaders: opcodes de pack/unpack do NV40 (PK4UB, UP4UB, PKG, UPG, PK4B, UP4B, PK2H, UP2H, PK2US, UP2US).
- [x] HLE: `cellGemGetMemorySize`, `cellCameraSetNotifyEventQueue2`/`RemoveNotifyEventQueue2`.
- [x] Entrada programada: `PAD_STICK` com vários intervalos numa execução; `PAD_SCRIPT` com botão segurado por N segundos (`seg:máscara:duração`).
- [x] `sys_lwcond_t` com layout errado (FIOS recusava toda espera). Só no upstream; o fork já tinha.
- [x] Metal: command buffers que terminam com erro (GPU travada, recurso inválido) são reportados com o código, em vez de passarem em silêncio.
- [x] cellGcm: o dump das palavras do FIFO numa recuperação (branch inválido, `get` parado) sai sem precisar de `GCM_RECDBG`, que é barulhento demais para uma execução longa.
- [x] Texturas de um canal (X16, X32_FLOAT, DEPTH16/24): o crossbar lia a lane vazia; o LUT 1024×8 X16 da iluminação deste jogo (crossbar "R ← B") dava zero. Com teste unitário.
- [x] Write-back GPU → memória do jogo nos blits NV3089: o jogo copia os buffers de normais e de profundidade para a XDR a cada frame (entrada da iluminação nas SPUs) e a cópia era feita pela CPU a partir de uma memória que nunca recebia o que a GPU desenhou. Agora o motor submete o trabalho pendente e lê o render target (ou o depth, em bytes Z24S8) de volta antes da cópia.
- [x] `GCM_SEMA_ACQUIRE=1` + `GCM_SEMA_ACQUIRE_MS`: o walker do FIFO espera o ACQUIRE de semáforo (com limite), e o watchdog "stuck-get" não ressincroniza enquanto espera. Desligado por padrão. `GCM_SEMA_TRACE=<n>` mostra o protocolo de labels.
- [x] cellPad: `PS3_PAD_SDL=0` ignora controles físicos; `tools/boot.sh` o liga, para uma execução sem janela não pegar o START do DualSense de quem está jogando.
- [x] `ps3_ms_now()` devolvia 0 no macOS (upstream 91ee29f): os limites `GCM_PARK_GRACE_MS`/`GCM_SEMA_ACQUIRE_MS` nunca expiravam e os logs de taxa liam 0. Agora usa `CLOCK_MONOTONIC`.
- [x] Máscara de cor e blend por alvo de MRT (`COLOR_MASK_MRT`/`BLEND_ENABLE_MRT`), padrão desde 2026-10-15 à noite, depois do ACQUIRE bloqueante (`PS3RECOMP_METAL_MRT_MASK=0` volta ao antigo) (ver Gráficos).

PRs abertos do upstream aplicados:

- [x] #297, #275, #274, #278, #293, #303.
- [x] #266, #302, #304 e #305, portados à mão para o motor do fork; #247 com uma adaptação de assinatura.
- [x] #268 já estava no fork.

### Ferramentas e infraestrutura

- [x] `tools/setup_toolkit.sh`: clona o fork do inFAMOUS 1 no commit usado e aplica o patch; testado num checkout limpo.
- [x] Montagem do toolkit por script (`tools/setup_toolkit.sh`).
- [x] Execução sem janela com tempo limite (`boot.sh`) e captura de um frame a cada 10 s (`capture.sh`); cada execução roda num diretório próprio e nunca grava por cima do save do jogador (`SAVE=<dir>` copia um save para dentro).
- [x] Entrada de controle por script (`PAD_SCRIPT`, `PAD_STICK`) para testar menus e gameplay sem controle; `tools/play_prologue.sh` joga o prólogo inteiro num comando.
- [x] Perfil por thread: `tools/profile_run.sh <nome> prologue|menu|newmarais` + `tools/profile_report.py`.
- [x] Diagnóstico de travamento: `tools/watch_stall.sh`.
- [x] Diagnóstico do renderizador: pass log por material, limiares configuráveis, dump de shaders e render targets, e análise com `passlog.py`, `psfeatures.py`, `surfstats.py` e `greenshare.py`; `tools/play_capture.sh` para sessões com janela.
- [x] Extrator dos 27 módulos de job do EBOOT (`extract_eboot_jobmods.py`), tradução dos jobs SPURS capturados (`lift_spu_jobs.sh`), re-tradução de um módulo com entradas extras (`relift_jobmod.py`), tradução dos cinco módulos `DA7ABA5E` (`lift_datamods.py`) e dos jobs relocáveis (`lift_relocmods.py`).
- [x] `rap2rifkey.py`: chave RIF de um EDAT a partir do `.rap` do jogador.
- [x] Repositório no GitHub com README, créditos, licença e `.gitignore` que bloqueia dados do jogo, do firmware e saves.

---

## Contribuir de volta

Branches em [github.com/erikomis/ps3recomp](https://github.com/erikomis/ps3recomp);
cada correção é um PR, com teste onde dava para ter um.

- [x] Correções deste port enviadas ao fork do inFamous 1 (NoahBPeterson, base do
  upstream PR #213): jump table atrás de tail-call no lifter ([#1](https://github.com/NoahBPeterson/ps3recomp/pull/1)),
  FIFO estacionado do cellGcm e dump da recuperação ([#2](https://github.com/NoahBPeterson/ps3recomp/pull/2)),
  overlay residente no lookup do SPU ([#3](https://github.com/NoahBPeterson/ps3recomp/pull/3)),
  overflow do decrementer ([#4](https://github.com/NoahBPeterson/ps3recomp/pull/4)),
  opcodes de pack/unpack ([#5](https://github.com/NoahBPeterson/ps3recomp/pull/5)),
  texturas de um canal no crossbar ([#6](https://github.com/NoahBPeterson/ps3recomp/pull/6)),
  cellGem/cellCamera ([#7](https://github.com/NoahBPeterson/ps3recomp/pull/7)),
  PAD_SCRIPT com botão segurado e PAD_STICK com vários intervalos ([#8](https://github.com/NoahBPeterson/ps3recomp/pull/8)),
  command buffer do Metal com erro ([#9](https://github.com/NoahBPeterson/ps3recomp/pull/9)),
  diagnósticos do travamento ([#10](https://github.com/NoahBPeterson/ps3recomp/pull/10)).
- [x] Jump table do lifter enviada ao upstream ([sp00nznet/ps3recomp#311](https://github.com/sp00nznet/ps3recomp/pull/311)).
- [x] Opcodes de pack/unpack: fork (#5) e upstream ([#314](https://github.com/sp00nznet/ps3recomp/pull/314)).
  Também no upstream, porque o código é o mesmo no master: FIFO estacionado
  ([#312](https://github.com/sp00nznet/ps3recomp/pull/312)), decrementer
  ([#313](https://github.com/sp00nznet/ps3recomp/pull/313)), cellGem/cellCamera
  ([#315](https://github.com/sp00nznet/ps3recomp/pull/315)) e command buffer do Metal
  ([#316](https://github.com/sp00nznet/ps3recomp/pull/316)).
- [x] Write-back dos render targets para a memória do guest antes dos blits NV3089
  ([#11](https://github.com/NoahBPeterson/ps3recomp/pull/11)), na versão validada
  (linha própria do blit, snapshot de profundidade invalidado, `PS3RECOMP_ENG_WRITEBACK`).
- [ ] Pedir para listar o port no README do ps3recomp (só quando este repositório for público).
- [ ] Ficaram de fora, por enquanto: o layout do `sys_lwcond_t` para o upstream (o PR #213
  já o traz) e as ferramentas da investigação do travamento ainda em aberto
  (`GCM_SEMA_ACQUIRE`/`GCM_SEMA_TRACE`, limites de `SPU_PUT_EA`, `LWCOND_TRACE`, a espera de
  250 ms do push de evento do SPU), que vão quando a causa estiver fechada.
- [ ] (novo) Ainda não enviados: FACING/TC8/TC9 e `perspective_corr` no decompilador de fragment programs,
  o watchdog que respeita o estacionamento do FIFO, os jobs SPU relocáveis
  (`--relocatable`) e o mouse para a câmera.

Os branches vivem em dois worktrees do `ps3recomp-fork/`: `ps3recomp-pr/` (base 770660a,
para o fork) e `ps3recomp-up/` (base no master do upstream); o remote `mine` aponta para
o fork no GitHub.

---

O repositório nunca terá dados do jogo, do firmware ou nada traduzido a
partir deles (`game/`, `fw/`, `vfs/`, `recompiled/`, `lle/`, `lle_spu/`,
`spu/`, `prx/`). Cada pessoa gera isso localmente a partir da própria cópia.
