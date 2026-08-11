# Bugs e Pontos de Atenção

Lista viva de problemas/riscos encontrados lendo o código. Objetivo: não
tomar decisões de "incrementar o algoritmo X" sobre uma base que já tem um
bug de corretude escondido. Atualizar esta lista conforme formos mexendo.

Itens já corrigidos/removidos ficam com uma explicação breve (o histórico
completo de investigação/tentativas está no `git log`); itens ainda em
aberto mantêm todo o detalhe.

## 1. [CORRIGIDO] `genRot` sempre gerava/reusava a porta `"Rot_0"` — afetava o Shor

**Onde:** `src/algorithms/lib_shor_circuits.cpp`.

`genRot` montava o nome da porta (`"Rot_" + int2str(phase_bits)`) **depois**
de um laço que já tinha consumido `phase_bits` até zero — toda porta virava
`"Rot_0"`, e como `Gates::addGate` não sobrescreve um nome já existente, só
a primeira correção de fase de cada execução do Shor era realmente
aplicada. Corrigido guardando o valor original antes do laço consumi-lo.

## 2. [REMOVIDO] Código morto: `CpuExecution2_*`, `CpuExecution3_*` e `CU()`

Duas implementações alternativas do mesmo cálculo (diagonal principal/
diagonal secundária) e uma função com o corpo inteiro comentado, nenhuma
chamada em lugar nenhum do projeto (`DGM::execute()` só despacha pra
`CpuExecution1_*`). Removidas após confirmar por grep que não havia
nenhum chamador.

## 3. [CORRIGIDO] `PT::destructor()` nunca liberava `control_bit_positions`/`control_rest`

**Onde:** `src/core/common.cpp`.

As três condições de `free()` estavam invertidas (só rodavam quando o
ponteiro já era `NULL`), então nada útil era liberado. `matrix` foi
deixado intocado de propósito — é um ponteiro emprestado de
`Gates::list`, não pertence ao `PT`. `control_bit_positions`/
`control_rest` passaram a ser liberados corretamente. Essa correção,
sozinha, expôs o bug do item 3.1 abaixo.

## 3.1. [CORRIGIDO] `DGM::genPTs` aloca `PT` com `malloc()` sem inicializar `control_bit_positions`/`control_rest`

**Onde:** `src/core/dgm_parser.cpp`.

Consequência direta do item 3: `malloc()` não zera os campos, então o
`free()` recém-corrigido passou a liberar ponteiros de lixo
(`free(): invalid pointer`) em toda porta sem controle. Corrigido
inicializando os campos explicitamente, e depois superado de vez
trocando o par `malloc`/`destructor()` manual por `new PT()`/`~PT()`
real em todo o projeto.

## 4. [REMOVIDO] Código morto: `GenericExecute` e `GpuExecution`/`GpuExecution2`/`GpuExecution3`

`GenericExecute` (duas sobrecargas) nunca tinha chamador em lugar nenhum
do repositório; `GpuExecution`/`GpuExecution2`/`GpuExecution3` nunca
tiveram implementação alguma, só a declaração. Removidas após
confirmação por grep. O risco de posse ambígua de memória que motivava
o item original (quem chamava `GenericExecute` precisava não deixar a
`DGM` liberar um ponteiro que não era dela) deixou de existir junto com
o código.

## 5. [CORRIGIDO] `Gates::list` era `static` (compartilhado entre todas as instâncias)

**Onde:** `include/core/gates.h`, `src/core/gates.cpp`.

Causa raiz de por que o bug do item 1 se manifestava como se
manifestava: como o cache de matrizes era global, portas com o mesmo
nome geradas em execuções (ou rodadas) diferentes colidiam. Virou um
campo não-estático (`DGM::gates`), passado por referência por todas as
funções que constroem circuito dinamicamente.

## 6. [CORRIGIDO] Segfault em `t_PAR_CPU`/`t_HYBRID` quando `region_bits > qubits` disponíveis

**Onde:** `src/core/dgm_par_exec.cpp` (`PCpuExecution1` e
`HybridExecution`, dois pontos).

Valores de `region_bits` fixos (14 na CPU, 20 no HYBRID) sem checar
contra o `qubits` pedido causavam deslocamento por expoente negativo
(`1 << (qubits - region_bits)`), escrevendo memória fora do vetor
alocado. Corrigido com um clamp (`if (region_bits > qubits) region_bits
= qubits`) nos três pontos onde a conta aparecia.

