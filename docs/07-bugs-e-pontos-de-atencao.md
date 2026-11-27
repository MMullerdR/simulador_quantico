# Bugs e Pontos de Atenção

Lista viva de problemas e riscos, para não tomar decisões de "incrementar o algoritmo X" sobre uma
base que já tem um bug de corretude escondido. Tem duas partes:

1. **Pontos de atenção da versão em Rust** (seção 1): o que vale saber antes de mexer ou de
   confiar nos resultados.
2. **O histórico dos 31 itens do C++** (seção 2): o que cada um foi e **o que aconteceu com ele no
   Rust**. A numeração é a original, porque comentários no código e outros documentos citam
   "item N". O texto completo dos itens 1 a 30 (investigação, tentativas, medições) está no
   `docs/07-bugs-e-pontos-de-atencao.md` do tag `v1.1.0` e da branch `cpp-legacy`. O item 31,
   achado durante a porta para Rust, ainda está aberto e por isso vem em detalhe na seção 3.

## 1. Pontos de atenção da versão em Rust

### Em aberto

- **A correção de fase do Shor não faz nada (item 31, abaixo).** O Rust reproduz o `v1.1.0` de
  propósito, para manter a paridade bit a bit com os vetores de `tests/golden/data/runs.txt`.
  Para `N = 15` é irrelevante. Para outros `N`, não use a taxa de "Found factors" para julgar a
  qualidade do Shor.
- **Compatibilidade de GPU: só uma foi testada de verdade.** Verificado na NVIDIA RTX A4500
  (Linux, Vulkan); parcial na Intel UHD (Windows, toolchain GNU) e no Metal virtualizado do CI;
  **não testado** em AMD, Apple Silicon real, outras NVIDIA, Intel Arc e Windows com GPU real via
  MSVC. A bateria de [11](11-bateria-testes-gpu.md) não foi executada inteira em nenhuma GPU. A
  tabela completa está em [12](12-versao-rust.md), seção 5.
- **Medições de desempenho são de uma execução por célula, numa máquina** ([12](12-versao-rust.md),
  seção 4). Grover e Shor são probabilísticos e as sementes dos dois lados são diferentes.
- **`unsafe` existe em dois módulos de `hgm-cpu`** e em mais nenhum lugar (`[workspace.lints]`
  proíbe no resto): `shared.rs`, que entrega a cada thread uma região disjunta do vetor (cada acesso
  é checado em tempo de execução contra a região, e cada bloco tem `// SAFETY:`), e `simd.rs`, que só
  chama a versão AVX2 depois de `is_x86_feature_detected!`. **Não rodei `miri` nem `loom`.**
- **Precisão simples em tudo.** O estado é `Complex32`. As somas de probabilidade da medição usam
  `f64` no host, mas na GPU cada workgroup soma em `f32` antes do fechamento em `f64` (WGSL não tem
  `f64` portátil). O estado final da GPU difere da CPU serial por até ~1,9e-7 (medido na Intel
  UHD); os testes comparam com tolerância, não por igualdade, exceto serial × paralelo, que é
  idêntico bit a bit (sem FMA).

### Armadilhas conhecidas

- **Um `GpuContext` por processo.** Vários dispositivos de GPU destruídos ao mesmo tempo travaram
  ou derrubaram o processo na A4500 (`crates/hgm-gpu/examples/stress.rs`). `shared_context()`
  guarda um contexto num estático que nunca é destruído; não crie um por thread.
- **Adaptador de software é lento por construção.** No Windows do CI (WARP) a região é limitada a
  8 bits; com 12 bits a partida passou de 15 minutos. Serve para testes, não para uso.
- **O recuo do `t_HYBRID` é de mão única.** Se a GPU fica sem memória no meio da execução, o
  estado vai para o host e não volta para a GPU sozinho.
- **Sem memória fixada (pinned).** O wgpu não tem o equivalente do `cudaHostRegister`; a
  transferência host↔GPU fica em ~2,1 GB/s na A4500. Por isso o híbrido do Rust não divide o estado
  entre CPU e GPU.
