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

## 7. [NÃO REPRODUZIDO] Build de `kernel.cu` anormalmente lento (`nvcc`/`cicc`) numa máquina sem GPU

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

**Atualização (2026-10-21, qflex, CUDA 12.0.140, Ubuntu 24.04 nativo):**
`touch src/core/kernel.cu && make GPU=real` recompila o `kernel.cu` em
~2 s. Junto com os ~2,5 s da RTX 4070 acima, são duas máquinas onde o `nvcc`
compila rápido, então o caso das 2 horas segue sendo específico daquele
ambiente; o item fica aberto só como registro, sem nada a corrigir no
código.

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

## 13. [ARQUIVADO] Multi-GPU (`gpu_count > 1`): nunca testado com hardware real — um bug corrigido, um suspeito documentado

> **Arquivado em 2026-10-21 por decisão do Ramiro:** não há previsão de ter uma
> máquina com várias GPUs, então o item fica como registro e **não será
> trabalhado**. O código de `gpu_count > 1` continua como está, sem teste, e a
> suspeita sobre `is_peer` abaixo continua sem verificação. Quem um dia tiver 3+
> GPUs deve começar por aqui.

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

## 17. [RESOLVIDO] `t_HYBRID` não bate CPU nem GPU de forma consistente — hipótese A confirmada, hipótese B refinada

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

**Tentativa 4 — [VERIFICADA EM GPU: CORRETA, SEM GANHO] içar `cudaMalloc`/`cudaFree`
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

**Dados de dois benchmarks completos (`benchmark.sh`/`benchmark2.sh`,
logs na raiz do repositório, 26-28/09/2026).** Baterias de General
(15-30 qubits), Shor (17-27, ímpares) e Grover (15-27), em CPU serial,
CPU paralela, GPU e híbrido, com 16 threads, 1 GPU, `block_size=64`,
`repeat_count=2`, `gpu_region_bits=8`. Nos testes longos os dois logs
concordam em cerca de 1% (exceto o híbrido a 19-20 qubits no Grover, ver
abaixo). Os logs não
registram o commit nem a máquina usados, então **não dá pra afirmar que já
incluem a tentativa 4**. Tempos em segundos:

| Caso | CPU serial | CPU paralela | GPU | Híbrido |
|---|---|---|---|---|
| Grover 17 | 1,66 | 0,24 | 0,22 | 1,65 |
| Grover 20 | 44,0 | 3,10 | 0,70 | 0,59 (rodada 1) / 43,6 (rodada 2) |
| Grover 21 | 131 | 9,1 | 1,41 | 73,8 |
| Grover 22 | 397 | 26,4 | 3,41 | 108 |
| Grover 25 | timeout | 738 | 73,0 | 415 |
| Shor 21 | 107 | 6,8 | 2,14 | 45,6 |
| Shor 23 | 746 | 35,0 | 10,0 | 66,8 |
| Shor 27 | timeout | 1022 | 278 | 273 |
| General 30 | não rodado | 19,4 | 7,83 | 11,9 |

O que isso mostra:

- **Hipótese A confirmada em escala.** Com `qubits <= 20` só existe uma
  região, e quem a pega faz tudo. Grover a 20 qubits: o híbrido deu 0,59s
  numa rodada (GPU pegou, igual ao tempo da GPU) e 43,6s na outra (CPU
  pegou, igual ao serial) — até 60x de diferença entre execuções
  idênticas. A 15, 16 e 17 qubits o híbrido também ficou igual ao serial
  (0,185 / 0,551 / 1,646s).
- **Acima do limite, o híbrido parece ficar perto do serial dividido pelo
  número de regiões** (leitura dos números, não testada): Grover a 21
  qubits, 131s / 2 regiões = 65s (medido 74s); a 22 qubits, 397s / 4
  regiões = 99s (medido 108s). Isso sugere que cada região é processada
  por uma thread só e que o paralelismo fica limitado pelo número de
  regiões (2^(qubits-20)), não pelas 16 threads.
- **O híbrido não bate a GPU sozinha em nenhum caso relevante.** General a
  28-30 qubits: 1,27x a 1,52x mais lento que a GPU (a 30 qubits, ideal
  seria ~5,6s a partir de 19,4s da CPU paralela e 7,8s da GPU; medido
  11,9s). Shor a 17-23 qubits: 5,9x a 21x mais lento. Grover a 25 qubits:
  5,7x mais lento. Únicas exceções: Shor a 27 (273s contra 278s, empate) e
  General a 25 qubits (0,370s contra 0,392s, 6% melhor).
- **Cold-start do contexto CUDA visível:** a GPU no General fica em
  0,13-0,14s de 16 a 20 qubits, independente do tamanho — o custo fixo de
  ~200ms da tentativa 3, na RTX A4500.

**Tentativa 5 — [(a) IMPLEMENTADA E VERIFICADA EM GPU; (b) NÃO IMPLEMENTADA]
(2026-10-20).** Direções que os dados acima sugerem: (a) usar só a GPU
quando `qubits <= qubits_limit` (acaba com a loteria), (b) fazer a CPU
processar cada região com todas as threads em vez de uma por região.

**(a) implementada:** `DGM::HybridExecution` agora retorna cedo quando
`qubits <= qubits_limit` (20), delegando o circuito inteiro a
`GpuExecutionWrapper` -- o mesmo caminho do `t_GPU` --, em vez de abrir uma
corrida por uma região única. Resultado esperado nesses tamanhos: tempo
igual ao da GPU sozinha, sempre (hoje varia entre o tempo da GPU e o
serial, até 60x). Perde um pouco em circuitos triviais (General a 20
qubits: GPU ~0,14s contra CPU paralela ~0,015s), ganha muito nos pesados
(Grover a 20: 0,7s contra 3,1s). Todos os chamadores usam `execute(1)`
(as iterações já vêm dobradas em `pts`), então `iterations=1` na chamada
equivale ao comportamento anterior.

**(b) não implementada:** exige mexer em como a região global é dividida
entre threads de CPU -- mudança maior, melhor decidida com medição real
(ver o que sobra de ganho depois de (a)).

**Verificado:** `make test` completo em WSL (`GPU=stub`) sem regressão e
medição em GPU real, ver abaixo.

**Verificação em GPU real (qflex, RTX A4500, 2026-10-20), tentativas 4 e
5(a):** o driver do qflex foi recarregado (`rmmod`/`modprobe`, sem reboot)
e a GPU passou a funcionar.

