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
| `hgm-gpu` | GPU portátil (wgpu + WGSL) | vazio; é o spike da fase 2 (go/no-go) |
| `hgm-engine` | `Simulator`: escolhe o backend, executa circuitos, mede | `CpuSerial` e `CpuParallel(Tuning)`; medição, colapso, `modmult`, semente; fusão opcional (desligada) |
| `hgm-algos` | algoritmos | portado: teoria dos números do Shor. **Falta:** `general`, `grover`, `shor` |
| `hgm-cli` | binário `hgm` | só uma demonstração (estado de Bell) |
| `hgm-golden` | leitor dos vetores de `tests/golden` | só testes |

## Rodar

```bash
cargo test --workspace      # paridade com o C++ (tests/golden) + testes unitários
cargo run -p hgm-cli        # demonstração

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
