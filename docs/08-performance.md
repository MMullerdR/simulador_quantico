# Performance — o que foi medido e onde o tempo vai

Esta página reúne os números de desempenho da versão em Rust, como medi-los de novo, e o que se
sabe sobre onde o tempo se perde. A tabela completa Rust × C++ está em
[12](12-versao-rust.md), seção 4; aqui estão a metodologia, a leitura dos números e o que foi
descartado. Os números do C++ (tag `v1.1.0`) servem de referência e vêm das mesmas máquinas.

**Limites de tudo o que segue:** uma execução por célula (sem repetições nem mediana, salvo onde
dito), tempo de parede, uma máquina de teste com uma GPU (RTX A4500), mais um notebook com GPU
integrada. Grover e Shor são probabilísticos e as sementes dos dois lados são diferentes.

## 1. Metodologia

| O que mede | Como | Onde |
|---|---|---|
| Ponta a ponta, Rust × C++ | as mesmas linhas de comando nos dois (`general`, `grover`, `shor` em serial, CPU paralela, GPU e híbrido); imprime tempo de parede, se achou o resultado e a razão Rust/C++ | `benches/end_to_end.sh DIR_CPP DIR_RUST [FILTRO]` |
| CPU paralela, cargas sintéticas | `hadamard` (H em todos), `phase` (R1..R3 alternados), `mixed` (H + 8 passos controlados); o `checksum` impresso confere que os dois lados calcularam o mesmo estado | `cargo run --release -p hgm-cpu --example bench_par -- QUBITS CARGA THREADS REGIAO COALESCIDA REPS [serial] [otimizado]` (e `benches/cpp/bench_par.cpp` do lado C++, [`benches/README.md`](../benches/README.md)) |
| Custo por passo do Grover na CPU | tempo de cada tipo de passo | `cargo run --release -p hgm-algos --example cpu_step_profile -- QUBITS THREADS` |
| GPU: kernels, banda, Grover | tempo medido pela própria GPU quando o adaptador oferece `TIMESTAMP_QUERY` | `hgm-gpu --example spike`, `hgm-algos --example gpu_grover_bench` |
| Transferência host↔GPU | banda de cópia | `hgm-gpu --example transfer_bench` |

O benchmark do `general` aplica `H` em todos os qubits, **3 vezes seguidas**, e mede as três juntas:
divida por 3 para uma aplicação. O tempo de parede dos executáveis inclui abrir a GPU (contexto,
compilação dos kernels), e as medições "só kernels" do spike não. Para o C++ vale o mesmo cuidado:
o tempo impresso por `general.out` não inclui a abertura do dispositivo.

## 2. Rust contra o C++ `v1.1.0` (resumo)

Servidor com Threadripper PRO 5955WX e RTX A4500, razão = tempo do Rust ÷ tempo do C++ (menor é
melhor); o critério de aceitação era CPU >= 90% da velocidade do C++ (razão <= 1,11) e GPU até
1,5x o CUDA ([09](09-plano-migracao-rust.md), seção 1):

| Backend | Razão | Observação |
|---|---|---|
| CPU serial | 0,88 a 0,93 | Shor 17/19, Grover 18 |
| CPU paralela | 0,65 a 1,06 | `general` 24/26/32 (0,65 a 0,68), Grover 22 (1,04), Shor 21 (1,00) |
| GPU | 0,42 a 1,18 | `general 26` (0,42), Grover 24 (1,05), Shor 21 (0,99), Shor 25/27 em circuito (1,16/1,18) |
| híbrido | 0,49 a 1,06 | `general 26` (0,49), Grover 24 (1,06), `general 32` fora da GPU (1,06) |

O `shor 27` em GPU com a emulação da multiplicação modular leva 0,85 s (0,66x do C++ com
`SHOR_EMULATE=1`). A primeira rodada da fase 7 deu 1,3x a 2,1x em Grover e Shor na CPU paralela e
1,55x no Shor em GPU; o que mudou depois está descrito em [12](12-versao-rust.md), seção 4. Os
vetores de `tests/golden` garantem que esses números são de programas que calculam o mesmo
estado.

