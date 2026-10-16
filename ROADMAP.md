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

O que separa isso de "100% jogável" está logo abaixo: o fps (7–16, o jogo foi
feito para 30), um travamento aleatório numa cutscene do prólogo, erros
gráficos que restam em New Marais e o fato de que ninguém passou ainda de 1% do
jogo.

---

## O que falta para ficar 100% jogável

### Bloqueantes

- [ ] Fps em New Marais (7–16): perfilar por thread para achar o gargalo (a sessão do `tools/profile_run.sh`).
  - Atualização: o prólogo já foi perfilado (ver "Desempenho" em Feito): 12 fps nas cutscenes e no píer, com a thread principal do PPU ~80% parada, 59% num laço `usleep(10)` (`func_00350CE0`) esperando as "SPU light tasks" do quadro. Em New Marais o gargalo **não** é o SPU (traduzir os jobs relocáveis tirou 98% do interpretado e o fps não mexeu); falta o perfil de lá. O jogo trava em 30 fps, então esse é o alvo.
- [ ] Reduzir o que ainda passa pelo interpretador SPU: no prólogo já é só 3% do tempo de SPU (quase tudo no stub de LS 0x3F000); em New Marais os jobs relocáveis foram traduzidos (7a74368) e o interpretado caiu de 14,8 G para 0,2 G instruções por 8 min. Falta perfilar New Marais com o mesmo relatório.
- [ ] Cortar a sobrecarga do runtime SPU: tentado em 2026-10-15 (checagens por passo, `spu_overlay_note_get`, conversões NEON, LS inline, QoS das threads SPU). A sobrecarga medida caiu de 23–31% para 15–22%, mas o fps não mudou (12 no cutscene), a espera do PPU pelas light tasks ficou igual, e o Erik viu cutscenes lentas, áudio ruim e iluminação piscando/branca na janela logo depois. Revertido. O tempo das SPUs está no próprio código de iluminação traduzido (`jobmod_c6e7383dc013`, datamods `0087f880`/`0087ec80`: fm/fma/shufb), não na sobrecarga; os fallbacks de float são só 1,5–2,6%.
  - (novo) Próximo passo: olhar a qualidade do código que o lifter gera para esse job de iluminação (vetorização das operações de float e shuffle).
  - Tentado em 2026-10-15, com as ideias do port do Ben 10 (andrebrumdev/ben10-omniverse-recomp, `notes/`): `shufb` NEON sem `rev32` (6 operações em vez de 11), `spu__perm` sem `rev32` (família `rotqby`/`shlqby`) e `fceq`/`fcgt`/`fcmeq`/`fcmgt` em NEON, todos bit a bit iguais às referências. A/B intercalado com `profile_run.sh newmarais 480` (4 corridas): gameplay 12,9 e 14,7 fps (antes) contra 13,7 e 13,9 (depois), média ~13,8 nos dois; a função quente `datamod_0087f880 func_7320` caiu ~9% nas amostras. Sem ganho de fps; revertido. O código está em `infamous2-lle/patches/experiments/spu-neon-helpers.patch` (com os testes) para quando o gargalo de verdade sair. A variação entre duas corridas do mesmo binário (12,9 contra 14,7) é maior que o efeito: um A/B de fps aqui precisa de pelo menos 2 corridas por lado, intercaladas.