1. `make GPU=real` compila limpo -- feito (`nvcc` 12.0, `sm_86`).
2. **Corretude:** `general.out <q> 3 16 64 2 8` (`t_HYBRID`) dá a
   amplitude exata esperada (2^(-q/2)) em 10, 18, 20, 21, 24, 26, 28 e 30
   qubits, igual ao `t_GPU`; Grover e Shor também acham o resultado.
3. **Tentativa 4, A/B contra o build anterior (`3ad037c`) no mesmo
   servidor, mesmos parâmetros, `t_HYBRID`:** General 28 qubits 2,82s ->
   2,77s; General 30: 12,15s -> 11,92s; Grover 21: 74,0s -> 73,0s; Grover
   22: 108,5s -> 108,8s; Shor 21: 46,0s -> 45,9s. **Diferença de 0-2%,
   dentro do ruído** (General 24: 0,329s -> 0,242s, mas é uma execução
   só, na faixa de 0,1s onde o cold-start domina). Ou seja, a tentativa 4
   está correta mas **não resolve o problema** -- os números pós-mudança
   são iguais aos dos benchmarks de setembro. A suspeita sobre
   `cudaMalloc`/`cudaFree` por região estava errada, ou ao menos não é o
   gargalo; a mudança fica no código (é inofensiva e evita
   alocações repetidas), mas o item continua aberto.
4. Caso `gpu_count > 1`: continua não testado (só há 1 GPU no qflex, ver
   item 13).

**Tentativa 5(a), verificada:** `t_HYBRID` a 10, 18 e 20 qubits agora dá o
mesmo tempo que `t_GPU` (0,14s contra 0,14s no General). Grover a 20
qubits, 5 execuções seguidas em `t_HYBRID`: 0,61 / 0,59 / 0,61 / 0,58 /
0,58s (GPU sozinha: 0,58s) -- contra 0,59s ou 43,6s antes. A loteria
acabou nesse tamanho.

**Tentativa 6 — [IMPLEMENTADA E VERIFICADA EM GPU (qflex, 2026-10-20)]:
memória travada + modelo de custo pra decidir quanto a CPU pega.** Duas
mudanças em `DGM::HybridExecution`, medidas antes de escrever (por região
de 2^20 amplitudes, General a 28 qubits: `ProjectState` ~2ms +
`GetState` ~3ms contra 0,17ms do kernel -- a GPU gasta ~98% do tempo
copiando; e cada região de CPU leva ~25-34ms em 1 thread):

1. **Vetor de estado travado (`cudaHostRegister`)** pela thread de GPU na
   primeira região (assim o cold-start do contexto CUDA continua
   sobreposto ao trabalho da CPU), destravado no fim. Em teste isolado a
   cópia vai de ~12,8 para ~22 GB/s; sem concorrência de CPU. Ganho
   medido no híbrido (`HYBRID_PIN=0` desliga, pra A/B): General a 28
   qubits 2,75s -> 2,23s; a 30 qubits 11,84s -> 9,54s (~19%). Se o
   `cudaHostRegister` falhar, segue com memória comum.
2. **Regra de custo pra CPU** (`hybrid_cpu_may_claim`): uma thread de CPU
   só pega mais uma região se ela terminar antes de a GPU, sozinha,
   limpar as que sobraram. Custo de CPU = portas x 2^bits x
   `cpu_ns_per_amp_op` (inicial 1,4 ns, medido); custo de GPU = escala x
   2^bits x (cópia + portas x ns_porta) (iniciais medidos no qflex);
   ambos refinados online por média móvel a cada região concluída
   (descartando a 1ª região de GPU do processo, que paga o cold-start).
   `HYBRID_GATING=0` desliga. Antes, a CPU pegava regiões na mesma taxa
   que a GPU mesmo sendo 100x mais lenta por região em circuitos
   pesados.

Resultados, `t_HYBRID` (16 threads) antes -> depois, com `t_GPU` de
referência (segundos):

| Caso | Híbrido antes | Híbrido depois | `t_GPU` |
|---|---|---|---|
| Grover 21 | 73,8 | 4,5 | 1,3 |
| Grover 22 | 108 | 11,3 | 3,3 |
| Grover 23 | 165 | 31,7 | 9,4 |
| Grover 24 | 221 | 88,9 | 27,7 |
| Shor 21 | 45,6 | 2,1 | 2,0 |
| Shor 23 | 66,8 | 9,8 | 10,0 |
| Shor 25 | 92,5 | 52,1 | 51,2 |
| General 28 | 2,75 | 2,20 | 2,02 |
| General 30 | 11,84 | 9,57 | 7,73 |

Corretude conferida: amplitude exata no General, `Found value: 10` no
Grover, e taxa de sucesso do Shor equivalente entre backends (17 qubits,
16 execuções: híbrido 11, GPU 12, CPU paralela 8). `make test` em WSL sem
regressão.

**Limite que sobrou:** o híbrido agora fica de 1x a ~3,4x o `t_GPU`, mas
**não bate o `t_GPU`** em nenhum caso. Motivo medido no Grover a 22
qubits: o circuito vira ~2.275 passadas e 9.102 regiões, cada uma pagando
cópia (~0,4ms de ida + ~0,4ms de volta) + kernel (~0,32ms), enquanto o
`t_GPU` mantém o estado inteiro residente na GPU e copia uma vez só. A
CPU quase não participa (2 regiões em 9.102: o modelo recusa o resto).
Qualquer ganho real do híbrido sobre a GPU exigiria manter parte do
estado residente na GPU entre passadas -- redesenho maior, não feito.
Opção mais simples e sem redesenho: quando o vetor de estado inteiro
cabe na memória da GPU, delegar tudo ao caminho do `t_GPU` (o híbrido de
regiões só seria necessário acima disso) -- implementada na tentativa 7.

**Tentativa 7 — [IMPLEMENTADA E VERIFICADA EM GPU (qflex, 2026-10-20)]:
delegar à GPU quando o estado cabe nela, e `QB_LIMIT` 30 -> 32.**
Decisão tomada em cima do limite medido na tentativa 6: o modo de regiões
copia o estado a cada passada e nunca bate o `t_GPU` residente. Agora
`DGM::HybridExecution` consulta a memória livre das GPUs
(`GpuMinFreeBytes`, `cudaMemGetInfo`) e, se o vetor de estado inteiro
(+512 MB de folga) cabe, roda tudo pelo caminho do `t_GPU`, como já fazia
a 20 qubits ou menos (tentativa 5). O modo de regiões com CPU+GPU só roda
quando o estado **não** cabe na GPU. `HYBRID_FORCE_REGIONS=1` desliga o
desvio "cabe na GPU" pra medir o modo de regiões.

Medido no qflex (General, 16 threads, GPU de 20 GB), segundos:

| Qubits | Estado | `t_PAR_CPU` | `t_GPU` | `t_HYBRID` |
|---|---|---|---|---|
| 28 | 2 GB | 4,4 | 2,08 | 2,12 (delegado) |
| 30 | 8 GB | 19,4 | 7,78 | 7,78 (delegado) |
| 31 | 16 GB | 40,7 | 15,8 | 16,0 (delegado) |
| 32 | 32 GB | 85,4 | sem memória | **~42 (modo de regiões)** |

Grover 21 e 22: híbrido 1,29s e 3,32s (= `t_GPU`; eram 4,5s e 11,3s na
tentativa 6, 74s e 108s antes). Shor 21: 2,03s. A 32 qubits a GPU
processa ~18,7 mil regiões e a CPU ~26,3 mil, e o híbrido fica ~2x mais
rápido que a CPU paralela -- o único tamanho em que ele é a melhor opção
com GPU. Amplitudes conferidas em todos os tamanhos.

**`QB_LIMIT` 32:** a 32 qubits o vetor de estado tem 32 GB, então foi
preciso corrigir overflows de `int` latentes (nada quebrava até 31 qubits):
`1 << bit` virou `1L << bit` nos arquivos do núcleo (`dgm_par_exec.cpp`,
`dgm_cpu_exec.cpp`, `dgm_core.cpp`, `common.cpp`, `dgm_parser.cpp`) e
`PCpuExecution1_0` passou a receber `region_id`/`region_mask`/`pos_count`/
`pts_start`/`pts_end` como `long` (a 32 qubits `t_PAR_CPU` dava segfault).
`DGM::allocateMemory` agora aborta com mensagem clara se o `calloc` falhar
(antes: segfault no primeiro acesso a `state[]`). Em 33 qubits o CLI
recusa. Não testado: Grover/Shor a 31-32 qubits (inviáveis em tempo), e
multi-GPU (`gpu_count > 1`, item 13).

**Cobertura de regressão (`tests/smoke_test.sh`, sob `GPU=real`):** os
casos antigos de `t_HYBRID` (10-14 qubits) só passavam pelo desvio "região
única -> GPU" e nunca exerciam o modo de regiões. Agora há também: General
a 22 e 24 qubits com amplitude exata nos dois caminhos (estado cabe na GPU
-> delegado; `HYBRID_FORCE_REGIONS=1` -> modo de regiões, com CPU e GPU
participando), Grover a 21 qubits em modo de regiões (taxa de sucesso) e
Shor a 21 qubits em modo de regiões (várias chamadas seguidas de
`HybridExecution`, só confere que não quebra). A tolerância de
`check_amplitude` caiu de 1e-4 para 2e-6 (a saída tem 6 casas): com 1e-4,
uma amplitude de 22 qubits (~4,9e-4) errada em 20% ainda passaria. Suíte
completa em GPU real: 38s, tudo OK; em `GPU=stub` (WSL) continua igual.

**Teste da decisão de delegação (2026-10-21):** o corte "o estado cabe ou
não na GPU" (`GpuMinFreeBytes` + 512 MB de margem) só era coberto com a GPU
vazia. `tests/gpu_hog.cu` ocupa memória da GPU deixando livre uma
quantidade pedida; o smoke test (só em `GPU=real`) roda `general.out 24 3 4`
com `HYBRID_DEBUG=1` com pouca memória livre (tem que dizer "não cabe" e cair
no modo de regiões) e com bastante (tem que dizer "cabe" e delegar à GPU),
e confere a amplitude nos dois casos.

**O que continua aberto:** o híbrido só ganha de verdade quando o estado
não cabe na GPU; abaixo disso ele é o `t_GPU`.

**Avaliado em 2026-10-21 e não implementado: manter parte do estado
residente na GPU.** Depois dos itens 22 e 23 a CPU ficou muito mais perto
da GPU, então refiz a conta com números novos da qflex (A4500, Threadripper
PRO 5955WX, PCIe ~20 GB/s medido nas cópias de região):

- Estado cabe na GPU, circuito pesado (`grover.out 22`): GPU 1,34 s,
  `t_PAR_CPU` com 32 threads 6,5 s, ou seja, a GPU é ~4,9x mais rápida.
  Dividir o estado entre as duas teria teto de 1 + 1/4,9 = ~1,2x.
- Estado cabe na GPU, limitado por banda (`general.out 26`): GPU 0,59 s
  contra CPU 0,83 s; teto ~1,7x.
- Estado não cabe (32 qubits, 32 GB contra 20 GB de GPU): híbrido atual
  36 s contra `t_PAR_CPU` 61 s; com a maior parte residente o melhor caso
  seria ~24-30 s, e só nessa faixa estreita de 32-33 qubits.

Esses tetos ignoram o custo de dividir o vetor: toda porta com alvo no bit
de divisão exige trocar metades do estado pelo PCIe (no Grover são 2 portas
H por iteração, e mover metade do estado leva um tempo parecido com o da
iteração inteira na GPU), o que come a maior parte do ganho. Em troca
traria um quarto caminho de execução, estado a manter coerente entre CPU e
GPU e troca de qubits globais por locais, validável de verdade só a 32-33
qubits (dezenas de segundos por teste). Decisão: não implementar. Se
alguém retomar, o ponto de partida é este cálculo, não a estimativa antiga
de "CPU ~100x mais lenta", que o item 23 invalidou.

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


## 21. [CORRIGIDO] `DGM::measure()`/`colapse()` somavam probabilidades em `float`, e `Grover()`/`Shor()` usavam `1<<` em `int`

**Onde:** `src/core/dgm_core.cpp` (`measure`, `colapse`),
`src/algorithms/lib_grover.cpp`, `src/algorithms/lib_shor.cpp`.

Achado na auditoria de "o que quebra acima de 30 qubits", não por um
resultado errado visto na prática.

- `measure()` acumulava `|amplitude|²` de 2^qubits termos num `float`. Um
  `float` para de crescer quando o termo é menor que ~2^-24 da soma: com
  2^30 amplitudes iguais, a soma de 2^30 termos de 2^-30 dá **0,015625**,
  não 1 (conferido num programa de teste na qflex: `float=0.015625
  double=1`). Em estados espalhados a probabilidade de cada lado do qubit
  medido sai errada e a decisão do colapso também. A 27 qubits (o maior
  Shor) o mesmo efeito já aparece com amplitudes uniformes (a soma trava
  em ~0,125). Medido na qflex: 20 rodadas alternadas de `shor.out 25 2 1`
  com o build antigo (`62ddb7b`) e o novo deram **18/20 nos dois**, ou seja,
  o `float` não prejudicava a taxa de sucesso do Shor a 25 qubits (a 27
  qubits não foi medido). Corrigido mesmo assim, com acumuladores `double`
  em `measure()` e `colapse()`, porque a soma está errada por construção
  nos estados que a travam.
