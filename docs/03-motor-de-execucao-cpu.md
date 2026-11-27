# O Motor de Execução em CPU

Esta é a parte mais importante para entender "como isso deveria funcionar": como aplicar uma
matriz 2x2 (uma porta de 1 qubit) em um vetor de `2^n` amplitudes sem nunca montar a matriz
gigante `2^n x 2^n`. O código está em [`crates/hgm-cpu`](../crates/hgm-cpu): o backend serial
(`serial.rs`), o paralelo (`parallel.rs`, `region.rs`, `kernels.rs`) e dois módulos pequenos com
`unsafe` (`shared.rs`, `simd.rs`).

## 1. A ideia matemática

Aplicar uma porta de 1 qubit no qubit alvo `q` não mistura *todas* as amplitudes — ela só
mistura, aos pares, as amplitudes cujos índices diferem **apenas no bit de `q`**. Se
`matrix = [m00, m01, m10, m11]` e `pos0`/`pos1` são dois índices que só diferem no bit de `q`
(`pos0` tem esse bit em 0, `pos1` tem esse bit em 1):

```
novo_state[pos0] = m00 * state[pos0] + m01 * state[pos1]
novo_state[pos1] = m10 * state[pos0] + m11 * state[pos1]
```

Isso é exatamente o produto matriz-vetor 2x2, aplicado independentemente para cada um dos
`2^(n-1)` pares de índices. O trabalho todo do motor de execução é: **gerar rapidamente todos os
pares `(pos0, pos1)`** e aplicar essa fórmula.

## 2. O caso denso, sem controle (`serial::dense`)

[`serial.rs`](../crates/hgm-cpu/src/serial.rs):

```rust
let target_mask = 1u64 << pt.target_bit;        // máscara com o bit do qubit alvo
for pos in 0..mem_size / 2 {                     // só percorremos metade dos índices
    let pos0 = (pos * 2) - (pos & (target_mask - 1));
    let pos1 = pos0 | target_mask;
    let (a, b) = (state[pos0], state[pos1]);
    state[pos1] = m[2] * a + m[3] * b;
    state[pos0] = m[0] * a + m[1] * b;
}
```

A parte não óbvia é `pos0 = (pos*2) - (pos & (target_mask-1))`. Isso é um truque para "inserir um
bit 0 na posição `target_bit` de `pos`", **sem** fazer dois shifts variáveis
(`((pos >> target_bit) << (target_bit+1)) | (pos & (target_mask-1))`, que seria a forma óbvia mas
mais cara). Prova rápida com números pequenos: se `target_bit=2` (`target_mask=4`), separe `pos`
em "parte alta" `H` e "2 bits baixos" `L` (`pos = 4H + L`). Então `pos*2 - L = 8H + 2L - L = 8H + L`,
que é exatamente `H` deslocado 3 bits (abriu espaço no bit 2) mais `L` nos 2 bits baixos — ou
seja, o mesmo valor que inserir um `0` no bit 2 de `pos`. `pos1` é só `pos0` com o bit do qubit
alvo ligado (`| target_mask`).

Percorrendo `pos` de `0` a `2^(n-1)-1`, esse par `(pos0, pos1)` cobre **todos** os `2^n` índices
do vetor de estado, cada um exatamente uma vez — sem nunca calcular um shift variável dentro do
laço quente.

## 3. Versão controlada (mesma função, ramo `else`)

```rust
let mask = !(pt.control_mask | target_mask);
let inc  = (!mask).wrapping_add(1);
let mut pos = 0u64;
while pos < mem_size {
    let pos0 = pos | pt.control_value;
    let pos1 = pos0 | target_mask;
    // ... mesma conta do caso sem controle ...
    pos = pos.wrapping_add(inc) & mask;
}
```

Aqui o laço percorre o tamanho **total** do estado (não dividido por 2). A ideia: os bits de
controle (`control_mask`) e o bit alvo (`target_mask`) são **fixos** (controle = `control_value`,
alvo começa em 0); só os bits "livres" (nem controle, nem alvo) variam. `mask` marca esses bits
livres. A expressão `pos = (pos + inc) & mask` é o truque clássico de "contar só dentro de um
subconjunto de bits" (comum em Hacker's Delight): cada iteração pula diretamente para a próxima
combinação válida dos bits livres, sem nunca visitar um índice que viole o controle. Isso é
*muito* mais rápido do que percorrer todo o vetor e testar
`if (pos & control_mask) == control_value` a cada posição — a alternativa mais simples e mais
lenta que existia no C++ sob os nomes `CpuExecution2_*`/`CpuExecution3_*`, removidos por serem
código morto ([07](07-bugs-e-pontos-de-atencao.md), item 2) e que o Rust nunca teve.

O `Backend::execute` do serial ainda valida o programa antes de rodar (`validate`): alvo, máscara
e valor de controle precisam caber no estado e o alvo não pode ser também controle. Um programa
fora disso devolve `HgmError::PtOutOfRange`. No C++ isso era confiança no parser.

