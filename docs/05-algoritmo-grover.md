# Algoritmo de Grover — mapeado ao código

Arquivo: [`grover.rs`](../crates/hgm-algos/src/grover.rs) (crate `hgm-algos`). Busca, em uma lista
não estruturada de `2^(qubits-1)` itens, o item marcado por `search_value`, usando `O(sqrt(N))`
consultas ao oráculo em vez de `O(N)`. Os geradores de passos (`hadamard`, `pauli_x`, `concatena`)
vêm de [`steps.rs`](../crates/hgm-algos/src/steps.rs); o C++ equivalente está em `lib_grover.cpp`
no tag `v1.1.0`.

## 1. Layout de qubits

`grover(config, qubits, search_value)` usa **`qubits` qubits no total**, mas só `qubits-1` deles
são o "registrador de busca":

- **qubit 0**: qubit auxiliar (*ancilla*) usado para o truque de "phase kickback" (inversão de
  fase via porta X + estado `|->`).
- **qubits 1..qubits-1**: o registrador de busca de `qubits-1` bits — o espaço de busca tem
  `2^(qubits-1)` itens.

A função só aceita de 2 a 32 qubits, e `search_value` precisa estar em `0..2^(qubits-1)`. Um
valor maior seria truncado em silêncio pelo oráculo (que só olha `qubits-1` bits) e a busca seria
por outro valor; por isso devolve `HgmError::InvalidArgument`.

## 2. Estado inicial e o truque do "phase kickback"

```rust
simulator.set_basis(1usize << (qubits - 1))?;        // seta o bit do qubit 0 para 1
let prepare = hadamard(qubits, 0, qubits);           // H em TODOS os qubits, inclusive o 0
```

O qubit 0 começa em `|1>` e depois de `H` vira `(|0> - |1>)/sqrt2` (o estado `|->`). Esse é o
estado clássico usado para transformar um oráculo do tipo "inverte o bit alvo se a condição bater"
em um oráculo de **fase**: como `X|-> = -|->`, aplicar um `X` condicional no qubit 0 tem o mesmo
efeito de multiplicar por `-1` a amplitude do estado do registrador de busca que satisfez a
condição — sem nunca "gastar" o qubit 0 (ele permanece em `|->` o tempo todo). Os demais qubits
(1..qubits-1) recebem `H` normal, indo para superposição uniforme.

## 3. O oráculo — `oracle1`

Monta um step onde cada qubit do registrador de busca é um **controle** (com valor = o bit
correspondente de `search_value`) do mesmo grupo, e o qubit 0 é o **alvo** com porta `X`. Ou
seja: "se o registrador de busca == `search_value`, aplique X no qubit 0" — que, graças ao truque
acima, vira "se o registrador de busca == `search_value`, multiplique a amplitude por -1". Esse é
o oráculo `Uf` do algoritmo de Grover. Para 4 qubits e `search_value = 5` (0b101) o passo é

```
Target1(X),Control1(1),Control1(0),Control1(1)
```

(o teste `oracle_puts_search_bits_on_the_controls` confere isso).

## 4. O difusor (inversão sobre a média) — `grover_step`

```rust
// para cada qubit de 1 a qubits-1:   H
// X no último qubit
// controlled_z(qubits)               // Z multi-controlado, controles com valor 0
// X no último qubit
// para cada qubit de qubits-1 a 1:   H
```

Isso implementa o difusor `2|s><s| - I` como `H^n (inverte a fase de |00...0>) H^n`. A forma de
livro faria `X` em cada qubit antes e depois de um `Z` multi-controlado de `|11...1>` para levar
`|00...0>` até `|11...1>`; aqui os controles do `Z` valem 0 (`controlled_z`), então só o qubit
alvo precisa do `X` antes e depois (`X Z X = diag(-1, 1)`), e o resultado é o mesmo operador com
`2*(qubits-1)` passadas de `X` a menos sobre o vetor de estado (item 22 em
[07](07-bugs-e-pontos-de-atencao.md)). O efeito líquido é inverter a fase do estado `|00...0>`, o
que equivale a refletir as amplitudes em torno da média.

`grover_step` devolve os passos de **uma iteração** (oráculo + difusor): para `qubits = 4`, são
`1 + 3 + 3 + 3` passos.

## 5. Número de iterações e execução

```rust
pub fn iteration_count(qubits: usize) -> usize {
    (PI / 4.0 * ((1u64 << (qubits - 1)) as f64).sqrt()) as usize
}
const PT_BLOCK_LIMIT: usize = 20_000;
let chunk = (block_limit / step.len()).max(1);   // iterações por bloco
let first = total.min(chunk);

let mut program = simulator.compile_steps(&[prepare], 1)?;       // superposição inicial
program.append(simulator.compile_steps(&step, first)?)?;         // + primeiro bloco
simulator.run_program(&program, 1)?;
// o resto: blocos de `chunk` iterações repetidos por run_program(&bloco, left / chunk),
// e as iterações que sobram numa última execução
```

`floor(pi/4 * sqrt(N))` é o número ótimo de iterações do Grover para um espaço de busca de
tamanho `N = 2^(qubits-1)` (maximiza a probabilidade de medir o item marcado). O
`Program::append` junta a preparação ao primeiro bloco num programa só (o `setFunction(...,
reset = false)` do C++), emendando as repetições do passo de amplificação.

**Por que em blocos (item 25 de [07](07-bugs-e-pontos-de-atencao.md)):** compilar o passo
`iteration_count` vezes de uma vez gera centenas de MB de portas idênticas perto de 31 qubits
(25 mil iterações x ~100 portas), mas repetir só por `repetitions` custou 12% na GPU e 5% na CPU,
porque a lista contínua deixava o agrupamento fundir o fim de uma iteração com o começo da
seguinte. O meio-termo é compilar o passo em blocos de ~20000 portas e repetir o bloco por
`run_program(bloco, n)`; a memória fica limitada ao bloco. O teste
`block_size_does_not_change_the_state` garante que o tamanho do bloco não muda o estado final.

## 6. Medição

```rust
for qubit in 1..qubits {
    result = (result << 1) | u64::from(simulator.measure(qubit)?.bit);
}
```

Mede cada qubit do registrador de busca (não o qubit ancilla 0) e monta o inteiro `result` — que,
com alta probabilidade após as iterações corretas, é igual a `search_value`. `grover` **devolve o
valor medido** (`Result<u64>`); quem chama compara com o buscado. O executável `grover` imprime o
resultado e o tempo.

Grover continua **probabilístico por natureza**: com o número de iterações próximo do ótimo, a
probabilidade de acerto passa de 94% nos tamanhos pequenos testados, sem garantia. Por isso o
teste `finds_the_value_on_small_searches` fixa as sementes (o gerador `Mt19937` é determinístico)
e exige ao menos 10 de 12 acertos, e o smoke test (`tests/smoke_test_rust.sh`) confere uma taxa de sucesso (pelo menos 7 de 10
execuções achando o valor na CPU), não um valor exato numa rodada só.

**Bug histórico do C++ (corrigido 2026-08-11):** até essa data o `Grover()` do C++ calculava o
resultado mas nunca o expunha; o executável só imprimia o tempo, e não havia como saber se a
busca tinha dado certo. O Rust nasceu já devolvendo o valor medido, e o teste de paridade com o
C++ (`tests/golden/data/runs.txt`, 40 execuções de Grover com sementes fixas) exige que o `t_CPU`
do Rust repita exatamente o valor que o C++ achou.
