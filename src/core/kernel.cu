#include <cuComplex.h>
#include <stdio.h>
#include <stdlib.h>
#include <math.h>
#include <chrono>
#include "../../include/core/common.h"

#define M_RANGE 512
#define M_PREC 10000

// Confere cudaGetLastError() e aborta o processo se algo deu errado —
// chamada depois de toda operação CUDA relevante abaixo.
bool error();
static int error_check_count = 0;
static int call_count = 0;
static int call_peer_count = 0;

///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

// Equivalente, do lado da GPU, do struct PT do lado da CPU: matriz 2x2 +
// os argumentos de controle/deslocamento pré-calculados por
// PT::setArgsGPU (ver docs/04-gpu-cuda.md).
struct DEV_OP{
	long arg[TAM_ARG];
	cuFloatComplex matrix[4];
	int kind;		// OP_STD ou OP_FUSED
	int term_start;		// OP_FUSED: primeiro termo em d_terms
	int term_count;		// OP_FUSED: quantos termos
};

// OP_STD: porta 2x2 normal (matrix/arg). OP_FUSED: porta de fase fundida
// (item 27 em docs/07-bugs-e-pontos-de-atencao.md): ignora matrix/arg e
// multiplica cada amplitude do bloco por e^{i*fase(índice global)}.
#define OP_STD 0
#define OP_FUSED 1

// Um termo de fase fundida (ver PhaseTerm em common.h): soma "steps" ao
// ângulo quando o índice global bate com mask/value. O host já separa o
// termo em duas metades, pelo lote atual: os bits que ficam FORA da região
// do bloco (omask/oval, comparados uma vez por bloco com block_base) e os
// bits DENTRO da região, já reindexados pro índice da shared memory
// (lmask/lval, inteiros de 32 bits comparados por amplitude).
struct DEV_TERM{
	long omask;
	long oval;
	int lmask;
	int lval;
	int steps;
};
#define MAX_FUSED_TERMS (OPS_BLOCK * FUSED_MAX_TERMS)

// Inicializa a GPU escolhida (cudaFree(0) força a inicialização do
// contexto CUDA sem alocar nada de verdade).
extern "C" bool setDevice(int device_id = 0){
	return cudaFree(0);
}

__constant__ long c_arg[1][1];
__constant__ cuFloatComplex cmatrix[1][1];

__constant__ DEV_OP op[OPS_BLOCK];

// Termos das portas fundidas do lote atual e tabela e^{i*2*pi*k/2^16}, em
// memória global (ficam grandes demais pra memória constante, que também
// guarda op[]). A tabela é enviada uma vez por processo, no primeiro lote que
// tem porta fundida.
__device__ DEV_TERM d_terms[MAX_FUSED_TERMS];
__device__ cuFloatComplex d_phase_table[FUSED_PHASE_STEPS];
static bool phase_table_uploaded = false;

static cuFloatComplex *gpu_mem[4];
__constant__ cuFloatComplex *gpu_pointer[4];


// Insere "bit_count" bits 0 em "value" a partir do bit "from_bit" — usado
// em ApplyValuesC01 pra mapear a posição de uma thread dentro do bloco
// pro índice global do vetor de estado (mesmo espírito do truque de
// inserir bit usado no motor de CPU, ver docs/03-motor-de-execucao-cpu.md).
__device__ long OPEN_SPACE(long value, int from_bit, int bit_count){
	return ((value >> from_bit) << (from_bit + bit_count)) | (value & ((1 << from_bit) - 1));
}

///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

