# A Linguagem de Circuitos (texto → `Pt`)

O simulador não tem uma API tipo `circuit.h(0); circuit.cnot(0,1);`. Em vez disso, um "passo"
(*step*) do circuito — ou seja, uma camada de portas que atuam simultaneamente em qubits
diferentes — é escrito como uma **string com um token por qubit, separados por vírgula**. Os
geradores de [`steps.rs`](../crates/hgm-algos/src/steps.rs) produzem essas strings e o parser de
[`circuit.rs`](../crates/hgm-core/src/circuit.rs) as interpreta (`gen_groups` + `gen_pts`).

## 1. Os tokens

Para um circuito de `qubits` qubits, um step é uma string com exatamente `qubits` tokens (um
por posição/qubit), unidos por `,`:

- `"ID"` — não faz nada nesse qubit.
- `"H"`, `"X"`, `"Z"`, `"Y"`, `"R1"`, `"R2"`, `"R3"`, ou qualquer nome registrado no
  `GateCatalog` — aplica essa porta de 1 qubit, sem controle.
- `"ControlN(v)"` — esse qubit é um **controle** do grupo `N`, exigindo que o valor do qubit
  seja `v` (0 ou 1). Ex: `"Control1(1)"`.
- `"TargetN(nome_da_porta)"` — esse qubit é o **alvo** do grupo `N`, e recebe a porta
  `nome_da_porta` se todos os controles do grupo `N` forem satisfeitos.

O "grupo" (o número `N` em `ControlN`/`TargetN`) é o que liga controles ao seu alvo. Isso
permite colocar **várias operações controladas independentes** e **vários controles
multi-qubit** no mesmo step, desde que usem números de grupo diferentes.

### Exemplo: CNOT

`cnot(qubits, ctrl, target, cv)` em `steps.rs` gera, para `qubits=3, ctrl=0, target=2, cv=1`:

```
"Control1(1),ID,Target1(X)"
```

### Exemplo: Toffoli (CCNOT)

`toffoli(qubits, c1, c2, target, control_value)` gera, para `qubits=3`, controles em 0 e 1 (com
valor 1), alvo em 2:

```
"Control1(1),Control1(1),Target1(X)"
```

Repare que os dois controles usam o **mesmo grupo `1`** — é assim que o parser sabe que ambos
devem ser satisfeitos para que o `X` no qubit 2 seja aplicado.

### Exemplo: Hadamard em vários qubits + porta controlada no mesmo step

Nada impede um step como:

```
"H,Control2(1),Target2(Z),H"
```

(H nos qubits 0 e 3, um Z controlado entre os qubits 1 e 2). Isso é possível porque cada grupo
(`0` para portas soltas, `2` aqui) é tratado independentemente por `gen_groups`.

## 2. `gen_groups` — de string para grupos

Recebe um step (string), separa por vírgula (`tokenize`, que ignora tokens vazios) e percorre
token a token, acumulando um `BTreeMap<i64, Group>` (grupo → controles e alvos), em ordem
crescente do número do grupo:

- Se o token **começa** com `"Control"`: extrai o número do grupo (o que está entre `Control` e
  `(`) e o valor entre parênteses, e guarda em `control_values` / `control_positions`.
- Se começa com `"Target"`: extrai o número do grupo e o nome da porta (o que está entre
  parênteses), guarda em `ops` / `op_positions`.
- Senão, se não for `"ID"`: é uma porta solta, vai para o grupo `0`, que nunca tem controle.

Um token `Control...`/`Target...` sem `(` (ou `Target` sem `)`) devolve `HgmError::BadToken`.
O C++ procurava `"Control"`/`"Target"` em qualquer ponto do token (`find`), então um nome de
porta que contivesse essas palavras seria lido errado; o Rust só reconhece o prefixo.

## 3. `gen_pts` — de grupos para `Pt`

Para cada grupo:

1. Calcula `control_mask` e `control_value` (inteiros de `qubits` bits) a partir das posições e
   valores dos controles daquele grupo — convertendo a posição no circuito para o bit do índice
   (`qubits - posição - 1`, a mesma convenção do estado, ver [01](01-arquitetura-geral.md)).
2. Para cada alvo do grupo, cria um [`Pt`](../crates/hgm-core/src/pt.rs):
   - `matrix` = a matriz 2x2 da porta (`GateCatalog::get(nome)`); nome desconhecido devolve
     `HgmError::UnknownGate`;
   - `target_bit` = bit do qubit alvo no vetor de estado;
   - `control_mask` / `control_value` = o controle calculado acima;
   - `control_bits` = os bits de cada qubit de controle (o `control_count` é o tamanho dele).

