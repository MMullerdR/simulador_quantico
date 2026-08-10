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
([common.cpp:92](../src/core/common.cpp#L92)), que ajusta as máscaras
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

## 4. `GpuExecutionWrapper` / `GEWrapper2` — por que tantos `switch`

CUDA templates precisam que os parâmetros (`t_block_size`, `t_repeat_count`,
`t_coalesced_bits`) sejam conhecidos em **tempo de compilação**. Como os
valores reais (`block_size`, `repeat_count`, `coalesced_bits`) só são
conhecidos em tempo de execução (vêm de linha de comando/tuning), o código
faz uma cascata de `switch` (`GEWrapper2`,
[kernel.cu:283](../src/core/kernel.cu#L283), e `GpuExecutionWrapper`,
[kernel.cu:302](../src/core/kernel.cu#L302)) que, para cada combinação
suportada de valores, chama a instanciação certa do template. É verboso,
mas é a forma padrão de "despachar" para templates CUDA a partir de
parâmetros dinâmicos.

Hoje esse `switch` cobre **uma única combinação** (`coalesced_bits=4,
block_size=64, repeat_count=2` — o combo usado por padrão em
`lib_grover.cpp`/`lib_shor.cpp`/`lib_general.cpp`), reduzida de ~260
instanciações como diagnóstico para a lentidão de build do `nvcc` nesta
máquina (ver [07-bugs-e-pontos-de-atencao.md](07-bugs-e-pontos-de-atencao.md),
item 7). Restaurar o conjunto completo é só devolver os `case`s removidos,
seguindo o mesmo padrão dos que restaram.

## 5. Múltiplas GPUs

Quando `gpu_count > 1`, o vetor de estado é dividido em fatias iguais, uma
por GPU (`gpu_slice_size = mem_size/gpu_count`), cada fatia copiada para o
`cudaMalloc` daquele device. `cudaDeviceEnablePeerAccess` permite que uma
GPU acesse diretamente a memória da outra (DMA peer-to-peer) quando uma
porta precisa combinar amplitudes que caíram em GPUs diferentes (`is_peer`,
calculado a partir de `region_start_bit` e do número de GPUs) — nesse caso,
todas as GPUs são sincronizadas antes de rodar o kernel para evitar
condição de corrida entre elas.

## 6. `ProjectState` / `GetState` — a ponte para o modo híbrido

[kernel.cu:315](../src/core/kernel.cu#L315) e
[kernel.cu:356](../src/core/kernel.cu#L356). Copiam **apenas uma
região** (um subconjunto de índices que compartilham os bits fora de
`region_mask` iguais a `region_id`) entre a memória do host e a GPU, em vez
do vetor inteiro. É isso que permite ao `HybridExecution` (ver
[03-motor-de-execucao-cpu.md](03-motor-de-execucao-cpu.md), seção 5, e o
código em [dgm_par_exec.cpp:229](../src/core/dgm_par_exec.cpp#L229)) mandar
só a fatia do estado que a GPU vai processar naquele momento, enquanto o
resto do estado continua sendo processado pelas threads de CPU em paralelo.
