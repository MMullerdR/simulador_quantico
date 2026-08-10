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

**Verificado:** compila limpo; `shor.out`/`grover.out`/`general.out`
rodados no WSL sem crash (bom teste de estresse, já que cada
`setFunction(reset=true)` aloca e libera um lote de `PT`s).

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

## 6. [CORRIGIDO] Segfault em `t_PAR_CPU` quando `cpu_region_bits > qubits`

**Onde:** `src/cli/general.cpp` (defaults do `main()`) +
`PCpuExecution1` em [dgm_par_exec.cpp:8](../src/core/dgm_par_exec.cpp#L8).

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

**Pendente:** `HybridExecution` (modo `t_HYBRID`, que precisa de GPU real
pra testar) tem uma lógica de região parecida e pode ter o mesmo problema
latente — não corrigido ainda, só sinalizado aqui.

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