## 3. Onde o tempo vai

### Na CPU

- **O laço antigo não vetorizava (~9 ciclos por par de amplitudes, item 23 de
  [07](07-bugs-e-pontos-de-atencao.md)).** Percorrer blocos contíguos de amplitudes com um laço
  simples sobre fatias deu 1,8x no microbenchmark sem `-march` e 7 a 8x com AVX2 (alvo no bit >= 2).
  Compilar tudo com `-march=native` sozinho dava só 8 a 18%, e amarraria o binário a uma máquina;
  por isso o Rust compila o mesmo código duas vezes e escolhe AVX2 em tempo de execução
  ([03](03-motor-de-execucao-cpu.md), seção 5).
- **`general` e os casos de 32 qubits são limitados pela banda de memória.** O vetor de 32 qubits
  tem 32 GiB e cada porta o percorre; mais threads ou instruções mais largas quase não mudam o
  tempo. No `general 32` o Rust ficou 35% mais rápido que o C++ (35,2 s contra 54,0 s); **não
  investiguei o motivo**.
- **O Grover é uma cadeia de dependências.** Cada passo depende do anterior, então reordenar portas
  não ajuda (0% medido). O ganho do Grover veio de tirar passadas (difusor sem os `X` extras, item
  22) e de vetorizar.
- **Reordenar portas para a CPU, fundir diagonais idênticas e um backend esparso foram medidos e
  descartados** (no Shor o estado ocupa de 44% a 97% do vetor, então não é esparso; reordenar dá
  1,00x; fundir diagonais idênticas dá 0 fusões).
- **A fusão de fase não ajuda na CPU:** a porta fundida percorre os termos por amplitude, e ficou
  mais lenta que as portas separadas na carga de fases puras. É por isso que o padrão da CPU é
  desligada.
- O **serial é a referência dos testes** e não é otimizado de propósito.

### Na GPU

- **As passadas dominam, não as contas.** Desligar as portas dentro do kernel só reduz o tempo para
  43% a 68%; o resto é ler e escrever o estado. A cópia de banda mede 599 GB/s (buffer de 512 MiB)
  na A4500, e o custo de uma porta dentro da região é 0,181 ms em 26 qubits (7800 portas),
  **igual ao do CUDA**. Por isso o que mais rende é colocar mais portas em cada passada: o
  agendamento por comutação (item 24) reduziu o `general 26` de 9 para 3 passadas (26,9 ms → 16,5 ms
  só de kernels) e deu ~9% no Shor 25/27.
- **O Shor é ~85% rotações diagonais** da QFT e dos somadores. Fundi-las (item 27) e reescrever o
  corpo do kernel fundido (termos ativos compactados, 4 amplitudes por leitura de termo) deu de 5% a
  25%. No C++ o kernel levava 92% do tempo do Shor 25, repartido em ~2,4 s de carga/gravação, ~1,2 s
  de portas normais e ~5,3 s de portas fundidas (uma fundida custava ~4x uma normal). Juntar portas
  fundidas consecutivas foi descartado: das 21032 fundidas do Shor 25, só 18 junções (o `H` entre
  elas não comuta). Uma tabela de fase em dois níveis na memória de workgroup também não ganhou.
- **O tuning importa pouco depois do padrão.** Região 12 e coalescida 4 (o melhor combo da
  A4500) deu 2 a 2,5x sobre o padrão antigo do C++ (região 8); região 13 piora (64 KiB de memória
  de workgroup); a coalescida entre 3 e 7 quase não muda; especializar o kernel por tipo de matriz
  rende 0%.