- `Grover()` fazia `dgm.setMemoryValue(1<<(qubits-1))` e
  `sqrt(1<<(qubits-1))` em `int`: a 32 qubits isso é `1<<31`, comportamento
  indefinido (negativo na prática) → índice de estado negativo. `Shor()`
  tinha o mesmo padrão em `1<<(bit_width+2)` (sem risco real, `bit_width` ≤
  12, mas igualado). `DGM::setMemoryValue` agora recebe `long`.

Verificação: `grover.out 26` em `t_GPU` na qflex (218 s) achou o valor
certo depois das mudanças. O Grover a 31/32 qubits não foi validado até o
fim (ver `docs/08-performance.md`); a correção de overflow a 32 qubits
está só inspecionada no código, não exercitada de ponta a ponta.

## 22. [CORRIGIDO] Tuning padrão da GPU lento, difusor do Grover com passadas de sobra, e `t_GPU` abortando com menos de 8 qubits

**Onde:** `include/cli/cli_common.h` (`TuningDefaults`), `DGM::validateTuning()`
em `src/core/dgm_core.cpp`, `Grover()`/`ControledZ()` em
`src/algorithms/lib_grover.cpp`, `ApplyValuesC01` em `src/core/kernel.cu`.

Achados numa investigação de desempenho em 2026-10-21 (qflex, RTX A4500;
circuito/parse e `measure()` somam menos de ~5% do tempo, quase tudo é
execução de portas). Medido antes de mexer; os três passos abaixo
foram implementados e conferidos em GPU real.

1. **Região de GPU padrão pequena demais.** O default era
   `gpu_region_bits=8` (bloco 64 x repeat 2). Agora é 12 (bloco 512 x
   repeat 4 = 4096 amplitudes, 32 KB de shared memory). Mesmo binário, só
   o default mudou: Shor 21 2,07 → 0,91 s, Shor 25 50 → 19,4 s, Grover 22
   3,2 → 1,7 s, Grover 24 27 → 11,7 s; `general.out 28` só ganha ~10%
   (limitado por banda de memória). Região de 13 bits piora (64 KB de
   shared memory derruba a ocupação); `gpu_coalesced_bits` entre 3 e 7
   quase não muda nada (mantido em 4). Vale também pro `t_HYBRID`, que usa
   os mesmos parâmetros ao delegar à GPU.
2. **Difusor do Grover com 2*(qubits-1) passadas de X desnecessárias.** O
   difusor era `H X ... Z-multi-controlado(|11..1>) ... X H` em cada qubit.
   Os X só existiam pra levar |00..0> até |11..1>; o simulador já suporta
   controle com valor 0 (`Control1(0)`, usado pelo oráculo). Agora o Z
   multi-controlado usa controles 0 e só o qubit alvo leva um X antes e
   outro depois (X Z X = diag(-1, 1), então a fase de |00..0> é invertida
   exatamente como antes, sem fase global de diferença). Mesmo resultado,
   ~45% menos passadas: Grover 18 em `t_CPU` 4,88 → 3,34 s, Grover 22 em
   `t_PAR_CPU` com 16 threads 26,5 → 17,9 s. Somado ao passo 1, Grover 24
   em GPU vai de 27 s a 7,7 s.
3. **`gpu_slice_size` era `int` no kernel.** Estouraria com 2^31 ou mais
   amplitudes por GPU (GPU de 40 GB ou mais com 32+ qubits); virou `long`.
   Só regressão verificada, o estouro em si não foi exercitado (a GPU da
   qflex não comporta).
4. **Bug que o passo 1 ia ampliar: GPU com menos qubits que a região
   abortava.** `validateTuning()` já reduzia `gpu_region_bits` para o
   número de qubits, mas deixava `block_size`/`repeat_count` como estavam,
   e a checagem seguinte (`2*block_size*repeat_count == 2^gpu_region_bits`)
   abortava com "Erro de tuning". Com o default antigo isso já acontecia
   abaixo de 8 qubits (reproduzido: `general.out 6 2 1` e `grover.out 6 2
   1`); com o novo apareceria abaixo de 12. Agora, quando é o clamp por
   qubits que quebra a relação, `block_size`, `repeat_count` e
   `gpu_coalesced_bits` encolhem junto. Uma combinação inválida pedida de
   propósito continua sendo recusada.

Verificação: `make test GPU=real` na qflex (24 OK, 0 falhas, incluindo os
casos novos de poucos qubits), `make test` no WSL (15 OK); `general.out`
com 2 a 16 qubits em `t_PAR_CPU`/`t_GPU`/`t_HYBRID` dá a mesma amplitude;
`grover.out 6` achou o valor em 60/60 rodadas em cada um de
`t_CPU`/`t_PAR_CPU`/`t_GPU`/`t_HYBRID`. Não medido: o efeito do tuning novo
numa GPU diferente da A4500 (a região ótima depende da shared memory por
SM, ver `docs/04-gpu-cuda.md`).

Ficaram de fora e medidos como sem ganho: fusão de portas diagonais
consecutivas (sequências com o mesmo controle têm média de 1,2 portas),
cópia assíncrona das portas pra GPU sem `cudaDeviceSynchronize` (0%),
trocar `/` e `%` por `>>` e `&` no kernel (<2%) e paralelizar `measure()`
(~5% do Shor 25). Ainda aberto: reescrever os loops de CPU
(`PCpuExecution1_0`, `CpuExecution1_*`) em blocos contíguos vetorizáveis —
num microbenchmark isolado deu 1,8x sem AVX2 e 7-8x com AVX2, mas não foi
medido no sistema inteiro.

## 23. [CORRIGIDO] Loops de CPU do `t_PAR_CPU`/`t_HYBRID` não vetorizavam (~9 ciclos por par de amplitudes)

**Onde:** `PCpuExecution1_0` em `src/core/dgm_par_exec.cpp` (agora
`apply_gate()` + kernels `dense_*`/`diag_*`/`sec_*`), `DGM::validateTuning()`
em `src/core/dgm_core.cpp`, teste novo `tests/test_cpu_parallel.cpp`.