- [ ] Travamento intermitente na cutscene (9 execuções em 14): na cena "cityfalls" (Cole, Kuo e Zeke olhando a cidade) o jogo continua renderizando, mas a cena nunca termina. Diagnóstico com `tools/watch_stall.sh` (amostra o processo quando a cena não acaba) e os contadores novos do toolkit (`SPU_INTERP_HIST`, `SPU_INTERP_ENTRY_LOG`, `SPU_IRQSTAT`): um dos seis SPUs fica preso no job de **geometria Edge** (`jobmod_d0d15ec83992`, strings "EDGE ASSERTION FAILURE", skinning, culling), rodando código traduzido num laço que só sai por orçamento de tempo ou quando o trabalho acaba; sem interrupções, sem DMA; o loop passa pelas rotinas atômicas da alocação do ring de saída. A thread principal fica em `cellSpursGetWorkloadInfo` + `usleep(1)` esperando o job. Duas entradas que faltavam noutro módulo (`ab654d30fd90`, LS 0x15280/0x155EC) foram re-traduzidas pelo caminho, e o decrementer do SPU tinha um overflow de 64 bits aos 231 s/462 s (corrigido). Descartado depois: o job chain do Edge (`SPU_ATOMICSTAT`), o áudio (`AUDIO_WAV` continua crescendo a 384 KB/s), as `lwcond` do FIOS (`LWCOND_TRACE`), filas de evento cheias e corrida de `lseek`+`read` entre threads (`FS_READ_ALL` com tid) — tudo normal na parada. O que acontece (visto com `PPU_WWATCH` na lista de grupos de atualização em 0xA8EE50 e lldb no processo vivo): no início da cena o jogo troca de "mundo": `func_002F4E20` libera o grupo 0 (`func_001BF4C4`) e zera a entrada; numa execução que passa, os assets do mundo novo entram no registro e o grupo 0 é recriado (`func_002F02FC`); numa que trava, a thread principal fica consultando o registro por hash (`func_00662EF0`) para sempre. A diferença nas leituras do `infamous1.psarc_s`: a execução boa relê `/jbi_intro_001#31.xpps` depois da troca; a travada não (acha que já tem o pré-carregamento feito segundos antes, que pertencia ao grupo liberado). É uma corrida do próprio jogo entre o pré-carregamento do stream da cutscene e a troca de mundo, que o console nunca perde porque o inflate em SPU (Edge Zlib) termina a tempo. Medido depois (play33, saudável): o pré-carregamento do #31 termina ~158 s antes da troca, então lentidão pura não é o mecanismo. O inflate é a task SPURS Edge Zlib (`spu/images/spu_0000_at_007FA100.elf`, imagem 1), e o log de entradas do interpretador mostra que ela roda em parte **interpretada** (entradas em LS 0x9C84 e 0x9DB8, dentro do código da task, que a descoberta automática de funções não via); uma task de inflate interrompida no meio pelo scheduler enquanto está no interpretador é a hipótese atual para dados descomprimidos inválidos de vez em quando. Em teste: imagem 1 re-traduzida com essas entradas; se não bastar, `SPU_LOCKSTEP=1` e a substituição do inflate por zlib do host (recomendada nos docs do ps3recomp).
- [ ] Jogar daí em diante (a Besta, a chegada a New Marais) com teclado ou controle numa execução com janela; a entrada por script só vai até onde o roteiro alcança.
  - Atualização: com DualSense e janela o Erik passou do prólogo e chegou a New Marais (autosave "3. Forward Momentum", 1%). Com teclado ainda não.
- [ ] (novo) Jogar o jogo inteiro e anotar o que quebra: todas as missões da história, os dois caminhos de karma, os poderes novos (gelo/fogo), as cutscenes em quadrinhos (Bink), os chefes, a parte alagada e a de Gas Works, as missões secundárias e os dois finais. `tools/play_capture.sh` grava o que um bug gráfico precisa numa sessão com janela.

### Gráficos