// Kernel principal: copia um bloco de amplitudes da memória global pra
// shared memory, aplica até op_count portas seguidas ali (evitando idas
// e vindas à memória global a cada porta — o "coalescimento" do
// projeto), e escreve o resultado de volta. Ver docs/04-gpu-cuda.md.
//
// block_size/repeat_count/coalesced_bits eram parâmetros de template
// (cada combinação exigia uma instanciação própria do kernel inteiro,
// ~260 no total — item 06 do design de arquitetura de 2026-08-10, ver
// docs/07-bugs-e-pontos-de-atencao.md item 7). Viraram parâmetros comuns
// de runtime; a shared memory, que antes precisava de
// t_repeat_count*t_block_size conhecido em tempo de compilação pra ter
// tamanho fixo, agora é alocada dinamicamente (extern __shared__, tamanho
// vem do terceiro argumento do <<<...>>> em GpuExecution01).
__global__ void ApplyValuesC01(int const block_size, int const repeat_count, int const coalesced_bits, int const region_start_bit, int const extra_region_bits, int const op_count, int const rept_bits, long const gpu_slice_size, int const block_offset){
	long local_pos, global_index0, global_index1, block = (blockIdx.x + block_offset);

	int rep_index, op_index, thread_id = threadIdx.x;

	extern __shared__ cuFloatComplex shared_amplitudes[];

	long block_base;

	block_base = block << coalesced_bits;
	block_base = OPEN_SPACE(block_base, region_start_bit, extra_region_bits);

	long pair_offset_mask = (1 << (region_start_bit+extra_region_bits-rept_bits-1));

	// copy amplitudes from global memory to shared memory
	for (rep_index = 0; rep_index < repeat_count; rep_index++){
		local_pos = thread_id + rep_index*block_size*2; //another start

		global_index0 = block_base | ((local_pos >> coalesced_bits) << region_start_bit) | (local_pos & ((1 << coalesced_bits)-1));
		global_index1 = global_index0 | pair_offset_mask;

		shared_amplitudes[local_pos] = gpu_pointer[global_index0/gpu_slice_size][global_index0%gpu_slice_size];
		shared_amplitudes[local_pos+block_size] = gpu_pointer[global_index1/gpu_slice_size][global_index1%gpu_slice_size];
	}

	int pos0, pos1, target_bit_mask;
	cuFloatComplex tmp;

	// compute the operators for the amplitudes on the shared memory
	for (op_index = 0; op_index < op_count; op_index++){
		__syncthreads();

		if (op[op_index].kind == OP_FUSED){
			// Cada amplitude do bloco é multiplicada por uma fase que só depende
			// do índice. Os termos vão pra shared memory uma vez por bloco; um
			// termo cujos bits de fora da região não batem com este bloco vira
			// "nunca" (lmask = ~0, lval = -1) em vez de um desvio por amplitude.
			__shared__ int fused_lmask[FUSED_MAX_TERMS];
			__shared__ int fused_lval[FUSED_MAX_TERMS];
			__shared__ int fused_steps[FUSED_MAX_TERMS];
			int term_start = op[op_index].term_start, term_count = op[op_index].term_count;
			for (int t = thread_id; t < term_count; t += block_size){
				DEV_TERM term = d_terms[term_start + t];
				bool active = ((block_base & term.omask) == term.oval);
				fused_lmask[t] = active ? term.lmask : ~0;
				fused_lval[t] = active ? term.lval : -1;
				fused_steps[t] = term.steps;
			}
			__syncthreads();

			int region_size = 2 * block_size * repeat_count;
			for (int p = thread_id; p < region_size; p += block_size){
				int steps = 0;
				for (int t = 0; t < term_count; t++)
					steps += ((p & fused_lmask[t]) == fused_lval[t]) ? fused_steps[t] : 0;
				shared_amplitudes[p] = cuCmulf(shared_amplitudes[p], d_phase_table[steps & (FUSED_PHASE_STEPS - 1)]);
			}
			continue;
		}

		target_bit_mask = 1 << op[op_index].arg[SHIFT];

		if (((block_base & op[op_index].arg[CTRL_MASK]) == op[op_index].arg[CTRL_VALUE])){
			for (rep_index = 0; rep_index < repeat_count; rep_index++){
				local_pos = thread_id + rep_index*block_size;

				pos0 = (local_pos * 2) - (local_pos & (target_bit_mask - 1));
				pos1 = pos0 | target_bit_mask;
				if ((pos0 & op[op_index].arg[CTRL_REG_MASK]) == op[op_index].arg[CTRL_REG_VALUE]){
					tmp = cuCaddf(cuCmulf(shared_amplitudes[pos0], op[op_index].matrix[0]), cuCmulf(shared_amplitudes[pos1], op[op_index].matrix[1]));
					shared_amplitudes[pos1] = cuCaddf(cuCmulf(shared_amplitudes[pos0], op[op_index].matrix[2]), cuCmulf(shared_amplitudes[pos1], op[op_index].matrix[3]));
					shared_amplitudes[pos0] = tmp;
				}
			}
		}
	}
	__syncthreads();

	// copy results from shared memory to global memory
	for (rep_index = 0; rep_index < repeat_count; rep_index++){
		local_pos = thread_id + rep_index*block_size*2; //another start

		global_index0 = block_base | ((local_pos >> coalesced_bits) << region_start_bit) | (local_pos & ((1 << coalesced_bits)-1));
		global_index1 = global_index0 | pair_offset_mask;

		gpu_pointer[global_index0/gpu_slice_size][global_index0%gpu_slice_size] = shared_amplitudes[local_pos];
		gpu_pointer[global_index1/gpu_slice_size][global_index1%gpu_slice_size] = shared_amplitudes[local_pos+block_size];
	}
}

