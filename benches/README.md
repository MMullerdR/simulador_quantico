# Benchmarks de CPU: C++ x Rust

Compara o `t_PAR_CPU` do C++ (tag `v1.1.0`) com o backend paralelo do Rust, com as
**mesmas cargas** nos dois lados (critério 2 de [docs/09](../docs/09-plano-migracao-rust.md)).

| Carga | Circuito |
|---|---|
| `hadamard` | um passo com `H` em todos os qubits |
| `phase` | um passo com `R1,R2,R3` alternados em todos os qubits |
| `mixed` | `H` em todos + 8 passos com um controle e um alvo (`X`,`Z`,`Y`,`R2`) |

Cada lado imprime `tempo_s=... a0=... a1=... checksum=...`. O `checksum` é uma soma
ponderada do vetor inteiro: se diferir além da 6ª casa, os dois não calcularam o mesmo
estado e o tempo não vale.

## Rust

```bash
cargo run --release -p hgm-cpu --example bench_par -- QUBITS CARGA THREADS REGIAO COALESCIDA REPS [serial] [otimizado]
```

`otimizado` liga fusão de fase e agendamento em lotes.

## C++

O C++ não está mais na `main`. Num checkout do tag `v1.1.0` (`git clone --branch v1.1.0 <repo>`),
copie `benches/cpp/bench_par.cpp` para a raiz do checkout, rode `make` (gera `outputs/*.o`) e:

```bash
g++ -O3 -fopenmp -Iinclude bench_par.cpp <objetos do core> -o bench_par.out
./bench_par.out QUBITS CARGA THREADS REGIAO COALESCIDA REPS [serial]
```

## Ponta a ponta: C++ x Rust (fase 7)

`benches/end_to_end.sh DIR_CPP DIR_RUST [FILTRO]` (`DIR_CPP` = checkout do tag `v1.1.0` compilado com `make GPU=real`) roda as mesmas linhas de comando nos dois
(general, grover e shor em serial, CPU paralela, GPU e híbrido) e imprime tempo de parede, se
achou o resultado e a razão Rust/C++. Resultado e leitura em
[docs/12](../docs/12-versao-rust.md#4-desempenho-contra-o-c-v110).

## Resultado registrado (2026-11-21)

qflex (Threadripper PRO 5955WX), 26 qubits, 16 threads, região 14, coalescida 11,
3 repetições, uma execução por célula, Rust em `--release` sem `target-cpu`:

| Carga | C++ | Rust |
|---|---|---|
| `hadamard` | 0,759 s | 0,426 s |
| `phase` | 0,697 s | 0,350 s |
| `mixed` | 0,887 s | 0,555 s |

Limites: três cargas sintéticas, uma máquina, só Linux, sem GPU. Não é um benchmark
do Grover/Shor completos.