## 7. Build de `kernel.cu` anormalmente lento (`nvcc`/`cicc`) numa máquina sem GPU

**Onde:** `src/core/kernel.cu`, build via WSL2 sem hardware NVIDIA.

Compilar `kernel.cu` com `nvcc` chegou a levar mais de 2 horas (processo
`cicc` preso em ~100% de CPU) mesmo depois de reduzir drasticamente o
número de instanciações de template (`GEWrapper2`/`GpuExecutionWrapper`,
normalmente ~260 combinações de `block_size`/`repeat_count`/`coalesced_bits` — ver
[docs/04-gpu-cuda.md](04-gpu-cuda.md)) e de baixar a otimização pra
`-O0`. Mesmo com **uma única instanciação**, não terminou em 5 minutos —
ou seja, não é sobre volume de templates nem nível de otimização; é algo
mais fundamental (ambiente, WSL2 acessando `/mnt/c`, antivírus escaneando
arquivos temporários, ou algum problema específico dessa instalação do
`nvcc`/`cicc`) que não foi diagnosticado até o momento.

**Contorno aplicado:** `src/core/kernel_stub.cpp` — implementação das
mesmas funções `extern "C"` de `kernel.cu`, compilada com `g++` (sem
`nvcc` nenhum), que só imprime um aviso e retorna se alguém tentar usar
`t_GPU`/`t_HYBRID`. É o padrão do `makefile` agora (`GPU=stub`); use
`make GPU=real` para compilar o `kernel.cu` de verdade quando for
investigar isso com calma ou tiver acesso a uma máquina com GPU NVIDIA.
`kernel.cu` em si não foi alterado por causa disso.

**Como investigar no futuro:** testar compilar de dentro do sistema de
arquivos nativo do WSL (`~/...`, não `/mnt/c/...`) com uma única
instanciação isolada; testar com o Windows Defender desligado
temporariamente pra pasta do projeto; testar uma versão do CUDA Toolkit
diferente.

**Atualização (2026-08-11, máquina nova com GPU NVIDIA real):** nesta
segunda máquina (RTX 4070, Ada Lovelace) foi possível, pela primeira vez,
testar `kernel.cu` com hardware de verdade — WSL2 (Ubuntu 24.04) + CUDA
Toolkit 13.3.1 instalado via `apt` (repo `wsl-ubuntu`, sem instalar driver
Linux — o WSL usa o driver do Windows por baixo), repositório clonado
dentro do filesystem nativo do WSL (`~/projetos/simulador_quantico`, não
`/mnt/c/...`).

Primeiro build com `make GPU=real` revelou um bug real, não relacionado à
lentidão — ver item 9. Com esse fix, `make GPU=real` (as ~260
instanciações de template intactas, sem nenhuma das reduções do item 06
aplicadas) rodou em **~2.5s**, não horas. Ou seja: a lentidão documentada
acima não é inerente ao volume de templates do `kernel.cu` — é específica
de alguma característica do ambiente da máquina anterior (mais provável:
acesso via `/mnt/c/`, já que essa é a única variável que mudou junto com
a arquitetura corrigida aqui; não dá pra descartar antivírus ou a
instalação do `nvcc` daquela máquina especificamente, já que nenhuma
delas foi reproduzida/testada aqui). O item 06 (reduzir templates, ver
item 10) segue sendo uma proposta válida por outros motivos (simplicidade,
tempo de build em máquinas mais fracas), mas deixou de ser urgente
**naquela máquina** especificamente.

**Verificado nesta máquina:** `general.out 10 2 1` (`t_GPU`) e
`general.out 10 3 2` (`t_HYBRID`) reproduzem a amplitude uniforme exata
`0.03125`, confirmando que o kernel real está calculando certo (não só
"não crasha"). `shor.out 15 2 1` (`t_GPU`) rodado 8×: 5/8 sucesso, contra
2/8 do `t_CPU` nas mesmas 8 rodadas — dentro da variação probabilística já
documentada do algoritmo, sem indício de bug específico de GPU.

## 8. [CORRIGIDO] `grover.cpp`/`shor.cpp` ignoravam `exec_type` na hora de chamar o algoritmo

**Onde:** `src/cli/grover.cpp`, `src/cli/shor.cpp`.