//Kernel para execução com múltiplas GPUs se comunicando usando DMA (Direct Memory Access)
void GpuExecution01(float* state, PT **pts, int qubits, int gpu_region_bits, int gpu_count, int block_size, int repeat_count, int coalesced_bits, int iterations){
	DEV_OP operators[OPS_BLOCK];
	static DEV_TERM host_terms[MAX_FUSED_TERMS];	// static: ~460 KB, grande demais pra pilha

	error_check_count = 0;

	dim3 block, grid;

	long mem_size = pow(2.0, qubits);
	long gpu_slice_size = mem_size/gpu_count;

	int rept_bits = log2(repeat_count);

	long thread_total = mem_size/gpu_count/repeat_count/2;	// /2 porque cada thread fica responsável por duas posições & /2 pelas 2 GPUS

	long malloc_size = (mem_size * (sizeof(float complex)))/gpu_count;

	// tamanho da shared memory dinâmica de ApplyValuesC01 (ver comentário
	// acima da função) — precisa ser setado por device antes do primeiro
	// launch, via cudaFuncSetAttribute, porque o default da CUDA runtime
	// pra shared memory dinâmica (48KB) pode ser menor que isso dependendo
	// da combinação de block_size/repeat_count.
	size_t shared_mem_bytes = (size_t)repeat_count * block_size * 2 * sizeof(cuFloatComplex);

	block.x = block_size;
	(thread_total > block.x)? grid.x = thread_total/block.x : block.x = thread_total;

	int block_region_size = gpu_region_bits;

	if (block_region_size < gpu_region_bits){
		printf("ERRO: Região do bloco menor que a região de qubits\n");
		exit(1);
	}

	for (int device_index = 0; device_index < gpu_count; device_index++){
		cudaSetDevice(device_index);
		cudaFuncSetAttribute(ApplyValuesC01, cudaFuncAttributeMaxDynamicSharedMemorySize, (int) shared_mem_bytes); error();
	}

	if (gpu_count > 1){
		for (int device_index = 0; device_index < gpu_count; device_index++){
			cudaSetDevice(device_index);
			for (int peer_device_index = 0; peer_device_index < gpu_count; peer_device_index++)
				if (device_index!=peer_device_index) cudaDeviceEnablePeerAccess(peer_device_index, 0);
		}
		cudaGetLastError();
	}

	// NULL state means it should already be on the gpu's memory (projection)
	if (state != NULL){
		for (int device_index = 0; device_index < gpu_count; device_index++){
			cudaSetDevice(device_index);
			cudaMalloc(&gpu_mem[device_index], malloc_size); error();
			cudaMemcpy(gpu_mem[device_index], state + (gpu_slice_size*2)*device_index, malloc_size, cudaMemcpyHostToDevice); error();
		}
		for (int device_index = 0; device_index < gpu_count; device_index++){
			cudaSetDevice(device_index);
			cudaMemcpyToSymbol(gpu_pointer, gpu_mem, gpu_count*sizeof(cuFloatComplex*)); error();
		}
	}

	int op_index;
	for (int iteration_index = 0; iteration_index < iterations; iteration_index++){
		op_index = 0;

		while (pts[op_index]!= NULL){
			int region_start_bit, is_peer, op_count = 0;
			is_peer = 0;

			while (pts[op_index+op_count] != NULL &&
				pts[op_index+op_count]->target_bit < coalesced_bits &&
				op_count < OPS_BLOCK)
			{
				op_count++;
			}

			int max_target_bit, min_target_bit = coalesced_bits;

			int extra_region_bits = (block_region_size - coalesced_bits);

			if (pts[op_index+op_count] != NULL &&
				op_count < OPS_BLOCK)
			{
				min_target_bit = max_target_bit = pts[op_index+op_count]->target_bit;

				do
				{
					int op_target_bit = pts[op_index+op_count]->target_bit;
					if (op_target_bit < coalesced_bits){
					}
					else if ((op_target_bit >= min_target_bit) && ((op_target_bit - min_target_bit) < extra_region_bits)){
						max_target_bit = max(max_target_bit, op_target_bit);
					}
					else if ((op_target_bit <= max_target_bit) && ((max_target_bit - op_target_bit) < extra_region_bits)){
						min_target_bit = min(min_target_bit, op_target_bit);
					}
					else{
						break;
					}

					op_count++;
				}
				while (pts[op_index+op_count] != NULL &&
					op_count < OPS_BLOCK);
			}
			region_start_bit = max(coalesced_bits, max_target_bit - extra_region_bits+1);

			// SUSPEITO (não verificado, ver docs/07-bugs-e-pontos-de-atencao.md
			// item 13): "qubits-gpu_count+1" trata gpu_count como se fosse
			// diretamente o número de bits que separam as fatias entre GPUs,
			// mas o número de bits certo é log2(gpu_count) (é esse valor que
			// determina em qual fatia/GPU um índice cai, via
			// global_index/gpu_slice_size). As duas contas só coincidem pra
			// gpu_count<=2 (log2(2)=1=2-1); pra gpu_count=4, por exemplo,
			// devia ser "qubits-2" e não "qubits-3". Nunca testado com
			// hardware real (nenhuma sessão deste projeto teve acesso a mais
			// de 1 GPU) — não corrigido às cegas.
			is_peer = ((region_start_bit + (block_region_size - coalesced_bits)) > (qubits-gpu_count+1));

			int term_cursor = 0;
			for (int batch_index = 0; batch_index < op_count; batch_index++){
				PT *term = pts[op_index+batch_index];
				if (term->phase_terms){
					const std::vector<PhaseTerm> &terms = *term->phase_terms;
					operators[batch_index].kind = OP_FUSED;
					operators[batch_index].term_start = term_cursor;
					operators[batch_index].term_count = (int) terms.size();
					for (size_t t = 0; t < terms.size(); t++){
						// separa os bits do termo em "fora da região" (omask/oval) e
						// "dentro" (lmask/lval, no índice da shared memory): bit global
						// g < coalesced_bits -> local g; g na janela [region_start_bit,
						// region_start_bit + extra) -> local coalesced_bits + (g - region_start_bit)
						DEV_TERM &dev = host_terms[term_cursor];
						dev.omask = dev.oval = 0;
						dev.lmask = dev.lval = 0;
						for (int bit = 0; bit < 40; bit++){
							long bit_mask = 1L << bit;
							if (!(terms[t].mask & bit_mask)) continue;
							bool wanted = (terms[t].value & bit_mask) != 0;
							if (bit < coalesced_bits){
								dev.lmask |= (1 << bit);
								if (wanted) dev.lval |= (1 << bit);
							}
							else if (bit >= region_start_bit && bit < region_start_bit + (block_region_size - coalesced_bits)){
								int local = coalesced_bits + (bit - region_start_bit);
								dev.lmask |= (1 << local);
								if (wanted) dev.lval |= (1 << local);
							}
							else{
								dev.omask |= bit_mask;
								if (wanted) dev.oval |= bit_mask;
							}
						}
						dev.steps = (int) terms[t].steps;
						term_cursor++;
					}
					continue;
				}
				operators[batch_index].kind = OP_STD;
				memcpy(operators[batch_index].matrix, term->matrix, 4*sizeof(float complex)); error();
				term->setArgsGPU(operators[batch_index].arg, region_start_bit, block_region_size, coalesced_bits);
			}

			if (is_peer){
				for (int device_index = 0; device_index < gpu_count; device_index++){
					cudaSetDevice(device_index);
					cudaDeviceSynchronize();
				}
			}

			for (int device_index = 0; device_index < gpu_count; device_index++){
				cudaSetDevice(device_index); error();
				cudaMemcpyToSymbol(op, operators, op_count*sizeof(DEV_OP)); error();
				if (term_cursor > 0){
					if (!phase_table_uploaded){
						// primeira porta fundida do processo: tabela de fases em todas as GPUs usadas
						for (int table_device = 0; table_device < gpu_count; table_device++){
							cudaSetDevice(table_device); error();
							cudaMemcpyToSymbol(d_phase_table, PhaseTable(), FUSED_PHASE_STEPS*sizeof(cuFloatComplex)); error();
						}
						cudaSetDevice(device_index); error();
						phase_table_uploaded = true;
					}
					cudaMemcpyToSymbol(d_terms, host_terms, term_cursor*sizeof(DEV_TERM)); error();
				}
			}

			for (int device_index = 0; device_index < gpu_count; device_index++){
				cudaSetDevice(device_index); error();
				ApplyValuesC01<<<grid,block,shared_mem_bytes>>> (block_size, repeat_count, coalesced_bits, region_start_bit, extra_region_bits, op_count, rept_bits, gpu_slice_size, grid.x*device_index); error();
			}
			cudaDeviceSynchronize(); error();

			for (int device_index = 0; device_index < gpu_count; device_index++){
				cudaSetDevice(device_index); error();
				cudaDeviceSynchronize(); error();
			}

			call_count++;

			if (is_peer) call_peer_count++;

			op_index += op_count;
		}
	}

	if (state != NULL){
		for (int device_index = 0; device_index < gpu_count; device_index++){
			cudaMemcpy(state + (gpu_slice_size*2)*device_index, gpu_mem[device_index], malloc_size, cudaMemcpyDeviceToHost); error();
			cudaFree(gpu_mem[device_index]); error();
		}
	}
}


