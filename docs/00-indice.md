# Documentação do HybriD-GM (Quantum-Simulator)

Este é um simulador de computação quântica baseado em vetor de estado (state-vector),
capaz de rodar em CPU serial, CPU paralela (OpenMP), GPU (CUDA) ou em modo híbrido
CPU+GPU simultâneo. Ele não usa nenhuma biblioteca de "circuito quântico" pronta —
tudo, desde o parser de portas até a aplicação da matriz na memória, é escrito à mão
em C/C++/CUDA, com muita manipulação de bits para performance.

## Estrutura de pastas

```
include/
 ├─ core/           common.h, dgm.h, gates.h
 ├─ algorithms/     lib_general.h, lib_grover.h, lib_shor.h
 └─ cli/            cli_common.h
src/
 ├─ core/           motor de simulação: common.cpp, dgm_*.cpp, gates.cpp, kernel.cu
 ├─ algorithms/     bibliotecas de algoritmo: lib_general.cpp, lib_grover.cpp, lib_shor.cpp
 └─ cli/            main() de cada executável: general.cpp, grover.cpp, shor.cpp
tests/               regressão de string (tests/test_qft_addf.cpp), regressão de
                     execução via DGM (tests/test_gates.cpp) e smoke test fim a
                     fim (tests/smoke_test.sh) — `make test`
outputs/            gerado pelo `make` (.o e .out) — não versionado
docs/                esta documentação
```

`include/` espelha a divisão de `src/` — cada header vive na mesma
subpasta (`core`/`algorithms`/`cli`) do(s) `.cpp` que ele declara.

Esta documentação foi escrita para dar entendimento *profundo* de como o simulador
funciona por dentro, não apenas listar o que cada arquivo faz. Ordem sugerida de leitura:

1. [01-arquitetura-geral.md](01-arquitetura-geral.md) — visão geral: representação do
   estado, a classe `DGM`, os "backends" de execução (CPU/GPU/híbrido).
2. [02-linguagem-de-circuitos.md](02-linguagem-de-circuitos.md) — o "mini-idioma" de
   texto usado para descrever portas e como ele vira estruturas `PT` executáveis.
3. [03-motor-de-execucao-cpu.md](03-motor-de-execucao-cpu.md) — o algoritmo central:
   como uma matriz 2x2 é aplicada sobre um vetor de 2^n amplitudes usando truques de bits.
4. [04-gpu-cuda.md](04-gpu-cuda.md) — como o mesmo algoritmo é levado para a GPU
   (kernel.cu), memória compartilhada, "coalescimento" e multi-GPU.
5. [05-algoritmo-grover.md](05-algoritmo-grover.md) — Grover explicado linha a linha
   em cima do código real.
6. [06-algoritmo-shor.md](06-algoritmo-shor.md) — Shor explicado: layout de registradores,
   QFT, somador em Fourier, estimação de fase semi-clássica, pós-processamento clássico.
7. [07-bugs-e-pontos-de-atencao.md](07-bugs-e-pontos-de-atencao.md) — lista viva de bugs,
   código morto e pontos frágeis encontrados durante a leitura (útil antes de mexer em algo).
8. [08-performance.md](08-performance.md) — números de tempo de execução
   medidos com GPU real, por backend e número de qubits — quando cada
   backend vale a pena, e até onde `QB_LIMIT` aguenta na prática.

## Mapa mental rápido

```
main() (general.cpp / grover.cpp / shor.cpp)
   |
   v
lib_general.cpp / lib_grover.cpp / lib_shor.cpp   <- monta o circuito como strings
   |                                                  ("Hadamard", "CNot", "QFT", ...)
   v
DGM::setFunction() / executeFunction()   (dgm_parser.cpp)  <- faz o parsing das strings -> PT[]
   |
   v
DGM::execute()   (dgm_core.cpp)
   |
   +-- t_CPU      -> CpuExecution1_*        (dgm_cpu_exec.cpp, um core, sem paralelismo)
   +-- t_PAR_CPU  -> PCpuExecution1         (dgm_par_exec.cpp, OpenMP, vários cores CPU)
   +-- t_GPU      -> GpuExecutionWrapper    (kernel.cu, CUDA)
   +-- t_HYBRID   -> HybridExecution        (dgm_par_exec.cpp, CPU + GPU ao mesmo tempo)
```

Todos os quatro caminhos implementam **a mesma operação matemática** (aplicar uma
porta 2x2 controlada sobre um vetor de estado), só que com estratégias de
paralelização e acesso à memória diferentes. Entender bem o caminho `t_CPU`
(o mais simples) é a chave para entender todos os outros.
