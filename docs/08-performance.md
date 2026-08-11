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
15GB RAM disponível pro WSL2. Tuning de GPU no default
(`block_size=64, repeat_count=2, gpu_coalesced_bits=4, gpu_region_bits=8`).

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

## Como reproduzir

```bash
make GPU=real  # precisa de GPU NVIDIA real + nvcc, ver docs/04-gpu-cuda.md
for q in 10 14 18 20 22 24 26 28 30; do
  outputs/general.out $q 1 16   # t_PAR_CPU, 16 threads
  outputs/general.out $q 2 1    # t_GPU
done
```
