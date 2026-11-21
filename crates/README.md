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
| `hgm-core` | catálogo de portas, parser da linguagem de circuitos, `Pt`, `StateVector`, trait `Backend` | portado: parser (ordenação por passo), classificação de matriz. **Falta:** fusão de fase, agendamento, medição |
| `hgm-cpu` | backends de CPU | portado: serial (`t_CPU`). **Falta:** paralelo (regiões + rayon) |
| `hgm-gpu` | GPU portátil (wgpu + WGSL) | vazio; é o spike da fase 2 (go/no-go) |
| `hgm-engine` | `Simulator`: escolhe o backend, executa circuitos | mínimo (só `CpuSerial`) |
| `hgm-algos` | algoritmos | portado: teoria dos números do Shor. **Falta:** `general`, `grover`, `shor` |
| `hgm-cli` | binário `hgm` | só uma demonstração (estado de Bell) |
| `hgm-golden` | leitor dos vetores de `tests/golden` | só testes |

## Rodar

```bash
cargo test --workspace      # paridade com o C++ (tests/golden) + testes unitários
cargo run -p hgm-cli        # demonstração
```

## Convenções

- `unsafe` é proibido em todo o workspace (`[workspace.lints]`). Quando o backend
  paralelo precisar dele, será num único módulo de `hgm-cpu`, com `// SAFETY:` e
  testes (loom/miri se couber).
- Onde o C++ tinha `exit`, ponteiro nulo ou comportamento indefinido, o Rust devolve
  `Result`/`Option` (porta desconhecida, passos com números de qubits diferentes,
  falta de memória, inverso modular inexistente). Cada desvio está comentado no
  código do item.
- Matrizes de portas `R1`..`R3` são calculadas em `f64` e arredondadas; o C++ usa
  `cpowf`. Os testes comparam matrizes com tolerância de 2e-7.