O laço antigo caminhava pelos pares com `pos = (pos + inc) & pos_mask`: o
índice de cada iteração depende do anterior, o compilador não vetoriza, e
cada par de amplitudes custava ~1,3 ns (~9 ciclos). Os bits baixos da região
(`cpu_coalesced_bits`) são sempre contíguos, então agora só os blocos
"externos" usam a recorrência e cada bloco contíguo vai para um laço simples
que o compilador vetoriza. Há um kernel por tipo de matriz (densa,
diagonal principal, anti-diagonal), com casos especiais: porta de fase com
`m00 = 1` (Z, R1-R3) só toca o lado do bit 1, e `X` é uma troca de memória
sem multiplicação. Alvos nos bits 0, 1 e 2 têm blocos curtos demais e ganharam
laços próprios.

`target_clones("avx2", "default")` gera uma versão AVX2 e uma genérica e
escolhe na execução, então o binário continua rodando em qualquer x86-64
sem `-march=native` (que sozinho só deu 8-18%). Armadilha encontrada no
caminho: a primeira tentativa usou `target_clones("arch=haswell", ...)`, cujo
dispatcher testa o **modelo exato** do processador e não as instruções — num
Zen 3 (qflex) caía em silêncio na versão genérica (0,575 ns por
amplitude-porta contra 0,15 ns com a AVX2 de fato escolhida). Fora de
x86-64/Linux/GCC>=6 (ou clang), por exemplo no MinGW, o atributo some e só a
versão genérica é compilada.

Medido na qflex (Threadripper PRO 5955WX), mesmo build antes/depois, melhor
de 2 execuções:

| caso | antes | depois |
|---|---|---|
| `general.out 24 1 1` (H, 1 thread) | 1,61 s | 0,37 s |
| `general.out 26 1 16` | 0,97 s | 0,80 s (limitado por banda de memória) |
| `grover.out 22 1 16` | 17,3 s | 6,95 s |
| `shor.out 21 1 16` | 6,66 s | 2,64 s |
| `shor.out 19 1 1` | 15,5 s | 5,3 s |
| `general.out 32 1 32` | ~85 s | 60,9 s |
| `general.out 32 3 32` (híbrido, região; também pega o tuning novo de GPU do item 22) | ~42 s | 36 s |

Custo por amplitude-porta, 1 thread, em cache: alvo >= 3: H 0,14-0,19 ns,
Z 0,05-0,08 ns, X 0,07-0,11 ns (antes ~1,3 ns); bit 0: H 0,35 ns; bit 1: H
0,47 ns; bit 2: H 0,29 ns. Com estados grandes o limite passa a ser a banda
de memória (`cpu_ns_por_amp_porta` do híbrido a 32 qubits fica em ~10 ns).
`cpu_region_bits`/`cpu_coalesced_bits` (14/11 por padrão) foram variados
(13-18 / 5-11) depois da mudança e ficaram praticamente iguais; mantidos. A
constante inicial de 1,4 ns do modelo de custo do híbrido
(`hybrid_cpu_ns_per_amp_op`) foi medida com o laço antigo: o valor real em
cache caiu uns 5-10x, mas a média móvel já corrige no começo da execução e
não mexi nela.

**Teste novo, `tests/test_cpu_parallel.cpp` (`make test`):** compara o
vetor de estado final de `t_PAR_CPU` contra `t_CPU` (o motor serial, que
continua com os laços antigos e serve de referência) em 90 circuitos
aleatórios, a partir de um estado inicial aleatório, com portas H, X, Y, Z,
R1-R3 mais quatro registradas no teste (densas e anti-diagonais com parte
imaginária, diagonal com `m00 != 1`), com e sem controles de valor 0 ou 1,
em várias combinações de qubits/região/coalescida/threads. Conferido por
mutação: trocar um sinal em cada um dos kernels (densa, diagonal,
anti-diagonal, os ramos de bits baixos e a troca pura do `X`) faz o teste
falhar.

Dois bugs que o teste achou, ambos no `validateTuning()` (pré-existentes ou
expostos por esta mudança):
- Uma região de CPU não maior que a faixa coalescida (e menor que o
  circuito) não absorvia nenhuma porta com alvo fora dela e
  `PCpuExecution1` ficava em laço infinito (`pts_start == pts_end`). Agora
  `cpu_coalesced_bits` desce para `cpu_region_bits - 1` nesse caso.
- `cpu_coalesced_bits` (padrão 11) maior que os qubits de um circuito
  pequeno (10 qubits, por exemplo) fazia a máscara da região incluir bits
  que não existem. Como os kernels novos medem a região pela máscara,
  escreveriam fora do vetor (visto como `double free or corruption` ao
  retirar o clamp de propósito). Agora `cpu_coalesced_bits` é limitado aos
  qubits.

**Não feito:** o `t_CPU` serial (`CpuExecution1_*`) continua com os laços
antigos de propósito, como referência do teste acima. Não medido: CPUs sem
AVX2 (só a versão genérica é exercitada lá, a mesma do MinGW), Intel, e
`t_HYBRID` em máquinas onde o modelo de custo parta do valor inicial.

## 24. [CORRIGIDO] Passadas da GPU no Shor: portas que comutam não eram agrupadas; e o que NÃO ajudou

**Onde:** `src/core/gate_schedule.cpp` (`ScheduleForBatching`, novo),
`DGM::scheduleForGpu()` em `src/core/dgm_core.cpp`, chamada de `DGM::execute()`
(`t_GPU`) e do desvio "delegado à GPU" de `DGM::HybridExecution`.

Pergunta de partida: dá pra reduzir as passadas da GPU sobre o vetor de
estado? Medido antes de mexer (qflex, A4500, `GpuExecution01` contado por
instrumentação descartável):

| caso | lotes (passadas) | portas por lote | com todas as portas desligadas no kernel |
|---|---|---|---|
| `shor.out 21` | 594 | 150 | 0,58 s de 1,02 s |
| `shor.out 25` | ~5800 | 32 | 8,2 s de 19 s |
| `grover.out 24` | ~9100 | 12,5 | 5,2 s de 7,7 s |

Ou seja, mesmo sem executar nenhuma porta o custo das passadas (leitura e
escrita do estado por lote, mais cópia host/GPU e launch) já é 43-68% do
tempo. Reduzir lotes ajuda; reduzir o custo por porta não.

- **Grover:** não há o que ganhar. A sequência H..H, X, Z, X, H..H é uma cadeia
  de dependências (nenhuma porta comuta com a vizinha), e uma região de 12 bits
  (4 coalescidos + 8 de janela) cobre no máximo 8 bits altos por lote. Simulei
  o agrupamento com reordenação sobre o Grover real: 4551 lotes antes e depois.