// Ponte extern "C" chamada por dgm_core.cpp/dgm_par_exec.cpp (assinatura
// fixa em include/core/dgm.h — não mexer aqui sem atualizar os dois
// lados, kernel_stub.cpp incluso). Antes havia duas camadas de `switch`
// aqui (GEWrapper2 + este wrapper) despachando pra uma das ~260
// instanciações possíveis de template de GpuExecution01/ApplyValuesC01 —
// eliminadas junto com os templates (ver comentário acima de
// ApplyValuesC01, item 06 do design de arquitetura de 2026-08-10): os
// valores agora fluem direto como parâmetros de runtime, sem
// recompilar/reinstanciar nada por combinação.
extern "C" float* GpuExecutionWrapper(float* state, PT **pts, int qubits, int coalesced_bits, int gpu_region_bits, int gpu_count, int block_size, int repeat_count, int iterations){
	static bool hybrid_debug = (getenv("HYBRID_DEBUG") != NULL);
	std::chrono::steady_clock::time_point t_start;
	if (hybrid_debug) t_start = std::chrono::steady_clock::now();

	GpuExecution01(state, pts, qubits, gpu_region_bits, gpu_count, block_size, repeat_count, coalesced_bits, iterations);

	if (hybrid_debug){
		double elapsed_ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t_start).count();
		fprintf(stderr, "[GPUEXEC]   qubits=%d elapsed_ms=%.3f\n", qubits, elapsed_ms);
	}

	return state;
}

