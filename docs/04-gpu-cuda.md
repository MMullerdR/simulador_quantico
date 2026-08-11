# Execução em GPU (CUDA) — `kernel.cu`

A GPU implementa a mesma matemática do [motor de CPU](03-motor-de-execucao-cpu.md)
(pares `(pos0, pos1)`, matriz 2x2), mas com uma otimização extra: em vez de
processar **uma porta por vez** relendo o vetor de estado inteiro da memória
global a cada porta, ela copia um pedaço do vetor para a memória
compartilhada (*shared memory*, muito mais rápida) e aplica **várias portas
seguidas** ali, só voltando para a memória global no final. Isso é o que o
projeto chama de "coalescimento".

## 1. Por que "coalescer"

Memória global da GPU é lenta comparada à shared memory. Se um circuito tem
20 portas seguidas que atuam todas nos primeiros `k` qubits (os bits menos
significativos do índice), cada uma dessas portas só mistura amplitudes
*dentro* de blocos de tamanho `2^k` — então dá pra copiar um bloco de `2^k`
amplitudes pra shared memory **uma vez**, aplicar as 20 portas ali dentro
(rapidíssimo), e escrever de volta **uma vez**. Isso evita 19 idas e vindas
desnecessárias à memória global.

`coalesced_bits` (campo `gpu_coalesced_bits` da `DGM`, parâmetro de template
`t_coalesced_bits` no kernel) é justamente **quantos bits menos
significativos** do índice formam esse bloco "sempre presente" na shared
memory.

## 2. `ApplyValuesC01` — o kernel principal