- **Shor:** portas de fase e controles são diagonais e comutam entre si, mas o
  agrupamento só olhava a ordem em que as portas chegam. Reordenando portas que
  comutam (dependência por qubit: uma porta com alvo não diagonal em um qubit
  espera todas as anteriores que tocam esse qubit; portas diagonais e
  controles só esperam o último alvo não diagonal), a simulação offline do
  mesmo laço de lotes deu `shor 25`: 5798 -> 2200 lotes; `shor 21`: 594 -> 378.
  Implementado em C++ com o mesmo critério de janela do kernel.

Efeito medido (`GPU_SCHEDULE=0` desliga, pra comparar): `shor.out 21`
1,018 -> 0,974 s (4%), `shor.out 25` 19,04 -> 17,34 s (9%), `shor.out 27`
112,4 -> 101,8 s (9%), Grover sem diferença. É bem menos que a redução de
lotes (-62%) porque o custo passa a ser de computação nas portas, não de
passadas. Pra referência, `shor.out 27` levava 274 s antes do item 22.

Só vale chamar a reordenação onde `pts[]` é exclusivo de quem chama: no modo
de regiões do `t_HYBRID` threads de CPU leem o mesmo array enquanto a GPU
trabalha (a lista da GPU ali é outra, projetada por região), então ela fica
fora desse caminho.

**Avaliado para a CPU e descartado (simulação, 2026-10-21):** a mesma
reordenação não serve ao `t_PAR_CPU`. Com `cpu_region_bits=14` e
`cpu_coalesced_bits=11` (3 alvos distintos acima da faixa baixa), o Shor já cabe em
poucas passadas porque quase todos os alvos caem nos bits baixos: `shor 21`
72 passadas (~1270 portas cada) e `shor 25` 132 passadas, e a reordenação não
muda nenhuma (razão 1,00x); o Grover (6826 passadas, 7,7 portas por passada) é
uma cadeia de dependências, também 1,00x. Também simulei janelas de 8 bits
**não contíguos** na GPU (em vez dos 8 consecutivos do kernel): depois da
reordenação dá 2068 lotes no `shor 25`, contra 2200 com a janela contígua, ou
seja, nada que justifique generalizar o `OPEN_SPACE` do kernel.

**Tentado e descartado (medido): especializar o kernel por tipo de matriz.**
`ApplyValuesC01` paga 4 produtos complexos por par mesmo em portas diagonais ou
X. Tipei as portas no host (fase com `m00=1`, diagonal, troca, anti-diagonal) e
despachei por `switch` no kernel: nenhuma diferença (Shor 21 1,02 -> 1,06 s,
Grover 24 7,7 -> 8,9 s, ou seja, ligeiramente pior). Não commitado.

**Testes:** `tests/test_cpu_parallel.cpp` ganhou (a) `t_CPU` na ordem
reordenada contra a ordem original, 30 circuitos aleatórios com janelas
pequenas pra forçar muitos lotes (confere a regra de comutação
numericamente, roda também em `GPU=stub`); (b) `t_GPU` contra `t_CPU`, 24
circuitos, só com kernel real. Por mutação: ignorar as portas diagonais
anteriores quando uma porta não diagonal chega faz os dois grupos falharem;
tratar a anti-diagonal como diagonal fez o grupo de `t_GPU` falhar (o grupo
da reordenação nesse caso não foi conferido).

---

## 25. [CORRIGIDO] Grover guardava centenas de milhares de `PT` idênticos (um por iteração)

**Onde:** `Grover()` em `src/algorithms/lib_grover.cpp`; `PCpuExecution1` e
`DGM::HybridExecution` em `src/core/dgm_par_exec.cpp`.

`setFunction(grover_step, iteration_count, false)` replicava (e re-parseava) o
passo de Grover `iteration_count` vezes: a 31 qubits são ~25 mil iterações x
~120 portas = ~3 milhões de `PT` (~255 MB, a ~85 B cada; medido no `grover.out
22`: 53 mil portas, ~4,5 MB). Só `execute(iteration_count)` com uma cópia do
passo seria o mais simples, mas **custou 12% na GPU e 5% na CPU** (Grover 24
7,7 -> 8,6 s; Grover 22 em 16 threads 6,96 -> 7,33 s): com a lista contínua o
agrupamento por lote/região funde o fim de uma iteração com o começo da
seguinte, e cada iteração separada perde essa fusão. A solução é um
meio-termo: o passo é replicado em blocos de ~20000 portas (a emenda entre
blocos é rara) e o bloco é repetido por `execute(blocos)`. Memória limitada ao
bloco, independente de quantos qubits; tempo igual ao anterior (Grover 22 em
16 threads 6,93 s contra 6,96 s, Grover 24 em GPU 7,78 s contra 7,7 s).

Pra isso `PCpuExecution1` e `HybridExecution` passaram a respeitar o parâmetro
`iterations` (antes `PCpuExecution1` o ignorava e o híbrido nem o recebia;
`CpuExecution1` e `GpuExecution01` já respeitavam). Coberto por
`tests/test_cpu_parallel.cpp` (casos com `iterations` 3 e 4 contra `t_CPU`;
conferido por mutação) e pelas taxas de sucesso do Grover no smoke test, que
passam pelos quatro backends.

Não medido: o ganho de memória a 28+ qubits (o Grover leva horas lá); o número
de 255 MB é a conta 3 milhões x 85 B, e os 85 B vêm do `grover.out 22`.

## 26. [CORRIGIDO] Shor: ~85% das portas eram rotações da QFT — QFT aproximada

**Onde:** `QFT_impl` em `src/algorithms/lib_shor_circuits.cpp`
(`set_qft_degree`, `qft_degree_for_width`), `Shor()` em
`src/algorithms/lib_shor.cpp`, teste em `tests/test_qft_addf.cpp`.

Contagem de portas do circuito do Shor (25 qubits = registrador de largura 11):
cada soma modular (`C2AddMod`) refaz **quatro** QFTs/QFTs inversas (78 portas
cada) e só ~65 portas de somadores e controles; ~85% das portas são rotações
controladas, 75% delas com um controle. Rotações de ângulo pequeno quase não
mudam o resultado (a QFT aproximada de Coppersmith/Barenco é padrão para o
Shor), então `QFT`/`RQFT` agora descartam as rotações de nível maior que um
grau `m` (ângulo < 2π/2^m). Padrão: `m = ceil(log2(largura)) + 2` (6 para
largura 9 a 16, 5 para 8). `SHOR_AQFT=0` volta ao circuito exato; `SHOR_AQFT=N` fixa o
grau. O grau é estado global do módulo (`set_qft_degree`) porque `QFT_impl` é
chamada de dentro de `C2AddMod`/`CMultMod` e passar o grau por todas essas
assinaturas mudaria metade do `lib_shor.h`.

