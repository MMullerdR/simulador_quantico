# Vetores de referência (golden) do C++

Fase 1 de [docs/09-plano-migracao-rust.md](../../docs/09-plano-migracao-rust.md).
Esta pasta guarda o que o C++ do tag `v1.1.0` produz para circuitos fixos, em
formato legível sem o C++, para o porte em Rust provar paridade e para o C++ poder
sair do repositório sem perder o oráculo.

- `export_golden.cpp`: gera tudo (`make golden`).
- `data/`: a saída (310 casos, ~3,7 MB). **Não editar à mão**: regenerar.
- `check_golden.py`: confere `data/` com um simulador independente em Python/numpy.

Proveniência dos dados atuais: tag `v1.1.0` (`d2c17c2`), compilado com
`g++ 13.3.0` (Ubuntu 24.04, x86-64) pelo `makefile` (sem `-O` e sem `-march=native`,
logo sem FMA), backend `t_CPU` serial. Duas execuções seguidas produzem arquivos
idênticos byte a byte.

## Convenções

- Estado: `2^qubits` amplitudes; **qubit `q` do circuito = bit `qubits-1-q` do
  índice** ([docs/01](../../docs/01-arquitetura-geral.md)).
- Arquivo de estado (`*.f32`): float32 little-endian, pares `(re, im)` por amplitude,
  sem cabeçalho; tamanho = `2^qubits * 8` bytes.
- Texto: números de ponto flutuante com `%.9g` (ida e volta exata de float32),
  inteiros em decimal, campos separados por espaço, uma informação por linha.
- Steps de circuito: a linguagem de [docs/02](../../docs/02-linguagem-de-circuitos.md),
  um step por linha, sem espaços.
- Caminhos dentro de um `.case` são relativos a `data/`.

## Formato de um `.case`

Todo caso começa com `format 1`, `name <id>` e `kind <tipo>`. `data/INDEX.txt`
lista os casos (`format 1`, `cases N`, depois um caminho por linha). Linhas
`gate NOME m0r m0i m1r m1i m2r m2i m3r m3i` declaram portas fora do catálogo base
(`H X Y Z R1 R2 R3`), com a matriz 2x2 em ordem de linhas; incluem as portas
criadas pelos geradores (`Rot_N`, `R...`). `steps N` e `lines N` são seguidos de
exatamente N linhas.

| `kind` | Campos | O que o Rust deve reproduzir |
|---|---|---|
| `state` | `qubits`, `iterations`, `qft_degree` (opcional), `init basis I` ou `init file F`, `gate...`, `steps N` + steps, `final file F` | aplicar os steps (repetidos `iterations` vezes) ao estado inicial dá o estado final |
| `text` | `qft_degree`, `call ...` (descrição), `gate...`, `lines N` + steps | o gerador de circuito, chamado com os parâmetros de `call` e o grau de QFT, devolve exatamente estas linhas e cria estas portas |
| `pts` | `qubits`, `mode`, `schedule C E B` (se houver), `absorbed K` (se houver), `gate...`, `steps N`, `pts N` + uma linha por porta compilada | ver abaixo |
| `measure` | `qubits`, `init file`, `seed S`, `measure N q1..qN`, `bits`, `values`, `final file` | medir os qubits em ordem, com `mt19937` semeado com `S`, dá estes bits, estas probabilidades e este estado |
| `colapse` | `qubits`, `init file`, `colapse Q V`, `final file` | forçar o qubit `Q` a `V` e renormalizar |
| `modmult` | `qubits`, `modmult CTRL REG0 WIDTH MULT MOD`, `init file`, `final file` | `applyModMult` (permutação do estado, exata) |
| `table` | linhas `funcao args = resultado` | `modular_pow`, `mul_inv`, `quantum_gcd`, `revert_bits`, `qft_degree_for_width`, `quantum_frac_approx` |

### Casos `pts` (parser, fusão, agendamento)

Cada linha de porta compilada:

- `pt TARGET_BIT CONTROL_MASK CONTROL_VALUE CONTROL_COUNT MATRIX_TYPE m0r m0i ... m3r m3i POS...`
  com `MATRIX_TYPE` 0 = denso, 1 = diagonal, 2 = anti-diagonal (enum de
  `common.h`) e `POS...` as `CONTROL_COUNT` posições de bit dos controles;
- `fused N MASK VALUE STEPS ...` (N termos): porta de fase fundida, `amplitude[i] *=
  e^{2 pi i * soma(STEPS dos termos com (i & MASK) == VALUE) / 65536}`.