// Marca se gpu_mem[] está alocado agora (ver AllocGpuState/FreeGpuState
// abaixo) — ProjectState/GetState checam isso e abortam com uma
// mensagem clara em vez de ler/escrever um ponteiro NULL ou já liberado
// se alguém chamá-las fora de ordem (achado na revisão de código do
// commit que introduziu essa API, ver item 17 em
// docs/07-bugs-e-pontos-de-atencao.md).
static bool gpu_state_ready = false;

// Aloca o buffer de GPU usado por ProjectState/GetState — item 17 em
// docs/07-bugs-e-pontos-de-atencao.md: antes, ProjectState fazia
// cudaMalloc e GetState fazia cudaFree a cada região individual do modo
// híbrido (dezenas de vezes por execução, mesmo tamanho de buffer toda
// vez, já que region_size não muda dentro de um mesmo lote). Separado
// em Alloc/FreeGpuState pra quem chama (DGM::HybridExecution) alocar
// uma única vez por lote e reaproveitar entre regiões.
extern "C" bool AllocGpuState(int region_size, int gpu_count){
	if (gpu_state_ready){
		printf("ERRO: AllocGpuState chamada de novo sem FreeGpuState antes -- vazaria o buffer já alocado.\n");
		exit(1);
	}

	float malloc_size = (1 << region_size)/gpu_count * sizeof(float)*2;

	for (int device_index = 0; device_index < gpu_count; device_index++){
		cudaSetDevice(device_index);
		cudaMalloc(&gpu_mem[device_index], malloc_size); error();
	}

	gpu_state_ready = true;
	return true;
}

// Inverso de AllocGpuState — libera o buffer alocado por ela. Só deve
// ser chamada depois de AllocGpuState, no fim do mesmo lote de regiões.
extern "C" bool FreeGpuState(int gpu_count){
	if (!gpu_state_ready){
		printf("ERRO: FreeGpuState chamada sem AllocGpuState correspondente antes.\n");
		exit(1);
	}

	for (int device_index = 0; device_index < gpu_count; device_index++){
		cudaSetDevice(device_index);
		cudaFree(gpu_mem[device_index]); error();
	}

	gpu_state_ready = false;
	return true;
}

// Item 17, tentativa 7: menor memória livre entre as primeiras gpu_count
// GPUs, em bytes (-1 se não conseguiu consultar). Usada por
// DGM::HybridExecution pra decidir se o vetor de estado inteiro cabe na
// GPU -- nesse caso o modo híbrido por regiões só perde tempo copiando e
// o caminho residente do t_GPU é mais rápido (docs/07, item 17).
// ---------------------------------------------------------------------------
// Sessão de GPU (item 28): estado residente + measure/colapso na GPU
// ---------------------------------------------------------------------------
static bool gpu_session_active = false;
#define REDUCE_THREADS 256
#define REDUCE_BLOCKS 512
static double *session_partials = NULL;	// REDUCE_BLOCKS * 2 doubles, na GPU