Os dois CLIs faziam o parsing e a validação de `exec_type` normalmente,
mas na hora de chamar `Grover(...)`/`Shor(...)` passavam o literal
`t_CPU` fixo — pedir `t_PAR_CPU`/`t_GPU`/`t_HYBRID` na linha de comando
era aceito e ajustava `thread_count`, mas o programa rodava em `t_CPU`
mesmo assim. `general.cpp` nunca teve esse problema. Corrigido trocando
o literal pela variável `exec_type`. Qualquer teste de `t_HYBRID`/`t_GPU`
feito com `grover.out`/`shor.out` antes dessa correção rodou, na
prática, em `t_CPU`.

## 9. [CORRIGIDO] `makefile` mirava em `-arch=sm_52`, arquitetura que o CUDA 13.x nem compila mais

**Onde:** `makefile`.

Descoberto ao testar `make GPU=real` pela primeira vez com uma GPU
NVIDIA de verdade (RTX 4070). `nvcc fatal: Unsupported gpu architecture
'sm_52'` — Maxwell, arquitetura antiga demais pro CUDA 13. Corrigido pra
`-arch=sm_89` (Ada Lovelace) — quem usar outra geração de GPU precisa
ajustar esse valor pra sua própria compute capability, não há detecção
automática.

## 10. [CORRIGIDO] Item 06 do design de arquitetura implementado — templates de `kernel.cu` viraram parâmetros de runtime

**Onde:** `src/core/kernel.cu`, `src/core/dgm_core.cpp`. Detalhes
completos em [04-gpu-cuda.md](04-gpu-cuda.md) seção 4.

As ~260 instanciações de template (`block_size`/`repeat_count`/
`coalesced_bits`) viraram parâmetros comuns em runtime, com shared
memory dinâmica (`extern __shared__`) no lugar de tamanho fixo por
template, e as duas camadas de `switch` de despacho foram removidas.
Expôs um bug real (não introduzido pela mudança): combinações
inconsistentes de tuning causavam `illegal memory access` na GPU,
corrigido com validação explícita em `DGM::validateTuning()`. Verificado
com GPU real: amplitude correta em combos antigos e novos, `make test`
completo sem regressão.

## 11. [REMOVIDO] Segunda leva de código morto: `enablePeerAccess`, `GET_BLOCK_ID`, `report_num_threads`, `quantum_ipow`, `HadamardNQubits_PAR_CPU`/`_GPU`, macros `ACUMM`/`SHIFT_READ`/`SHIFT_WRITE`/`MAT_START`/`MAT_SIZE`/`MAT_END`, `LINE`/`BASE`, `t_SPEC`, `DGM::genMatrix`, `PT::ctrlAffect`/`setArgs`/`setArgs_soft`

Confirmado por grep em todo o repositório: zero chamadores pra cada um
desses símbolos, boa parte já marcada como "sem uso" numa passada de
comentários anterior. `PT::span_start_bit` e `PT::control_rest`/
`control_rest_count` foram mantidos de propósito — placeholders de
features nunca finalizadas (portas multi-qubit, otimização de controle
parcial), não código órfão de verdade.

## 12. [CORRIGIDO] `Grover()` media o resultado certo e descartava — `grover.out` nunca dizia se a busca funcionou

**Onde:** `src/algorithms/lib_grover.cpp`, `src/cli/grover.cpp`.

`Grover()` montava `result` a partir de `dgm.measure()`, mas só
retornava `float elapsed` — o resultado nunca saía da função, e
`grover.out` nunca dizia nada além de "não crashou". Corrigido: `Grover()`
agora retorna `long result` (o timing migrou pra `grover.cpp`, mesmo
padrão que `Shor()`/`shor.cpp` já usavam), e `grover.cpp` imprime se
encontrou o valor buscado.

## 13. Multi-GPU (`gpu_count > 1`): nunca testado com hardware real — um bug corrigido, um suspeito documentado

**Onde:** `src/core/kernel.cu` (`GpuExecution01`),
`src/core/dgm_core.cpp` (`DGM::validateTuning()`).

Nenhuma sessão deste projeto teve acesso a mais de 1 GPU NVIDIA real —
todo o caminho de código pra `gpu_count > 1` nunca foi exercitado de
verdade, só lido.

