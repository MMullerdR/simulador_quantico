# Workspace Rust

Migração do HybriD-GM para Rust, conforme [docs/09](../docs/09-plano-migracao-rust.md).
O C++ original continua na raiz do repositório, no tag `v1.1.0` e na branch
`cpp-legacy`; nada aqui o substitui até a paridade ser provada contra
[`tests/golden`](../tests/golden/README.md).

```text
cli -> algos -> engine -> {cpu, gpu} -> core        (golden: só em testes)
```

| Crate | Papel | Estado |
|---|---|---|
| `hgm-core` | catálogo de portas, parser da linguagem de circuitos, `Pt`/`Op`, `StateVector`, trait `Backend` | portado: parser, classificação de matriz, fusão de fase, agendamento em lotes, medição/colapso/probabilidades, `modmult`, `Mt19937` (igual ao `std::mt19937`) |
| `hgm-cpu` | backends de CPU | portado: serial (`t_CPU`) e paralelo por regiões com rayon (`t_PAR_CPU`). `unsafe` só em `src/shared.rs`. **Falta:** modo híbrido (fase 6) |
| `hgm-gpu` | GPU portátil (wgpu + WGSL) | kernels de região/global/medição/banda, planejador de passadas, contexto (`prepare`/`run`), paridade com a CPU (spike da fase 2, `docs/09` §4.2) e `GpuBackend` (fase 4: `Backend` com o estado no host, em ida e volta, ou **residente** na GPU com medição, colapso e `modmult` lá; o `Simulator` usa o residente). fase fundida no kernel de região (opcional, `HGM_FUSE_PHASE=1`; empata com as portas separadas). **Falta (fase 4):** estados > 27 qubits |
| `hgm-engine` | `Simulator`: escolhe o backend, executa circuitos, mede | `CpuSerial`, `CpuParallel(Tuning)` e `Gpu(GpuTuning)`; medição, colapso, `modmult`, semente (`SimConfig`); fusão opcional (desligada) |
| `hgm-algos` | algoritmos | portado: `general`, `grover` (blocos de iterações), `shor` (circuito e emulação, com a correção do item 30), teoria dos números, geradores de circuito (QFT, somadores de Draper, mult. modular) |
| `hgm-cli` | executáveis `general`, `grover`, `shor` | portado, argumentos posicionais como os `*.out` do C++. Tipo 2 (GPU) usa o `GpuBackend`; `BLOCK_SIZE`, `REPEAT_COUNT` e `GPUS` são ignorados, `GPU_REGION_BITS` vale. O tipo 3 (híbrido) é recusado até a fase 6 |
| `hgm-golden` | leitor dos vetores de `tests/golden` | só testes |

## Rodar

```bash
cargo test --workspace      # paridade com o C++ (tests/golden) + testes unitários
bash tests/smoke_test_rust.sh   # compila em release e confere os 3 executáveis (parte de CPU do smoke test do C++)

# QUBITS [EXEC_TYPE: 0 serial, 1 paralela] [THREADS]; HGM_SEED fixa a semente
cargo run --release -p hgm-cli --bin general -- 14 1 4
cargo run --release -p hgm-cli --bin grover  -- 12 0
HGM_SEED=7 SHOR_EMULATE=1 cargo run --release -p hgm-cli --bin shor -- 15 0

# GPU (precisa de adaptador; HGM_REQUIRE_GPU=1 faz a falta dele virar falha)
HGM_REQUIRE_GPU=1 cargo test --release -p hgm-gpu
cargo run --release -p hgm-gpu --example spike -- 26 12 4 5 3 300
cargo run --release -p hgm-algos --example gpu_grover_bench -- 24 10
HGM_SEED=7 cargo run --release -p hgm-cli --bin grover -- 12 2   # GPU (tipo de execução 2)

# comparação de CPU com o C++ (ver ../benches/README.md)
cargo run --release -p hgm-cpu --example bench_par -- 26 mixed 16 14 11 3
```

## Convenções

- `unsafe` é proibido em todo o workspace (`[workspace.lints]`), com uma exceção: o
  módulo `crates/hgm-cpu/src/shared.rs`, onde o backend paralelo entrega a cada
  thread uma região disjunta do vetor. Cada acesso é checado em tempo de execução
  contra a região (índice fora dela vira pânico) e cada bloco `unsafe` tem
  `// SAFETY:`. Não rodei `miri`/`loom`.
- Onde o C++ tinha `exit`, ponteiro nulo ou comportamento indefinido, o Rust devolve
  `Result`/`Option` (porta desconhecida, passos com números de qubits diferentes,
  falta de memória, inverso modular inexistente). Cada desvio está comentado no
  código do item.
- Matrizes de portas `R1`..`R3` são calculadas em `f64` e arredondadas; o C++ usa
  `cpowf`. Os testes comparam matrizes com tolerância de 2e-7.