## 4. Os três "tipos de matriz" — a otimização mais importante

`Pt::kind()` ([`pt.rs`](../crates/hgm-core/src/pt.rs)) olha para a matriz 2x2 da porta e classifica
em três formatos, **sem olhar para o vetor de estado**:

| `MatrixKind` | Condição                           | Forma                           | Exemplos de portas |
|--------------|-------------------------------------|----------------------------------|---------------------|
| `Dense`      | `m01 != 0` e/ou `m10 != 0`          | `[[m00,m01],[m10,m11]]` (cheia)  | `H`                 |
| `DiagPri`    | `m01 == 0` e `m10 == 0`             | `[[m00,0],[0,m11]]` (diagonal)   | `Z`, `R1`, `R2`, `R3`, rotações de fase |
| `DiagSec`    | `m00 == 0` e `m11 == 0`             | `[[0,m01],[m10,0]]` (anti-diag.) | `X`, `Y`            |

(`Y` tem `m00 = m11 = 0`, então é anti-diagonal, como o `X`; há teste dessa classificação.)

Essa classificação existe porque **portas diagonais não misturam amplitudes** — elas só
multiplicam cada amplitude por um número, dependendo do valor do bit do qubit alvo. Não precisa
nem separar em pares `(pos0, pos1)`: dá pra percorrer o vetor inteiro, uma posição de cada vez
(`serial::diagonal_main`):

```rust
*amplitude = m[((pos >> target) & 1) * 3] * *amplitude;
```

(`(pos>>target)&1` é o bit do qubit alvo; `*3` escolhe `m[0]` ou `m[3]`, os dois elementos da
diagonal). Isso custa **1 multiplicação por amplitude**, contra as 4 multiplicações + 2 somas do
caso denso.

O tipo `DiagSec` (anti-diagonal, como o `X`) também economiza: como `m00=m11=0`, a fórmula geral
simplifica para `novo[pos0] = m01*state[pos1]` e `novo[pos1] = m10*state[pos0]`
(`serial::diagonal_anti`) — metade das multiplicações do caso denso.

`SerialBackend::execute` só percorre `program.ops` e, para cada `Op::Gate`, chama `apply_pt`,
que faz um `match` em `pt.kind()`; uma `Op::FusedPhase` vai para `apply_fused_phase` (uma
multiplicação por amplitude com o ângulo vindo da tabela de fases, [04](04-gpu-wgpu.md), seção 5).
Esse "roteamento por formato de matriz" é reaproveitado no backend paralelo e, conceitualmente
igual, nos kernels de GPU ([04](04-gpu-wgpu.md)).

O serial é **o oráculo**: os testes comparam todos os outros backends com ele, e os vetores de
`tests/golden` (gerados pelo C++ `t_CPU`) conferem com ele. Mantém de propósito os mesmos laços
do C++, sem FMA, para o resultado coincidir bit a bit.

## 5. Execução paralela em CPU: `ParallelBackend` — o conceito de "região"

[`parallel.rs`](../crates/hgm-cpu/src/parallel.rs), com `rayon` no lugar do OpenMP. Ideia: em vez de
rodar uma porta de cada vez sobre o vetor inteiro, o vetor de `2^qubits` posições é dividido em
**regiões** — blocos definidos por fixar um conjunto de bits do índice (`mask` são os bits que
variam, `id` o valor dos demais) e variar o resto. Cada região pode ser processada **de forma
totalmente independente** por uma thread diferente, desde que todas as portas daquele lote só
toquem qubits dentro da região. Por isso `compute_region`
([`region.rs`](../crates/hgm-cpu/src/region.rs)) primeiro agrupa quantas portas consecutivas
"cabem" em uma região de `region_bits` bits, olhando o bit do alvo de cada uma, antes de disparar
as threads. Cada lote é uma só passada pelo vetor de estado, em vez de uma passada por porta.

O laço de `execute`, resumido:

```rust
while op_index < ops.len() {
    let plan = compute_region(ops, op_index, coalesced_bits, region_bits, qubits)?;
    let batch = &ops[op_index..plan.op_end];
    pool.install(|| par_for_each_region(amplitudes, qubits, plan.mask, |view| apply(view, batch)));
    op_index = plan.op_end;
}
```

`compute_region` é um helper único. No C++ existiam três cópias quase idênticas dessa lógica,
e foi por isso que o bug do deslocamento negativo (item 6 de
[07](07-bugs-e-pontos-de-atencao.md)) precisou ser corrigido em mais de um lugar. Se nenhuma
porta cabe na região planejada, o Rust devolve `HgmError::Backend`; o C++ ficava em laço
infinito.

