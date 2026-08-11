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
emprestado de `Gates::list` (`gates.getMatrix(...)`, o cache estático de
matrizes de porta, ver item 5). Se essa condição também fosse invertida para
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

**Onde:** [dgm_parser.cpp:132](../src/core/dgm_parser.cpp#L132)

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
[dgm_par_exec.cpp:397](../src/core/dgm_par_exec.cpp#L397)) já usa `new
PT()` de verdade, então não tinha esse problema.

**Superado em seguida (2026-08-11):** esse fix pontual foi substituído
por uma correção estrutural — `genPTs` passou a usar `new PT()` (a
mesma forma que `HybridExecution` já usava), e `PT` ganhou um `~PT()`
de verdade no lugar do `destructor()` manual. Ver
[dgm_parser.cpp:137](../src/core/dgm_parser.cpp#L137). Duas formas de
alocação inconsistentes para o mesmo struct era, junto com o
`destructor()` de chamada manual, a causa raiz dessa classe inteira de
bug — agora estruturalmente impossível de esquecer.

**Verificado no WSL (execução real, não só compilação):**
`make clean && make && outputs/general.out 10 1 4 && outputs/grover.out
12 1 4 && outputs/shor.out 15 0` — os três rodam sem crash;
`general.out` reproduz a amplitude uniforme `0.03125` esperada. Também
confirmado `general.out 10 3 4` (`t_HYBRID`, o cenário que crashava)
sem crash.

## 4. `DGM::freeMemory()` chamado sobre estado que não foi alocado por `DGM`

**Onde:** `GenericExecute` ([dgm_core.cpp:28](../src/core/dgm_core.cpp#L28)) usa
`dgm.setMemory(state)` (que não copia, só aponta `state` para o ponteiro
recebido). Se o chamador espera manter posse desse ponteiro depois, é
preciso ter cuidado: `DGM::freeMemory()`/o destrutor da `DGM` chamam
`free(state)` sobre esse mesmo ponteiro. Vale revisar caso a caso quem é
"dono" do buffer antes de usar essas funções em código novo.

## 5. `Gates::list` é `static` (compartilhado entre todas as instâncias)

**Onde:** [gates.h:34](../include/core/gates.h#L34),
[gates.cpp:7](../src/core/gates.cpp#L7)

Não é um bug isoladamente, mas é a causa raiz de por que o problema do item
1 se manifesta como está: como `Gates::list` é global/estático, portas
registradas com o mesmo nome em execuções diferentes (ou até rodadas
diferentes do mesmo `Shor()`) colidem. Vale ter isso em mente ao criar
qualquer porta nova dinamicamente (sempre garantir nomes realmente únicos,
ou aceitar que o cache seja intencional quando o valor for de fato o mesmo).

## 6. [CORRIGIDO] Segfault em `t_PAR_CPU`/`t_HYBRID` quando `region_bits > qubits` disponíveis

**Onde:** `src/cli/general.cpp` (defaults do `main()`) +
`PCpuExecution1` em [dgm_par_exec.cpp:78](../src/core/dgm_par_exec.cpp#L78).

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
em [dgm_par_exec.cpp:263](../src/core/dgm_par_exec.cpp#L263). Diferente do
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

---

*Achados durante a leitura de documentação em 2026-08-06, com adições em
2026-08-10 durante os testes de build da Fase 1 da renomeação. Atualizar
esta lista conforme novos pontos forem encontrados ou os existentes forem
corrigidos.*