**Taxa de sucesso** (execuções em `t_GPU`; o Shor é probabilístico, então
medi com muitas rodadas — com 40 rodadas a variação de amostragem enganava):

| qubits (largura) | exato | grau 7 | 6 | 5 | 4 | padrão |
|---|---|---|---|---|---|---|
| 19 (largura 8) | 88/200 | 88 | 87 | 87 | 90 | — |
| 21 (largura 9) | 154/200 | 164 | 165 | 157 | 158 | — |
| 23 (largura 10) | 97/100 | — | — | — | — | 100/100 |
| 25 (largura 11) | 21/30 | — | — | — | — | 24/30 |

**Tempo** (padrão contra `SHOR_AQFT=0`): Shor 21 em GPU 1,03 -> 0,86 s, em
`t_PAR_CPU` com 16 threads 2,64 -> 2,12 s; Shor 25 em GPU 17,4 -> 13,9 s. Em
`shor 25` o tempo por grau foi 17,4 s (exato), 14,1 (6), 12,9 (5), 11,6 (4),
10,3 (3).

**Não medido:** taxa de sucesso a 27 qubits ou mais, e do grau 3 com amostra
grande; a teoria pede ~log2(largura)+2, e o padrão segue isso. O circuito deixa
de ser exatamente o da literatura quando o grau é menor que a largura+1, por
isso o `SHOR_AQFT=0`.

**Testes:** `test_qft_addf` confere que, com grau `m`, `QFT`/`RQFT` são o
circuito exato sem os passos de rotação de nível > `m`, na mesma ordem, e que o
grau 0 volta ao exato (88 casos).

## 27. [CORRIGIDO] Portas de fase do Shor viravam uma passada de kernel cada — fusão em `t_GPU`

**Onde:** `src/core/gate_fuse.cpp` (`FusePhaseRuns`, novo), `PT::phase_terms` e
`PhaseTerm` em `include/core/common.h`, `ApplyFusedPhase`/`PhaseTable` em
`src/core/common.cpp`, kernel `ApplyValuesC01` em `src/core/kernel.cu`,
`DGM::scheduleForGpu()` em `src/core/dgm_core.cpp`.

~85% das portas do Shor são `diag(1, e^{iθ})` (rotações da QFT, Z, R1-R3), e
corridas delas são consecutivas (média de 6,8 portas por corrida no Shor 25,
medido nas listas reais). Elas comutam entre si, então uma corrida vira **uma**
porta: `amplitude[idx] *= e^{i·fase(idx)}`, com
`fase = Σ steps` dos termos cujo `(idx & mask) == value`, em unidades de
2π/2^16. `steps` é inteiro, então a soma é exata e a exponencial sai de uma
tabela de 65536 entradas. Só são fundidas portas cujo ângulo é múltiplo de
2π/2^16 dentro de 0,02 passo (o resto continua como porta normal). Reduz as
portas do circuito exato do Shor 25 em 3,7x (50 mil para 13,6 mil nas 6
primeiras chamadas, simulado sobre as listas reais).

Só o `t_GPU` (e o `t_HYBRID` quando delega à GPU) funde por padrão, depois da
reordenação do item 24; `GPU_FUSE=0` desliga. O `t_CPU` e o `t_PAR_CPU` entendem
a porta fundida (caminho escalar com a tabela), mas não a usam: o laço AVX2 do
item 23 já faz uma porta de fase em ~0,05 ns por amplitude, mais barato que
uma consulta de tabela por amplitude. O modo de regiões do `t_HYBRID` não
projeta portas fundidas (aborta com mensagem se receber uma), e como ele nunca
passa por `scheduleForGpu()` isso não acontece.

**A primeira versão ficou mais lenta** (Shor 25 15,6 s contra 13,5 s sem
fusão; Shor 21 1,02 contra 0,86 s): cada termo lia 24 bytes da memória global
por amplitude, com contas de 64 bits. A versão atual separa cada termo, no
host e por lote, em bits **fora** da região do bloco (conferidos uma vez por
bloco contra `block_base`; um termo que não bate vira "nunca") e bits
**dentro** (reindexados para a shared memory, 32 bits), copia os termos para a
shared memory uma vez por bloco e soma `steps` sem desvio.

Medido na qflex, mesmo binário com `GPU_FUSE=1` contra `GPU_FUSE=0`, com a
QFT aproximada do item 26 ligada nos dois:

| caso | sem fusão | com fusão |
|---|---|---|
| `shor.out 21` | 0,87 s | 0,84 s (-3%) |
| `shor.out 25` | 14,5 s | 13,1 s (-10%) |
| `shor.out 27` | 82,7 s | 75,5 s (-9%) |

A redução de portas (3,7x) rende bem menos que isso porque o tempo restante é
dominado por outra coisa: nas medidas anteriores (item 24), com as portas
desligadas no kernel o tempo já era 43-68% do total (passadas sobre o estado,
cópias host/GPU por rodada, `measure()` no host). Para referência, o `shor.out
27` em GPU levava 274 s antes do item 22; o acumulado até aqui é 112 s (item
22), 102 s (item 24), 82,7 s (item 26) e 75,5 s (este item).

**Bug achado pelo smoke test no caminho:** a primeira versão trocava o
armazenamento de `vec_pts` por um novo e o `t_HYBRID` delegado à GPU, que guarda
o parâmetro `pts` (esconde o membro `DGM::pts`), passou a ler memória liberada
(7 falhas no smoke test, "sem saída"). `FusePhaseRuns` agora reescreve a lista
no lugar e só a encolhe, então nenhum ponteiro guardado fica inválido.

**Testes:** `tests/test_cpu_parallel.cpp` (`make test`) ganhou (a) 18 circuitos
ricos em portas de fase (inclusive um ângulo que não é múltiplo do passo, que
tem que ficar como porta normal), `t_CPU` e `t_PAR_CPU` depois da fusão contra
o `t_CPU` na lista original, e (b) casos ricos em fase no grupo `t_GPU`
contra `t_CPU`. Conferido por mutação: somar um passo a mais no kernel ou no
caminho de CPU faz os testes falharem.

**Custo/benefício:** ~250 linhas e um novo tipo de PT por -9 a -10% a 25-27
qubits (-3% a 21). Fica ligado por padrão; `GPU_FUSE=0` desliga.