Um `Pt` ("Pauli Term"/porta compilada) é a unidade mínima que o motor de execução sabe aplicar:
**uma matriz 2x2 em um qubit alvo, opcionalmente condicionada a um padrão de bits de
controle**. Um grupo só com controles e sem alvo não gera porta nenhuma.

`Pt::kind()` classifica a matriz (comparação exata com zero), e é isso que escolhe o caminho de
execução mais barato nos backends:

| `MatrixKind` | Valor | Quando | Exemplos |
|---|---|---|---|
| `Dense` | 0 | mistura o par de amplitudes | `H` |
| `DiagPri` | 1 | só a diagonal principal: uma multiplicação por amplitude | `Z`, `R1`..`R3` |
| `DiagSec` | 2 | só a anti-diagonal: troca o par multiplicando | `X`, `Y` |

Os valores numéricos são os mesmos do C++ e dos arquivos de `tests/golden`.

O que um programa executa é uma lista de [`Op`](../crates/hgm-core/src/pt.rs): ou `Op::Gate(Pt)`
(o que o parser produz), ou `Op::FusedPhase(FusedPhase)`, que só nasce do otimizador quando uma
corrida de portas de fase é fundida numa só ([04](04-gpu-wgpu.md), seção 5). No C++ a porta
fundida era um `PT` com o campo `phase_terms` não nulo e chamar `matrixType()` nela era
`exit(1)`; no Rust o tipo diz qual é, e esse erro deixou de ser possível.

## 4. Circuitos com vários steps e repetição

Um circuito completo pode ter vários steps separados por `;` (`compile` faz
`tokenize(function, ";")`), ou já vir como uma lista de strings (`compile_steps`, um item = um
step). Para cada step, o parser gera os `Pt`s daquele step e os **ordena pelo bit do alvo**,
alternando a direção a cada step: passos de índice par em ordem crescente do bit, ímpares em
ordem decrescente — uma heurística para melhorar a localidade/coalescimento entre passos
vizinhos. Os alvos de um step são todos distintos (um token por qubit), então a ordem é total.
Depois concatena tudo no `Program`.

O parâmetro `iterations` repete a sequência inteira de steps `iterations` vezes (a paridade do
índice do passo, que decide a direção, reinicia a cada repetição). O Grover repete o bloco
oráculo+difusão por esse parâmetro, mas em blocos de iterações e não de uma vez só; ver
[05](05-algoritmo-grover.md), seção 5.

Todos os steps de um circuito precisam ter o mesmo número de tokens: um step com número
diferente devolve `HgmError::InconsistentQubits` (o C++ ficava em silêncio com o do último
passo), e mais de 32 tokens devolve `HgmError::TooManyQubits`.

`Program` é um `Vec<Op>`, sem o terminador `NULL` do C++. `Program::append` emenda um programa
no fim de outro (o Grover usa para juntar a preparação ao primeiro bloco de iterações).

O `Backend::execute` ainda recebe um `repetitions`, distinto do `iterations` do parser: o
primeiro repete o programa **já compilado** (sem recompilar), o segundo repete os passos na
hora de compilar.

## 5. Catálogo de portas (`GateCatalog`)

[`gates.rs`](../crates/hgm-core/src/gates.rs). `GateCatalog` é um `HashMap<String, Matrix2>` com as
portas base: `H`, `X`, `Y`, `Z`, `R1`, `R2`, `R3`. Cada `Simulator` tem o seu próprio catálogo,
então o cache dura a vida de uma execução, não do processo inteiro (o C++ também tinha uma
instância por `DGM`; ver [07](07-bugs-e-pontos-de-atencao.md), item 5). Cada matriz é um array
de 4 complexos em ordem de linhas: `[m00, m01, m10, m11]`.

Novas portas (como as rotações usadas na QFT e no somador do Shor) são adicionadas
dinamicamente com `GateCatalog::add_gate(nome, matriz)` — que **não sobrescreve** um nome já
existente (devolve `false` silenciosamente). Isso é importante para entender o bug do item 1 do
mesmo documento, e o Rust manteve o comportamento de propósito, coberto por teste
(`add_gate_does_not_overwrite`).

As rotações `R1`..`R3` são calculadas em `f64` e arredondadas uma vez para `f32` (`phase(theta)`);
o C++ usava `cpowf` em `float`, cujos últimos bits dependem da libm (a parte imaginária de `R1`
saía ~8e-9 em vez de 0). A diferença é menor que 1e-7, e os testes de paridade comparam as
matrizes com tolerância de 2e-7.