- **`BLOCK_SIZE`, `REPEAT_COUNT` e `GPUS` são lidos e ignorados** pelos executáveis (não existem
  no wgpu). `GPU_REGION_BITS` vale, e o contexto o reduz sozinho se o adaptador oferece menos
  memória de workgroup.
- **Limites de tamanho:** 32 qubits na CPU e 31 na GPU (índices de 32 bits no WGSL). Acima de 27
  qubits na A4500 o estado é dividido em blocos de buffer.
- **A fusão de fase é padrão só na GPU e no híbrido.** Na CPU ela foi medida mais lenta (a porta
  fundida percorre os termos por amplitude); `HGM_FUSE_PHASE=0/1` força.
- **A semente padrão é `tempo ^ pid`** (como no C++), então duas execuções diferem; `HGM_SEED` a
  fixa. Medir um qubit cujo ramo sorteado tem probabilidade zero (só por arredondamento, em estados
  quase determinísticos) mede o outro ramo, em vez do NaN do C++.
- **A porta fundida de fase não é uma matriz.** `Op::FusedPhase` não tem `Pt::kind()`; só os
  backends sabem executá-la. Quem escrever um backend novo precisa tratar as duas variantes de
  `Op`.

### Desvios deliberados do C++

Onde o C++ tinha `exit`, ponteiro nulo ou comportamento indefinido, o Rust devolve `Result` com o
caso de erro nomeado (`HgmError`): porta desconhecida, passos com números de qubits diferentes,
falta de memória (do host ou da GPU), inverso modular inexistente, região que não absorve nenhuma
porta, `search_value` fora do espaço de busca. Argumento inválido nos executáveis sai com código 2
(uso) ou 1 (erro de execução), e não 0. A lista completa está em [12](12-versao-rust.md), seção 3, e
nos cabeçalhos de cada módulo.

## 2. Os 31 itens do C++ e o que aconteceu no Rust

Legenda da coluna Rust: **não existe** = o problema não tem como ocorrer em Rust (ownership, tipos,
ou código que não foi portado); **evitado** = o Rust foi escrito já sem o defeito; **portado** =
a correção ou otimização está no Rust, com teste; **aberto** = continua valendo.

