# Versão em Rust

Esta página descreve a versão em Rust do simulador (workspace em [`crates/`](../crates/README.md)):
como compilar e usar, o que ela faz de diferente do C++, quanto custa em relação ao C++ e **em
quais máquinas ela foi de fato testada**. O plano e as decisões estão em
[09](09-plano-migracao-rust.md); o C++ continua no tag `v1.1.0` e na branch `cpp-legacy`.

## 1. Compilar e usar

```bash
cargo build --release          # sem nvcc, sem makefile; gera target/release/{general,grover,shor}
cargo test --workspace         # paridade com o C++ (tests/golden) e testes unitários
bash tests/smoke_test_rust.sh  # os três executáveis, fim a fim
```

Precisa só do Rust (o compilador está fixado em `rust-toolchain.toml`: `1.100.0`). Os
argumentos são os mesmos dos `*.out` do C++:

```text
general QUBITS [EXEC_TYPE] [THREADS] [BLOCK_SIZE] [REPEAT_COUNT] [GPU_REGION_BITS]
grover  QUBITS [EXEC_TYPE] [THREADS] [BLOCK_SIZE] [REPEAT_COUNT] [GPU_REGION_BITS] [SEARCH_VALUE]
shor    QUBITS [EXEC_TYPE] [THREADS] [BLOCK_SIZE] [REPEAT_COUNT] [GPU_REGION_BITS]
```

| `EXEC_TYPE` | Backend | Observação |
|---|---|---|
| `0` | CPU serial (`t_CPU`) | o oráculo dos demais |
| `1` | CPU paralela (`t_PAR_CPU`) | regiões em paralelo com rayon; `THREADS` vale; usa AVX2 se a CPU tem |
| `2` | GPU (`t_GPU`) | wgpu (Vulkan, Metal, DX12); estado residente na GPU |
| `3` | híbrido (`t_HYBRID`) | GPU enquanto o estado cabe, CPU paralela quando não; `THREADS` vale |

`BLOCK_SIZE`, `REPEAT_COUNT` e `GPUS` são lidos e ignorados (não existem no wgpu);
`GPU_REGION_BITS` vale e o contexto o reduz sozinho se o adaptador oferece menos memória de
workgroup. Variáveis de ambiente:

| Variável | Efeito |
|---|---|
| `HGM_SEED` | fixa a semente (sem ela é `tempo ^ pid`, como no C++) |
| `HGM_FUSE_PHASE=0/1` | desliga/liga a fusão de portas de fase (padrão: ligada na GPU e no híbrido, desligada na CPU) |
| `SHOR_EMULATE=1` | o Shor troca a multiplicação modular pela permutação direta dos índices |
| `HGM_REQUIRE_GPU=1` | nos testes: a falta de adaptador vira falha em vez de "pulado" |

## 2. Limites

- **Qubits:** a CPU vai até 32 (o C++ ia até 30); a GPU, até 31 (índices de 32 bits).
  O `t_HYBRID` com mais de 31 qubits roda na CPU.
- **Memória da GPU:** o estado ocupa `8 * 2^n` bytes. Acima do limite de *binding* do
  dispositivo (27 qubits na RTX A4500) o estado é dividido em blocos, um buffer por bloco, até
  31 qubits se a placa aguenta. Sem memória, o `t_GPU` falha com mensagem clara e o
  `t_HYBRID` traz o estado para o host e continua na CPU (de mão única).
- **Shor:** os mesmos números mapeados do C++ (15, 17, 19, 21, 23, 25, 27 qubits).
- **Adaptador de software** (Windows/WARP no CI): funciona, mas a região de GPU fica limitada
  a 8 bits e tudo é lento; serve para os testes, não para uso.

## 3. Diferenças em relação ao C++

- Argumento inválido sai com código diferente de zero (2 para uso, 1 para erro de execução);
  o C++ imprimia a mensagem e saía com 0.
- Sem OpenMP, sem CUDA, sem `rand()`: o gerador é o `mt19937` e a semente é controlável.
- O `t_HYBRID` do C++ dividia o estado entre CPU e GPU quando ele não cabia na GPU, com
  memória fixada (`cudaHostRegister`). O wgpu não tem memória fixada; medido (A4500,
  `docs/09` fase 6) a transferência fica em ~2,1 GB/s e o ganho só existiria em lotes grandes
  de portas. O híbrido do Rust, quando não cabe, usa a CPU paralela.
- A fusão de portas de fase na GPU lê um termo para quatro amplitudes (como o CUDA); ligada
  por padrão na GPU.
- `Shor` com a correção do item 30 (`a^-1` na multiplicação modular), como no C++ `v1.1.0`.

## 4. Desempenho contra o C++ `v1.1.0`

Servidor qflex (Threadripper PRO 5955WX, RTX A4500), **uma execução por célula**, tempo de
parede incluindo abrir a GPU, mesmas linhas de comando nos dois lados
([`benches/end_to_end.sh`](../benches/end_to_end.sh), 2026-11-23). Razão = Rust ÷ C++ (menor é
melhor). O critério de aceitação é CPU >= 90% da velocidade do C++ (razão <= 1,11) e GPU até
1,5x o CUDA.