- **Estado residente é o que faz a GPU competir.** O `general 26` com cópia completa do estado
  (host → GPU → host) levou 0,92 a 0,97 s; com o estado residente, 0,24 a 0,28 s. A transferência
  do wgpu fica em ~2,1 GB/s para a GPU e ~2,3 GB/s de volta (sem memória fixada), 1,5 a 1,6x acima
  do `cudaMemcpy` do C++, e por isso o `Simulator` mantém o estado na placa ([04](04-gpu-wgpu.md),
  seção 6).
- **A emulação da multiplicação modular** (`SHOR_EMULATE=1`) troca centenas de portas por uma
  permutação: `shor 27` em GPU de 51,7 s para 0,85 s. Deixa de ser um circuito (item 29).

### No híbrido

Com o estado na GPU o `t_HYBRID` é o `t_GPU` (os números dos dois ficam a poucos por cento um do
outro: `general 26` 0,33 s nos dois; Grover 24 8,89 s contra 8,94 s). Só difere quando o estado não
cabe: aí roda na CPU paralela (`general 32`: 35,0 s no híbrido, 35,2 s na CPU paralela). Dividir
blocos do estado entre CPU e GPU só compensaria em lotes grandes de portas, por causa da transferência
([04](04-gpu-wgpu.md), seção 9).

## 4. Qual backend usar (medido na Intel UHD, notebook com Core i7-1355U)

O custo fixo de abrir a GPU e lançar kernels faz a GPU perder nos estados pequenos. Mediana de 5
execuções do tempo impresso (não inclui abrir o dispositivo), notebook na bateria,
**GPU integrada**, antes de a fusão de fase virar padrão (não afeta estes casos):

`general` (3 aplicações de H em todos os qubits):

| qubits | CPU 12 thr | CPU 6 thr | GPU | híbrido |
|---|---|---|---|---|
| 18 | 0,006 s | 0,006 s | 0,270 s | 0,274 s |
| 20 | 0,018 s | 0,020 s | 0,276 s | 0,192 s |
| 22 | 0,069 s | 0,083 s | 0,495 s | 0,628 s |
| 24 | 0,483 s | 0,410 s | 0,709 s | 0,924 s |
| 26 | 2,290 s | 1,828 s | 1,602 s | 1,124 s |

`grover`:

| qubits | serial | CPU 12 thr | GPU | híbrido |
|---|---|---|---|---|
| 14 | 0,067 s | 0,022 s | 0,300 s | 0,311 s |
| 16 | 0,656 s | 0,166 s | 0,481 s | 0,414 s |
| 18 | 12,39 s | 0,866 s | 0,804 s | 0,735 s |
| 20 | 81,48 s | 5,849 s | 2,731 s | 2,326 s |

Leitura: na integrada a GPU só ganha da CPU a partir de ~26 qubits no `general` e de ~18 no Grover.
Os tempos de GPU em 18 a 22 qubits são quase constantes (~0,27 a 0,5 s): é custo de lançamento, não
de trabalho. Mais threads também pode piorar em estados pequenos (o custo de coordenar as threads
passa do trabalho). As diferenças de até ~30% entre GPU e híbrido nessa tabela são ruído de uma
máquina na bateria, com CPU e GPU no mesmo orçamento térmico: o híbrido deve ser igual à GPU
quando o estado cabe.

Na RTX A4500 do servidor, o `general 26` em GPU leva 0,33 s contra 0,55 s da CPU paralela com 16
threads. Para estados de 28 a 31 qubits o Rust foi **conferido contra a CPU** na A4500, mas **não
tenho tempos do Rust** para eles; como referência, o C++ levava ~15,8 s na GPU contra ~40,7 s na CPU
paralela a 31 qubits, e ~42 s no híbrido contra ~85 s na CPU a 32 qubits (antes da vetorização da
CPU).

## 5. Armadilhas ao medir

