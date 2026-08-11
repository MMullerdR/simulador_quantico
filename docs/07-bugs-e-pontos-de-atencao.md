# Bugs e Pontos de Atenção

Lista viva de problemas/riscos encontrados lendo o código. Objetivo: não
tomar decisões de "incrementar o algoritmo X" sobre uma base que já tem um
bug de corretude escondido. Atualizar esta lista conforme formos mexendo.

## 1. [CORRIGIDO] `genRot` sempre gerava/reusava a porta `"Rot_0"` — afetava o Shor

**Onde:** [lib_shor_circuits.cpp:11-43](../src/algorithms/lib_shor_circuits.cpp#L11)

**O bug:** `genRot` recebia `phase_bits` por valor e o consumia bit a bit
num `while` pra calcular a rotação — no fim do laço `phase_bits` sempre
valia `0`, independente do valor original. O nome da porta era montado
**depois** desse laço (`name = "Rot_" + int2str(phase_bits)`), então
sempre virava `"Rot_0"`, não importa qual correção de fase estivesse
sendo aplicada. Como `Gates::addGate`
([gates.cpp:33](../src/core/gates.cpp#L33)) não sobrescreve um nome já
existente, só a **primeira** correção de fase de cada execução do Shor
era realmente registrada — todas as chamadas seguintes reaplicavam essa
mesma matriz antiga, em vez da correção certa daquela rodada.

**Correção aplicada (2026-08-10):** guardar o valor original antes do
laço consumi-lo:

```c
long phase_bits_orig = phase_bits;
// ... laço consome phase_bits normalmente ...
name = "Rot_" + int2str(phase_bits_orig);
```

Cada correção de fase distinta agora gera um nome único, então
`Gates::addGate` não colide mais entre rodadas diferentes.

**Verificado:** rodando `shor.out 15 0` no WSL depois da correção, achou
os fatores de `57` (`3 × 19`) já na primeira tentativa.

## 2. [REMOVIDO] Código morto: `CpuExecution2_*`, `CpuExecution3_*` e `CU()`

Três famílias de funções, então ainda em `dgm.cpp`/`dgm.h` (arquivo
monolítico na época, mais tarde dividido em `dgm_core.cpp`/`dgm_parser.cpp`/
`dgm_cpu_exec.cpp`/`dgm_par_exec.cpp` — ver [00-indice.md](00-indice.md))
(`_1_*`, `_2_*`, `_3_*`)
implementavam o mesmo cálculo (denso / diagonal principal / diagonal
secundária) de formas diferentes. `DGM::execute()` só despachava pra
`CpuExecution1` — as famílias `2` e `3` nunca eram chamadas em lugar
nenhum do projeto. Removidas em 2026-08-10 (junto com `CU()` em
`lib_shor.cpp`/`lib_shor.h`, que tinha o corpo inteiro comentado e também
nunca era chamada) depois de confirmado por grep que não havia nenhuma
chamada a nenhuma delas. `CpuExecution1_*` (a família realmente usada)
não foi tocada.

## 3. [CORRIGIDO] `PT::destructor()` nunca liberava `control_bit_positions`/`control_rest`

**Onde:** [common.cpp:11-16](../src/core/common.cpp#L11)

**O bug:** as três condições estavam invertidas (`!matrix`/`!control_bit_positions`/
`!control_rest` em vez de sem a negação) — ou seja, cada `free()` só rodava
quando o ponteiro já era `NULL`, o que não libera nada de útil.

**Não é tão simples quanto inverter as três condições de volta.** Investigando
antes de corrigir: `matrix` **não pertence** ao `PT` — é sempre um ponteiro
emprestado de `Gates::list` (`gates.getMatrix(...)`, o cache de matrizes de
porta da execução, ver item 5). Se essa condição também fosse invertida para
`if (matrix) free(matrix);`, o resultado seria bem pior que o leak original:
`free()` numa matriz **compartilhada**, causando double-free/use-after-free
na próxima vez que qualquer outro `PT` (ou execução futura) referenciasse a
mesma porta pelo nome.

**Correção aplicada (2026-08-10):** `control_bit_positions` (alocado com
`malloc` por `PT` em `DGM::genPTs`, portanto de posse exclusiva daquele
`PT` — esse sim um vazamento real) e `control_rest` (hoje sempre `NULL` na
prática, corrigido por consistência) passaram a ser liberados corretamente.
`matrix` foi deixado como estava, com um comentário explicando por que
nunca deve ser liberado ali.

**Essa correção, sozinha, introduziu um bug novo e mais grave** — ver
item abaixo. Na hora, só tínhamos "compila limpo" como verificação (o
link local estava quebrado); o "verificado" que constava aqui antes
estava errado, baseado em compilação, não em execução real. Lição:
compilar não é o mesmo que rodar, principalmente quando o fix mexe em
`free()`.

## 3.1. [CORRIGIDO] `DGM::genPTs` aloca `PT` com `malloc()` sem inicializar `control_bit_positions`/`control_rest`

**Onde:** [dgm_parser.cpp:138](../src/core/dgm_parser.cpp#L138)

**O bug:** `genPTs` cria cada `PT` com `term = (PT*) malloc(sizeof(PT));`
— `malloc()` puro, que **não chama** `PT::PT()`. Antes da correção do
item 3, `control_bit_positions` só era atribuído dentro do
`if (group_control_count){...}` (quando a porta tem controle). Pra toda
porta **sem controle** — ou seja, praticamente todo `H`/`X` solto,
virtualmente todo circuito de Grover e de Shor — `control_bit_positions`
ficava com lixo de memória não inicializado, não `NULL`.

Antes da correção do item 3, isso não importava: a condição invertida
(`if (!control_bit_positions) free(...)`) quase nunca era verdadeira
pra um ponteiro de lixo, então na prática nunca chamava `free()` nesse
caso também — dois bugs se cancelando por acidente. Depois da correção
do item 3 (`if (control_bit_positions) free(...)`), isso passou a
chamar `free()` em cima de um ponteiro de lixo toda vez que
`DGM::erase()` rodava — **`free(): invalid pointer`**, reproduzido com
`general.out 10 3 4` (`t_HYBRID`).

**Correção aplicada (2026-08-10):** inicializar explicitamente
`control_bit_positions = NULL`, `control_rest = NULL` e
`control_rest_count = 0` logo após o `malloc()`, antes de qualquer uso —
não só dentro do `if (group_control_count)`. O caminho equivalente em
`HybridExecution` (`projected_term = new PT();`, em
[dgm_par_exec.cpp:392](../src/core/dgm_par_exec.cpp#L392)) já usa `new
PT()` de verdade, então não tinha esse problema.

**Superado em seguida (2026-08-11):** esse fix pontual foi substituído
por uma correção estrutural — `genPTs` passou a usar `new PT()` (a
mesma forma que `HybridExecution` já usava), e `PT` ganhou um `~PT()`
de verdade no lugar do `destructor()` manual. Ver
[dgm_parser.cpp:138](../src/core/dgm_parser.cpp#L138). Duas formas de
alocação inconsistentes para o mesmo struct era, junto com o
`destructor()` de chamada manual, a causa raiz dessa classe inteira de
bug — agora estruturalmente impossível de esquecer.

**Verificado no WSL (execução real, não só compilação):**
`make clean && make && outputs/general.out 10 1 4 && outputs/grover.out
12 1 4 && outputs/shor.out 15 0` — os três rodam sem crash;
`general.out` reproduz a amplitude uniforme `0.03125` esperada. Também
confirmado `general.out 10 3 4` (`t_HYBRID`, o cenário que crashava)
sem crash.

## 4. [REMOVIDO] Código morto: `GenericExecute` e `GpuExecution`/`GpuExecution2`/`GpuExecution3`

**Onde:** `GenericExecute` (duas sobrecargas, antes em
[dgm_core.cpp](../src/core/dgm_core.cpp) e declaradas em
[dgm.h](../include/core/dgm.h)) e as declarações `extern "C"`
`GpuExecution`/`GpuExecution2`/`GpuExecution3` em `dgm.h`.

Originalmente este item documentava um risco de posse ambígua de memória:
`GenericExecute` chamava `dgm.setMemory(state)` (que não copia, só aponta
`state` pro ponteiro recebido), e quem chamasse `GenericExecute` precisava
ter cuidado pra não deixar `DGM::freeMemory()`/o destrutor da `DGM`
liberarem um ponteiro que não era dela. Investigando pra corrigir isso
(2026-08-11): `grep` em `src/`, `include/` e `tests/` não achou **nenhum
chamador** de `GenericExecute` em lugar nenhum do projeto — é código
morto, mesmo padrão do item 2 acima. O comentário que a introduzia já
sinalizava isso ("usada por código de teste/benchmark fora dos CLIs" — que
não existe mais neste repositório, se é que já existiu).

Investigando o arquivo por perto, `GpuExecution`/`GpuExecution2`/
`GpuExecution3` (declaradas em `dgm.h`, mesmo bloco de
`GpuExecutionWrapper`) eram uma categoria ainda mais morta: nunca tiveram
implementação em lugar nenhum (nem `kernel.cu`, nem `kernel_stub.cpp`) —
só a declaração, já com um comentário reconhecendo isso ("sem
implementação atual"). `GpuExecutionWrapper` é a única realmente usada por
`DGM::execute()`.

**Removidas (2026-08-11)** as duas sobrecargas de `GenericExecute` de
`dgm_core.cpp`/`dgm.h`, e as três declarações `GpuExecution`/
`GpuExecution2`/`GpuExecution3` de `dgm.h`. O risco de posse ambígua que
motivava o item original deixou de existir junto com o código que o
causava.

**Verificado:** `make test` (66/66 + smoke test) local (Windows,
`GPU=stub`) e no WSL com GPU real (`GPU=real`), sem regressão.

## 5. [CORRIGIDO] `Gates::list` era `static` (compartilhado entre todas as instâncias)

**Onde:** [gates.h:39](../include/core/gates.h#L39),
[gates.cpp:7](../src/core/gates.cpp#L7)

Não era um bug isoladamente, mas era a causa raiz de por que o problema do
item 1 se manifestava como se manifestava: como `Gates::list` era
global/estático, portas registradas com o mesmo nome em execuções
diferentes (ou até rodadas diferentes do mesmo `Shor()`) colidiam.

**Correção aplicada (2026-08-11):** `Gates::list` deixou de ser `static` e
virou um campo não-estático de `Gates`; `DGM` passou a ter uma instância
própria (`DGM::gates`, [dgm.h](../include/core/dgm.h)), passada por
referência (`Gates &g`) por todas as funções que constroem circuito em
`lib_shor_circuits.cpp`. O cache continua vivo durante toda uma execução
(uma única instância de `DGM` do início ao fim de um `Shor()`/`Grover()`,
preservando o reaproveitamento de matrizes já cacheadas — `SubF(N)`, as
rotações da QFT, etc.), mas não sobrevive entre execuções diferentes,
eliminando a classe de bug do item 1 por construção, não por disciplina de
nomes únicos.

Escopo dessa mudança: só as funções que realmente chamam `addGate`/
`getMatrix` (`genRot`, `QFT_impl`, `QFT2`, `AddSubF_impl`, `DGM::genPTs`) e
quem está no caminho entre elas e o `DGM` (`CMultMod`, `CRMultMod`,
`C2AddMod`, `C2SubMod`, `CAddF`/`C2AddF`/`CSubF`/`C2SubF`, `AddF`/`SubF`,
`QFT`/`RQFT`, `Shor`, `ApplyQFT`) precisaram de um parâmetro `Gates &g` a
mais — `Hadamard`/`CNot`/`Toffoli`/etc. (que só montam tokens de string,
nunca tocam o cache) e `lib_general.cpp`/`lib_grover.cpp` (que não usam
portas geradas dinamicamente) ficaram de fora.

**Verificado:** compila limpo em todo o projeto; `make test` local (66/66
+ 8/8) e no WSL; comparação A/B interativa contra o binário do commit
anterior a esta mudança, rodando `shor.out 15 0` 8 vezes intercaladas —
mesma taxa de sucesso dos dois lados, confirmando que não é regressão (o
0/8 observado é a mesma limitação probabilística já documentada deste
ambiente, não algo introduzido por esta mudança).

## 6. [CORRIGIDO] Segfault em `t_PAR_CPU`/`t_HYBRID` quando `region_bits > qubits` disponíveis

**Onde:** `src/cli/general.cpp` (defaults do `main()`) +
`PCpuExecution1` em [dgm_par_exec.cpp:81](../src/core/dgm_par_exec.cpp#L81).

**O bug:** `general.cpp` usa `cpu_region_bits = 14` fixo como valor
padrão, independente de quantos qubits o usuário pedir na linha de
comando. Se `qubits < cpu_region_bits` (ex: `general.out 10 1 2`, pedindo
só 10 qubits), dentro de `PCpuExecution1`:

```c
long region_count = (1 << (qubits - region_bits)) + 1;
```

com `qubits=10` e `region_bits=14`, virava `1 << (10 - 14)` = **`1 << -4`**
— deslocamento por expoente negativo, comportamento indefinido em C/C++.
Na prática isso produzia um `region_count` absurdamente grande, e o laço
paralelo seguinte escrevia em `state[pos]` muito além do vetor alocado →
**segmentation fault** (reproduzido: `general.out 10 1 2` crashava;
`general.out 16 1 2` não).

**Correção aplicada (2026-08-10):** clamp logo no início de
`PCpuExecution1`:

```c
if (region_bits > qubits) region_bits = qubits;
```

**Verificado:** `general.out 10 1 2` no WSL, depois da correção, roda
normalmente e imprime a amplitude uniforme correta (`0.03125 = 1/√2¹⁰`),
sem precisar mais do workaround de pedir mais qubits.

**[CORRIGIDO] `HybridExecution` tinha o mesmo bug, em dois lugares:**
`global_region_bits` (inicializado com o valor fixo `qubits_limit = 20`) e
`cpu_region_bits` (mesmo default de 14 do `t_PAR_CPU`) — ambos usados como
expoente de `1 << (... - region_bits)` do mesmo jeito que `PCpuExecution1`,
em [dgm_par_exec.cpp:258](../src/core/dgm_par_exec.cpp#L258). Diferente do
bug do `t_PAR_CPU` (que só aparecia com uma combinação específica de
argumentos de linha de comando), este dispara **sempre** que `qubits <
qubits_limit` (ou seja, em praticamente qualquer circuito de teste, já que
20 qubits é um registrador bem grande pra simulação em CPU).

Corrigido com o mesmo clamp em ambos os pontos (`global_region_bits` contra
`qubits`, e uma cópia local de `cpu_region_bits` contra `global_region_bits`,
já que a região de CPU é recortada de dentro da região global — não pode
usar o member `cpu_region_bits` diretamente porque ele é compartilhado
entre as threads OpenMP).

Ao contrário do que este item dizia antes, **não é necessário ter uma GPU
real pra exercitar esse caminho**: o branch de CPU dentro de
`HybridExecution` (`omp_get_thread_num()!=0`) roda `PCpuExecution1_0` puro
em C++, sem depender de `kernel.cu`/`nvcc` — só o branch da thread 0 (GPU)
depende do backend real. Então `t_HYBRID` com `thread_count > 1` já
exercita o código corrigido mesmo num build `GPU=stub`.

**Verificado no WSL:** `general.out 10 3 4` (10 qubits — abaixo de
`qubits_limit`/`global_coalesced_bits`, o cenário que dispara o bug) dava
`free(): invalid pointer` antes da correção; com ela, roda sem crash. Com
`general.out 16 3 4` (qubits suficiente pra não colidir com o
`global_coalesced_bits` fixo em 15) também roda limpo.

**Nota sobre corretude (não é bug, é limitação de teste sem GPU):** com
`GPU=stub`, o resultado do circuito pode aparecer como se nada tivesse
sido aplicado (estado permanece em `|0>`). Isso acontece porque, com
`global_coalesced_bits` fixo em 15 e um circuito pequeno, o circuito
inteiro acaba cabendo numa única "região" — e o desempate entre threads
por essa única região frequentemente entrega o lote inteiro pra thread 0
(o branch de GPU), que no `kernel_stub.cpp` é *no-op* por definição.
Validar a execução híbrida de fato (CPU **e** GPU processando partes
diferentes do estado ao mesmo tempo) só é possível com `make GPU=real` e
hardware NVIDIA — segue como limitação conhecida do ambiente de
desenvolvimento atual, não deste fix.

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
lentidão: o `makefile` (`ARCH = -arch=sm_52`, linha 2) mirava em Maxwell
(`sm_52`) — arquitetura que o CUDA 13.x **nem compila mais**
(`nvcc fatal: Unsupported gpu architecture 'sm_52'`), e que já estaria
errada pra qualquer GPU atual de qualquer forma. Corrigido pra
`-arch=sm_89` (Ada Lovelace, a arquitetura da RTX 4070 — quem usar outra
geração de GPU precisa ajustar esse valor pra sua própria compute
capability).

Com o fix de arquitetura, `make GPU=real` (as ~260 instanciações de
template intactas, sem nenhuma das reduções do item 06 aplicadas) rodou em
**~2.5s**, não horas. Ou seja: a lentidão documentada acima não é
inerente ao volume de templates do `kernel.cu` — é específica de alguma
característica do ambiente da máquina anterior (mais provável: acesso via
`/mnt/c/`, já que essa é a única variável que mudou junto com a
arquitetura corrigida aqui; não dá pra descartar antivírus ou a instalação
do `nvcc` daquela máquina especificamente, já que nenhuma delas foi
reproduzida/testada aqui). O item 06 (reduzir templates) segue sendo uma
proposta válida por outros motivos (simplicidade, tempo de build em
máquinas mais fracas), mas deixou de ser urgente **nesta máquina**
especificamente — vale reavaliar com o usuário antes de implementar
qualquer uma das três opções.

**Verificado nesta máquina:** `general.out 10 2 1` (`t_GPU`) e
`general.out 10 3 2` (`t_HYBRID`) reproduzem a amplitude uniforme exata
`0.03125`, confirmando que o kernel real está calculando certo (não só
"não crasha"). `shor.out 15 2 1` (`t_GPU`) rodado 8×: 5/8 sucesso, contra
2/8 do `t_CPU` nas mesmas 8 rodadas — dentro da variação probabilística já
documentada do algoritmo, sem indício de bug específico de GPU.

**Pendente (item 6 do design de arquitetura de 2026-08-11, ver
[artefato de design](https://claude.ai/code/artifact/6c5e1ac0-7f89-46e0-a884-fa7d34539c7f)):**
três direções propostas pra reduzir a explosão de templates, nenhuma
verificável sem GPU NVIDIA real:
- **(a)** reduzir deliberadamente o conjunto de combinações suportadas
  de `block_size`/`repeat_count`/`coalesced_bits` — formalizar como
  decisão permanente o que hoje é só o workaround temporário do
  diagnóstico acima.
- **(b)** shared memory de tamanho dinâmico (`extern __shared__`) em vez
  de parâmetro de template, eliminando a necessidade de
  `t_block_size`/`t_repeat_count` serem conhecidos em tempo de
  compilação.
- **(c)** dispatch em runtime (ponteiro de função/switch) sem
  reinstanciar o kernel inteiro por combinação.

Fica pra quando houver acesso a uma máquina com GPU NVIDIA de verdade —
sem isso, qualquer mudança em `kernel.cu` não tem como ser testada além
de "compila" (e nem isso, dado o problema acima).

## 8. [CORRIGIDO] `grover.cpp`/`shor.cpp` ignoravam `exec_type` na hora de chamar o algoritmo

**Onde:** [grover.cpp:38](../src/cli/grover.cpp#L38),
[shor.cpp:61](../src/cli/shor.cpp#L61).

**O bug:** os dois CLIs faziam o parsing e a validação de `exec_type` a
partir de `argv[2]` normalmente, e usavam esse valor pra decidir se
`argv[3]` era `thread_count` ou `gpu_count` (via `parse_backend_arg`) —
mas na hora de chamar `Grover(...)`/`Shor(...)`, passavam o literal
`t_CPU` no lugar da variável `exec_type`. Ou seja, pedir `t_PAR_CPU`/
`t_GPU`/`t_HYBRID` na linha de comando (ex: `shor.out 15 3 4`) fazia o
programa aceitar e validar o pedido, ajustar `thread_count`, e depois
**rodar em `t_CPU` (serial) mesmo assim** — os outros backends nunca
eram exercitados por esses dois CLIs.

`general.cpp` nunca teve esse problema — sempre passou `exec_type`
corretamente pra `HadamardNQubits(...)`.

**Impacto no que já tinha sido testado nesta sessão antes da correção:**
os testes de `t_HYBRID` feitos com `grover.out`/`shor.out` ao longo
desta sessão (incluindo em `tests/smoke_test.sh`) na prática rodaram em
`t_CPU`, não exercitaram `HybridExecution` de verdade. Os testes de
`t_HYBRID` feitos com `general.out` continuam válidos (esse CLI nunca
teve o bug).

**Correção aplicada (2026-08-11):** trocado o `t_CPU` literal por
`exec_type` nas duas chamadas.

**Verificado:** `grover.out 10 2 1`/`shor.out 15 2 1` (`t_GPU`) agora
imprimem o aviso do `kernel_stub.cpp` (prova de que `GpuExecutionWrapper`
está sendo chamado de verdade, o que não acontecia antes); `shor.out 15
0` (`t_CPU`) continua achando os fatores certos; `make test` (66/66 +
8/8) local e no WSL.

Encontrado durante uma passada de comentários no código (2026-08-11),
corrigido logo em seguida na mesma data.

## 9. [CORRIGIDO] `makefile` mirava em `-arch=sm_52`, arquitetura que o CUDA 13.x nem compila mais

**Onde:** [makefile:2](../makefile#L2).

Descoberto ao testar `make GPU=real` pela primeira vez com uma GPU NVIDIA
de verdade (RTX 4070, ver item 7 acima). `nvcc fatal: Unsupported gpu
architecture 'sm_52'` — Maxwell, arquitetura antiga demais pro CUDA 13.
Corrigido pra `-arch=sm_89` (Ada Lovelace, compute capability real da RTX
4070). **Quem usar outra geração de GPU precisa ajustar esse valor pra sua
própria compute capability** — não há detecção automática.

## 10. [CORRIGIDO] Item 06 do design de arquitetura implementado — templates de `kernel.cu` viraram parâmetros de runtime

**Onde:** [kernel.cu](../src/core/kernel.cu),
[dgm_core.cpp:207](../src/core/dgm_core.cpp#L207). Detalhes completos em
[04-gpu-cuda.md](04-gpu-cuda.md) seção 4.

`ApplyValuesC01`/`GpuExecution01` eram `template <int t_block_size, int
t_repeat_count, int t_coalesced_bits>` — item 06 do design de arquitetura
de 2026-08-10 propunha eliminar isso (opção "c": dispatch em runtime).
Implementado em 2026-08-11 assim que uma GPU NVIDIA real ficou disponível
pra testar de verdade: os três viraram parâmetros comuns, a shared memory
do kernel virou dinâmica (`extern __shared__`, com
`cudaFuncSetAttribute(..., cudaFuncAttributeMaxDynamicSharedMemorySize,
...)` pra liberar mais que os 48KB padrão), e as duas camadas de `switch`
(`GEWrapper2` + `GpuExecutionWrapper`) foram removidas — os valores fluem
direto como argumentos, sem recompilar nada por combinação.

**Bug exposto pela mudança (não introduzido por ela):** o endereçamento
global do kernel assume que cada bloco CUDA cobre exatamente
`2^gpu_region_bits` amplitudes, o que só é verdade se
`2*block_size*repeat_count == 2^gpu_region_bits`. Antes do item 06 essa
invariante nunca podia ser violada (só existia uma combinação
selecionável, a default, já consistente por construção). Com qualquer
combinação aceita em runtime, uma combinação inconsistente (testado:
`coalesced_bits=6, block_size=128, repeat_count=4, gpu_region_bits=8`,
onde `2*128*4=1024 ≠ 2^8=256`) causava `illegal memory access` na GPU.
Corrigido adicionando a validação em `DGM::validateTuning()`, que agora
aborta com mensagem clara em vez de deixar o kernel corromper memória.

**Verificado com GPU real (RTX 4070, CUDA Toolkit 13.3.1, WSL2 Ubuntu
24.04):** `make clean && make GPU=real` compila em ~2.5-7s (não mais
horas — ver item 7); combo default (`coalesced_bits=4, block_size=64,
repeat_count=2, gpu_region_bits=8`) segue reproduzindo a amplitude exata
`0.03125` em `t_GPU`/`t_HYBRID`; um combo **novo**, nunca antes alcançável
sem adicionar `case` e recompilar (`block_size=128, repeat_count=2,
gpu_region_bits=9`), também reproduziu a amplitude correta em 10 e 16
qubits; o combo inconsistente citado acima passou a falhar com a mensagem
de `validateTuning()` em vez de crashar; `make test` completo (66/66 +
smoke test) sem regressão; `shor.out 15` em `t_GPU` rodado 8× (6/8
sucesso) dentro da variação probabilística já documentada, sem indício de
bug específico de GPU.

---

## 11. [REMOVIDO] Segunda leva de código morto: `enablePeerAccess`, `GET_BLOCK_ID`, `report_num_threads`, `quantum_ipow`, `HadamardNQubits_PAR_CPU`/`_GPU`, macros `ACUMM`/`SHIFT_READ`/`SHIFT_WRITE`/`MAT_START`/`MAT_SIZE`/`MAT_END`, `LINE`/`BASE`, `t_SPEC`, `DGM::genMatrix`, `PT::ctrlAffect`/`setArgs`/`setArgs_soft`

**Onde:** `src/core/kernel.cu`, `src/core/dgm_par_exec.cpp`,
`src/algorithms/lib_shor_number_theory.cpp` + `lib_shor.h`,
`src/algorithms/lib_general.cpp` + `lib_general.h`, `include/core/common.h`
+ `src/core/common.cpp`, `include/core/dgm.h` + `src/core/dgm_parser.cpp`.

Boa parte do código já vinha marcada com comentários "sem uso no código
atual"/"nunca chegou a ser usado"/"não é chamada de lugar nenhum" desde a
passada de comentários de 2026-08-11 (ver commit `ed00b97`) — mas ninguém
tinha voltado pra de fato confirmar e remover. Confirmado por `grep` em
todo o repositório (2026-08-11, mesma sessão dos itens 9/10): zero
chamadores pra cada um dos símbolos acima. `PT::ctrlAffect`/`setArgs`/
`setArgs_soft` eram um caso à parte, sem o comentário explícito de "sem
uso" — parte de uma otimização de controle parcial nunca finalizada
(`setArgs`/`setArgs_soft` eram idênticas na prática, `ctrlAffect` só era
chamada por `setArgs`); o motor de CPU usa `target_bit`/`control_mask`/
`control_value` do `PT` diretamente, nunca precisou do `arg[]` empacotado
que essas três montavam (só o lado GPU precisa, via `setArgsGPU`, que
**não** foi tocada).

**Não removido, de propósito:** os campos de struct `PT::span_start_bit`
e `PT::control_rest`/`control_rest_count` — também "sem uso", mas por um
motivo diferente (documentado nos próprios comentários): são parte de
features nunca finalizadas (portas multi-qubit e a mesma otimização de
controle parcial acima) e foram deliberadamente **mantidos** como
placeholder em vez de removidos, numa decisão já tomada antes desta
sessão. Remover um campo de struct é uma decisão de design diferente de
remover uma função órfã — não revertida aqui.

**Verificado:** `make test` (66/66 + smoke test) no Windows (`GPU=stub`)
e no WSL com GPU real (`GPU=real`, `KERNEL_OPT=-O3`), sem regressão.

---

*Achados durante a leitura de documentação em 2026-08-06, com adições em
2026-08-10 durante os testes de build da Fase 1 da renomeação e em
2026-08-11 durante a rodada de arquitetura, a passada de comentários e um
checkup geral do projeto. Atualizar esta lista conforme novos pontos
forem encontrados ou os existentes forem corrigidos.*