- [ ] Gráficos em New Marais: janelas do barco do Zeke saem como retângulos pretos e blocos de madeira do guindaste saem azul-turquesa (conferir de novo depois do conserto do FACING). Retângulos escuros translúcidos perto do Cole (captura de tela do Erik, 01:35). Inimigos "pretos e sem textura" relatados pelo Erik ainda não reconferidos depois do conserto do FACING.
- [x] Objetos estourados em branco (barco no reboque, amplificador do Cole, caixote do píer, lixeira, guincho) e retângulos de brilho alinhados à tela perto dos personagens (capturas do Erik, 11:59). Causa, achada em 2026-10-15 à noite: o walker do FIFO não bloqueava nos ACQUIREs de semáforo (padrão do toolkit), então o compositor diferido (FP 0xF384F111AD5956BA) amostrava os buffers XDR 0x07000B80/0x07384B80 enquanto ainda tinham as cópias NV3089 das normais e da profundidade; o "SPU Light Task" escreve a luz ali alguns ms depois (tiles de 32x40 em volta de cada luz, zero no resto, dump no píer). Com as normais no lugar da luz, `tex5.a` (byte alto da profundidade, ≈1) ligava o reflexo do cubemap em tudo que tem brilho (branco estourado); com máscaras de MRT por alvo a luz saía na cor da normal (verde/magenta); os tiles já escritos pelo SPU eram os retângulos. Conserto: `run.sh` exporta `GCM_SEMA_ACQUIRE=1` e `GCM_SEMA_ACQUIRE_MS=200` (ACQUIRE bloqueante com limite, igual ao hardware; nenhum estourou do boot ao píer; fps igual dentro do ruído). O amplificador volta a ser metal escuro, as cintas das tábuas e o tambor ficam normais. Para olhar os buffers: `PS3RECOMP_ENG_MEMDUMP=47500B80:708000:238s:<arquivo>` (os dois, no píer). Também corrigido no caminho: a expansão `UNSIGNED_REMAP_BIASED` (bit 12 de SET_TEXTURE_ADDRESS) vale para toda leitura da textura, não só TEX com EXP_TEX (o compositor lê as normais assim). Os retângulos escuros translúcidos em volta de props (máquina de venda, caixotes) tinham outra causa, achada em 2026-10-16 de madrugada: o passe de volumes de oclusão (p113) consulta uma textura **3D 8×8×8** (FORMAT dimensão 3, CONTROL3 profundidade 8) com direção/ângulo de cone por célula, e o motor não tinha texturas 3D (subia a primeira fatia como 2D e o shader amostrava `.xy`). Texturas 3D implementadas de ponta a ponta (layout Morton 3D, upload por fatia, `MTLTextureType3D`, `Texture3D`/`.Sample(xyz)` no decompilador, chave de pipeline): o shader do p113 agora declara `Texture3D rsx_tex2` e a caixa da máquina de venda some. Ainda conferir com o Erik na praça ao entardecer (luzes de rua) e o reflexo do céu no rio.
- [x] **Cubos pretos em volta de props (mesas do café no terraço do EXPO, foto do Erik 22:19) e normal maps sem efeito (2026-10-16):** o decompilador só expandia (x·2−1) leituras de textura quando o registrador da unidade pedia `UNSIGNED_REMAP_BIASED`; mas o modificador `_bx2` (EXP_TEX, bit 21 do OPDEST) é da instrução, não da textura, e expande sempre. 213 dos ~615 fragment programs do jogo usam `TEX_bx2`: os normal maps tangentes (p181 `tex0`) e a LUT 8×8×8 das caixas de oclusão (p113, valores simétricos em torno de 128) chegavam sem sinal → normais erradas em todo material com normal map e direção do cone sempre no octante positivo → cobertura≈1 → caixa inteira preta. Agora `_bx2` expande as 4 lanes sempre (`rsx_fp_texture_bx2_units`). Também: o snapshot de profundidade entrega o **stencil** na lane B (o compositor lê as flags de material dali: bit 7 escala do ambiente, bit 5 desliga o SSAO; p113/p114 excluem personagens pelo bit 6), antes era 0. Ferramentas novas: `tools/play_dump.sh <nome>` (sessão na janela; Enter no terminal grava o quadro, todos os render targets, pass log e shaders — `PS3RECOMP_METAL_DUMP_TRIGGER=<arquivo>`), `PS3RECOMP_METAL_SURF_DUMP_AT=<s>`, analógico direito no `PAD_SCRIPT` (0x100000 cima, 0x200000 baixo, 0x400000/0x800000 lados), log `texture bind FAILED`, `DUMPTEX` para R16/R8, comentário `/* EXP_TEX unexpanded */` nos shaders dumpados.
- [ ] (novo) Retângulos **claros** translúcidos em volta de personagens/NPCs (fundo atrás deles mais claro, bordas retas): vêm dos passes de luz em volume (pular todos os FPs p116–p122/p303 os remove; só o p120 — luz em caixa com o mesmo VS dos volumes de oclusão — não basta). Esses passes desenham as faces traseiras do volume com LEQUAL e reconstroem a posição da profundidade (a simulação offline do p114, mesmo VS e mesma leitura de profundidade, dá alpha≈1 longe da caixa, então a entrada parece certa); descartados (2026-10-16): convenção do `position.w` (`PS3RECOMP_FRAGCOORD_W=w` não muda), rampa 1024×8 (é a LUT de especular, lida de volta), constantes por draw (staging por record), load das attachments de profundidade/stencil, stencil (p116/p120 rodam com teste desligado), cull/winding/compare (iguais ao RPCS3). O p118 (luz de personagem) com constantes + profundidade reais da cena "FOR RENT" simula atenuação ≈ 0 (as luzes capturadas estavam a ~110 m); pular p116, p117/118/303, p120, p122 ou p119+p121 **isoladamente** deixa caixas fracas, só pular todos limpa. Teste rápido na janela: `PS3RECOMP_ENG_SKIP_FP=d98fefc56d43594b,83d934dc18efcea3,db0f414789b4ad56,6d715ad65444fa49,c5e0d94ef8e23ba3,7f104f9b0f3b938f ./run.sh` (sem luzes-volume) e depois um hash por vez. Dumps por hash: `PS3RECOMP_ENG_FPK_FP=<hash> PS3RECOMP_ENG_FPK_AFTER=<s>` (constantes FP+VP) e `PS3RECOMP_ENG_DUMPTEX_FP=<hash>:<unidade>:<s>:<arquivo>` (textura lida de volta); simuladores `p118_sim.py`/`p120_sim.py`/`p114_sim.py` reproduzem a conta dos shaders a partir desses dumps. Ferramentas: `PS3RECOMP_ENG_SKIP_FP=<hash,..>` (pula draws por hash do FP, estável entre execuções; `manifest.txt` no `PS3RECOMP_ENG_SHADER_DUMP` lista p= → fp/vp), `PS3RECOMP_METAL_DBG_DUMPTEX=<p>:<unidade>:<seg>:<arquivo>` (lê de volta a textura que um pipeline amostra), `PS3RECOMP_ENG_TEXSTATE_PIPE=<p>` (registradores de textura de um pipeline, com dimensão/profundidade/cor de borda), `PS3RECOMP_ENG_FPK_AFTER`/`[vpk]` (constantes FP e VP de um draw numa cena), `PS3RECOMP_ENG_VPC_WATCH=<p>:<c>`, `TCONST_SLOT`/`TCONST_AFTER`, e o analógico esquerdo no `PAD_SCRIPT` (bits 0x10000 cima, 0x20000 baixo, 0x40000/0x80000 lados, `:<seg>` segura) — `175:0x10000:9,184:0x90000:8,192:0x10000:25,218:0x80000:3,221:0x10000:30` leva o Cole do píer à rua do Club Bar (não determinístico: ~1 em 3).
- [x] "SPU Light Task": roda a cada frame, consome as cópias XDR das normais/profundidade e escreve de volta a luz difusa (0x07000B80: RGB luz, A intensidade) e a especular (0x07384B80), em tiles de 32x40 só onde há luz; no menu não há luz nenhuma, por isso parecia não escrever. O RSX precisa esperar o label 137 antes do compositor (ver item acima).
- [ ] (novo) Menu principal: o contorno cinza-azulado do Cole e os destroços escuros não aparecem; há um borrão cinza estático sob a barra que não existe no original.