- **O primeiro uso da GPU custa mais.** Criar o contexto e compilar os kernels entra no tempo de
  parede dos executáveis; no adaptador de software (WARP) esse custo cresce depressa com o tamanho
  da região, e por isso a região é limitada a 8 bits ali ([04](04-gpu-wgpu.md), seção 8). Não use o
  WARP para medir desempenho.
- **Um contexto de GPU por processo** ([07](07-bugs-e-pontos-de-atencao.md)): não meça vários
  dispositivos em threads.
- **Compare processos inteiros com processos inteiros.** O spike mede só os kernels (16,5 ms no
  `general 26`) e o executável mede o processo (0,33 s): são números de coisas diferentes.
- **`perf` e `ncu` ficam bloqueados nas máquinas de teste** (permissão de contadores), então a
  atribuição de tempo foi feita por ablação (desligar uma parte e medir) e por instrumentação no
  próprio código.
- **Shor e Grover variam por acaso.** Um Shor que acha fatores por `gcd(a, N) > 1` ou erra cedo é
  mais curto que um que roda a ordem inteira; só os casos longos (circuito) comparam. A métrica
  certa de qualidade do Shor é a fase medida, não a taxa de "Found factors" (item 31 de
  [07](07-bugs-e-pontos-de-atencao.md)).

## 6. Histórico do C++ (referência)

O C++ original foi otimizado em outubro de 2026, antes da porta, e o Rust partiu desses números.
Na qflex (Threadripper PRO 5955WX, RTX A4500), com a mesma linha de comando e uma execução por
célula: **primeira** = código original (`9d36e42`), **início** = começo das otimizações,
**v1.1.0** = o tag. O texto completo e as tabelas por qubit da RTX 4070 estão no
`docs/08-performance.md` do tag `v1.1.0`.

| Caso | Primeira | Início | v1.1.0 |
|---|---|---|---|
| shor 17, serial | 2,96 s | 2,97 s | 2,68 s |
| shor 19, serial | 18,05 s | 18,12 s | 15,63 s |
| grover 18, serial | 4,91 s | 4,93 s | 3,33 s |
| general 26, CPU paralela 16 thr | 0,977 s | 0,978 s | 0,81 s |
| general 26, GPU | 0,709 s | 0,630 s | 0,78 s |
| grover 24, GPU | (1) | 27,4 s | 8,43 s |
| shor 21, GPU | (1) | 2,13 s | 0,78 s |
| shor 25, GPU | (1) | 50,5 s | 9,10 s |
| shor 27, GPU | (1) | 278,8 s | 43,93 s |

(1) O `grover.out` e o `shor.out` originais ignoravam o tipo de execução e sempre rodavam em CPU
serial (item 8 de [07](07-bugs-e-pontos-de-atencao.md)); só o caso serial é comparável com o original.
O ganho vem do tuning de GPU e do Grover sem os `X` (item 22), dos laços de CPU vetorizados (23), da
reordenação de portas (24), da QFT aproximada (26), da fusão de fases (27) e do estado residente (28).
Os circuitos de hoje não são os originais (o original tinha o bug do item 1 e o difusor do Grover com
mais portas), então a comparação com a "primeira" mede o conjunto, não só o motor. As colunas "Primeira" e "Início" vêm do registro antigo (tempo impresso pelo programa); a coluna
v1.1.0 vem de [12](12-versao-rust.md), seção 4 (tempo de parede, que inclui abrir a GPU). Por isso
o `general 26` em GPU parece subir de 0,63 s para 0,78 s: não é regressão, é o que se está
cronometrando.

## 7. Como reproduzir

```bash
cargo build --release
# C++ do tag, no mesmo servidor:
git clone --branch v1.1.0 <repositório> cpp_v110 && make -C cpp_v110 GPU=real
benches/end_to_end.sh cpp_v110 .          # tabela da seção 2 (e de 12)

for q in 10 14 18 20 22 24 26; do
  target/release/general $q 1 16          # CPU paralela, 16 threads
  target/release/general $q 2 1           # GPU
done
```
