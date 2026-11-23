# Simulador Quântico — HybriD-GM

Simulador de computação quântica baseado em vetor de estado (*state-vector*),
capaz de rodar em CPU serial, CPU paralela, GPU (qualquer uma que o `wgpu`
suporte: Vulkan, Metal, DX12) ou em modo híbrido GPU+CPU. Não usa nenhuma
biblioteca de circuito quântico pronta — parser de portas, motor de execução e
kernels são escritos à mão em Rust (kernels de GPU em WGSL).

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

## Requisitos e build

Só o [Rust](https://rustup.rs) (o compilador está fixado em `rust-toolchain.toml`).
Funciona em Windows, Linux e macOS, sem `nvcc` nem `makefile`.

```bash
cargo build --release          # gera target/release/{general,grover,shor}
cargo test --workspace         # paridade com o C++ (tests/golden) e testes unitários
bash tests/smoke_test_rust.sh  # os três executáveis, fim a fim
```

## Uso

```bash
# general QUBITS [EXEC_TYPE] [THREADS] [BLOCK_SIZE] [REPEAT_COUNT] [GPU_REGION_BITS]
target/release/general 20 1 4        # benchmark de Hadamard, 20 qubits, CPU paralela, 4 threads

# grover QUBITS [EXEC_TYPE] [THREADS] [BLOCK_SIZE] [REPEAT_COUNT] [GPU_REGION_BITS] [SEARCH_VALUE]
target/release/grover 10 0           # busca de Grover, 10 qubits, CPU serial, procurando o valor 10 (padrão)
target/release/grover 10 0 1 1 1 1 42  # mesma busca, procurando o valor 42

# shor QUBITS [EXEC_TYPE] [THREADS] [BLOCK_SIZE] [REPEAT_COUNT] [GPU_REGION_BITS]
# qubits precisa ser um dos valores mapeados: 15->57, 17->119, 19->253, 21->485, 23->1017, 25->2045, 27->2863
target/release/shor 15 0
```

Tipos de execução: `0` CPU serial, `1` CPU paralela, `2` GPU, `3` híbrido (GPU enquanto o
estado cabe, CPU paralela quando não). Os limites, as variáveis de ambiente, as diferenças em
relação ao C++, o desempenho contra o `v1.1.0` e a tabela de hardware **verificado / não
testado** estão em [docs/12](docs/12-versao-rust.md).

## Estrutura do projeto

```
crates/              workspace Rust (ver crates/README.md): hgm-core, hgm-cpu, hgm-gpu,
                     hgm-engine, hgm-algos, hgm-cli, hgm-golden
tests/golden/        vetores de referência gerados pelo C++ do tag v1.1.0 (o oráculo de paridade)
tests/gpu-battery/   bateria de testes de GPU (docs/11) e o baseline do C++
benches/             benchmarks C++ x Rust
docs/                documentação
```

## Versão em C++/CUDA (histórica)

A implementação original em C++/CUDA (OpenMP + `nvcc`, só Linux/WSL2) saiu da `main` na
versão 2.0.0. Continua no tag [`v1.1.0`](https://github.com/MMullerdR/simulador_quantico/releases/tag/v1.1.0)
e na branch `cpp-legacy`, com o `makefile`, os testes e o README daquela época.

## Documentação

A pasta [docs/](docs/00-indice.md) tem uma explicação aprofundada de como o
simulador funciona por dentro (os documentos 01 a 08 descrevem a implementação em
C++/CUDA do tag `v1.1.0`; o Rust está em [docs/12](docs/12-versao-rust.md)) (representação do estado, linguagem de
circuitos, motor de execução em CPU/GPU, e os algoritmos de Grover e Shor
mapeados linha a linha ao código) — comece por
[docs/00-indice.md](docs/00-indice.md). Bugs e pontos de atenção conhecidos
estão listados em
[docs/07-bugs-e-pontos-de-atencao.md](docs/07-bugs-e-pontos-de-atencao.md).
