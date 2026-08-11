# Simulador Quântico — HybriD-GM

Simulador de computação quântica baseado em vetor de estado (*state-vector*),
capaz de rodar em CPU serial, CPU paralela (OpenMP), GPU (CUDA) ou em modo
híbrido CPU+GPU simultâneo. Não usa nenhuma biblioteca de circuito quântico
pronta — parser de portas, motor de execução e kernels CUDA são escritos à
mão em C/C++/CUDA.

O modelo **HybriD-GM** foi concebido como uma metodologia computacional para
simulação de computação quântica voltada a arquiteturas paralelas híbridas
clássicas, para apoiar o estudo de algoritmos quânticos. Ele explora
Computação de Alto Desempenho (HPC) para melhorar performance por meio de
(i) otimização de recursos e escalabilidade independente de hardware, e
(ii) operadores de composição e projeção, incluindo gerenciamento de memória
coalescida — combinando arquiteturas CPU e/ou GPU numa estrutura híbrida.

Traz duas implementações de algoritmos quânticos como carga de
teste/benchmark: **Grover** (busca não estruturada) e **Shor** (fatoração
de inteiros).

## Documentação

A pasta [docs/](docs/00-indice.md) tem uma explicação aprofundada de como o
simulador funciona por dentro (representação do estado, linguagem de
circuitos, motor de execução em CPU/GPU, e os algoritmos de Grover e Shor
mapeados linha a linha ao código) — comece por
[docs/00-indice.md](docs/00-indice.md). Bugs e pontos de atenção conhecidos
estão listados em
[docs/07-bugs-e-pontos-de-atencao.md](docs/07-bugs-e-pontos-de-atencao.md).

## Estrutura do projeto

```
include/
 ├─ core/           common.h, dgm.h, gates.h
 ├─ algorithms/     lib_general.h, lib_grover.h, lib_shor.h
 └─ cli/            cli_common.h
src/
 ├─ core/           motor de simulação: common.cpp, dgm_*.cpp, gates.cpp, kernel.cu
 ├─ algorithms/     bibliotecas de algoritmo: lib_general.cpp, lib_grover.cpp, lib_shor.cpp
 └─ cli/            main() de cada executável: general.cpp, grover.cpp, shor.cpp
tests/               regressão (tests/test_qft_addf.cpp) e smoke test fim a fim (tests/smoke_test.sh)
outputs/            gerado pelo build (.o e .out) — não versionado
docs/                documentação
makefile
```

## Requisitos

O projeto usa `nvcc` (CUDA toolkit), `g++` com OpenMP e headers POSIX
(`sys/time.h`, `unistd.h`) — precisa de **Linux** ou **WSL2** com o
[CUDA toolkit](https://developer.nvidia.com/cuda/wsl) instalado. Não
compila nativamente no Windows/MSVC.

## Build

```bash
make            # compila os três executáveis (shor, grover, general)
make shor       # só o shor.out
make grover     # só o grover.out
make general    # só o general.out
make clean      # remove outputs/
make test       # builda tudo, roda a regressão de circuito e o smoke test fim a fim
```

Os binários e objetos vão para `outputs/` (ex: `outputs/shor.out`).

Dois parâmetros de tuning no topo do `makefile`:
- `ARCH` — *compute capability* da GPU alvo (`nvidia-smi --query-gpu=compute_cap --format=csv` para descobrir a sua).
- `OPS_BLOCK` — quantas portas cabem coalescidas num lote enviado à GPU por vez.

## Uso

Tipos de execução (`enum` em `include/core/dgm.h`): `0=t_CPU, 1=t_PAR_CPU,
2=t_GPU, 3=t_HYBRID`.

```bash
# outputs/general.out <qubits> <tipo_execução> [threads|gpus] [block_size] [repeat_count] [gpu_region_bits]
outputs/general.out 20 1 4        # benchmark de Hadamard, 20 qubits, CPU paralela, 4 threads

# outputs/grover.out <qubits> [tipo_execução] [threads|gpus] [block_size] [repeat_count] [gpu_region_bits]
outputs/grover.out 10 0            # busca de Grover, 10 qubits, CPU serial

# outputs/shor.out <qubits> [tipo_execução] [threads|gpus] [block_size] [repeat_count] [gpu_region_bits]
# qubits precisa ser um dos valores mapeados internamente:
# 15->57, 17->119, 19->253, 21->485, 23->1017, 25->2045, 27->2863
outputs/shor.out 15 0
```

`block_size`/`repeat_count`/`gpu_region_bits` só valem pra `t_GPU`/`t_HYBRID`
(exec_type 2/3) — ajustam o tuning de execução em GPU (ver
[docs/04-gpu-cuda.md](docs/04-gpu-cuda.md)). Precisam satisfazer
`2*block_size*repeat_count == 2^gpu_region_bits` e `repeat_count` ser
potência de 2 — `DGM::validateTuning()` recusa a combinação com uma
mensagem clara em vez de deixar a GPU acessar memória fora dos limites.
Exemplo explorando um tuning diferente do default
(`gpu_coalesced_bits=4, block_size=64, repeat_count=2, gpu_region_bits=8`):

```bash
outputs/general.out 16 2 1 128 2 9   # t_GPU, block_size=128, repeat_count=2, gpu_region_bits=9
```

Veja [docs/05-algoritmo-grover.md](docs/05-algoritmo-grover.md) e
[docs/06-algoritmo-shor.md](docs/06-algoritmo-shor.md) para o que cada
algoritmo faz e como está implementado.
