# Arquitetura Geral

Este documento descreve a versão em Rust (workspace em [`crates/`](../crates/README.md)).
A implementação original em C++/CUDA está no tag `v1.1.0` e na branch `cpp-legacy`; onde
vale a pena, o texto diz o que o C++ fazia de diferente.

## 1. Representação do estado quântico

Um registrador de `qubits` qubits é representado por um vetor de `2^qubits` números
complexos de precisão simples (`Complex32`, o alias `C32` de `hgm-core`), guardado em
[`StateVector`](../crates/hgm-core/src/state.rs):

```rust
pub struct StateVector {
    qubits: usize,
    amplitudes: Vec<C32>,          // 2^qubits posições, 8 bytes cada
    scratch: Option<Vec<C32>>,     // buffer auxiliar da multiplicação modular
}
```

`StateVector::basis(qubits, index)` aloca o vetor **de forma falível** (a memória já vem
zerada do alocador, sem escrever zeros) e põe amplitude 1 em `index`. Falta de memória vira
`HgmError::OutOfMemory`, e mais de `MAX_QUBITS` (32) vira `HgmError::TooManyQubits`; o C++
terminava o processo com `exit`.

Cada posição `i` do vetor é a amplitude do estado da base computacional de índice binário
`i`. A convenção de qual bit do índice pertence a qual qubit é **a coisa mais importante para
entender antes de ler qualquer outra parte do código**:

> **O qubit de índice `q` na descrição do circuito corresponde ao bit
> `(qubits - 1 - q)` do índice `i` no vetor de estado.**

Ou seja, o qubit "0" do circuito é o bit mais significativo (MSB) do índice, e o último qubit
(`qubits-1`) é o bit menos significativo (LSB). A conversão está em um único lugar para a
medição, [`qubit_shift`](../crates/hgm-core/src/measure.rs):

```rust
pub fn qubit_shift(qubits: usize, qubit: usize) -> Result<u32> {
    // erro se qubit >= qubits
    Ok((qubits - 1 - qubit) as u32)
}
```

e na hora de transformar a posição de uma porta no passo do circuito (contada da esquerda) no
"bit do alvo" (`target_bit`) usado na execução, em `gen_pts`
([`circuit.rs`](../crates/hgm-core/src/circuit.rs)):

```rust
let bit = (qubits - position - 1) as u32;          // para cada controle
target_bit: (qubits - position - 1) as u32,        // para o alvo
```

Exemplo com 3 qubits (`q0 q1 q2`), índice `i` em binário `b2 b1 b0`: `q0` → bit 2 (`b2`,
MSB), `q1` → bit 1, `q2` → bit 0 (LSB). Se `amplitudes[0b100] = 1`, esse é o estado
`|q0=1, q1=0, q2=0>`.

## 2. O workspace e o `Simulator`

No C++ tudo girava em torno de uma classe `DGM`. No Rust as responsabilidades estão em
crates separados, com dependências em uma só direção:

```text
cli -> algos -> engine -> {cpu, gpu} -> core        (golden: só em testes)
```

| Crate | O que guarda |
|---|---|
| [`hgm-core`](../crates/hgm-core) | o que independe de CPU paralela e GPU: catálogo de portas (`GateCatalog`), parser da linguagem de circuitos, a porta compilada (`Pt`) e `Op`, o otimizador (fusão de fase e agendamento), `StateVector`, medição, `modmult`, o gerador `Mt19937` e a trait `Backend` |
| [`hgm-cpu`](../crates/hgm-cpu) | os backends de CPU: serial e paralelo por regiões (ver [03](03-motor-de-execucao-cpu.md)) |
| [`hgm-gpu`](../crates/hgm-gpu) | o backend de GPU em wgpu/WGSL (ver [04](04-gpu-wgpu.md)) |
| [`hgm-engine`](../crates/hgm-engine) | o `Simulator`, que junta o parser, o estado, o otimizador e um backend; e o `HybridBackend` |
| [`hgm-algos`](../crates/hgm-algos) | `general`, Grover e Shor ([05](05-algoritmo-grover.md), [06](06-algoritmo-shor.md)) |
| [`hgm-cli`](../crates/hgm-cli) | os executáveis `general`, `grover` e `shor` |
| [`hgm-golden`](../crates/hgm-golden) | leitor dos vetores de referência de `tests/golden`; só testes |

O objeto central do dia a dia é o [`Simulator`](../crates/hgm-engine/src/lib.rs) (o papel do
`DGM`). Ele guarda:

- o `StateVector` (no backend de GPU, a cópia que vale mora na placa, ver abaixo);
- um `Box<dyn Backend>`, o executor escolhido;
- o `GateCatalog` da execução (as portas base e as rotações criadas pelo Shor);
- o gerador `Mt19937` compartilhado por medições e sorteios;
- se a fusão de portas de fase está ligada.

Um uso típico (o que `Grover` e `Shor` fazem) é:

```rust
use hgm_engine::{BackendKind, Simulator};

let mut sim = Simulator::new(BackendKind::CpuSerial, qubits)?;  // aloca em |0...0>
sim.set_basis(pos)?;                    // estado inicial |pos>

sim.run(circuito, 1, 1)?;               // faz o parsing do texto e executa
// ou, em partes:
let programa = sim.compile_steps(&passos, 1)?;   // só o parsing (texto -> Program)
sim.run_program(&programa, 1)?;                  // só a execução

let m = sim.measure(q_pos)?;            // mede um qubit e colapsa o estado
println!("{}", m.bit);
```

`Simulator::with_config(&SimConfig, qubits)` também escolhe a semente e se a fusão de fase fica
ligada. Erros são `Result<_, HgmError>`; nenhum caminho do motor encerra o processo.

### A trait `Backend`

Quem executa programas implementa [`Backend`](../crates/hgm-core/src/backend.rs). O método
obrigatório é `execute(state, program, repetitions)`; os demais têm implementação padrão que
opera sobre o `StateVector` do host (medir, colapsar, `modmult`, ler uma amplitude). Um backend
cujo estado mora fora do host (a GPU) sobrescreve todos eles: `initial_state_ready`,
`host_state_replaced`, `set_basis`, `sync_to_host`, `measure_qubit`, `collapse`,
`apply_mod_mult` e `amplitude`. O `Simulator::state()` chama `sync_to_host` e, com a GPU, custa
uma cópia do vetor inteiro; para ler pouco, use `Simulator::amplitude(i)`.

## 3. Os quatro backends de execução

`BackendKind` ([`hgm-engine/src/lib.rs`](../crates/hgm-engine/src/lib.rs)) escolhe um de quatro
caminhos, todos calculando a mesma coisa. O número é o `EXEC_TYPE` dos executáveis:

| `EXEC_TYPE` | `BackendKind` | Onde | Ideia |
|---|---|---|---|
| `0` | `CpuSerial` | `hgm-cpu`, `serial.rs` | 1 thread, laço simples sobre o vetor de estado; o oráculo dos demais |
| `1` | `CpuParallel(Tuning)` | `hgm-cpu`, `parallel.rs` | rayon: divide o vetor em "regiões" e processa várias em paralelo (AVX2 se a CPU tem) |
| `2` | `Gpu(GpuTuning)` | `hgm-gpu` | wgpu/WGSL: usa memória de workgroup e agrupa portas em passadas; estado residente na GPU |
| `3` | `Hybrid(HybridTuning)` | `hgm-engine`, `hybrid.rs` | a GPU enquanto o estado cabe; a CPU paralela quando não |

A ideia geral de "região" (usada na CPU paralela e na GPU) é: como o vetor de estado é gigante
(`2^qubits`) e não muda de tamanho, dá para particioná-lo em blocos independentes (fixando
alguns bits do índice) e processar cada bloco em paralelo — em threads diferentes na CPU, em
workgroups diferentes na GPU.

**O modo híbrido mudou em relação ao C++.** Lá, o `t_HYBRID` dividia as regiões entre CPU e GPU
ao mesmo tempo, com memória fixada (`cudaHostRegister`). O wgpu não oferece memória fixada, e o
ganho medido não compensava (a transferência fica em ~2,1 GB/s na RTX A4500). O `Hybrid` do
Rust roda tudo na GPU enquanto o estado cabe e, se a GPU recusa o estado (`DeviceOutOfMemory`),
traz o estado para o host e continua na CPU paralela; a troca é de mão única. Detalhes em
[04](04-gpu-wgpu.md) e em [12](12-versao-rust.md).

Antes de chegar ao backend, o `Simulator` pode passar o programa pelo otimizador:
`fuse_phase_runs` troca corridas de portas de fase por uma única porta fundida. A fusão é
ligada por padrão na GPU e no híbrido, e desligada na CPU
([`default_fuse_phase`](../crates/hgm-engine/src/lib.rs); `HGM_FUSE_PHASE=0/1` força). O
agendamento por comutação (`schedule_for_batching`) é feito pelo backend de GPU, que é o que se
beneficia dele ([04](04-gpu-wgpu.md), seção 4).

## 4. Fluxo de um algoritmo (ex: Grover)

1. `grover()` (em [`hgm-algos`](../crates/hgm-algos/src/grover.rs)) monta o circuito **como
   texto** usando os geradores de [`steps.rs`](../crates/hgm-algos/src/steps.rs) (`hadamard`,
   `pauli_x`, ...) e funções próprias (oráculo, `ControledZ`).
2. Chama `sim.compile_steps(...)`, que faz o parsing dessas strings e produz um `Program`: uma
   lista de `Op`, cada uma uma "porta compilada" (`Pt`: matriz 2x2, bit do alvo, máscara e valor
   de controle) ou uma fase fundida.
3. Chama `sim.run_program(&programa, repetições)`, que percorre as `Op` e realmente multiplica
   as amplitudes do estado pelas matrizes.
4. Chama `sim.measure(q_pos)` para obter um bit clássico, colapsando o estado.

Esse padrão (montar texto → compilar → executar → medir) se repete em Grover e em cada uma das
`2n` rodadas do Shor.
