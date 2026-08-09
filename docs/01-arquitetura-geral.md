# Arquitetura Geral

## 1. Representação do estado quântico

Um registrador de `qubits` qubits é representado por um vetor de
`2^qubits` números complexos (`float complex *state`, alocado em
`DGM::allocateMemory()`, [dgm.cu:101](../src/core/dgm.cu#L101)):

```c
state = (float complex*) calloc(pow(2, qubits), sizeof(float complex));
```

Cada posição `i` desse vetor é a amplitude do estado da base computacional
correspondente ao índice binário `i`. A convenção de qual bit do índice `i`
pertence a qual qubit é **a coisa mais importante para entender antes de ler
qualquer outra parte do código**:

> **O qubit de índice `q` na descrição do circuito corresponde ao bit
> `(qubits - 1 - q)` do índice `i` no vetor de estado.**

Ou seja, o qubit "0" do circuito é o bit mais significativo (MSB) do índice,
e o último qubit (`qubits-1`) é o bit menos significativo (LSB). Isso aparece
em `DGM::measure()`:

```c
long shift = (qubits - 1 - q_pos);   // dgm.cu:122
```

e na hora de transformar a posição de uma porta no circuito (`pos_ops`, contado
da esquerda) em um "bit de deslocamento" (`end`) usado durante a execução
(`DGM::genPTs`, [dgm.cu:314](../src/core/dgm.cu#L314)):

```c
pt->start = qubits - gp.pos_ops[p];
pt->end   = pt->start - 1;   // == qubits - 1 - pos_ops[p]
```

Exemplo com 3 qubits (`q0 q1 q2`), índice `i` em binário `b2 b1 b0`:
`q0` → bit 2 (`b2`, MSB), `q1` → bit 1, `q2` → bit 0 (LSB).
Se `state[0b100] = 1` isso é o estado `|q0=1, q1=0, q2=0>`.

## 2. A classe `DGM`

`DGM` ([dgm.h](../include/dgm.h)) é o objeto central. Ela guarda:

- `state` — o vetor de amplitudes (o "hardware" quântico simulado).
- `qubits` — quantos qubits o registrador tem.
- `pts` / `vec_pts` — a lista de operações **já compiladas** (ver
  [02-linguagem-de-circuitos.md](02-linguagem-de-circuitos.md)), terminada por `NULL`.
- `exec_type` — qual dos 4 backends usar (`t_CPU`, `t_PAR_CPU`, `t_GPU`, `t_HYBRID`,
  enum em [dgm.h:44](../include/dgm.h#L44)).
- parâmetros de tuning de performance: `n_threads`, `cpu_region`/`cpu_coales`,
  `gpu_region`/`gpu_coales`/`tam_block`/`rept`, `multi_gpu` (explicados em
  [03](03-motor-de-execucao-cpu.md) e [04](04-gpu-cuda.md)).

Um uso típico (visto em `Grover()`, `Shor()`, etc.) é:

```c
DGM dgm;
dgm.qubits = qubits;
dgm.exec_type = type;
// ... parâmetros de tuning ...
dgm.allocateMemory();          // aloca state, zera tudo
dgm.setMemoryValue(pos);       // seta state[pos] = 1 (estado inicial |pos>)

dgm.executeFunction(circuito); // faz parsing da string e executa
// ou, em partes:
dgm.setFunction(circuito);     // só faz o parsing (string -> PT[])
dgm.execute(it);               // só executa (PT[] -> aplica no state)

int bit = dgm.measure(q_pos);  // mede um qubit, colapsa o estado
dgm.freeMemory();
```

## 3. Os quatro backends de execução

`DGM::execute()` ([dgm.cu:361](../src/core/dgm.cu#L361)) despacha para um
dos quatro caminhos, todos calculando a mesma coisa:

| `exec_type`   | Função                                    | Onde                          | Ideia                                                                 |
|---------------|--------------------------------------------|--------------------------------|------------------------------------------------------------------------|
| `t_CPU`       | `CpuExecution1(it)`                        | dgm.cu                        | 1 thread, laço simples sobre o vetor de estado                        |
| `t_PAR_CPU`   | `PCpuExecution1(...)`                      | dgm.cu                        | OpenMP: divide o vetor em "regiões" e processa várias em paralelo     |
| `t_GPU`       | `GpuExecutionWrapper(...)`                 | kernel.cu (`extern "C"`)      | CUDA: usa memória compartilhada e agrupa portas em blocos              |
| `t_HYBRID`    | `HybridExecution(pts)`                     | dgm.cu                        | Um thread OpenMP comanda a GPU, os demais processam regiões na CPU    |

A ideia geral de "região" (usada em `t_PAR_CPU`, `t_GPU` e `t_HYBRID`) é:
como o vetor de estado é gigante (`2^qubits`) e não muda de tamanho, dá para
particioná-lo em blocos independentes (fixando alguns bits do índice) e
processar cada bloco em paralelo — seja em threads CPU diferentes, seja em
blocos CUDA diferentes, seja mandando alguns blocos pra CPU e outros pra GPU
ao mesmo tempo (o modo híbrido, que é a contribuição central do "HybriD-GM").

## 4. Fluxo de um algoritmo (ex: Grover)

1. `lib_grover.cpp::Grover()` monta o circuito **como texto** usando os
   geradores de `gates.cpp` (`Hadamard(...)`, `Pauli_X(...)`, etc.) e funções
   próprias (`Oracle1`, `ControledZ`).
2. Chama `dgm.setFunction(...)`, que faz o parsing dessas strings e produz uma
   lista de structs `PT` (uma "porta compilada": matriz 2x2 + posição do
   qubit + máscara/valor de controle).
3. Chama `dgm.execute(it)`, que percorre a lista de `PT` e realmente
   multiplica as amplitudes do `state` pelas matrizes.
4. Chama `dgm.measure(q_pos)` para obter um bit clássico, colapsando o estado.

Esse padrão (montar texto → `setFunction`/`executeFunction` → `execute` →
`measure`) se repete em Grover e em cada uma das `2n` iterações do Shor.
