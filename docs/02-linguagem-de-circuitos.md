# A Linguagem de Circuitos (texto → `PT`)

O simulador não tem uma API tipo `circuit.h(0); circuit.cnot(0,1);`. Em vez
disso, um "passo" (*step*) do circuito — ou seja, uma camada de portas que
atuam simultaneamente em qubits diferentes — é escrito como uma **string com
um token por qubit, separados por vírgula**. Isso é gerado pelas funções de
[gates.cpp](../src/core/gates.cpp) e depois interpretado por
`DGM::genGroups` + `DGM::genPTs` em [dgm_parser.cpp](../src/core/dgm_parser.cpp).

## 1. Os tokens

Para um circuito de `qubits` qubits, um step é uma string com exatamente
`qubits` tokens (um por posição/qubit), unidos por `,`:

- `"ID"` — não faz nada nesse qubit.
- `"H"`, `"X"`, `"Z"`, `"Y"`, `"R1"`, `"R2"`, `"R3"`, ou qualquer nome
  registrado em `Gates::list` — aplica essa porta de 1 qubit, sem controle.
- `"ControlN(v)"` — esse qubit é um **controle** do grupo `N`, exigindo que
  o valor do qubit seja `v` (0 ou 1). Ex: `"Control1(1)"`.
- `"TargetN(nome_da_porta)"` — esse qubit é o **alvo** do grupo `N`, e recebe
  a porta `nome_da_porta` se todos os controles do grupo `N` forem satisfeitos.

O "grupo" (o número `N` em `ControlN`/`TargetN`) é o que liga controles ao seu
alvo. Isso permite colocar **várias operações controladas independentes** e
**vários controles multi-qubit** no mesmo step, desde que usem números de
grupo diferentes.

### Exemplo: CNOT

`gates.cpp::CNot(qubits, ctrl, target, cv)` ([gates.cpp:71](../src/core/gates.cpp#L71))
gera, para `qubits=3, ctrl=0, target=2, cv=1`:

```
"Control1(1),ID,Target1(X)"
```

### Exemplo: Toffoli (CCNOT)

`Toffoli(qubits, ctrl1, ctrl2, target)` gera, para `qubits=3`, controles em 0
e 1, alvo em 2:

```
"Control1(1),Control1(1),Target1(X)"
```

Repare que os dois controles usam o **mesmo grupo `1`** — é assim que o
parser sabe que ambos devem ser satisfeitos para que o `X` no qubit 2 seja
aplicado.

### Exemplo: Hadamard em vários qubits + porta controlada no mesmo step

Nada impede um step como:

```
"H,Control2(1),Target2(Z),H"
```

(H nos qubits 0 e 3, um Z controlado entre os qubits 1 e 2). Isso é possível
porque cada grupo (`0` para portas soltas, `2` aqui) é tratado
independentemente por `genGroups`.

## 2. `DGM::genGroups` — de string para grupos

[dgm_parser.cpp:60](../src/core/dgm_parser.cpp#L60). Recebe um step (string), separa
por vírgula (`Tokenize`) e percorre token a token, acumulando um
`map<long, Group>` (grupo → controles e alvos):

- Se o token contém `"Control"`: extrai o número do grupo (dígito logo após
  "Control") e o valor entre parênteses, e guarda em `groups[N].ctrl` /
  `groups[N].pos_ctrl`.
- Se contém `"Target"`: extrai o número do grupo e o nome da porta (o que
  está entre parênteses), guarda em `groups[N].ops` / `groups[N].pos_ops`.
- Senão, se não for `"ID"`: é uma porta solta, vai para o grupo `0`
  (`groups[0]`), que nunca tem controle.

## 3. `DGM::genPTs` — de grupos para `PT`

[dgm_parser.cpp:107](../src/core/dgm_parser.cpp#L107). Para cada grupo:

1. Calcula `group_control_mask` e `group_control_value` (inteiros de `qubits` bits) a partir das
   posições e valores dos controles daquele grupo — convertendo a posição no
   circuito para o bit de deslocamento (`qubits - pos - 1`, a mesma convenção
   do estado, ver [01](01-arquitetura-geral.md)).
2. Para cada alvo do grupo, cria um `struct PT` ([common.h:49](../include/core/common.h#L49)):
   - `matrix` = a matriz 2x2 da porta (`Gates::getMatrix(nome)`);
   - `span_start_bit`/`target_bit` = bit de deslocamento do qubit alvo no vetor de estado;
   - `control_mask`/`control_value`/`control_count` = o controle calculado acima;
   - `control_bit_positions` = array com as posições (bits) de cada qubit de controle.

Um `PT` ("Pauli Term"/porta compilada) é a unidade mínima que o motor de
execução sabe aplicar: **uma matriz 2x2 em um qubit alvo, opcionalmente
condicionada a um padrão de bits de controle**.

## 4. Circuitos com vários steps e repetição

Um circuito completo pode ter vários steps separados por `;`
(`DGM::setFunction(string function, ...)` faz `Tokenize(function, steps, ";")`,
[dgm_parser.cpp:26](../src/core/dgm_parser.cpp#L26)), ou já vir como
`vector<string>` (um item = um step). `setFunction` monta, para cada step, os
`PT`s daquele step, ordena (`increasing`/`decreasing`, alternando a cada step
— uma heurística de ordenação para melhorar localidade/coalescimento) e
concatena tudo em `vec_pts`. O parâmetro `it` repete a sequência inteira de
steps `it` vezes (usado por Grover para repetir o bloco oráculo+difusão
`num_of_it` vezes sem precisar gerar a string várias vezes).

O array final `pts` é **terminado por `NULL`**: todo laço de execução no
motor (`while (pts[i] != NULL)`) depende disso.

## 5. Catálogo de portas (`Gates`)

[gates.h](../include/core/gates.h) / [gates.cpp](../src/core/gates.cpp).
`Gates::list` é um `map<string, float complex*>` com as portas base: `H`,
`X`, `Y`, `Z`, `R1`, `R2`, `R3`. Cada `DGM` tem sua própria instância de
`Gates` (campo `DGM::gates`) — o cache dura a vida de uma execução, não do
processo inteiro (ver [07-bugs-e-pontos-de-atencao.md](07-bugs-e-pontos-de-atencao.md),
item 5). Cada matriz é um array de 4 complexos em ordem *row-major*:
`[m00, m01, m10, m11]`. Novas portas (como as rotações usadas no QFT e no
somador do Shor) são adicionadas dinamicamente com `Gates::addGate(nome, a0,
a1, a2, a3)`, recebendo o cache da execução atual por referência — mas
`addGate` **não sobrescreve** um nome já existente (retorna `false`
silenciosamente), o que é importante para entender o bug do item 1 do
mesmo documento.