### Estabilidade

- [ ] Congelamento no gameplay (a imagem para de vez; ~1 execução em 5 em New Marais, e no píer diurno do `m2` aos ~40 s de jogo): o walker do FIFO para num JUMP para si mesmo (o "estacionamento" no fim de um bloco de comandos montado pelas SPUs, que o jogo depois troca por um JUMP para o bloco seguinte) e o watchdog "stuck-get" desiste dele depois de 8 passadas, poucos milissegundos, antes do prazo próprio do estacionamento (`GCM_PARK_GRACE_MS`), pulando para o `put` e perdendo os comandos do quadro. Corrigido: o watchdog não conta passadas paradas num estacionamento (o prazo de 50 ms decide). Validação em andamento (3 execuções a partir do save). Não é a causa do travamento da cena "cityfalls": a maioria das execuções travadas ali não tem nenhum stuck-get.
  - Atualização: as 3 execuções de validação rodaram sem congelar (ee6ed84). Falta confirmar em sessões longas com janela.

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
  - [ ] MetalFX (macOS, equivalente da Apple ao DLSS) — caminho principal neste Mac.
  - [ ] AMD FSR (espacial, portável).
  - [ ] NVIDIA DLSS e Intel XeSS: só fazem sentido numa build Windows com GPU NVIDIA/Intel.
- [ ] Tela cheia, janela redimensionável e alternância por atalho.
- [ ] Widescreen/ultrawide (21:9): FOV e HUD corretos.
- [ ] Taxa de quadros: desbloquear de 30 fps onde a lógica do jogo permitir.
- [ ] #307 (snapshot de render target) do upstream: exige terminar o merge do motor de desenho (`rsx_draw_engine.c`).

### Desempenho

- [ ] `libgcm_sys` e `libfs` em LLE (o RPCS3 roda os dois assim).

### Outras plataformas

- [ ] Linux (Vulkan, já existe no toolkit).
- [ ] Windows (D3D12, o backend mais maduro do toolkit).

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
