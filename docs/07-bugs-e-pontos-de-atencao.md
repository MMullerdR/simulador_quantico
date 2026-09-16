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

**Três tentativas de correção testadas com hardware real numa sessão
anterior, todas concluídas e revertidas por dados (não abandonadas):**

**Tentativa 1 — subir `qubits_limit`.** Rejeitada: resultado inconsistente
(varredura empírica com `qubits_limit` ∈ {20,22,24,26} — às vezes
melhora, às vezes piora muito, ex. 24 qubits com `limit=24` caiu pra
desempenho de single-thread). Causa: com região única, quem processa
tudo é loteria de agendamento do SO entre as threads, não CPU vs GPU de
verdade — subir o limite só desloca *onde* a loteria acontece, não
resolve o problema de fundo.

**Tentativa 2 — GPU reserva um bloco de regiões mescladas por vez, CPU
disputa dinamicamente o resto.** Implementada; corrigiu no caminho um
segfault real (a fórmula de "pular N posições na fila compartilhada" só
funcionava pra máscaras de bits contíguas — corrigida com o mesmo
incremento por acarreio já usado no resto do código, funciona pra
qualquer formato de máscara). Verificada correta com GPU real
(amplitude exata certa em 18-30 qubits, `make test` sem regressão). Mas
a comparação A/B mostrou que mesclar regiões **piora** a performance
(ex: 26 qubits, mais que o dobro do tempo) — invalidando a premissa de
que o custo era overhead fixo de lançamento de kernel (mais provável:
`cudaMemcpy` proporcional ao volume de dados). Revertida; só a
instrumentação `HYBRID_DEBUG` ficou no código.

**Tentativa 3 — instrumentar `ProjectState`/`GpuExecutionWrapper`/
`GetState` diretamente.** Confirmou que a cópia de memória domina o
kernel em ~25x (consistente com a tentativa 2), e achou algo novo: a
**primeira** chamada de GPU do processo inteiro custa sozinha ~250ms
(~100x mais que as chamadas seguintes, 1.5-2ms) — é o custo de
inicialização preguiçosa do contexto CUDA (driver), pago **uma vez por
processo**, não por região/chamada. Testado esconder esse custo com um
warm-up assíncrono disparado bem no início do `main()` dos três CLIs —
**sem ganho medível**, nem no benchmark trivial nem no Shor a 21
qubits: a janela de trabalho de CPU disponível antes da primeira
chamada real de GPU é curta demais pra escondê-lo. Revertido; a
instrumentação de timing ficou.

O cold-start de ~200ms é real e teoricamente evitável, mas só valeria a
pena numa arquitetura onde o contexto CUDA é aquecido uma vez e
reaproveitado entre muitas execuções (um processo de longa duração/
serviço), não no modelo atual de "um processo novo por execução de
circuito" — mudar isso é uma decisão de arquitetura bem maior que o
escopo deste item.

**Tentativa 4 — [IMPLEMENTADA, NÃO VERIFICADA] içar `cudaMalloc`/`cudaFree`
pra fora do laço de regiões (2026-09-16, sessão sem GPU disponível).**

Motivada por um relatório de investigação independente (outra sessão,
achado consistente com a tentativa 3 acima, mas mais específico):
`ProjectState` fazia `cudaMalloc` do zero e `GetState` fazia `cudaFree`
**a cada região individual** processada pela GPU no modo híbrido — 29 a
32 vezes numa execução típica —, mesmo o tamanho do buffer
(`global_region_bits`) sendo constante durante todo o laço de regiões de
um mesmo lote. O modo `t_GPU` puro (`GpuExecution01`), em contraste, já
alocava a memória uma única vez no início de toda a execução.

**Mudança aplicada:** duas novas funções `extern "C"` em `kernel.cu` —
`AllocGpuState(region_size, gpu_count)` (faz só o `cudaMalloc`, um por
GPU) e `FreeGpuState(gpu_count)` (faz só o `cudaFree`) — chamadas por
`DGM::HybridExecution` uma vez antes/depois do laço `while (gpu_proj_id
!= -1)` da thread de GPU, em vez de a cada iteração. `ProjectState`
perdeu o `cudaMalloc` que tinha (só copia agora, pressupondo o buffer já
alocado); `GetState` perdeu o `cudaFree` (só copia de volta). Assinaturas
espelhadas em `kernel_stub.cpp` (stub, sem GPU real) e declaradas em
`dgm.h`. `AllocGpuState` só é chamada se a thread de GPU realmente vai
processar ao menos 1 região no lote (evita um malloc+free vazio quando
todas as regiões de um lote pequeno caem só em CPU).

**Verificado nesta sessão (sem GPU real disponível aqui):** compila
limpo (`g++`, lado `.cpp`/stub); `make test` completo (66/66 + 32/32 +
smoke test) sem regressão em `GPU=stub` — inclusive `t_HYBRID`, que só
passa pelos novos stubs sem tocar em `kernel.cu` de verdade.
**`kernel.cu` em si não foi compilado nem executado nenhuma vez** — esta
máquina não tem `nvcc`/GPU. A lógica foi revisada por leitura cuidadosa
(mesmo protocolo de cautela extra já usado antes neste arquivo), não por
execução real.

**Achado numa revisão de código logo em seguida:** a API nova
(`AllocGpuState`/`FreeGpuState`) introduzia uma pré-condição não
verificada — `ProjectState`/`GetState` passaram a exigir que
`AllocGpuState` já tivesse rodado antes (pra `gpu_mem[]` estar
alocado), mas isso só estava documentado em comentário, sem checagem em
runtime. Hoje o único chamador (`HybridExecution`) sempre respeita a
ordem certa, mas um uso incorreto futuro leria um ponteiro `NULL`/já
liberado sem aviso claro. **Corrigido**: uma flag `gpu_state_ready`
(estática, em `kernel.cu`) agora é checada por `AllocGpuState` (recusa
alocar de novo sem `FreeGpuState` antes — evitaria vazar o buffer
anterior), `FreeGpuState` (recusa liberar sem alocação correspondente),
`ProjectState` e `GetState` (recusam rodar sem alocação prévia) — todas
abortam com `printf` + `exit(1)` explicando a violação, mesmo padrão já
usado em `GpuExecution01` pra outros erros fatais. Também não verificado
com GPU real ainda (mesma limitação acima).

**Precisa, antes de considerar resolvido:**
1. `make GPU=real` compilar sem erro (a mudança usa a mesma API CUDA já
   presente no arquivo, mas nunca foi testada por um compilador de
   verdade).
2. `general.out <q> 3 <threads>` (`t_HYBRID`) continuar reproduzindo a
   amplitude exata esperada em vários tamanhos de qubits (18-30) —
   corretude antes de performance, mesmo padrão já usado nas tentativas
   1-3.
3. Comparar tempo de execução antes/depois (mesmos parâmetros da
   tentativa 2/3, ex. `general.out 24 3 4`) — se a hipótese estiver
   certa, deve melhorar; se piorar ou não mudar nada (como aconteceu na
   tentativa 2, por um motivo diferente), documentar aqui e reverter,
   mesmo padrão das tentativas anteriores.
4. Testar o caso `gpu_count > 1` com cautela redobrada — não mudei a
   lógica de multi-GPU em si, só onde o malloc/free acontece no tempo,
   mas essa combinação nunca foi testada com hardware real neste
   projeto (ver item 13).

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