| # | Situação no C++ | O que era | No Rust |
|---|---|---|---|
| 1 | corrigido | `genRot` calculava o nome da porta depois de consumir os bits e sempre gerava `Rot_0`; como `addGate` não sobrescreve, só a primeira correção de fase do Shor valia | **evitado**: o nome é `Rot_<bits>` (`gen_rot`), com teste; `add_gate` continua sem sobrescrever, de propósito |
| 2 | removido | `CpuExecution2_*`, `CpuExecution3_*` e `CU()`: código morto | **não existe**: só a família que valia foi portada |
| 3, 3.1 | corrigido | `PT::destructor()` com as condições de `free()` invertidas; `genPTs` com `malloc` sem inicializar | **não existe**: `Pt` é um valor com `Vec`, liberado por RAII |
| 4 | removido | `GenericExecute` e `GpuExecution`/`2`/`3` (declaração sem implementação): código morto | **não existe** |
| 5 | corrigido | `Gates::list` era `static`, compartilhado entre execuções | **evitado**: o `GateCatalog` é do `Simulator` (uma instância por execução) |
| 6 | corrigido | segfault em `t_PAR_CPU`/`t_HYBRID` com `region_bits > qubits` (deslocamento por expoente negativo) | **portado**: `Tuning::validated` faz o clamp e `compute_region` devolve erro se nenhuma porta cabe (o C++ ficava em laço infinito) |
| 7 | não reproduzido | build de `kernel.cu` anormalmente lento com `nvcc` numa máquina sem GPU | **não existe**: sem `nvcc`. Há um análogo: o custo de partida em adaptador de software (WARP), tratado limitando a região a 8 bits |
| 8 | corrigido | `grover.cpp`/`shor.cpp` ignoravam `exec_type` ao chamar o algoritmo | **evitado**: `hgm-cli` converte o tipo em `BackendKind` e o passa pelo `SimConfig` |
| 9 | corrigido | `makefile` mirava em `sm_52`, que o CUDA 13 não compila | **não existe**: sem CUDA nem `makefile` |
| 10 | corrigido | os templates de `kernel.cu` viraram parâmetros de runtime | **não existe** como problema: o kernel WGSL é gerado com o tamanho de região e de workgroup como constantes ([04](04-gpu-wgpu.md)) |
| 11 | removido | segunda leva de código morto (`enablePeerAccess`, macros, `t_SPEC`, ...) | **não existe** |
| 12 | corrigido | `Grover()` media o resultado e o descartava; `grover.out` nunca dizia se achou | **evitado**: `grover()` devolve o valor medido; 40 execuções com sementes fixas conferidas contra o C++ |
| 13 | arquivado | multi-GPU (`gpu_count > 1`): nunca testado em hardware real; fórmula de `is_peer` suspeita | **não portado**: um dispositivo por processo; `GPUS` é ignorado |
| 14 | removido | `ApplyQFT`/`QFT2`: código morto que o comentário dizia estar em uso | **não existe** |
| 15 | corrigido | `Shor()` nunca liberava `dgm.state` e vazava o vetor inteiro | **não existe**: `StateVector` é liberado ao sair de escopo |
| 16 | corrigido | `srand(time(NULL))` nos executáveis: processos no mesmo segundo repetiam a aleatoriedade | **evitado**: `Mt19937` semeado com `time ^ pid`, ou com `HGM_SEED` |
| 17 | resolvido | `t_HYBRID` não batia CPU nem GPU de forma consistente; com o estado na GPU ele se reduz ao `t_GPU` | **portado em forma nova**: `HybridBackend` roda na GPU enquanto cabe e na CPU paralela quando não ([04](04-gpu-wgpu.md), seção 9) |
| 18 | corrigido | `measure()` chamava `srand(time(NULL))` a cada medição, destruindo a independência entre amostras | **não existe**: um único gerador por `Simulator`, compartilhado por medições e sorteios |
| 19 | corrigido | `Gates::~Gates()` vazio: cada `DGM` vazava o cache de matrizes | **não existe** |
| 20 | corrigido | `genRot()` podia devolver `""` e corromper `DGM::qubits` no meio do Shor | **evitado**: `gen_rot` devolve um passo só de `ID` se a rotação é 1 |
| 21 | corrigido | `measure()`/`colapse()` somavam probabilidades em `float`; `Grover()`/`Shor()` usavam `1<<` em `int` | **evitado**: somas em `f64`, índices em `usize`/`u64` |
| 22 | corrigido | tuning padrão da GPU lento (64/2/8), difusor do Grover com passadas de sobra, `t_GPU` abortando com menos de 8 qubits | **portado**: padrões região 12 / coalescida 4, difusor com `controlled_z`, e `region_shape` limita a região ao tamanho do estado |
| 23 | corrigido | os laços de CPU de `t_PAR_CPU`/`t_HYBRID` não vetorizavam (~9 ciclos por par) | **portado**: `kernels.rs` com laços sobre fatias e despacho de AVX2 em `simd.rs` ([03](03-motor-de-execucao-cpu.md)) |
| 24 | corrigido | passadas da GPU no Shor: portas que comutam não eram agrupadas | **portado**: `schedule_for_batching`, aplicado pelo `GpuBackend` |
| 25 | corrigido | Grover guardava centenas de milhares de `PT` idênticos | **portado**: `PT_BLOCK_LIMIT` de 20000 portas, com bloco repetido por `run_program` |
| 26 | corrigido | ~85% das portas do Shor eram rotações da QFT: QFT aproximada | **portado**: `qft_degree` (argumento, não global) e `SHOR_AQFT` |
| 27 | corrigido | portas de fase do Shor viravam uma passada de kernel cada: fusão na GPU | **portado**: `Op::FusedPhase` / `fuse_phase_runs`, padrão na GPU e no híbrido |
| 28 | corrigido | o Shor copiava o estado host↔GPU e media no host a cada rodada: sessão de GPU | **portado**: o `GpuBackend` residente é o caminho do `Simulator`, sem opt-in |
| 29 | corrigido | multiplicação modular emulada como permutação: `SHOR_EMULATE=1` | **portado**: `apply_mod_mult` na CPU e `mod_mult_kernel` na GPU |
| 30 | corrigido | `CRMultMod` recebia `a` e não `a^-1`, deixando lixo em `reg2` | **evitado**: o Rust recebe o inverso; o teste `circuit_and_emulation_measure_the_same_bits_for_every_base` o pega |
| 31 | **aberto** | a correção de fase da estimação semiclássica entra depois do último `H` e não faz nada | **aberto**: reproduzido de propósito para manter a paridade com o `v1.1.0`; ver [06](06-algoritmo-shor.md), seção 6 |

