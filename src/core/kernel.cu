#include <cuComplex.h>
#include <stdio.h>
#include <stdlib.h>
#include <math.h>
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
};

// Inicializa a GPU escolhida (cudaFree(0) força a inicialização do
// contexto CUDA sem alocar nada de verdade).
extern "C" bool setDevice(int device_id = 0){
	return cudaFree(0);
}

// Habilita acesso direto (DMA) entre as GPUs 0 e 1; sem uso no código
// atual (GpuExecution01 habilita peer access por conta própria, abaixo).
extern "C" bool enablePeerAccess(){
	cudaSetDevice(0);
	cudaDeviceEnablePeerAccess(1, 0);

	cudaSetDevice(1);
	cudaDeviceEnablePeerAccess(0, 0);

	cudaGetLastError();

	return true;
}

__constant__ long c_arg[1][1];
__constant__ cuFloatComplex cmatrix[1][1];

__constant__ DEV_OP op[OPS_BLOCK];

static cuFloatComplex *gpu_mem[4];
__constant__ cuFloatComplex *gpu_pointer[4];


// Em qual bloco de região cairia o qubit alvo de "term"; sem uso no
// código atual.
inline int GET_BLOCK_ID(PT *term, int coalesced_bits, int gpu_region_bits){
	return (term->target_bit - coalesced_bits)/(gpu_region_bits-coalesced_bits);
}

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
__global__ void ApplyValuesC01(int const block_size, int const repeat_count, int const coalesced_bits, int const region_start_bit, int const extra_region_bits, int const op_count, int const rept_bits, int const gpu_slice_size, int const block_offset){
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
			int region_start_bit, is_peer, max_end, op_count = 0; //, qbs_block_id
			is_peer = 0;

			while (pts[op_index+op_count] != NULL &&
				pts[op_index+op_count]->target_bit < coalesced_bits &&
				op_count < OPS_BLOCK)
			{
				op_count++;
			}

			max_end = coalesced_bits;

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

			is_peer = ((region_start_bit + (block_region_size - coalesced_bits)) > (qubits-gpu_count+1));

			for (int batch_index = 0; batch_index < op_count; batch_index++){
				memcpy(operators[batch_index].matrix, pts[op_index+batch_index]->matrix, 4*sizeof(float complex)); error();
				pts[op_index+batch_index]->setArgsGPU(operators[batch_index].arg, region_start_bit, block_region_size, coalesced_bits);
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
	GpuExecution01(state, pts, qubits, gpu_region_bits, gpu_count, block_size, repeat_count, coalesced_bits, iterations);

	return state;
}

// Copia só a fatia do estado correspondente a uma região (region_id/
// region_mask) do host pra GPU — usado pelo modo híbrido pra mandar só
// a parte que a GPU vai processar naquela rodada, não o vetor inteiro.
extern "C" bool ProjectState(float* state, int qubits, int region_size, long region_id, long region_mask, int gpu_count){
	int coalesced_bits = 0;
	for (int bit_index = 0; bit_index < qubits; bit_index++){
		if ((region_mask >> bit_index) & 1)
			coalesced_bits++;
		else
			break;
	}

	int mem_portions = pow(2.0, region_size - coalesced_bits);
	int portion_size = 1 << coalesced_bits;

	float malloc_size = (1 << region_size)/gpu_count * sizeof(float)*2;
	long inc = ~(region_mask >> coalesced_bits);

	long dev_pos, pos, base = 0;
	for (int device_index = 0; device_index < gpu_count; device_index++){
		cudaSetDevice(device_index);
		cudaMalloc(&gpu_mem[device_index], malloc_size); error();

		dev_pos = 0;
		for (int portion_index = mem_portions/gpu_count*device_index; portion_index < mem_portions/gpu_count*(device_index+1); portion_index++){
			pos = (base << coalesced_bits) | region_id;

			cudaMemcpy(gpu_mem[device_index]+dev_pos, state + pos*2, portion_size*2*sizeof(float), cudaMemcpyHostToDevice);

			base = (base + inc + 1) & ~inc;
			dev_pos += portion_size;
		}
	}


	for (int device_index = 0; device_index < gpu_count; device_index++){
		cudaSetDevice(device_index);
		cudaDeviceSynchronize();
		cudaMemcpyToSymbol(gpu_pointer, gpu_mem, gpu_count*sizeof(cuFloatComplex*)); error();
	}

	return true;
}

// Inverso de ProjectState: copia a fatia processada de volta da GPU pro
// vetor de estado no host.
extern "C" bool GetState(float* state, int qubits, int region_size, long region_id, long region_mask, int gpu_count){
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

	long dev_pos, pos, base = 0;
	for (int device_index = 0; device_index < gpu_count; device_index++){
		cudaSetDevice(device_index);

		dev_pos = 0;
		for (int portion_index = mem_portions/gpu_count*device_index; portion_index < mem_portions/gpu_count*(device_index+1); portion_index++){
			pos = (base << coalesced_bits) | region_id;

			cudaMemcpy(state + pos*2, gpu_mem[device_index] + dev_pos, portion_size*2*sizeof(float), cudaMemcpyDeviceToHost); error();
			cudaDeviceSynchronize(); error();

			base = (base + inc + 1) & ~inc;
			dev_pos += portion_size;
		}
	}

	for (int device_index = 0; device_index < gpu_count; device_index++){
		cudaFree(gpu_mem[device_index]); error();
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