extern "C" bool GpuSessionBegin(float complex *state, int qubits, int gpu_count){
	if (gpu_session_active || gpu_state_ready || gpu_count != 1) return false;

	size_t bytes = (size_t) sizeof(cuFloatComplex) << qubits;
	size_t free_bytes = 0, total_bytes = 0;
	if (cudaSetDevice(0) != cudaSuccess || cudaMemGetInfo(&free_bytes, &total_bytes) != cudaSuccess){
		cudaGetLastError();
		return false;
	}
	if (free_bytes < bytes + (512UL << 20)) return false;	// + folga pro contexto/kernels

	if (cudaMalloc(&gpu_mem[0], bytes) != cudaSuccess){
		cudaGetLastError();
		return false;
	}
	cudaMemcpy(gpu_mem[0], state, bytes, cudaMemcpyHostToDevice); error();
	cudaMemcpyToSymbol(gpu_pointer, gpu_mem, sizeof(cuFloatComplex*)); error();
	cudaMalloc(&session_partials, REDUCE_BLOCKS * 2 * sizeof(double)); error();

	gpu_session_active = true;
	return true;
}

extern "C" void GpuSessionEnd(float complex *state, int qubits, bool copy_back){
	if (!gpu_session_active) return;
	if (copy_back){
		size_t bytes = (size_t) sizeof(cuFloatComplex) << qubits;
		cudaMemcpy(state, gpu_mem[0], bytes, cudaMemcpyDeviceToHost); error();
	}
	cudaFree(gpu_mem[0]); error();
	cudaFree(session_partials); error();
	gpu_mem[0] = NULL;
	session_partials = NULL;
	gpu_session_active = false;
}

// Redução em double: cada thread soma |a|^2 dos seus elementos (passo = grade
// inteira, leituras consecutivas por warp), o bloco reduz na shared memory e
// grava duas somas parciais (bit == 0, bit == 1); o host soma os blocos em
// ordem fixa, então o resultado é determinístico.
__global__ void MeasureSumsKernel(const cuFloatComplex *state, long size, int bit_shift, double *partials){
	__shared__ double zero_sums[REDUCE_THREADS];
	__shared__ double one_sums[REDUCE_THREADS];

	double zero = 0.0, one = 0.0;
	long stride = (long) gridDim.x * blockDim.x;
	for (long index = (long) blockIdx.x * blockDim.x + threadIdx.x; index < size; index += stride){
		cuFloatComplex value = state[index];
		double probability = (double) value.x * value.x + (double) value.y * value.y;
		if ((index >> bit_shift) & 1) one += probability;
		else zero += probability;
	}

	zero_sums[threadIdx.x] = zero;
	one_sums[threadIdx.x] = one;
	__syncthreads();
	for (int half = blockDim.x / 2; half > 0; half >>= 1){
		if (threadIdx.x < half){
			zero_sums[threadIdx.x] += zero_sums[threadIdx.x + half];
			one_sums[threadIdx.x] += one_sums[threadIdx.x + half];
		}
		__syncthreads();
	}
	if (threadIdx.x == 0){
		partials[2 * blockIdx.x] = zero_sums[0];
		partials[2 * blockIdx.x + 1] = one_sums[0];
	}
}

extern "C" void GpuMeasureSums(int qubits, int bit_shift, double *zero_sum, double *one_sum){
	long size = 1L << qubits;
	MeasureSumsKernel<<<REDUCE_BLOCKS, REDUCE_THREADS>>>(gpu_mem[0], size, bit_shift, session_partials); error();

	double host_partials[REDUCE_BLOCKS * 2];
	cudaMemcpy(host_partials, session_partials, sizeof(host_partials), cudaMemcpyDeviceToHost); error();

	double zero = 0.0, one = 0.0;
	for (int block = 0; block < REDUCE_BLOCKS; block++){
		zero += host_partials[2 * block];
		one += host_partials[2 * block + 1];
	}
	*zero_sum = zero;
	*one_sum = one;
}