**[CORRIGIDO]** `gpu_mem[4]`/`gpu_pointer[4]` são arrays de tamanho fixo
4 sem validação de `gpu_count` contra esse limite — `gpu_count > 4`
causaria escrita fora dos limites (undefined behavior silencioso, não
pego por `cudaGetLastError()`). Corrigido com validação explícita em
`DGM::validateTuning()`, testável sem hardware extra por depender só do
inteiro vindo da CLI.

**[SUSPEITO, NÃO CORRIGIDO] `is_peer` em `GpuExecution01`
([kernel.cu](../src/core/kernel.cu), comentário no local) provavelmente
calcula errado pra `gpu_count > 2`.** A fórmula
`qubits - gpu_count + 1` trata `gpu_count` como se fosse diretamente o
número de bits que separam as fatias entre GPUs — mas o número de bits
certo é `log2(gpu_count)` (é esse valor que decide em qual fatia/GPU um
índice cai, via `global_index/gpu_slice_size` no kernel). As duas contas
só coincidem pra `gpu_count <= 2` (`log2(2) = 1 = 2-1`); pra
`gpu_count = 4`, por exemplo, a conta usada dá `qubits-3` onde deveria
dar `qubits-2`. Pela direção do erro (o limiar calculado fica menor que
o correto), a suspeita é que isso torna `is_peer` **mais fácil** de dar
verdadeiro do que deveria — ou seja, provavelmente causa sincronização
extra desnecessária (bug de performance) em vez de sincronização faltando
(bug de corretude/race condition), mas isso é dedução por matemática, não
verificação empírica. **Não corrigido** — um ajuste às cegas numa
fórmula de sincronização de GPU sem poder testar contra hardware real (o
mínimo seria 3+ GPUs físicas, já que `gpu_count=2` é justamente o caso
em que a fórmula atual está certa) é mais arriscado que documentar e
esperar acesso a uma máquina com múltiplas GPUs de verdade.

## 14. [REMOVIDO] `ApplyQFT`/`QFT2` — código morto que o próprio comentário dizia estar em uso

**Onde:** `src/algorithms/lib_shor.cpp`, `src/algorithms/lib_shor_circuits.cpp`,
`include/algorithms/lib_shor.h`.

`ApplyQFT` tinha um comentário dizendo "usado como teste/benchmark
independente", mas grep não achou nenhum chamador em lugar nenhum do
repositório; `QFT2` só era chamada por `ApplyQFT`, ficou órfã junto.
Tinha de quebra um bug interno (nomes de campo trocados) que nunca seria
pego por ninguém rodando, já que a função é código morto. Removida em
vez de corrigida.

## 15. [CORRIGIDO] `Shor()` nunca liberava `dgm.state` — vazava o vetor de estado inteiro em toda chamada

**Onde:** `src/algorithms/lib_shor.cpp`, `src/core/dgm_core.cpp`,
`include/core/dgm.h`.

Nenhum dos 4 pontos de retorno de `Shor()` chamava `dgm.freeMemory()`, e
`DGM::~DGM()` só limpava `pts`, nunca `state` — de ~256KB a mais de 1GB
vazados por chamada, invisível na prática só porque `shor.cpp` roda
`Shor()` uma única vez por processo. Corrigido com RAII real:
`DGM::~DGM()` passou a chamar `freeMemory()` incondicionalmente (`state`
é sempre de posse exclusiva da `DGM` que o alocou, confirmado por grep).
Verificado com um harness dedicado: RSS estável (~12MB) em 30 chamadas
seguidas, contra crescimento linear até 128MB antes do fix.

## 16. [CORRIGIDO] `srand(time(NULL))` em `grover.cpp`/`shor.cpp` — processos lançados no mesmo segundo repetiam a "mesma aleatoriedade"

**Onde:** `src/cli/grover.cpp`, `src/cli/shor.cpp`.

Um laço de shell chamando o binário repetidas vezes rodava rápido o
bastante pra várias chamadas caírem no mesmo segundo de relógio,
repetindo exatamente a mesma sequência de `rand()` — reduzindo "N
tentativas independentes" a 1-2 sementes únicas repetidas. Corrigido com
`srand(time(NULL) ^ getpid())`. **Superado depois pelo item 18**, que
trocou `rand()`/`srand()` por `std::mt19937` em todo o projeto de vez.

---

## 17. [PARCIALMENTE CONFIRMADO] `t_HYBRID` não bate CPU nem GPU de forma consistente — hipótese A confirmada, hipótese B refinada

