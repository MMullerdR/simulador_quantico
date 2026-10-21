# Performance — números de verdade, medidos com GPU real

Até 2026-08-11 nunca tinha havido acesso a uma GPU NVIDIA real neste
projeto (nem nesta máquina, nem no notebook — ver
[07-bugs-e-pontos-de-atencao.md](07-bugs-e-pontos-de-atencao.md) item 7),
então nenhuma comparação de performance entre backends jamais tinha sido
medida — só "funciona"/"não funciona". Esta página registra a primeira
medição de verdade, feita nesta máquina (RTX 4070, WSL2 + CUDA Toolkit
13.3.1, `make GPU=real KERNEL_OPT=-O3` — o default atual do `makefile`).

## Metodologia

`outputs/general.out <qubits> <exec_type> <threads|gpus>` — benchmark de
Hadamard: aplica H em todos os qubits, 3 vezes seguidas
(`HadamardNQubits`, `iterations=3`), mede o tempo das 3 juntas. Os
números abaixo são esse tempo bruto (3 iterações), não por-iteração —
divida por 3 pra estimar uma única aplicação de H em todos os qubits.

`general.out` não aceita `t_CPU` (serial) diretamente — `t_PAR_CPU` com
`thread_count=1` é usado aqui como proxy (motor paralelo com 1 thread só,
não o motor serial dedicado `CpuExecution1_*` — os dois deviam ter
performance parecida pra esse benchmark, mas não são literalmente o mesmo
código).

Hardware: AMD Ryzen 9 7900X (12 cores/24 threads), RTX 4070 (16GB VRAM),
15GB RAM disponível pro WSL2. Tuning de GPU no default **da época**
(`block_size=64, repeat_count=2, gpu_coalesced_bits=4, gpu_region_bits=8`;
hoje o default é 512/4/12 e o Grover não faz mais os X do difusor, então os
tempos de GPU e Grover desta tabela estão defasados — ver item 22 em
[07-bugs-e-pontos-de-atencao.md](07-bugs-e-pontos-de-atencao.md)).

## Qubits pequenos a médios (10-24) — todos os backends

| qubits | PAR_CPU(1) | PAR_CPU(4) | PAR_CPU(8) | PAR_CPU(16) | GPU | HYBRID(4) |
|---|---|---|---|---|---|---|
| 10 | 0.00011s | 0.000255s | 0.000574s | 0.001085s | 0.235s | 0.000288s |
| 14 | 0.00081s | 0.00107s | 0.001824s | 0.002245s | 0.180s | 0.001149s |
| 18 | 0.0162s | 0.0056s | 0.0059s | 0.0059s | 0.191s | 0.0179s |
| 20 | 0.0724s | 0.0218s | 0.0165s | 0.0158s | 0.224s | 0.0726s |
| 22 | 0.3145s | 0.0903s | 0.0655s | 0.0582s | 0.214s | 0.2530s |
| 24 | 1.3805s | 0.4242s | 0.2796s | 0.2364s | 0.326s | 0.4103s |

**GPU é a opção mais lenta até ~22 qubits.** Faz sentido: o overhead fixo
de lançar kernel + copiar memória host↔device (~0.18-0.24s, quase
constante nessa faixa — dominado pelo overhead, não pelo trabalho em si)
é maior que o próprio cálculo pra um vetor de estado pequeno. Só a partir
de ~24 qubits o trabalho real começa a pesar mais que o overhead fixo.

**Mais threads pode ser** ***mais lento*** **em qubits pequenos.** Em 10
qubits, `PAR_CPU(16)` é ~10x mais lento que `PAR_CPU(1)`
(0.001085s vs 0.00011s) — o overhead de criar/coordenar 16 threads OpenMP
supera de longe o trabalho de aplicar H em 2^10 amplitudes. O ponto de
virada (mais threads passa a compensar) fica entre 14 e 18 qubits nesta
máquina.

**HYBRID não bate nem CPU nem GPU de forma consistente** nesta faixa —
em 22 qubits, por exemplo, fica pior que `PAR_CPU(8)`/`PAR_CPU(16)`
sozinhos. Não investigado a fundo aqui (possível relação com o modo
"HYBRID acaba mandando o lote inteiro pra uma só das duas partes",
documentado como limitação conhecida no item 6 de
[07-bugs-e-pontos-de-atencao.md](07-bugs-e-pontos-de-atencao.md) sob
`GPU=stub` — não confirmado se o mesmo padrão se aplica sob `GPU=real`).

## Qubits altos (26-30) — GPU vs melhor config de CPU