// Mesmo mapeamento de DGM::measure no host: para cada par (bit == 0, bit == 1),
// pos recebe a amplitude da metade medida dividida por norm e a metade com o bit
// em 1 zera -- o qubit medido volta a |0>, pronto pra ser reaproveitado pela
// estimação de fase semiclássica do Shor.
__global__ void CollapseKernel(cuFloatComplex *state, long size, int bit_shift, int measured_bit, double norm){
	long half_size = size >> 1;
	long low_mask = (1L << bit_shift) - 1;
	long stride = (long) gridDim.x * blockDim.x;
	for (long pair = (long) blockIdx.x * blockDim.x + threadIdx.x; pair < half_size; pair += stride){
		long pos = (pair << 1) - (pair & low_mask);
		cuFloatComplex value = state[pos | ((long) measured_bit << bit_shift)];
		state[pos] = make_cuFloatComplex((float) (value.x / norm), (float) (value.y / norm));
		state[pos | (1L << bit_shift)] = make_cuFloatComplex(0.0f, 0.0f);
	}
}

extern "C" void GpuCollapse(int qubits, int bit_shift, int measured_bit, double norm){
	long size = 1L << qubits;
	long blocks = (size / 2 + REDUCE_THREADS - 1) / REDUCE_THREADS;
	if (blocks > 65535L * 16) blocks = 65535L * 16;
	CollapseKernel<<<(unsigned int) blocks, REDUCE_THREADS>>>(gpu_mem[0], size, bit_shift, measured_bit, norm); error();
	cudaDeviceSynchronize(); error();
}

extern "C" int GpuMaxBatchOps(){
	return OPS_BLOCK;
}

extern "C" long GpuMinFreeBytes(int gpu_count){
	long min_free = -1;
	for (int device_index = 0; device_index < gpu_count; device_index++){
		if (cudaSetDevice(device_index) != cudaSuccess){
			cudaGetLastError();
			return -1;
		}
		size_t free_bytes = 0, total_bytes = 0;
		if (cudaMemGetInfo(&free_bytes, &total_bytes) != cudaSuccess){
			cudaGetLastError();
			return -1;
		}
		if (min_free < 0 || (long)free_bytes < min_free) min_free = (long)free_bytes;
	}
	return min_free;
}

// Item 17, tentativa 6: trava (page-lock) o vetor de estado do host durante
// o modo híbrido. Cópias de/para memória "pageable" passam por um buffer
// de staging do driver e disputam banda de memória com as threads de CPU;
// com memória travada o DMA é direto (medido no qflex: ~12,8 -> ~22 GB/s
// sem concorrência). Falha de forma graciosa -- devolve false e o híbrido
// segue com memória comum --, pois não é requisito de corretude.
extern "C" bool PinHostState(void* ptr, size_t bytes){
	cudaError_t status = cudaHostRegister(ptr, bytes, cudaHostRegisterDefault);
	if (status != cudaSuccess){
		cudaGetLastError();	// limpa o erro pendente pra não contaminar o error() seguinte
		return false;
	}
	return true;
}

extern "C" bool UnpinHostState(void* ptr){
	cudaError_t status = cudaHostUnregister(ptr);
	if (status != cudaSuccess){
		cudaGetLastError();
		return false;
	}
	return true;
}