Dentro da região, `apply_batch` percorre as portas do lote. Uma porta controlada só age na região
se a parte do controle que cai **fora** da região (`control_mask & id & !mask`) bate com o valor
exigido; os controles que caem dentro da região saem da máscara (não se itera sobre eles) e o
valor deles entra no `id`. `apply_gate` então aplica a porta sobre os pares da região, igual ao
serial, só que restrita à fatia de memória daquela região.

`Tuning` ([`region.rs`](../crates/hgm-cpu/src/region.rs)) reúne os parâmetros de ajuste
(`threads`, `coalesced_bits`, `region_bits`; os executáveis usam região 14 e coalescida 11):

- `region_bits`: quantos qubits (bits) cabem "dentro" de uma região processada de uma vez por uma
  thread antes de trocar de região (`2^region_bits` amplitudes por região).
- `coalesced_bits`: quantos desses bits são o "núcleo coalescido" (os bits menos significativos,
  sempre presentes na região, mantendo o acesso à memória sequencial e amigável ao cache).

`Tuning::validated(qubits)` faz o clamp que o C++ fazia em `validateTuning`: a região e a faixa
coalescida não passam do número de qubits, e uma região menor que o circuito precisa ser maior
que a faixa coalescida (senão não absorveria nenhuma porta com alvo fora dela). Sem o clamp, o
número de regiões seria `1 << (qubits - region_bits)` com expoente negativo (item 6 de
[07](07-bugs-e-pontos-de-atencao.md)).

### Blocos vetorizáveis (`apply_blocks`) e AVX2

Quando os bits baixos da máscara são contíguos (`low_bits >= 2`), `apply_gate` não percorre um par
por vez. Usa a recorrência `pos = (pos + inc) & outer_mask` só para andar pelos blocos
"externos" da região e processa cada bloco contíguo de `2^low_bits` amplitudes com um laço simples
sobre fatias ([`kernels.rs`](../crates/hgm-cpu/src/kernels.rs): `dense`, `diagonal`,
`anti_diagonal`, `pairs_in_block`), que o compilador vetoriza. Com `m0 == 1` (Z, R1..R3) o núcleo
diagonal só toca o lado `b`, e `X` vira uma troca de fatias (`swap_with_slice`). Se os bits baixos
não são contíguos (`low_bits < 2`), o caminho genérico `RegionView::for_each_pair` faz um par por
iteração.

Os núcleos são `#[inline(always)]` e **não usam multiplicação-soma fundida**: a ordem das contas é
a do C++ (`m0r*ar - m0i*ai + m1r*br - m1i*bi`, da esquerda para a direita).

O C++ compilava esses laços com `target_clones("avx2","default")`. O `rustc` gera por padrão o
x86-64 básico (SSE2), e `target-cpu=native` amarraria o binário a uma máquina; por isso
[`simd.rs`](../crates/hgm-cpu/src/simd.rs) compila o mesmo código duas vezes, uma delas com
`#[target_feature(enable = "avx2")]`, e `batch_applier()` escolhe a versão certa uma vez por
execução (`is_x86_feature_detected!("avx2")`). Só se habilita `avx2`, sem `fma`, e o Rust nunca
funde multiplicação e soma por conta própria; logo o resultado é **idêntico bit a bit** ao serial
(há teste). Em outras arquiteturas (ex.: Apple Silicon), cai na versão básica.

### O único `unsafe` do paralelo: `shared.rs`

As regiões de um mesmo passe são disjuntas, mas o compilador não vê isso (os índices de uma região
são espalhados pelo vetor, não um pedaço contíguo). [`shared.rs`](../crates/hgm-cpu/src/shared.rs) é
o único módulo de `hgm-cpu` com `unsafe` (fora o despacho de AVX2) e garante a disjunção por
construção:

- `par_for_each_region` recebe o vetor por `&mut` (exclusividade total durante a chamada) e
  entrega a cada tarefa um `RegionView` de uma região **distinta** (o `id` vem de espalhar os bits
  de um contador injetivo pelas posições fora da máscara);
- toda leitura/escrita por um `RegionView` é **checada em tempo de execução** contra a sua região
  (`índice & !mask == id`) e contra o tamanho do vetor, então um erro de índice em quem chama
  vira pânico, não comportamento indefinido;
- os métodos de bloco exigem `&mut self`, então dois blocos de um mesmo `RegionView` não podem
  coexistir (e os de `block_pair_mut` são checados como não sobrepostos).

Cada bloco `unsafe` tem um comentário `// SAFETY:`. Não rodei `miri` nem `loom`.

## 6. Nota sobre as "famílias" `CpuExecution2_*` e `CpuExecution3_*` do C++

O C++ original tinha três famílias de funções (`CpuExecution1_*`, `_2_*`, `_3_*`) implementando o
mesmo cálculo de formas diferentes, resquício de experimentos de otimização. Apenas a família `1`
estava conectada e as outras foram removidas ainda no C++ ([07](07-bugs-e-pontos-de-atencao.md),
item 2). O Rust portou só a que valia.