Itens do C++ que não aparecem no Rust, mas explicam decisões que o Rust herdou: o 5 e o 1 (por que
o nome da porta determina a matriz), o 6 (por que existe `Tuning::validated`), o 17 (por que o
híbrido é "GPU se cabe, senão CPU"), o 22 e o 24 (por que os padrões de GPU são os que são) e o 26 e o
27 (por que o Shor é fundido e aproximado).

## 3. Item 31 em detalhe (aberto): a correção de fase do Shor

`shor_detailed`/`run_rounds` ([`shor.rs`](../crates/hgm-algos/src/shor.rs); no C++, `Shor()` em
`lib_shor.cpp`) monta cada rodada como `H, (multiplicação modular), H, gen_rot(bits já medidos)` e
mede o qubit de controle em seguida. `gen_rot` devolve uma porta diagonal
`diag(1, e^{-2*pi*i*0.b})`. Uma porta diagonal aplicada imediatamente antes de uma medição na base
computacional não muda nenhuma probabilidade, e o qubit colapsa logo depois: a correção é uma
operação nula. Na estimação de fase semiclássica a rotação tem que vir **antes** do segundo `H`.

**Como foi percebido (2026-11-21, durante a porta do Shor para Rust):** quatro mutações propositais
no Rust sobreviveram à bateria inteira, inclusive à comparação bit a bit com o C++
(`tests/golden/data/runs.txt`): remover a correção (`!= 0` virando `> 1`) no caminho emulado e no de
circuito, e trocar `a^(2^k)` por `a^-(2^k)` na emulação. Todas são equivalentes se a correção não
faz nada: sem ela as marginais de cada bit medido são simétricas em `phi -> -phi`.

**Efeito medido (fraco, não conclusivo):** no Rust, com a rotação antes do último `H`, 60 sementes
por caso deram 52/60 contra 51/60 (N=21, a=2), 36/60 contra 30/60 (N=21, a=5), 55/60 contra 45/60
(N=35, a=2) e 41/60 nos dois (N=15, a=7), na emulação. É a direção esperada, mas são poucas amostras
e um único experimento, e a convenção de sinal da rotação não foi conferida contra um oráculo
independente. Para N=15 a correção é irrelevante: os períodos 2 e 4 dividem `2^(2w)`, e os picos de
fase são exatos com ou sem ela.

**Não foi corrigido:** a migração para Rust exige paridade com o `v1.1.0`, e o Rust reproduz este
comportamento de propósito. Se for corrigir, é uma mudança de algoritmo (os vetores de `runs.txt`
mudam, e `tests/golden/regenerar.sh` precisa de um gerador novo, já que o C++ não está mais na
`main`), e a métrica certa é a fase medida (a até `1/2^(2w)` de um múltiplo de `1/r`), não a taxa de
"Found factors".

---

*Histórico: a lista original nasceu da leitura da documentação em 2026-08-06 e foi condensada em
2026-08-11; o texto completo dos itens 1 a 30 está no tag `v1.1.0`. Esta versão foi reescrita para o Rust
na 2.0.1. Atualize as duas partes conforme pontos novos forem encontrados ou os existentes forem
corrigidos.*