**Onde:** `DGM::HybridExecution` em
[dgm_par_exec.cpp:258-472](../src/core/dgm_par_exec.cpp#L258) — agora com
instrumentação opt-in (`HYBRID_DEBUG=1` no ambiente, silenciosa por
padrão) que imprime, por região "global" processada, se foi CPU ou GPU.

Item aberto por uma sessão anterior (lendo o código, sem GPU real
disponível lá) a partir dos números de
[docs/08-performance.md](08-performance.md). Duas hipóteses propostas,
agora testadas com a instrumentação nesta máquina (GPU real):

**Hipótese A — confirmada.** `general.out 10 3 4` (10 qubits, abaixo de
`qubits_limit=20`): log mostra **exatamente 1 região no total**, ganha
por uma thread de CPU, **0 regiões pra GPU**. Bate exatamente com a
hipótese: abaixo do limite fixo, `global_region_bits == qubits` sempre,
então só existe uma região pra disputar, e quem ganha a corrida
(`#pragma omp critical`) faz 100% do trabalho sozinho.

**Hipótese B — parcialmente confirmada, mais nuançada que a formulação
original.** `general.out 24 3 4` (24 qubits, acima do limite): log
mostra **62 regiões CPU e 34 regiões GPU** — a GPU processou uma fatia
razoável (35%) do trabalho, não "só 1 região" como a hipótese original
especulava. O problema não é falta de regiões pra GPU; é que **cada
região processada pela GPU paga o overhead fixo de lançamento de kernel
inteiro** (~0.2s medido em `docs/08-performance.md` pra um único
lançamento cobrindo o estado inteiro) — 34 lançamentos fragmentados
custam mais overhead acumulado que teria custado 1 lançamento grande
cobrindo tudo de uma vez. A fila round-robin distribui **número de
regiões** igualmente entre CPU e GPU, não tempo esperado — daí o
desbalanceamento.

**Duas tentativas de correção nesta sessão, nenhuma das duas ficou —
ambas testadas com hardware real e descartadas por dados/segurança, não
por preguiça:**

**Tentativa 1 — só subir `qubits_limit`: rejeitada, dados mostram que
piora às vezes.** Varredura empírica (`general.out <q> 3 4`,
`qubits_limit` ∈ {20,22,24,26}):

| qubits_limit | 20q | 22q | 24q | 26q |
|---|---|---|---|---|
| 20 (atual) | 0.074s | 0.307s | **0.397s** | 1.252s |
| 22 | 0.074s | 0.317s | 0.495s | 1.179s |
| 24 | 0.074s | 0.317s | **1.388s** | 2.110s |
| 26 | 0.196s | 0.320s | 1.388s | **0.773s** |

Nada consistente: às vezes melhora (26q/limit=26), às vezes piora muito
(24q/limit=24, 1.388s — bate exato com `PAR_CPU(1)`, single-thread).
Causa: com região única (`qubits == qubits_limit`), quem processa tudo é
**loteria de agendamento do SO** entre as `thread_count` threads
disputando a mesma região — não CPU vs GPU de verdade. Às vezes a GPU
vence (bom, se for uma faixa de qubits onde GPU já é mais rápida — ver
`docs/08-performance.md`), às vezes uma única thread de CPU vence e
processa tudo sozinha, sem paralelismo nenhum (péssimo). Subir
`qubits_limit` só desloca *onde* essa loteria acontece, não resolve o
problema de fundo.

**Tentativa 2 — GPU reserva estaticamente um bloco de regiões
consecutivas (mescladas numa única chamada maior), CPU disputa
dinamicamente o resto: implementada, corrigida, verificada correta —
mas revertida por não melhorar a performance (ao contrário, piora).**

Primeira rodada desta tentativa deu **segfault** em `general.out 24 3 4`.
Investigando o porquê antes de tentar de novo (em vez de ajustar às
cegas): o `+1` de `RegionPlan::region_count` (`plan.region_count = (1 <<
(outer_bound_bits - region_bits)) + 1`) **não é ambíguo, é uma margem de
segurança correta e bem definida** — o laço de reivindicação atribui
`region_id` **antes** de decrementar/checar o contador
(`dgm_par_exec.cpp`, `#pragma omp critical`), então a última região
válida sempre seria descartada por engano sem essa margem extra pra
absorver o descarte. Confirmado por simulação numérica:
`região_count_de_verdade = region_count - 1`, sempre.

O bug real da primeira tentativa era outro, mais sutil: pra "pular"
`gpu_slots` posições da fila compartilhada, a fórmula usada foi
`next_proj_id = gpu_slots * (global_region_mask + 1)` — só válida se
`global_region_mask` for uma faixa de bits baixa **contígua**. Simulação
numérica confirmou que **não é sempre** (ex: 2º lote do Hadamard em 24
qubits, depois que o 1º já consumiu os qubits 0-19: o preenchimento de
`compute_region` deixa um buraco nos bits 16-19), e nesse caso a
multiplicação produzia um `proj_id` **maior que o vetor de estado
inteiro** (`63.176.704` contra `2^24 = 16.777.216`) — acesso de memória
fora dos limites, daí o segfault.

**Corrigido** trocando a multiplicação por um laço aplicando o mesmo
incremento por acarreio que o resto do código já usa
(`(id + mask+1) & ~mask`), `gpu_slots` vezes em sequência — funciona pra
`region_mask` de qualquer formato — e ajustando o limiar de
`use_gpu_batch` pra usar a contagem de verdade (`region_count - 1`, não
`region_count`). **Verificado com GPU real:** amplitude exata correta em
18-30 qubits, `general.out 24 3 4` repetido 5× sem falha nenhuma,
`HYBRID_DEBUG` mostrando múltiplos lotes (inclusive o 2º, o que
segfaultava antes) processados corretamente. `make test` completo sem
regressão.

**Mas a comparação A/B direta (com merge vs sem merge, mesma sessão,
`general.out <q> 3 4`) mostrou que mesclar regiões piora a performance,
não melhora:**

| qubits | sem merge | com merge |
|---|---|---|
| 22 | ~0.25s | ~0.28s |
| 24 | ~0.37s | ~0.50s |
| 26 | ~1.24s | **~2.60s** (mais que o dobro!) |

Isso invalida a premissa da hipótese B: **o custo de ~0.2s por chamada
de GPU provavelmente não é overhead fixo de lançamento de kernel** (que
seria da ordem de microssegundos, não décimos de segundo) — é mais
provável que seja dominado pelo `cudaMemcpy` dentro de
`ProjectState`/`GetState` ([kernel.cu](../src/core/kernel.cu)),
proporcional ao tamanho dos dados copiados, não uma constante por
chamada. Mesclar 4 regiões numa só não elimina esse custo (a mesma
quantidade de dados precisa atravessar host↔device de qualquer jeito) —
e pode até piorar, se o `coalesced_bits` calculado por `ProjectState` a
partir do mask mesclado (que pode ficar menos contíguo que o mask
original) resultar em mais `portions`/chamadas de `cudaMemcpy` menores
em vez de menos. Não confirmado a fundo (exigiria instrumentar
`ProjectState` também), mas é a explicação mais consistente com os
números.

**Revertida nesta sessão** — o mecanismo (agora corrigido e verificado)
não resolve o problema que motivou tentar. Só a instrumentação
`HYBRID_DEBUG` ficou no código.

**Tentativa 3 — instrumentar `ProjectState`/`GpuExecutionWrapper`/`GetState`
diretamente (não mais só `ApplyValuesC01`), como o próprio texto acima
sugeria como próximo passo. Achado real, mas o fix natural não rendeu
ganho medível.**

Adicionado timing (`std::chrono`, opt-in via `HYBRID_DEBUG=1`, mesmo
padrão já existente) às três funções em
[kernel.cu](../src/core/kernel.cu). `general.out 24 3 4` com GPU real:

| fase | total (35 chamadas) | média/chamada |
|---|---|---|
| `ProjectState` (H→D) | 314ms | 8.9ms |
| `GpuExecutionWrapper` (kernel) | 11ms | 0.3ms |
| `GetState` (D→H) | 72ms | 1.9ms |

Confirma de novo que a cópia domina o kernel em ~25x (consistente com a
tentativa 2). Mas decompondo `ProjectState` em `cudaMalloc`/`cudaMemcpy`
separadamente apareceu outra coisa: a **primeira chamada do processo
inteiro** leva sozinha ~253ms, enquanto `cudaMalloc`+`cudaMemcpy`
juntos, nessa mesma chamada, somam só ~2.5ms. Todas as chamadas
seguintes (mesmo formato, mesmo tamanho de dado) levam ~1.5-2ms —
100x mais rápido. Confirmado com um experimento isolado: chamar
`setDevice()` (`kernel.cu`, já existia, nunca era chamado em lugar
nenhum do projeto) antes da região paralela absorve sozinho ~170-250ms,
e depois disso a "primeira" `ProjectState` cai pra 3-6ms — igual às
demais. **É custo de inicialização preguiçosa do contexto CUDA (driver),
pago uma vez por processo — não um custo por região, por chamada, ou
por formato de `mask`/`coalesced_bits` como as tentativas 1 e 2
suspeitavam.**

Isso reabre uma dúvida maior: como `grover.out`/`shor.out`/`general.out`
são processos novos por execução, **todo benchmark de `t_GPU`/`t_HYBRID`
já feito neste projeto** (inclusive a tabela em
[docs/08-performance.md](08-performance.md)) carrega esse ~200ms fixo
embutido, disfarçado de custo do algoritmo — o que pode estar deslocando
artificialmente pra cima o ponto de cruzamento CPU/GPU relatado lá.

**Fix natural testado: warm-up assíncrono.** Implementado
`warmup_gpu_async(exec_type)` ([cli_common.cpp](../src/cli/cli_common.cpp)),
chamado o mais cedo possível em `main()` dos três CLIs — logo depois de
`exec_type` validado, antes de qualquer construção de circuito — que
dispara `setDevice()` numa `std::thread` solta (`.detach()`, sem
`join()`: `setDevice()` só toca o contexto CUDA do driver, não
compartilha estado com o resto do processo, então não tem corrida real
pra proteger). Ideia: sobrepor esse ~200ms com o trabalho de CPU que já
acontece antes da primeira chamada de GPU de verdade, em vez de pagá-lo
bloqueado dentro dela.

**Verificado com GPU real:** `make test` completo sem regressão
(WSL, GPU=real). Mas a comparação A/B (mesma sessão, mesmos parâmetros,
com/sem o warm-up assíncrono) não mostrou ganho medível — nem em
`general.out` (circuito trivial, H em N qubits, 18-26 qubits, 3
execuções cada) nem em `shor.out` (circuito grande de verdade, 21
qubits, 5 execuções cada, t_GPU e t_HYBRID):

| caso | com warm-up | sem warm-up |
|---|---|---|
| general.out 18-26q, t_GPU | 0.18-0.85s | 0.17-0.81s (igual, dentro do ruído) |
| general.out 18-26q, t_HYBRID | 0.02-1.24s | 0.02-1.23s (igual, dentro do ruído) |
| shor.out 21q, t_GPU (5x) | média 1.74s | média 1.76s |
| shor.out 21q, t_HYBRID (5x) | média 39.68s | média 39.81s |

Diferenças em ambos os sentidos, sempre menores que o desvio-padrão
entre execuções do mesmo lado. **Diagnóstico do cold-start confirmado;
o fix não ajudou.** Hipótese de por que não: a janela de trabalho de
CPU disponível entre "`exec_type` conhecido" e "primeira chamada real de
GPU" é curta demais pra esconder ~200ms atrás dela mesmo pro circuito
maior testado (Shor a 21 qubits) — construir a lista de portas (`PT`)
aparentemente não é o gargalo que se imaginava, mesmo pra circuitos
razoavelmente grandes. Sem essa janela, a thread de warm-up e a thread
principal chegam no mesmo lugar (o lock interno do driver pra inicializar
o contexto) quase ao mesmo tempo de qualquer forma — o resultado
matemático é o mesmo custo total, só que pago um pouco mais cedo.

**Revertido o warm-up assíncrono** (`cli_common.h/.cpp`,
`general.cpp`/`grover.cpp`/`shor.cpp`) por não ter efeito medível —
complexidade sem ganho comprovado não fica. **Mantida a instrumentação**
de timing em `ProjectState`/`GpuExecutionWrapper`/`GetState`
(`kernel.cu`, opt-in via `HYBRID_DEBUG=1`, custo zero desligada) — foi
ela que achou o cold-start de verdade, vale continuar tendo à mão.

Item 17 segue sem solução. Três tentativas testadas e rejeitadas
empiricamente (subir `qubits_limit`; mesclar regiões de GPU; esconder o
cold-start atrás de construção de circuito). Se alguém retomar: o
cold-start de ~200ms é real e teoricamente evitável, mas só valeria a
pena numa arquitetura onde o contexto CUDA é aquecido uma vez e
reaproveitado entre muitas execuções (um processo de longa duração/
serviço), não no modelo atual de "um processo novo por execução de
circuito" — mudar isso é uma decisão de arquitetura bem maior que o
escopo deste item.

---

## 18. [CORRIGIDO] `DGM::measure()` chamava `srand(time(NULL))` a cada medição — destruía a independência entre amostras, e pior ainda no Windows/MinGW

**Onde:** `src/core/dgm_core.cpp` (`DGM::measure`), `src/algorithms/lib_shor.cpp`
(escolha de `base_value`), `src/cli/grover.cpp`/`shor.cpp`,
`include/core/common.h` + `src/core/common.cpp` (`g_rng`/`seed_rng`, novos).

Causa raiz de verdade da taxa de sucesso baixa do Shor documentada desde
o item 7 — não era só o `srand` por-processo do item 16. `DGM::measure()`
chamava `srand(time(NULL))` **toda vez que era invocada**, não uma vez
só; como a estimação de fase semiclássica do Shor mede vários qubits em
sequência, várias medições caindo no mesmo segundo resetavam a semente
pro mesmo valor, destruindo a independência entre as amostras. Agravado
no Windows: `RAND_MAX` do MinGW é 32767 contra `2147483647` da glibc, com
bits baixos de qualidade pior — exatamente os mais usados pelo módulo e
pela amostragem de medição.

**Corrigido** com `std::mt19937` (`g_rng`) em todo o projeto, seedado uma
única vez no início de `grover.cpp`/`shor.cpp`, no lugar de `rand()`/
`srand()`. **Verificado:** Windows, `shor.out 15 0` × 30: 0/30 antes
(confirmado também no commit anterior a qualquer mudança desta sessão,
não era regressão) → 29/30 depois. WSL/Linux com GPU real, `shor.out 15`
× 16 cada: `t_CPU` 16/16, `t_GPU` 16/16, `t_HYBRID` 14/16 — bem acima da
faixa historicamente registrada no item 7, confirmando que o bug
prejudicava o Linux também, só mascarado pela qualidade melhor do
`rand()` da glibc. `make test` sem regressão nos dois ambientes.

## 19. [CORRIGIDO] `Gates::~Gates()` vazio — cada `DGM` vazava seu cache inteiro de matrizes de porta

**Onde:** `src/core/gates.cpp`, `src/core/common.cpp` (comentário).

Mesma classe de bug do item 15 (vazamento por falta de destrutor), só
que no cache de matrizes de porta em vez do vetor de estado: toda vez
que uma `DGM` era destruída, o cache inteiro (`Gates::list`) vazava, sem
nada liberando as alocações de `Gates::addGate()`. Corrigido com
`delete[]` em cada ponteiro dentro de `Gates::~Gates()` — seguro por
construção, já que todo ponteiro em `list` vem de um `new[]` dentro da
própria classe (confirmado por grep).

## 20. [CORRIGIDO] `genRot()` podia devolver `""` e corromper `DGM::qubits` a meio da execução do Shor

**Onde:** `src/algorithms/lib_shor_circuits.cpp` (`genRot`),
`src/core/dgm_parser.cpp` (`DGM::genGroups`).

Numa coincidência de arredondamento em ponto flutuante (`rot == 1`
exatamente, matematicamente quase inalcançável na prática), `genRot()`
devolvia string vazia em vez de um step válido — isso zerava
`DGM::qubits` a meio de uma execução via `DGM::genGroups()`, caminho
plausível pra crash ou resultado silenciosamente errado. Achado por
inspeção de código, não por reprodução. Corrigido pra devolver um step
`"ID"` (no-op) nesse caso em vez de string vazia.

---

*Achados durante a leitura de documentação em 2026-08-06, com adições em
2026-08-10 durante os testes de build da Fase 1 da renomeação e em
2026-08-11 durante a rodada de arquitetura, a passada de comentários e um
checkup geral do projeto. Itens corrigidos/removidos foram condensados
numa passada de limpeza em 2026-08-11 (histórico completo de investigação
no `git log`). Atualizar esta lista conforme novos pontos forem
encontrados ou os existentes forem corrigidos.*
