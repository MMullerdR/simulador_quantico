# Execução em GPU (wgpu + WGSL) — `hgm-gpu`

A GPU implementa a mesma matemática do [motor de CPU](03-motor-de-execucao-cpu.md) (pares
`(pos0, pos1)`, matriz 2x2), mas com uma otimização extra: em vez de processar **uma porta por
vez** relendo o vetor de estado inteiro da memória global a cada porta, ela copia um pedaço do
vetor para a memória de workgroup (*workgroup memory*, muito mais rápida) e aplica **várias
portas seguidas** ali, só voltando para a memória global no final. Isso é o que o projeto chama
de "coalescimento".

O código está em [`crates/hgm-gpu`](../crates/hgm-gpu): `plan.rs` (planejamento das passadas, sem
tocar na GPU), `shaders.rs` (os kernels em WGSL), `context.rs` (dispositivo, pipelines,
execução, leitura, medição) e `backend.rs` (`GpuBackend`, a trait `Backend` do núcleo). O
C++ original usava CUDA (`kernel.cu`, no tag `v1.1.0`); o Rust usa [wgpu](https://wgpu.rs), que
roda em Vulkan, Metal e DX12, sem `nvcc`.

## 1. Por que "coalescer"

Memória global da GPU é lenta comparada à memória de workgroup. Se um circuito tem 20 portas
seguidas que atuam todas nos primeiros `k` qubits (os bits menos significativos do índice), cada
uma dessas portas só mistura amplitudes *dentro* de blocos de tamanho `2^k` — então dá pra copiar
um bloco de `2^k` amplitudes pra memória de workgroup **uma vez**, aplicar as 20 portas ali dentro
(rapidíssimo), e escrever de volta **uma vez**. Isso evita 19 idas e vindas desnecessárias à
memória global.

`coalesced_bits` (`GpuTuning::coalesced_bits`, `RunTuning::coalesced_bits`) é justamente
**quantos bits menos significativos** do índice formam esse bloco "sempre presente" na memória
de workgroup, o que também faz a leitura da memória global ser contígua.

## 2. O kernel de região (`region_kernel`)

[`shaders.rs`](../crates/hgm-gpu/src/shaders.rs). O texto WGSL é **gerado com os parâmetros que são
constantes de compilação** (tamanho do workgroup e da região), para o compilador do driver poder
desenrolar os laços (WGSL não tem `#pragma unroll`). O estado é um `array<vec2<f32>>` (parte real
em `.x`, imaginária em `.y`, o mesmo layout do `Complex32`). Cada workgroup cuida de uma região de
`2^k` amplitudes (`k = c + w`: `c` bits baixos coalescidos mais uma **janela** contígua de `w`
bits a partir de `s`). Passos:

1. **Copiar para a memória de workgroup**: as threads leem a região do `state` global para o
   array `var<workgroup> tile`. O endereço global de cada posição local combina o número do
   workgroup (qual região) com o índice local (`g = origin | (j & low_mask) | ((j >> c) << s)`).
2. **Aplicar as portas**: um laço percorre as portas da passada (`ops[]`, um buffer de
   armazenamento só de leitura), aplicando a mesma fórmula `lo' = m00*a + m01*b; hi' = m10*a +
   m11*b` em `tile`, não no estado global. Há um `workgroupBarrier()` entre portas para garantir
   que toda a memória de workgroup esteja consistente antes da porta seguinte usar os resultados
   da anterior. Uma operação pode ser uma porta normal ou uma porta de fase fundida (seção 5).
3. **Copiar de volta**: mesmo padrão de endereço do passo 1, escrevendo `tile` de volta no
   `state`.

A estrutura `Op` do WGSL (alvo, máscara e valor de controle, máscara/valor de controle "de fora",
matriz) é o equivalente, do lado da GPU, do `Pt` do lado da CPU. `GpuGate::from_pt` converte um
`Pt` e **recusa** o que índices de 32 bits não representam (alvo ou controle acima do bit 31).

Os controles que caem **dentro** da região viram máscara local (conferida por par de
amplitudes); os que caem **fora** viram uma condição sobre o endereço-base da região, avaliada
**uma vez por workgroup** (`outer_mask`/`outer_value` contra `gorigin`): nas regiões que não
casam, a porta inteira é pulada. Como a condição é a mesma para o workgroup todo, pular a porta é
uniforme e a barreira seguinte continua válida.

## 3. Como as portas são agrupadas em passadas (`plan.rs`)

Antes de chamar um kernel, o programa é cortado em uma sequência de **passadas**
(`Step`), sem tocar na GPU (por isso o planejamento é testável em qualquer máquina, e há um
modelo em CPU dos kernels em `plan.rs` para conferir):

- `Step::Region(RegionPass)`: uma passada de região, que agrupa o máximo de portas consecutivas
  cujos **alvos** cabem na mesma região (só o alvo precisa caber, como no `GpuExecution01` do
  CUDA). Quando o alvo da próxima porta não cabe na passada aberta, ela é fechada e começa outra.
  A janela de `w` bits começa no menor bit usado, recuando se passaria do fim do bloco.
- `Step::Global(GpuGate)`: sem janela (`w = 0`), uma porta com alvo acima dos bits coalescidos
  vira uma passada **global**: uma porta por dispatch, direto na memória global, um par de
  amplitudes por thread (`global_kernel`). É o plano B para portas que não cabem numa região.
- `Step::Cross(GpuGate)`: uma porta com o alvo nos bits que escolhem o bloco do estado (seção 7).

Um `RegionPass` guarda `coalesced_bits`, `window_bits`, `window_start` e as operações já
reindexadas para a região (o bit global `b` vira `b` se `b < c`, senão `b - s + c`).

O número de workgroups de uma passada de região é `2^(n-k)`; passadas de estados grandes são
divididas em vários dispatches (`dispatch_size`) para respeitar o limite por dimensão.

## 4. Tuning, lotes e o agendamento por comutação

`GpuTuning` ([`hgm-engine`](../crates/hgm-engine/src/lib.rs)) reúne:

| Campo | Padrão | O que faz |
|---|---|---|
| `region_bits` | 12 | log2 de amplitudes de uma região na memória de workgroup (32 KiB); o contexto reduz sozinho se o adaptador oferece menos |
| `coalesced_bits` | 4 | bits baixos contíguos da região |
| `batch` | 300 | portas por lote do agendador por comutação (o `OPS_BLOCK` do C++); 0 desliga a reordenação |
| `max_chunk_bits` | sem limite | maior bloco do estado (seção 7); só serve para testar a divisão em estados pequenos |

O workgroup tem 256 threads (`GpuOptions::workgroup_size`). **O C++ tinha mais parâmetros que
não existem aqui:** `block_size` e `repeat_count` (threads por bloco CUDA e posições por thread)
e a invariante `2*block_size*repeat_count == 2^gpu_region_bits` que `validateTuning` precisava
impor; no wgpu o tamanho do workgroup é uma constante de compilação do kernel gerado, então não
há combinação inconsistente para recusar. Os executáveis ainda **leem** `BLOCK_SIZE`,
`REPEAT_COUNT` e `GPUS` na linha de comando, mas os ignoram; `GPU_REGION_BITS` vale
([12](12-versao-rust.md)). Os padrões de região 12 e coalescida 4 vêm do melhor combo medido na
RTX A4500 (item 22 de [07](07-bugs-e-pontos-de-atencao.md)).

Antes de planejar as passadas, `GpuBackend` reordena as portas que comutam com
`schedule_for_batching` ([`optimize.rs`](../crates/hgm-core/src/optimize.rs)) para caberem mais
portas por passada (item 24 de [07](07-bugs-e-pontos-de-atencao.md)). Duas operações comutam se
nenhum qubit compartilhado é alvo **não diagonal** de uma delas; monta-se o grafo de dependências
(por qubit: a última porta "N" e as "Z" desde então) e se esvazia uma lista de prontas em ordem
original, enchendo um lote por vez com o critério de janela de `GpuExecution01`: alvo abaixo de
`coalesced_bits` entra sempre; os demais precisam caber numa janela de bits consecutivos; no
máximo `batch` por lote. Programas com menos de 3 operações, ou `batch = 0`, ficam como estão. O
resultado não muda (a menos de arredondamento).

## 5. Portas de fase fundidas (`Op::FusedPhase` / `GpuOp::Fused`)

No Shor, ~85% das portas são rotações controladas da QFT, todas diagonais. O otimizador
`fuse_phase_runs` ([`optimize.rs`](../crates/hgm-core/src/optimize.rs)) troca cada corrida de portas
de fase consecutivas por **uma única `Op::FusedPhase`** com uma lista de termos
([`fused.rs`](../crates/hgm-core/src/fused.rs)): `PhaseTerm { mask, value, steps }`. A amplitude cujo
índice bate com `(i & mask) == value` ganha `steps` unidades de fase, em passos de 2π/65536:

```text
amplitude[i] *= e^{ i·2π·soma(steps dos termos com (i & mask) == value) / 2^16 }
```

Por isso só entram portas `diag(1, e^{iθ})` cujo ângulo cai a menos de 0,02 passo de um múltiplo
de 2π/65536; as outras ficam como portas normais. Uma porta fundida carrega até 64 termos
(`MAX_TERMS`), e termos cuja soma de passos é nula somem. A soma é de **inteiros** (exata, módulo
2^16) e a exponencial vem de uma tabela de 65536 entradas, calculada uma vez em `f64` e
arredondada para `f32` (`phase_table`). A fusão é ligada por padrão na GPU e no híbrido, e
desligada na CPU, onde a porta fundida percorre os termos por amplitude e foi medida mais lenta
que as portas separadas (`HGM_FUSE_PHASE=0/1` força).

No kernel de região, a operação fundida separa os bits de cada termo em "fora da região" (conferidos
**uma vez por workgroup** contra o endereço-base e, se casam, empacotados numa lista compacta
`active_terms` em memória de workgroup, com máscara e valor locais de 16 bits cada) e "dentro"
(reindexados para a região). Cada thread então soma os passos de **4 amplitudes por leitura de
termo** (a leitura do termo na memória de workgroup é o que mais pesa) e multiplica cada amplitude
pelo fator vindo da tabela de fases, em memória global. Isso ganhou de 5% a 25% no Shor
(27 qubits: 51,7 s contra 68,6 s) e virou o padrão na fase 7 ([09](09-plano-migracao-rust.md)).
Histórico, medidas e a alternativa descartada: item 27 de
[07](07-bugs-e-pontos-de-atencao.md).

## 6. Estado residente na GPU

`GpuBackend` tem dois modos ([`backend.rs`](../crates/hgm-gpu/src/backend.rs)):

- **ida e volta** (padrão de `GpuBackend::new`): cada `execute` sobe o estado do host, roda as
  portas e traz o estado de volta, como o `t_GPU` do `v1.1.0`. Seguro para quem chama
  `Backend::execute` direto com estados quaisquer.
- **residente** (`.resident(true)`, o que o `Simulator` usa): o estado sobe uma vez (ou nasce na
  GPU, em `initial_state_ready`/`set_basis`, sem alocar nem subir um vetor de `2^n` no host) e
  **fica na GPU**; execução, medição, colapso e multiplicação modular rodam lá. O vetor só volta ao
  host quando alguém o pede inteiro (`sync_to_host`, que `Simulator::state()` e `probabilities`
  chamam). O contrato é que o `StateVector` do host seja sempre o mesmo, trocado só pelos avisos
  `host_state_replaced` e `set_basis`.

No C++ isso era a "sessão de GPU" (`beginGpuSession`), que o Shor abria para manter o estado na
placa entre as `2*largura` rodadas (item 28 de [07](07-bugs-e-pontos-de-atencao.md)); no Rust vale
para todo uso do `Simulator` com a GPU, sem opt-in e sem `GPU_RESIDENT`.

### Medição e colapso

- **Medir um qubit** (`measure_kernel`): cada workgroup soma `|a|²` dos índices com o bit em 0 e
  em 1, em `f32`, com soma em árvore; o fechamento (soma dos parciais) é em **`f64` no host**, já
  que WGSL não tem `f64` portátil. O sorteio e a escolha do ramo são os do host (`pick_branch`
  de `hgm-core`): o mesmo gerador, gasto do mesmo jeito que na CPU.
- **Colapsar** (`collapse_kernel`, e `collapse_pair_kernel` quando o bit escolhe o bloco):
  `CollapseMode::Measure` é a medição do C++ (o ramo sorteado, escalado por `1/norma`, vai para
  a posição do bit em 0 e a do bit em 1 é zerada: depois de medir 1 o qubit passa a valer 0);
  `CollapseMode::InPlace` é o `colapse` do C++ (o ramo fica onde está).

### Multiplicação modular

`mod_mult_kernel` aplica a permutação de `apply_mod_mult` (Shor com `SHOR_EMULATE=1`, item 29 de
[07](07-bugs-e-pontos-de-atencao.md)) em forma de *gather*: cada thread escreve uma posição de
destino e lê a origem de uma tabela. Precisa de um buffer de destino separado (a origem de uma
thread é o destino de outra), alocado na primeira chamada; os dois trocam de lugar. Se o registrador
e o bit de controle cabem nos bits locais do bloco, basta um bloco auxiliar reaproveitado; se
cruzam o limite do bloco, é preciso um conjunto inteiro de blocos auxiliares (o dobro da memória do
estado), e a falta de memória vira `DeviceOutOfMemory`.

## 7. Estados grandes: blocos

Um buffer de armazenamento tem tamanho máximo (`max_storage_buffer_binding_size`: 27 qubits na
RTX A4500). Acima disso o estado é dividido em `2^(n-L)` **blocos** de `2^L` amplitudes, um buffer
por bloco, em ordem de índice (`GpuState::chunk_bits`). Cada kernel roda sobre **um** bloco, com os
índices locais a ele, e `params.chunk` diz qual bloco é, para as contas que dependem do índice
global (controles e termos de fase nos bits de bloco, `modmult`). Uma passada de região nunca
cruza blocos (a janela fica nos `L` bits baixos e roda uma vez por bloco). Uma porta com o alvo nos
bits de bloco vira um `Step::Cross`: o `cross_kernel` lê um par de blocos por dispatch, um par de
amplitudes por thread (os controles nos bits de bloco são conferidos no host, que pula os pares de
blocos que não casam).

O limite geral é `MAX_GPU_QUBITS = 31` (índices de 32 bits), se a memória da placa aguenta; sem
memória, o `t_GPU` falha com `DeviceOutOfMemory` e o `t_HYBRID` recua para a CPU (seção 9).
Estados de 28 a 31 qubits foram conferidos contra a CPU na RTX A4500 ([12](12-versao-rust.md)).

## 8. Um dispositivo por processo, adaptadores e limites do WGSL

- **Um `GpuContext` por processo** (`shared_context`, num estático que nunca é destruído): vários
  dispositivos destruídos ao mesmo tempo travaram ou derrubaram o processo na A4500
  (`examples/stress.rs`). Uma falha ao abrir (sem adaptador) também fica guardada.
- **Adaptador:** `PowerPreference::HighPerformance`, com os limites do próprio adaptador (o padrão
  do wgpu é bem abaixo do que a GPU oferece). Não há multi-GPU: o `GPUS` do C++ é ignorado, e o
  caminho multi-GPU (item 13 de [07](07-bugs-e-pontos-de-atencao.md)) nunca foi exercitado nem
  portado.
- **Adaptador de software** (`DeviceType::Cpu`, como o WARP do Windows no CI): a região é limitada
  a 8 bits, porque o custo de partida cresce depressa com o tamanho da região (com 12 bits passou
  de 15 minutos). Funciona, mas é lento; serve para os testes, não para uso.
- **Limites do WGSL:** sem `f64` (por isso a medição fecha em `f64` no host e a tabela de fases é
  `f32`), índices `u32` (no máximo 31 qubits na GPU), memória de workgroup pelo limite do
  adaptador (a região de 12 bits usa 32 KiB).
- **Memória fixada:** o wgpu não oferece o equivalente do `cudaHostRegister`. Medido na A4500, a
  transferência entre host e GPU fica em ~2,1 GB/s, e é por isso que o híbrido não divide o
  estado entre CPU e GPU como o C++ fazia.

## 9. O modo híbrido (`t_HYBRID`)

[`hybrid.rs`](../crates/hgm-engine/src/hybrid.rs). No C++ (item 17 de
[07](07-bugs-e-pontos-de-atencao.md)) o `t_HYBRID` com o estado na GPU se reduzia ao `t_GPU`, e
a divisão do trabalho entre CPU e GPU só importava quando o estado **não cabia**. O `HybridBackend`
do Rust escolhe pelo tamanho (`HybridTuning::gpu_max_qubits`, padrão 31), pela existência de
adaptador e, durante a execução, pela falta de memória da GPU
([`HgmError::DeviceOutOfMemory`](../crates/hgm-core/src/error.rs)):

- cabe na GPU: tudo roda no backend de GPU residente (`HybridMode::Gpu`);
- não cabe, ou não há adaptador: `HybridMode::Cpu`, a CPU paralela;
- a GPU fica sem memória no meio da execução: `fall_back` traz o estado para o host
  (`sync_to_host`), descarta a GPU e segue na CPU. **O recuo é de mão única** (o estado não volta
  para a GPU sozinho).

Levar blocos do estado à GPU e trazê-los de volta só compensaria em lotes grandes de portas
(medição da fase 6, [09](09-plano-migracao-rust.md)), e o híbrido não faz isso. O nome do backend
(`Backend::name`) diz em que modo está e por quê.