**Atualização (corpo fundido do kernel):** depois do item 28 o `shor.out 25`
em GPU gastava 10,1 s dos 11,0 s dentro do kernel (2112 lançamentos, 21032
portas fundidas e 29017 normais). Desligando cada tipo de porta no kernel
(só para medir, o resultado fica errado): carregar/gravar o bloco 2,4 s (~470
GB/s), portas normais ~1,2 s, **portas fundidas ~5,3 s**, mais da metade. Uma
porta fundida custava ~0,16 ms contra ~0,04 ms de uma normal, apesar de ter só
5,5 termos em média (85% ativos no bloco). O corpo antigo percorria todos os
termos para cada amplitude, com 3 leituras de shared memory por termo, inclusive
termos que não valem para o bloco. Agora só os termos ativos no bloco entram
numa lista compactada (`atomicAdd` num contador em shared; a soma de inteiros
não depende da ordem), cada termo é um `int4` lido numa instrução e cada thread
soma 4 amplitudes por leitura de termo (`p1/p2/p3` passam do fim da região
quando `repeat_count` é 1, por isso as guardas `< region_size`).

| `shor.out 25` (`t_GPU`) | kernel | total |
|---|---|---|
| antes | 10,13 s | 11,02 s |
| depois | 8,09-8,13 s (-20%) | 8,93-8,97 s (-19%) |

Testada e descartada: tabela de fase em dois níveis (256+256 entradas) na shared
memory no lugar da tabela de 512 KB em memória global: 8,19-8,25 s, sem ganho,
então a leitura da tabela não era o gargalo. A porta fundida ainda custa ~4x
uma normal; o que sobra é o custo fixo por porta (3 `__syncthreads`, compactação)
e a multiplicação por amplitude.

Testes: `make test GPU=real` passa; `tests/test_cpu_parallel.cpp` ganhou dois
combos com `repeat_count = 1`, o único caso em que as guardas novas valem.
Conferido por mutação: sem as guardas o teste `t_GPU` contra `t_CPU` aborta com
"illegal memory access". **Não medido:** `shor.out 27` com o corpo novo; só o
Shor 25 foi cronometrado (2 execuções por versão).

## 28. [CORRIGIDO] Shor copiava o estado inteiro host<->GPU e media no host a cada rodada — sessão de GPU

**Onde:** `GpuSessionBegin`/`GpuSessionEnd`/`GpuMeasureSums`/`GpuCollapse` em
`src/core/kernel.cu` (stubs em `kernel_stub.cpp`), `DGM::beginGpuSession()`/
`endGpuSession()` e o ramo de GPU em `DGM::measure()` em
`src/core/dgm_core.cpp`, `DGM::HybridExecution` em `src/core/dgm_par_exec.cpp`,
`Shor()` em `src/algorithms/lib_shor.cpp`.

O Shor roda `2*largura` rodadas, cada uma `executeFunction` + `measure`. Em
`t_GPU` cada `executeFunction` fazia `cudaMalloc` + cópia do estado inteiro pra
GPU, os kernels, cópia de volta e `cudaFree`; e o `measure()` percorria o vetor
de estado inteiro no host duas vezes (soma das probabilidades e colapso). Nas
medidas do item 24 isso aparecia como parte do piso de ~43-68% do tempo.

Agora o Shor abre uma **sessão de GPU** (`dgm.beginGpuSession()`): o estado
sobe uma vez, `execute()` passa a chamar o kernel com `state = NULL` ("já está
lá", o mesmo contrato que o modo de regiões do híbrido usa) e `measure()` roda
na GPU: um kernel de redução soma `|a|^2` em `double` (256 threads x 512
blocos, soma dos blocos em ordem fixa no host, determinística) e outro faz o
colapso com o mesmo mapeamento do host (a metade medida, dividida por `norm`,
vai pra metade com o bit em 0; a outra zera, o qubit medido volta a |0>). O
sorteio da medição continua no host com o mesmo `g_rng`. A sessão termina no
destrutor da `DGM`; `colapse()` e `measure(vector)` (que usam o host) encerram
a sessão copiando o estado de volta antes.

Escopo: só `gpu_count == 1` (o multi-GPU está arquivado, item 13), `t_GPU` e
`t_HYBRID` quando ele delegaria à GPU (`DGM::hybridDelegatesToGpu`, mesma regra
de `HybridExecution`); precisa de memória livre = estado + 512 MB, senão
`beginGpuSession()` devolve false e o caminho com cópia por execução segue como
estava. Só o Shor usa; o Grover roda um `execute` grande e `qubits-1`
`measure`, e as cópias ali somem diante de horas de cálculo. `GPU_RESIDENT=0`
desliga.

Medido na qflex, mesmo binário, `GPU_RESIDENT=1` contra `0` (com QFT
aproximada e fusão ligadas nos dois):

| caso | sem sessão | com sessão |
|---|---|---|
| `shor.out 21` | 0,81 s | 0,72 s (-11%) |
| `shor.out 25` | 12,9 s | 10,9-11,0 s (-15%) |
| `shor.out 27` | 76,2 s | 55,1 s (-28%) |
| `shor.out 25 3 4` (híbrido delegado) | 12,8 s | 11,1 s (-13%) |

O ganho cresce com o tamanho porque a cópia e o `measure` no host crescem com
2^qubits. Acumulado do Shor em GPU nesta sessão: `shor.out 27` 279 s no começo
das otimizações -> 55 s; `shor.out 25` 50,5 s -> 10,9 s.

**Testes:** `tests/test_cpu_parallel.cpp` (só com kernel real) ganhou 12
sessões de 6 rodadas "circuito aleatório + medição de um qubit aleatório" no
`t_GPU` residente contra o `t_CPU`, com `g_rng` re-semeado igual antes de cada
medição: o bit medido tem que ser igual e o estado final (copiado de volta)
tem que bater em 1e-5, com circuitos comuns e ricos em fase (cobre também a
fusão sobre estado residente). Por mutação: trocar zero/um na soma, não zerar a
outra metade ou não renormalizar no colapso fazem o teste falhar. A taxa de
sucesso do Shor com e sem sessão ficou dentro do ruído (Shor 19, 150 rodadas:
83 contra 67; Shor 21, 30 rodadas: 23 contra 27, em sentidos opostos); a
equivalência bit a bit com a mesma amostra é o que o teste acima garante.

**Não testado:** o caminho de recusa por falta de memória (estado perto da
capacidade da GPU) e a sessão com `gpu_count > 1` (recusada de propósito).

---

*Achados durante a leitura de documentação em 2026-08-06, com adições em
2026-08-10 durante os testes de build da Fase 1 da renomeação e em
2026-08-11 durante a rodada de arquitetura, a passada de comentários e um
checkup geral do projeto. Itens corrigidos/removidos foram condensados
numa passada de limpeza em 2026-08-11 (histórico completo de investigação
no `git log`). Atualizar esta lista conforme novos pontos forem
encontrados ou os existentes forem corrigidos.*