// Copia só a fatia do estado correspondente a uma região (region_id/
// region_mask) do host pra GPU — usado pelo modo híbrido pra mandar só
// a parte que a GPU vai processar naquela rodada, não o vetor inteiro.
// Pressupõe que AllocGpuState já foi chamada (não aloca o buffer) —
// checado abaixo, aborta com mensagem clara se não foi.
extern "C" bool ProjectState(float* state, int qubits, int region_size, long region_id, long region_mask, int gpu_count){
	if (!gpu_state_ready){
		printf("ERRO: ProjectState chamada sem AllocGpuState antes -- gpu_mem[] não está alocado.\n");
		exit(1);
	}

	// Instrumentação opt-in (item 17 em docs/07-bugs-e-pontos-de-atencao.md,
	// investigando se o custo de ProjectState/GetState é dominado pelo
	// número de cudaMemcpy (mem_portions pequenos e não-contíguos) ou pelo
	// volume de dados. Mesmo HYBRID_DEBUG=1 usado em dgm_par_exec.cpp.
	static bool hybrid_debug = (getenv("HYBRID_DEBUG") != NULL);
	std::chrono::steady_clock::time_point t_start;
	if (hybrid_debug) t_start = std::chrono::steady_clock::now();

	int coalesced_bits = 0;
	for (int bit_index = 0; bit_index < qubits; bit_index++){
		if ((region_mask >> bit_index) & 1)
			coalesced_bits++;
		else
			break;
	}

	int mem_portions = pow(2.0, region_size - coalesced_bits);
	int portion_size = 1 << coalesced_bits;

	long inc = ~(region_mask >> coalesced_bits);

	double memcpy_ms = 0;
	std::chrono::steady_clock::time_point t_phase;

	long dev_pos, pos, base = 0;
	for (int device_index = 0; device_index < gpu_count; device_index++){
		cudaSetDevice(device_index);

		dev_pos = 0;
		for (int portion_index = mem_portions/gpu_count*device_index; portion_index < mem_portions/gpu_count*(device_index+1); portion_index++){
			pos = (base << coalesced_bits) | region_id;

			if (hybrid_debug) t_phase = std::chrono::steady_clock::now();
			cudaMemcpy(gpu_mem[device_index]+dev_pos, state + pos*2, portion_size*2*sizeof(float), cudaMemcpyHostToDevice);
			if (hybrid_debug) memcpy_ms += std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t_phase).count();

			base = (base + inc + 1) & ~inc;
			dev_pos += portion_size;
		}
	}


	for (int device_index = 0; device_index < gpu_count; device_index++){
		cudaSetDevice(device_index);
		cudaDeviceSynchronize();
		cudaMemcpyToSymbol(gpu_pointer, gpu_mem, gpu_count*sizeof(cuFloatComplex*)); error();
	}

	if (hybrid_debug){
		double elapsed_ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t_start).count();
		fprintf(stderr, "[PROJSTATE] region_size=%d coalesced_bits=%d mem_portions=%d portion_size=%d memcpy_ms=%.3f elapsed_ms=%.3f\n", region_size, coalesced_bits, mem_portions, portion_size, memcpy_ms, elapsed_ms);
	}

	return true;
}

// Inverso de ProjectState: copia a fatia processada de volta da GPU pro
// vetor de estado no host. Não libera o buffer (ver FreeGpuState) — quem
// chama decide quando o lote de regiões acabou. Pressupõe AllocGpuState
// já chamada — checado abaixo, aborta com mensagem clara se não foi.
extern "C" bool GetState(float* state, int qubits, int region_size, long region_id, long region_mask, int gpu_count){
	if (!gpu_state_ready){
		printf("ERRO: GetState chamada sem AllocGpuState antes -- gpu_mem[] não está alocado.\n");
		exit(1);
	}

	static bool hybrid_debug = (getenv("HYBRID_DEBUG") != NULL);
	std::chrono::steady_clock::time_point t_start;
	if (hybrid_debug) t_start = std::chrono::steady_clock::now();

	int coalesced_bits = 0;
	for (int bit_index = 0; bit_index < qubits; bit_index++){
		if ((region_mask >> bit_index) & 1)
			coalesced_bits++;
		else
			break;
	}

	int mem_portions = pow(2.0, region_size - coalesced_bits);
	int portion_size = 1 << coalesced_bits;

	long inc = ~(region_mask >> coalesced_bits);

	double memcpy_ms = 0;
	std::chrono::steady_clock::time_point t_phase;

	long dev_pos, pos, base = 0;
	for (int device_index = 0; device_index < gpu_count; device_index++){
		cudaSetDevice(device_index);

		dev_pos = 0;
		for (int portion_index = mem_portions/gpu_count*device_index; portion_index < mem_portions/gpu_count*(device_index+1); portion_index++){
			pos = (base << coalesced_bits) | region_id;

			if (hybrid_debug) t_phase = std::chrono::steady_clock::now();
			cudaMemcpy(state + pos*2, gpu_mem[device_index] + dev_pos, portion_size*2*sizeof(float), cudaMemcpyDeviceToHost); error();
			cudaDeviceSynchronize(); error();
			if (hybrid_debug) memcpy_ms += std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t_phase).count();

			base = (base + inc + 1) & ~inc;
			dev_pos += portion_size;
		}
	}

	if (hybrid_debug){
		double elapsed_ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t_start).count();
		fprintf(stderr, "[GETSTATE]  region_size=%d coalesced_bits=%d mem_portions=%d portion_size=%d memcpy_ms=%.3f elapsed_ms=%.3f\n", region_size, coalesced_bits, mem_portions, portion_size, memcpy_ms, elapsed_ms);
	}

	return true;
}

// Confere se a última chamada CUDA falhou; se falhou, imprime o erro e
// aborta o processo (exit(1)).
bool error(){
	error_check_count++;
	cudaError_t cuda_error;
	cuda_error = cudaGetLastError();
	if (cuda_error == cudaSuccess) return false;
	printf("error_check: %d\nerror: %d - %s\n", error_check_count, cuda_error, cudaGetErrorString (cuda_error));
	exit(1);
	return true;
}
