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

`coalesc` (parâmetro `gpu_coales`/`t_COALESC`) é justamente **quantos bits
menos significativos** do índice formam esse bloco "sempre presente" na
shared memory.

## 2. `ApplyValuesC01` — o kernel principal

[kernel.cu:60](../src/core/kernel.cu#L60), um kernel *template* em
`t_TAM_BLOCK` (threads por bloco CUDA), `t_REPT` (quantas posições cada
thread processa) e `t_COALESC` (bits coalescidos). Passos:

1. **Copiar para shared memory**: cada thread lê `t_REPT` pares de amplitudes
   da memória global (`gpu_pointer`, ponteiro(s) para a(s) GPU(s)) para o
   array `__shared__ cuFloatComplex s[...]`. O índice de leitura combina o
   `blockIdx` (qual bloco/região da memória) com a posição da thread dentro
   do bloco — mesmo espírito do "inserir um bit" visto na CPU, só que agora
   posicionando dentro de uma região maior (`b_pos`, `n_bits` = tamanho da
   região que este bloco de threads é responsável por processar).
2. **Aplicar as portas**: um laço `for (c = 0; c < count; c++)` percorre até
   `OPS_BLOCK` portas (passadas via `__constant__ DEV_OP op[OPS_BLOCK]`, cada
   uma com sua matriz e máscara/valor de controle), aplicando a mesma fórmula
   `tmp = m00*s[pos0] + m01*s[pos1]; s[pos1] = m10*s[pos0] + m11*s[pos1];
   s[pos0] = tmp;` — mas em `s[]` (shared memory), não em `state[]` global.
   Há um `__syncthreads()` entre portas para garantir que toda a shared
   memory do bloco esteja consistente antes da próxima porta usar os
   resultados da anterior.
3. **Copiar de volta**: mesmo padrão de índice do passo 1, mas escrevendo
   `s[...]` de volta em `gpu_pointer[...]`.

`DEV_OP` ([kernel.cu:18](../src/core/kernel.cu#L18)) é o equivalente,
do lado da GPU, da struct `PT` do lado da CPU: matriz 2x2 + os argumentos de
controle/deslocamento pré-calculados por `PT::setArgsGPU`
([common.cpp:86](../src/core/common.cpp#L86)), que ajusta as máscaras
de controle considerando que parte dos qubits está "dentro" da região
coalescida e parte "fora".

## 3. Como as portas são agrupadas em blocos (`GpuExecution01`)

[kernel.cu:124](../src/core/kernel.cu#L124). Antes de chamar o
kernel, o código CPU percorre a lista de `PT`s tentando juntar o máximo de
portas consecutivas que **cabem na mesma região de qubits** (região de
`qbs_region` bits, respeitando `t_COALESC`), monta o array `operators[]`
(até `OPS_BLOCK` portas), copia para a GPU via
`cudaMemcpyToSymbol(op, operators, ...)`, e só então dispara o kernel para
aquele lote. Isso se repete até esgotar a lista de `PT`s.

## 4. `GpuExecutionWrapper` / `GEWrapper2` — por que tantos `switch`

CUDA templates precisam que os parâmetros (`t_TAM_BLOCK`, `t_REPT`,
`t_COALESC`) sejam conhecidos em **tempo de compilação**. Como os valores
reais (`tam_block`, `rept`, `coalesc`) só são conhecidos em tempo de
execução (vêm de linha de comando/tuning), o código faz uma cascata de
`switch` ([kernel.cu:270](../src/core/kernel.cu#L270) e
[kernel.cu:394](../src/core/kernel.cu#L394)) que, para cada
combinação suportada de valores, chama a instanciação certa do template.
É verboso, mas é a forma padrão de "despachar" para templates CUDA a partir
de parâmetros dinâmicos.

## 5. Múltiplas GPUs

Quando `multi_gpu > 1`, o vetor de estado é dividido em fatias iguais, uma
por GPU (`mem_desloc = mem_size/multi_gpu`), cada fatia copiada para o
`cudaMalloc` daquele device. `enablePeerAccess`/`cudaDeviceEnablePeerAccess`
permite que uma GPU acesse diretamente a memória da outra (DMA
peer-to-peer) quando uma porta precisa combinar amplitudes que caíram em
GPUs diferentes (`is_peer`, calculado a partir de `region_start` e do número
de GPUs) — nesse caso, todas as GPUs são sincronizadas antes de rodar o
kernel para evitar condição de corrida entre elas.

## 6. `ProjectState` / `GetState` — a ponte para o modo híbrido

[kernel.cu:434](../src/core/kernel.cu#L434) e
[kernel.cu:475](../src/core/kernel.cu#L475). Copiam **apenas uma
região** (um subconjunto de índices que compartilham os bits fora de
`reg_mask` iguais a `reg_id`) entre a memória do host e a GPU, em vez do
vetor inteiro. É isso que permite ao `HybridExecution` (ver
[03-motor-de-execucao-cpu.md](03-motor-de-execucao-cpu.md), seção 5, e o
código em [dgm.cu:1002](../src/core/dgm.cu#L1002)) mandar só a fatia
do estado que a GPU vai processar naquele momento, enquanto o resto do
estado continua sendo processado pelas threads de CPU em paralelo.