`mode` diz como a lista foi obtida: `parse` (só o parser, com a ordenação por step
que o C++ aplica), `fuse` (+ `FusePhaseRuns`), `schedule` (+ `ScheduleForBatching`
com `C E B` = bits coalescidos, bits extras, máximo de portas por lote),
`fuse_schedule` (as duas). `absorbed` é o valor devolvido por `FusePhaseRuns`:
portas do circuito = itens da lista - portas fundidas + `absorbed`.

A ordem da lista **faz parte** do que se compara em `parse` e `schedule`: é a ordem
que o C++ usa para executar.

## Tolerâncias sugeridas para o Rust

| Comparação | Tolerância |
|---|---|
| texto (`text`, `table`) e campos inteiros de `pts` (alvo, máscara, valor, contagem, tipo, posições) | igualdade exata |
| matrizes das linhas `pt`/`gate` | tolerância de 2e-7 por componente: `R1`..`R3` do C++ usam `cpowf` em `float`, cujos últimos bits dependem da libm (ex.: `R1` tem parte imaginária ~8e-9 em vez de 0). O Rust calcula em `f64` e arredonda; a diferença medida no cálculo é < 1e-7. Não afeta a classificação denso/diagonal |
| `modmult` | igualdade exata (é permutação) |
| `state`, `colapse`, `measure` (estado) em CPU serial | máx. \|diferença\| < 1e-5; o C++ e o Python independente (double) já diferem de até ~5e-6 nos casos atuais, então 1e-5 é justo, não folgado |
| `measure`: bits | iguais (o sorteio é `uniform_real_distribution<double>` do libstdc++ sobre o MT19937: dois sorteios de 32 bits `lo + hi * 2^32`, dividido por `2^64`, convertido para `float` antes de comparar com a probabilidade do bit 0) |
| qualquer coisa em GPU | a tolerância do `docs/11`, nunca igualdade (FMA e ordem de soma diferem) |

## O que o `check_golden.py` verifica de forma independente

Roda em segundos (`python tests/golden/check_golden.py`, precisa de numpy):

- integridade: o `INDEX`, os arquivos de estado (tamanho) e a norma;
- os 144 casos `state`: reexecuta os steps com um simulador próprio (escrito a
  partir de `docs/02`, não do C++) e compara com o estado final;
  rodadas do Shor: o registrador termina no valor esperado (`a*x mod N` com o
  controle em 1, `x` com o controle em 0); QFT seguida de RQFT devolve o estado
  inicial; o somador de Fourier soma;
- `measure`: reproduz o MT19937 com numpy e a regra de sorteio e confere os bits, as
  probabilidades e o colapso;
- `modmult`, `colapse`: recalcula;
- `pts`: reconstrói as portas de cada step e compara com a lista do parser; confere que
  fusão e agendamento preservam o estado e a contagem de `absorbed`;
- `table`: `pow`, `math.gcd` etc. do Python (a `frac_approx` só tem o contrato
  conferido, não o valor);
- `text`: gramática de todos os steps, e reimplementação dos geradores simples
  (`Hadamard`, `Oracle1`, `ControledZ`, `Toffoli`).

Foi validado com mutações propositais (um bit de estado, um bit medido, uma linha
da tabela, uma porta do `pts`, um valor de permutação): o verificador reprova
todas. Ele não vê diferença menor que ~5e-5 num estado.

## O que os vetores NÃO cobrem

- **GPU**: o C++ do tag compara GPU com CPU por tolerância em `make test`; não há
  vetor de GPU aqui (a referência é a CPU).
- **Execução paralela e híbrida**: igual ao serial por teste (`test_cpu_parallel`),
  então os vetores valem para os quatro backends.
- **Estados grandes**: no máximo 13 qubits (arquivos de 64 KiB). Desempenho e
  tamanhos grandes ficam com a bateria de [docs/11](../../docs/11-bateria-testes-gpu.md).
- **`Shor()` e `Grover()` ponta a ponta**: são probabilísticos. O que está aqui são
  os circuitos e o estado, não o resultado de uma execução sorteada. O teste por
  fase medida (item 30 de docs/07) continua pendente.
- **Funções com `cout`**: `colapse` imprime a soma de probabilidades no C++; isso não
  é exportado.

## Regenerar

```bash
make golden          # build do C++ (stub, sem GPU) e escreve tests/golden/data
python tests/golden/check_golden.py
```

Regenerar só faz sentido se o comportamento do C++ mudou de propósito; o diff dos
dados é o registro dessa mudança. Para reproduzir os dados do tag, usar o tag.
