# O Motor de Execução em CPU

Esta é a parte mais importante para entender "como isso deveria funcionar":
como aplicar uma matriz 2x2 (uma porta de 1 qubit) em um vetor de `2^n`
amplitudes sem nunca montar a matriz gigante `2^n x 2^n`.

## 1. A ideia matemática

Aplicar uma porta de 1 qubit no qubit alvo `q` não mistura *todas* as
amplitudes — ela só mistura, aos pares, as amplitudes cujos índices diferem
**apenas no bit de `q`**. Se `matrix = [m00, m01, m10, m11]` e `pos0`/`pos1`
são dois índices que só diferem no bit de `q` (`pos0` tem esse bit em 0,
`pos1` tem esse bit em 1):

```
novo_state[pos0] = m00 * state[pos0] + m01 * state[pos1]
novo_state[pos1] = m10 * state[pos0] + m11 * state[pos1]
```

Isso é exatamente o produto matriz-vetor 2x2, aplicado independentemente
para cada um dos `2^(n-1)` pares de índices. O trabalho todo do motor de
execução é: **gerar rapidamente todos os pares `(pos0, pos1)`** e aplicar essa
fórmula.

## 2. `CpuExecution1_1` — caso denso, sem controle

[dgm_cpu_exec.cpp:34](../src/core/dgm_cpu_exec.cpp#L34):

```c
target_bit_mask = 1 << term->target_bit;    // máscara com o bit do qubit alvo
mem_size /= 2;                               // só percorremos metade dos índices
for (long pos = 0; pos < mem_size; pos++){
    pos0 = (pos * 2) - (pos & (target_bit_mask-1));
    pos1 = pos0 | target_bit_mask;

    tmp         = term->matrix[0]*state[pos0] + term->matrix[1]*state[pos1];
    state[pos1] = term->matrix[2]*state[pos0] + term->matrix[3]*state[pos1];
    state[pos0] = tmp;
}
```

A parte não óbvia é `pos0 = (pos*2) - (pos & (target_bit_mask-1))`. Isso é um
truque para "inserir um bit 0 na posição `target_bit` de `pos`", **sem** fazer
dois shifts variáveis (`((pos >> target_bit) << (target_bit+1)) | (pos &
(target_bit_mask-1))`, que seria a forma óbvia mas mais cara). Prova rápida
com números pequenos: se `target_bit=2` (`target_bit_mask=4`), separe `pos`
em "parte alta" `H` e "2 bits baixos" `L` (`pos = 4H + L`). Então
`pos*2 - L = 8H + 2L - L = 8H + L`, que é exatamente `H` deslocado 3 bits
(abriu espaço no bit 2) mais `L` nos 2 bits baixos — ou seja, o mesmo valor
que inserir um `0` no bit 2 de `pos`. `pos1` é só `pos0` com o bit do qubit
alvo ligado (`| target_bit_mask`).

Percorrendo `pos` de `0` a `2^(n-1)-1`, esse par `(pos0, pos1)` cobre **todos**
os `2^n` índices do vetor de estado, cada um exatamente uma vez — sem nunca
calcular um shift variável dentro do laço quente.

## 3. Versão controlada (mesma função, ramo `else`)

```c
mask = ~(term->control_mask | target_bit_mask);
inc  = (~mask) + 1;
for (pos = 0; pos < mem_size; pos = (pos+inc) & mask){
    pos0 = pos | term->control_value;
    pos1 = pos0 | target_bit_mask;
    ...
}
```

Aqui `mem_size` volta a ser o tamanho **total** (não dividido por 2). A ideia:
os bits de controle (`control_mask`) e o bit alvo (`target_bit_mask`) são
**fixos** (controle = `control_value`, alvo começa em 0); só os bits "livres"
(nem controle, nem alvo) variam. `mask` marca esses bits livres. A expressão
`pos = (pos + inc) & mask` é o truque clássico de "contar só dentro de um
subconjunto de bits" (comum em Hacker's Delight): cada iteração pula
diretamente para a próxima combinação válida dos bits livres, sem nunca
visitar um índice que viole o controle. Isso é *muito* mais rápido do que
percorrer todo o vetor e testar `if ((pos & control_mask) == control_value)`
a cada posição — que é exatamente a alternativa mais simples (e mais lenta)
que existia no código sob os nomes `CpuExecution2_*`/`CpuExecution3_*`,
removidos por serem código morto (nunca chamados por `DGM::execute()`) —
ver [07-bugs-e-pontos-de-atencao.md](07-bugs-e-pontos-de-atencao.md), item 2.

## 4. Os três "tipos de matriz" — a otimização mais importante

`PT::matrixType()` ([common.cpp:29](../src/core/common.cpp#L29)) olha
para a matriz 2x2 da porta e classifica em três formatos, **sem olhar para o
vetor de estado**:

| Tipo         | Condição                          | Forma                          | Exemplos de portas   |
|--------------|-------------------------------------|----------------------------------|------------------------|
| `DENSE`      | `m01 != 0` e/ou `m10 != 0`         | `[[m00,m01],[m10,m11]]` (cheia) | `H`, `Y`               |
| `DIAG_PRI`   | `m01 == 0` e `m10 == 0`            | `[[m00,0],[0,m11]]` (diagonal)  | `Z`, `R1`, `R2`, `R3`, rotações de fase |
| `DIAG_SEC`   | `m00 == 0` e `m11 == 0`            | `[[0,m01],[m10,0]]` (anti-diag.)| `X`                     |

Essa classificação existe porque **portas diagonais não misturam
amplitudes** — elas só multiplicam cada amplitude por um número, dependendo
do valor do bit do qubit alvo. Não precisa nem separar em pares `(pos0,
pos1)`: dá pra percorrer o vetor inteiro, uma posição de cada vez
(`CpuExecution1_2`, [dgm_cpu_exec.cpp:67](../src/core/dgm_cpu_exec.cpp#L67)):

```c
state[pos] = term->matrix[((pos >> target_bit_index) & 1) * 3] * state[pos];
```

(`(pos>>target_bit_index)&1` é o bit do qubit alvo; `*3` escolhe `matrix[0]`
ou `matrix[3]`, os dois elementos da diagonal). Isso custa **1 multiplicação
por amplitude**, contra as 4 multiplicações + 2 somas do caso denso.

O tipo `DIAG_SEC` (anti-diagonal, como o `X`) também economiza: como
`m00=m11=0`, a fórmula geral simplifica para
`novo[pos0] = m01*state[pos1]` e `novo[pos1] = m10*state[pos0]`
(`CpuExecution1_3`, [dgm_cpu_exec.cpp:85](../src/core/dgm_cpu_exec.cpp#L85))
— metade das multiplicações do caso denso.

`DGM::CpuExecution1(it)` ([dgm_cpu_exec.cpp:8](../src/core/dgm_cpu_exec.cpp#L8))
só faz um `switch` em `pt->matrixType()` e chama a função certa para cada `PT`
da lista. Esse "roteamento por formato de matriz" é reaproveitado também no
kernel CUDA de forma conceitualmente igual (ver [04](04-gpu-cuda.md)) e no
código paralelo de CPU abaixo.

## 5. Execução paralela em CPU: `PCpuExecution1` — o conceito de "região"

[dgm_par_exec.cpp:81](../src/core/dgm_par_exec.cpp#L81). Ideia: em vez de rodar
um `PT` de cada vez sobre o vetor inteiro, o vetor de `2^qubits` posições é
dividido em **regiões** — blocos definidos por fixar um conjunto de bits do
índice (`region_mask`) em um valor (`region_id`), variando o resto. Cada
região pode ser processada **de forma totalmente independente** por uma
thread diferente, desde que todos os `PT`s daquele lote de operações só
toquem qubits dentro da região (por isso o código primeiro agrupa quantos
`PT`s consecutivos "cabem" em uma região de tamanho `cpu_region_bits`,
olhando os qubits que cada um afeta, antes de disparar as threads OpenMP).

Essa conta de agrupamento é feita por `compute_region`
([dgm_par_exec.cpp:37](../src/core/dgm_par_exec.cpp#L37)), um helper
compartilhado — antes existiam três cópias quase idênticas dessa lógica
(aqui e duas vezes dentro de `HybridExecution`), e foi exatamente por isso
que o bug do deslocamento negativo (item 6 de
[07](07-bugs-e-pontos-de-atencao.md)) precisou ser corrigido em mais de um
lugar. `compute_region` também é usado pelas duas passagens de região do
`HybridExecution` (ver [04-gpu-cuda.md](04-gpu-cuda.md)).

Dentro da região, `PCpuExecution1_0`
([dgm_par_exec.cpp:133](../src/core/dgm_par_exec.cpp#L133)) faz basicamente o
mesmo que `CpuExecution1_1/2/3`, mas usando `region_id`/`region_mask` no
lugar de todo o índice — ou seja, a mesma lógica de pares `(pos0, pos1)` e
tipos de matriz, só que restrita à fatia de memória daquela região/thread.

`cpu_region_bits` e `cpu_coalesced_bits` são os parâmetros de tuning:
- `cpu_region_bits`: quantos qubits (bits) cabem "dentro" de uma região
  processada de uma vez por uma thread antes de trocar de região.
- `cpu_coalesced_bits`: quantos desses bits são tratados como o "núcleo
  coalescido" (os bits menos significativos, sempre presentes na região,
  ajudando a manter acesso à memória mais sequencial/cache-friendly).

Se `cpu_region_bits` vier maior que `qubits` (ex: os CLIs usam 14 como
padrão, mas o usuário pode pedir menos qubits que isso na linha de comando),
`PCpuExecution1` faz o clamp `if (region_bits > qubits) region_bits =
qubits;` logo no início — sem isso, `1 << (qubits - region_bits)` mais
abaixo deslocaria por um expoente negativo (bug já corrigido, ver
[07-bugs-e-pontos-de-atencao.md](07-bugs-e-pontos-de-atencao.md), item 6).

## 6. Nota sobre `CpuExecution2_*` e `CpuExecution3_*`

O arquivo original tinha três "famílias" de funções (`CpuExecution1_*`,
`_2_*`, `_3_*`) implementando o mesmo cálculo de formas diferentes —
aparentavam ser experimentos de otimização feitos durante a pesquisa.
**Apenas a família `1` estava conectada** (`DGM::execute()` só chama
`CpuExecution1`); as famílias `2` e `3` nunca eram chamadas em lugar nenhum
do projeto e foram removidas — ver
[07-bugs-e-pontos-de-atencao.md](07-bugs-e-pontos-de-atencao.md), item 2.