[kernel.cu:60](../src/core/kernel.cu#L60), um kernel *template* em
`t_block_size` (threads por bloco CUDA), `t_repeat_count` (quantas posições
cada thread processa) e `t_coalesced_bits` (bits coalescidos). Passos:

1. **Copiar para shared memory**: cada thread lê `t_repeat_count` pares de
   amplitudes da memória global (`gpu_pointer`, ponteiro(s) para a(s)
   GPU(s)) para o array `__shared__ cuFloatComplex shared_amplitudes[...]`.
   O índice de leitura (`local_pos`) combina o `blockIdx` (qual bloco/região
   da memória) com a posição da thread dentro do bloco — mesmo espírito do
   "inserir um bit" visto na CPU (`OPEN_SPACE`), só que agora posicionando
   dentro de uma região maior (`region_start_bit`, `extra_region_bits` =
   tamanho da região que este bloco de threads é responsável por processar).
2. **Aplicar as portas**: um laço `for (op_index = 0; op_index < op_count;
   op_index++)` percorre até `OPS_BLOCK` portas (passadas via `__constant__
   DEV_OP op[OPS_BLOCK]`, cada uma com sua matriz e máscara/valor de
   controle), aplicando a mesma fórmula `tmp = m00*shared_amplitudes[pos0] +
   m01*shared_amplitudes[pos1]; shared_amplitudes[pos1] =
   m10*shared_amplitudes[pos0] + m11*shared_amplitudes[pos1];
   shared_amplitudes[pos0] = tmp;` — mas em `shared_amplitudes[]` (shared
   memory), não em `state[]` global. Há um `__syncthreads()` entre portas
   para garantir que toda a shared memory do bloco esteja consistente antes
   da próxima porta usar os resultados da anterior.
3. **Copiar de volta**: mesmo padrão de índice do passo 1, mas escrevendo
   `shared_amplitudes[...]` de volta em `gpu_pointer[...]`.

`DEV_OP` ([kernel.cu:18](../src/core/kernel.cu#L18)) é o equivalente,
do lado da GPU, da struct `PT` do lado da CPU: matriz 2x2 + os argumentos de
controle/deslocamento pré-calculados por `PT::setArgsGPU`
([common.cpp:94](../src/core/common.cpp#L94)), que ajusta as máscaras
de controle considerando que parte dos qubits está "dentro" da região
coalescida e parte "fora".

## 3. Como as portas são agrupadas em blocos (`GpuExecution01`)

[kernel.cu:124](../src/core/kernel.cu#L124). Antes de chamar o
kernel, o código CPU percorre a lista de `PT`s tentando juntar o máximo de
portas consecutivas que **cabem na mesma região de qubits** (região de
`gpu_region_bits` bits — chamada de `block_region_size` dentro da função —,
respeitando `t_coalesced_bits`), monta o array `operators[]` (até
`OPS_BLOCK` portas), copia para a GPU via `cudaMemcpyToSymbol(op, operators,
...)`, e só então dispara o kernel para aquele lote. Isso se repete até
esgotar a lista de `PT`s.

## 4. `GpuExecutionWrapper` — dispatch em runtime, sem `switch`

**Histórico (até 2026-08-11):** `ApplyValuesC01`/`GpuExecution01` eram
`template <int t_block_size, int t_repeat_count, int t_coalesced_bits>`.
CUDA templates precisam que esses parâmetros sejam conhecidos em **tempo
de compilação**, mas os valores reais só são conhecidos em tempo de
execução (vêm da configuração de tuning) — então o código fazia uma
cascata de `switch` (`GEWrapper2` + `GpuExecutionWrapper`) que, para cada
combinação suportada, despachava pra instanciação certa do template.
Cobrir todas as combinações plausíveis de `block_size`/`repeat_count`/
`coalesced_bits` gerava ~260 instanciações do kernel — build que chegou a
travar por horas numa máquina sem GPU (ver
[07-bugs-e-pontos-de-atencao.md](07-bugs-e-pontos-de-atencao.md), item 7).

**Desde 2026-08-11 (item 06 do design de arquitetura, implementado com GPU
NVIDIA real disponível pela primeira vez):** os templates foram removidos.
`block_size`/`repeat_count`/`coalesced_bits` viraram parâmetros comuns de
`ApplyValuesC01`/`GpuExecution01`, passados em runtime sem recompilar
nada. A shared memory do kernel, que dependia do tamanho ser conhecido em
tempo de compilação (`__shared__ cuFloatComplex shared_amplitudes[t_repeat_count*t_block_size*2]`),
virou dinâmica (`extern __shared__ cuFloatComplex shared_amplitudes[]`,
tamanho passado no terceiro argumento do `<<<grid,block,shared_mem_bytes>>>`
em `GpuExecution01`) — e como o *default* de shared memory dinâmica da
CUDA runtime é só 48KB, cada chamada registra o tamanho de verdade via
`cudaFuncSetAttribute(ApplyValuesC01, cudaFuncAttributeMaxDynamicSharedMemorySize, ...)`
antes do primeiro launch daquele device.

`GpuExecutionWrapper` (ponte `extern "C"`, assinatura fixa em
[dgm.h](../include/core/dgm.h)) hoje só repassa os parâmetros direto pra
`GpuExecution01` — nenhum `switch`, `GEWrapper2` foi removida.

**Invariantes que passaram a precisar de validação explícita** (todas em
`DGM::validateTuning()`, [dgm_core.cpp](../src/core/dgm_core.cpp), pra
`t_GPU`/`t_HYBRID`) — antes do item 06 nenhuma delas podia ser violada (só
existia uma combinação selecionável, já consistente por construção);
agora que qualquer combinação é aceita em runtime, uma inconsistente pode
corromper memória em vez de simplesmente falhar ao compilar:

- **`2*block_size*repeat_count == 2^gpu_region_bits`** — o endereçamento
  global do kernel (`OPEN_SPACE` com `region_start_bit`/`extra_region_bits`,
  derivados de `gpu_region_bits`) assume que cada bloco CUDA cobre
  exatamente `2^gpu_region_bits` amplitudes. Violar isso causa acesso
  ilegal de memória na GPU. Por fatoração única, essa igualdade já garante
  sozinha que `block_size`/`repeat_count` são potências de 2 positivas —
  não precisa de uma checagem separada pra isso (`rept_bits` em
  `GpuExecution01` vem de `log2(repeat_count)`, usado como expoente de um
  deslocamento de bit; só faz sentido pra potências de 2, e essa igualdade
  já exclui qualquer outro caso).
- **`gpu_region_bits >= gpu_coalesced_bits`** — `extra_region_bits =
  gpu_region_bits - gpu_coalesced_bits` precisa ser não-negativo.

### CLI: `block_size`/`repeat_count`/`gpu_region_bits` expostos (2026-08-11)

Antes fixos em `TuningDefaults` ([cli_common.h](../include/cli/cli_common.h)),
esses três agora são argumentos opcionais dos três CLIs
(`general.out`/`grover.out`/`shor.out <qubits> <exec_type> [threads|gpus]
[block_size] [repeat_count] [gpu_region_bits]` — só valem pra
`t_GPU`/`t_HYBRID`, ver [README.md](../README.md)). `gpu_coalesced_bits`
continua fixo (não exposto). As três invariantes acima são o que torna
essa exposição segura — sem elas, um usuário passando uma combinação
inválida por linha de comando teria o mesmo resultado do combo de teste
que motivou a validação em primeiro lugar (`illegal memory access`).

**Verificado com GPU real (RTX 4070, CUDA 13.3):** combo default
(`coalesced_bits=4, block_size=64, repeat_count=2, gpu_region_bits=8`)
continua reproduzindo a amplitude exata esperada; um combo **novo**, nunca
antes alcançável sem adicionar `case` e recompilar
(`block_size=128, repeat_count=2, gpu_region_bits=9`), também reproduziu a
amplitude correta; um combo inconsistente
(`coalesced_bits=6, block_size=128, repeat_count=4, gpu_region_bits=8`,
onde `2*128*4=1024 ≠ 2^8=256`) passou a falhar com a mensagem de
`validateTuning()` em vez de `illegal memory access`. `make test` completo
(66/66 + smoke test) e comparação de taxa de sucesso do `shor.out 15` em
`t_GPU` (6/8 numa rodada de 8) seguem dentro da variação probabilística já
documentada, sem regressão.

## 5. Múltiplas GPUs

Quando `gpu_count > 1`, o vetor de estado é dividido em fatias iguais, uma
por GPU (`gpu_slice_size = mem_size/gpu_count`), cada fatia copiada para o
`cudaMalloc` daquele device. `cudaDeviceEnablePeerAccess` permite que uma
GPU acesse diretamente a memória da outra (DMA peer-to-peer) quando uma
porta precisa combinar amplitudes que caíram em GPUs diferentes (`is_peer`,
calculado a partir de `region_start_bit` e do número de GPUs) — nesse caso,
todas as GPUs são sincronizadas antes de rodar o kernel para evitar
condição de corrida entre elas.

**Atenção:** este caminho (`gpu_count > 1`) nunca foi exercitado com
hardware real em nenhuma sessão deste projeto — só existiu acesso a 1 GPU
NVIDIA de cada vez. `gpu_count > 4` é validado e recusado (limite dos
arrays `gpu_mem`/`gpu_pointer` em `kernel.cu`), mas a fórmula de
`is_peer` é suspeita de estar incorreta pra `gpu_count > 2` (documentado,
não corrigido às cegas) — ver
[07-bugs-e-pontos-de-atencao.md](07-bugs-e-pontos-de-atencao.md) item 13.

## 6. `ProjectState` / `GetState` — a ponte para o modo híbrido

[kernel.cu:315](../src/core/kernel.cu#L315) e
[kernel.cu:356](../src/core/kernel.cu#L356). Copiam **apenas uma
região** (um subconjunto de índices que compartilham os bits fora de
`region_mask` iguais a `region_id`) entre a memória do host e a GPU, em vez
do vetor inteiro. É isso que permite ao `HybridExecution` (ver
[03-motor-de-execucao-cpu.md](03-motor-de-execucao-cpu.md), seção 5, e o
código em [dgm_par_exec.cpp:258](../src/core/dgm_par_exec.cpp#L258)) mandar
só a fatia do estado que a GPU vai processar naquele momento, enquanto o
resto do estado continua sendo processado pelas threads de CPU em paralelo.
