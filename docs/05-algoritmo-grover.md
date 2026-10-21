# Algoritmo de Grover — mapeado ao código

Arquivo: [lib_grover.cpp](../src/algorithms/lib_grover.cpp). Busca, em uma
lista não estruturada de `2^(qubits-1)` itens, o item marcado por `value`,
usando `O(sqrt(N))` consultas ao oráculo em vez de `O(N)`.

## 1. Layout de qubits

`Grover(qubits, value, ...)` usa **`qubits` qubits no total**, mas só
`qubits-1` deles são o "registrador de busca":

- **qubit 0**: qubit auxiliar (*ancilla*) usado para o truque de
  "phase kickback" (inversão de fase via porta X + estado `|->`).
- **qubits 1..qubits-1**: o registrador de busca de `qubits-1` bits — o
  espaço de busca tem `2^(qubits-1)` itens.

## 2. Estado inicial e o truque do "phase kickback"

```c
dgm.setMemoryValue(1<<(qubits-1));   // seta o bit do qubit 0 para 1
string hadamard_step = Hadamard(qubits, 0, qubits);  // H em TODOS os qubits, inclusive o 0
```

O qubit 0 começa em `|1>` e depois de `H` vira `(|0> - |1>)/sqrt2` (o estado
`|->`). Esse é o estado clássico usado para transformar um oráculo do tipo
"inverte o bit alvo se a condição bater" em um oráculo de **fase**: como
`X|-> = -|->`, aplicar um `X` condicional no qubit 0 tem o mesmo efeito de
multiplicar por `-1` a amplitude do estado do registrador de busca que
satisfez a condição — sem nunca "gastar" o qubit 0 (ele permanece em `|->`
o tempo todo). Os demais qubits (1..qubits-1) recebem `H` normal, indo para
superposição uniforme.

## 3. O oráculo — `Oracle1`

[lib_grover.cpp:97](../src/algorithms/lib_grover.cpp#L97). Monta um step
onde cada qubit do registrador de busca é um **controle** (com valor = o bit
correspondente de `value`) do mesmo grupo, e o qubit 0 é o **alvo** com
porta `X`. Ou seja: "se o registrador de busca == `value`, aplique X no
qubit 0" — que, graças ao truque acima, vira "se o registrador de busca ==
`value`, multiplique a amplitude por -1". Esse é o oráculo `Uf` do algoritmo
de Grover.

## 4. O difusor (inversão sobre a média) — `grover_step`

```c
for (i = 1; i < qubits; i++) H(i);
X(qubits-1);
CZ (Z multi-controlado, controles com valor 0, nos qubits de busca)
X(qubits-1);
for (i = qubits-1; i >= 1; i--) H(i);
```

Isso implementa o difusor `2|s><s| - I` como `H^n (inverte a fase de
|00...0>) H^n`. A forma de livro faria `X` em cada qubit antes e depois de
um `Z` multi-controlado de `|11...1>` para levar `|00...0>` até `|11...1>`;
aqui os controles do `Z` valem 0 (`ControledZ`,
[lib_grover.cpp](../src/algorithms/lib_grover.cpp)), então só o qubit alvo
precisa do `X` antes e depois (`X Z X = diag(-1, 1)`), e o resultado é o
mesmo operador com `2*(qubits-1)` passadas de `X` a menos sobre o vetor de
estado (item 22 em [07-bugs-e-pontos-de-atencao.md](07-bugs-e-pontos-de-atencao.md)).
O efeito líquido é inverter a fase do estado `|00...0>`, o que equivale a
refletir as amplitudes em torno da média.

## 5. Número de iterações e execução

```c
int iteration_count = (int)(M_PI/4.0*sqrt(1L<<(qubits-1)));
long chunk = PT_BLOCK_LIMIT / grover_step.size();         // PT_BLOCK_LIMIT = 20000 portas
dgm.setFunction(hadamard_step);                          // prepara superposição inicial
dgm.setFunction(grover_step, first, false);               // first = min(iteration_count, chunk)
dgm.execute(1);
// o resto: dgm.setFunction(grover_step, chunk); dgm.execute(left / chunk);
// e as iterações que sobram numa última execução
```

`M_PI/4*sqrt(N)` é exatamente o número ótimo de iterações do Grover para um
espaço de busca de tamanho `N = 2^(qubits-1)` (maximiza a probabilidade de
medir o item marcado). O `false` em `setFunction` diz para **não apagar** o
que já estava montado (a preparação `hadamard_step`), só emendar as
repetições do passo de amplificação (ver `DGM::setFunction`, que faz
`vec_pts.pop_back()` para remover o `NULL` sentinela antes de emendar mais
`PT`s — [02](02-linguagem-de-circuitos.md)).

**Por que em blocos (item 25 de [07](07-bugs-e-pontos-de-atencao.md)):**
replicar o passo `iteration_count` vezes dentro de `setFunction` gera centenas
de MB de `PT` idênticos perto de 31 qubits (25 mil iterações x ~100 portas), mas
repetir só por `execute(iterations)` custou 12% na GPU e 5% na CPU, porque a lista
contínua deixava o agrupamento fundir o fim de uma iteração com o começo da
seguinte. O meio-termo é replicar o passo em blocos de ~20000 portas e repetir o
bloco por `execute(blocos)`; a memória fica limitada ao bloco.

## 6. Medição

```c
for (i = 1; i < qubits; i++)
    result = (result << 1) | dgm.measure(i);
```

Mede cada qubit do registrador de busca (não o qubit ancilla 0) e monta o
inteiro `result` — que, com alta probabilidade após `num_of_it` iterações
corretas, é igual a `value`.

**Bug histórico (corrigido 2026-08-11):** até essa data, `Grover()`
calculava `result` mas nunca o expunha pra fora da função — só devolvia
o tempo decorrido (`float elapsed`), e `grover.cpp` só imprimia esse
tempo. Ou seja, o programa media a resposta certa e simplesmente a
descartava; não havia nenhum jeito de saber, rodando `grover.out`, se a
busca tinha de fato encontrado `search_value` ou não (só "não crashou").
Corrigido: `Grover()` agora retorna `long result` (o timing, que antes
era medido dentro da própria função, migrou pra `grover.cpp` — mesmo
padrão que `Shor()`/`shor.cpp` já usavam), e `grover.cpp` imprime "Found
value: N" ou "Failed to find value (...)" comparando contra
`search_value`. Verificado: 30/30 execuções bem-sucedidas em testes
manuais (`t_CPU`/`t_GPU`/`t_HYBRID`, 10 e 12 qubits) — com o número de
iterações escolhido próximo do ótimo (`M_PI/4*sqrt(N)`), Grover é bem
mais confiável que Shor nesse quesito; ainda assim, permanece
probabilístico por natureza, então `tests/smoke_test.sh` continua só
conferindo "sem crash" pra `grover.out`, não um resultado exato — mesmo
motivo que já vale pra `shor.out`.