| Caso | C++ | Rust | Razão |
|---|---|---|---|
| shor 17, serial | 2,68 s | 2,43 s | 0,91 |
| shor 19, serial | 15,63 s | 14,54 s | 0,93 |
| grover 18, serial | 3,33 s | 2,92 s | 0,88 |
| general 24, CPU paralela 16 thr | 0,18 s | 0,12 s | 0,67 |
| general 26, CPU paralela 16 thr | 0,81 s | 0,55 s | 0,68 |
| general 32, CPU paralela 32 thr | 53,99 s | 35,19 s | 0,65 |
| grover 22, CPU paralela 16 thr | 6,95 s | 7,22 s | 1,04 |
| shor 21, CPU paralela 16 thr | 2,13 s | 2,13 s | 1,00 |
| general 26, GPU | 0,78 s | 0,33 s | 0,42 |
| general 26, híbrido | 0,68 s | 0,33 s | 0,49 |
| grover 24, GPU | 8,43 s | 8,89 s | 1,05 |
| grover 24, híbrido | 8,42 s | 8,94 s | 1,06 |
| general 32, híbrido (não cabe na GPU) | 32,94 s | 35,02 s | 1,06 |
| shor 21, GPU | 0,78 s | 0,77 s | 0,99 |
| shor 25, GPU (circuito) | 9,10 s | 10,56 s | 1,16 |
| shor 27, GPU (circuito) | 43,93 s | 51,69 s | 1,18 |
| shor 27, GPU (emulação) | 1,29 s | 0,85 s | 0,66 |

Limites desta tabela: uma execução por célula, uma máquina, uma GPU. Grover e Shor são
probabilísticos e as sementes dos dois lados são diferentes; um Shor que acha fatores por
`gcd(a, N) > 1` ou erra cedo é mais curto que um que roda a ordem inteira, então só os casos
longos (circuito) comparam. No `general 32` em CPU o Rust ficou 35% mais rápido; não
investiguei o motivo (o estado de 32 qubits ocupa 32 GiB e o tempo é de banda de memória).

O que mudou durante a fase 7 para chegar aqui (a primeira rodada deu 1,3x a 2,1x em Grover e
Shor na CPU paralela e 1,55x no Shor em GPU): o C++ compilava os laços de CPU com AVX2
(`target_clones`) e o Rust usava o x86-64 básico; hoje o backend paralelo escolhe a versão AVX2
em tempo de execução (módulo `simd`, sem FMA, então o resultado é idêntico bit a bit ao
serial). O caminho genérico da CPU deixou de conferir o índice a cada acesso. Na GPU, o
kernel da porta de fase fundida passou a ler um termo para quatro amplitudes e a fusão virou
padrão. Também aparecia um bug do `t_HYBRID` com 32 qubits (a GPU recusava o estado e o híbrido
não tratava isso como "não cabe"), corrigido.

### Uma GPU integrada (Intel UHD, Windows 11)

Notebook com Core i7-1355U (10 núcleos, 12 threads), 15,7 GB de RAM, Intel UHD Graphics
(driver 32.0.101.7085, Vulkan), toolchain GNU do Rust, **na bateria**. Mediana de 5 execuções
do tempo impresso pelo executável (não inclui abrir o dispositivo). Medido antes de a fusão de
fase virar padrão (não afeta estes casos).

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

Na integrada a GPU só ganha da CPU a partir de ~26 qubits no `general` e de ~18 no Grover. O
híbrido deve ser igual à GPU quando o estado cabe; as diferenças entre as duas colunas (até
~30%) são ruído de uma máquina em bateria e com a CPU e a GPU no mesmo orçamento térmico.

## 5. Verificado e não testado

Só se declara compatibilidade com o que foi executado. "Bateria completa" é a de
[11](11-bateria-testes-gpu.md) (itens C1-C9, R1-R4, P1-P8), que **não foi rodada inteira em
nenhuma GPU**; cada linha diz o que foi feito.

| Sistema | GPU / API | O que foi executado | Estado |
|---|---|---|---|
| Linux x86-64 (Ubuntu 24.04, qflex) | NVIDIA RTX A4500, Vulkan | testes da workspace (paridade com o C++ e com a CPU serial, GPU incl. blocos), smoke test dos 3 executáveis, benchmark da seção 4, estados de 28 a 31 qubits contra a CPU, recuo por falta de memória real (31 qubits) | **verificado** nesses itens |
| Windows 11 (toolchain GNU) | Intel UHD Graphics (integrada), Vulkan | testes da workspace com `HGM_REQUIRE_GPU=1` (paridade da GPU até 1,9e-7 da CPU serial, híbrido na GPU), smoke test, `general` até 26 qubits e Grover até 20 | **parcial**: não rodou estados acima de 26 qubits (blocos só nos testes de blocos pequenos), nem Shor grande, nem a toolchain MSVC |
| Linux (CI `ubuntu-latest`) | sem GPU | testes, clippy, fmt, smoke test (CPU) | verificado só para CPU |
| macOS (CI `macos-latest`, Apple Silicon virtualizado) | Apple Paravirtual (Metal) | testes e smoke test, com o adaptador do runner | **parcial**: GPU virtualizada, sem hardware Apple real |
| Windows (CI `windows-latest`, MSVC) | WARP (software) | testes e smoke test | verificado só em adaptador de software |
| qualquer sistema | AMD (qualquer), Intel Arc ou outra discreta, NVIDIA que não seja a A4500, Apple Silicon real, Windows com GPU real via MSVC, DX12 | nada | **não testado** |

Para marcar uma GPU como "verificada" é preciso a bateria de [11](11-bateria-testes-gpu.md);
os resultados vão em `tests/gpu-battery/resultados/`.

## 6. Como reproduzir os números

```bash
# C++ do tag, no mesmo servidor:
git clone --branch v1.1.0 <repositório> cpp_v110 && make -C cpp_v110 GPU=real
cargo build --release
benches/end_to_end.sh cpp_v110 .          # tabela da seção 4
cargo run --release -p hgm-algos --example cpu_step_profile -- 20 4   # custo por passo do Grover na CPU
```