`QB_LIMIT` (definido em [common.h](../include/core/common.h)) é **30** —
nunca tinha sido testado até onde esse limite realmente aguenta na
prática. Testado aqui pela primeira vez, sem crash em nenhum ponto:

| qubits | tamanho do vetor de estado | GPU | PAR_CPU(16) | GPU vs CPU(16) |
|---|---|---|---|---|
| 26 | 512 MB | 0.800s | 1.088s | 1.36x mais rápido |
| 27 | 1 GB | 1.339s | 2.418s | 1.81x mais rápido |
| 28 | 2 GB | 2.727s | 5.168s | 1.90x mais rápido |
| 29 | 4 GB | 5.293s | 11.401s | 2.15x mais rápido |
| 30 | 8 GB | 10.283s | 24.284s | 2.36x mais rápido |

**A GPU ultrapassa a melhor configuração de CPU (16 threads) entre 24 e
26 qubits**, e a vantagem cresce com o tamanho do problema — em 30
qubits (o teto de `QB_LIMIT`) a GPU já é mais que o dobro mais rápida.
Padrão esperado: overhead fixo de GPU vira irrelevante perto do trabalho
real conforme o vetor de estado cresce, e a GPU tem muito mais
paralelismo bruto disponível que 16 threads de CPU.

**`QB_LIMIT=30` funciona de verdade nesta máquina** — 8GB de vetor de
estado, dentro do orçamento de VRAM (16GB) e RAM do WSL2 (15GB) com
folga. Não testado além de 30 (o código recusa qualquer valor maior, ver
`QB_LIMIT`).

**Atualização (2026-10-20, servidor qflex, RTX A4500 de 20 GB):** `QB_LIMIT`
subiu para 32. A 31 qubits (16 GB) a GPU leva ~15,8s contra ~40,7s da CPU
paralela; a 32 qubits (32 GB, não cabe na GPU) o `t_GPU` falha com "out of
memory", a CPU paralela leva ~85s e o `t_HYBRID` ~42s. Detalhes e a
delegação automática à GPU quando o estado cabe: item 17, tentativa 7, em
[07-bugs-e-pontos-de-atencao.md](07-bugs-e-pontos-de-atencao.md).

**Atualização (2026-10-21, passadas da GPU):** portas que comutam passaram a
ser reordenadas antes do `t_GPU` agrupá-las em lotes (item 24 de
[07-bugs-e-pontos-de-atencao.md](07-bugs-e-pontos-de-atencao.md)): Shor 25
19,0 -> 17,3 s e Shor 27 112 -> 102 s na qflex (já com o tuning do item 22;
eram 274 s antes dele). Grover não muda.

**Atualização (2026-10-21, CPU vetorizada):** os laços de `t_PAR_CPU`/
`t_HYBRID` foram reescritos em blocos contíguos com AVX2 escolhido em
execução (item 23 de [07-bugs-e-pontos-de-atencao.md](07-bugs-e-pontos-de-atencao.md)).
Na qflex: Grover 22 em 16 threads 17,3 → 6,95 s, Shor 21 6,66 → 2,64 s,
`general.out 24` em 1 thread 1,61 → 0,37 s, e a 32 qubits (limitado por
banda de memória) `t_PAR_CPU` ~85 → 61 s e `t_HYBRID` ~42 → 36 s. Os
números de CPU das tabelas abaixo são anteriores a essa mudança.

**Atualização (2026-10-21, qflex):** `QB_LIMIT` subiu para 33 (64 GB de
vetor de estado; a máquina tem 125 GB de RAM, então 34 qubits — 128 GB —
não cabe e não foi testado). `general.out 33`: `t_PAR_CPU` com 32 threads
149 s, `t_HYBRID` 76 s (modo de regiões; pico de RSS 64 GB, a amplitude
impressa é a mesma nos dois). Grover e Shor **não** foram rodados até o fim
nesses tamanhos: o tempo cresce ~2,9× por qubit no Grover (22 qubits 3,2 s,
24 qubits 27 s, 26 qubits 218 s na GPU, ou seja ~13 h a 31 qubits) e o Shor
a 27 qubits (o maior mapeado em `shor.cpp`) já leva 274 s. A 31 qubits
(`t_GPU`) e a 32 qubits (`t_HYBRID`, modo de regiões) o Grover foi só
rodado por 100 s sem crash — isso mostra que aloca e progride, não que
acha o valor.

## Como reproduzir

```bash
make GPU=real  # precisa de GPU NVIDIA real + nvcc, ver docs/04-gpu-cuda.md
for q in 10 14 18 20 22 24 26 28 30; do
  outputs/general.out $q 1 16   # t_PAR_CPU, 16 threads
  outputs/general.out $q 2 1    # t_GPU
done
```
