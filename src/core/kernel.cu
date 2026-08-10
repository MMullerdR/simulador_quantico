#include <cuComplex.h>
#include <stdio.h>
#include <stdlib.h>
#include <math.h>
#include "../../include/core/common.h"

#define M_RANGE 512
#define M_PREC 10000

bool error();
static int error_check_count = 0;
static int call_count = 0;
static int call_peer_count = 0;

///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

struct DEV_OP{
	long arg[TAM_ARG];
	cuFloatComplex matrix[4];
};

extern "C" bool setDevice(int device_id = 0){
	return cudaFree(0);
}

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


inline int GET_BLOCK_ID(PT *term, int coalesced_bits, int gpu_region_bits){
	return (term->target_bit - coalesced_bits)/(gpu_region_bits-coalesced_bits);
}

__device__ long OPEN_SPACE(long value, int from_bit, int bit_count){
	return ((value >> from_bit) << (from_bit + bit_count)) | (value & ((1 << from_bit) - 1));
}

///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

//extern "C"
template <int t_block_size, int t_repeat_count, int t_coalesced_bits>
__global__ void ApplyValuesC01(int const region_start_bit, int const extra_region_bits, int const op_count, int const rept_bits, int const gpu_slice_size, int const block_offset){
	long local_pos, global_index0, global_index1, block = (blockIdx.x + block_offset);

	int rep_index, op_index, thread_id = threadIdx.x;

	__shared__ cuFloatComplex shared_amplitudes[t_repeat_count*t_block_size*2];

	long block_base;

	block_base = block << t_coalesced_bits;
	block_base = OPEN_SPACE(block_base, region_start_bit, extra_region_bits);

	long pair_offset_mask = (1 << (region_start_bit+extra_region_bits-rept_bits-1));

	// copy amplitudes from global memory to shared memory
	for (rep_index = 0; rep_index < t_repeat_count; rep_index++){
		local_pos = thread_id + rep_index*t_block_size*2; //another start

		global_index0 = block_base | ((local_pos >> t_coalesced_bits) << region_start_bit) | (local_pos & ((1 << t_coalesced_bits)-1));
		global_index1 = global_index0 | pair_offset_mask;

		shared_amplitudes[local_pos] = gpu_pointer[global_index0/gpu_slice_size][global_index0%gpu_slice_size];
		shared_amplitudes[local_pos+t_block_size] = gpu_pointer[global_index1/gpu_slice_size][global_index1%gpu_slice_size];
	}

	int pos0, pos1, target_bit_mask;
	cuFloatComplex tmp;

	// compute the operators for the amplitudes on the shared memory
	for (op_index = 0; op_index < op_count; op_index++){
		__syncthreads();

		target_bit_mask = 1 << op[op_index].arg[SHIFT];

		if (((block_base & op[op_index].arg[CTRL_MASK]) == op[op_index].arg[CTRL_VALUE])){
			for (rep_index = 0; rep_index < t_repeat_count; rep_index++){
				local_pos = thread_id + rep_index*t_block_size;

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
	for (rep_index = 0; rep_index < t_repeat_count; rep_index++){
		local_pos = thread_id + rep_index*t_block_size*2; //another start

		global_index0 = block_base | ((local_pos >> t_coalesced_bits) << region_start_bit) | (local_pos & ((1 << t_coalesced_bits)-1));
		global_index1 = global_index0 | pair_offset_mask;

		gpu_pointer[global_index0/gpu_slice_size][global_index0%gpu_slice_size] = shared_amplitudes[local_pos];
		gpu_pointer[global_index1/gpu_slice_size][global_index1%gpu_slice_size] = shared_amplitudes[local_pos+t_block_size];
	}
}

//Kernel para execução com múltiplas GPUs se comunicando usando DMA (Direct Memory Access)
template <int t_block_size, int t_repeat_count, int t_coalesced_bits>
void GpuExecution01(float* state, PT **pts, int qubits, int gpu_region_bits, int gpu_count, int iterations){
	DEV_OP operators[OPS_BLOCK];

	error_check_count = 0;

	dim3 block, grid;

	long mem_size = pow(2.0, qubits);
	long gpu_slice_size = mem_size/gpu_count;

	int rept_bits = log2(t_repeat_count);

	long thread_total = mem_size/gpu_count/t_repeat_count/2;	// /2 porque cada thread fica responsável por duas posições & /2 pelas 2 GPUS

	long malloc_size = (mem_size * (sizeof(float complex)))/gpu_count;


	block.x = t_block_size;
	(thread_total > block.x)? grid.x = thread_total/block.x : block.x = thread_total;

	int block_region_size = gpu_region_bits;

	if (block_region_size < gpu_region_bits){
		printf("ERRO: Região do bloco menor que a região de qubits\n");
		exit(1);
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
				pts[op_index+op_count]->target_bit < t_coalesced_bits &&
				op_count < OPS_BLOCK)
			{
				op_count++;
			}

			max_end = t_coalesced_bits;

			int max_target_bit, min_target_bit = t_coalesced_bits;

			int extra_region_bits = (block_region_size - t_coalesced_bits);

			if (pts[op_index+op_count] != NULL &&
				op_count < OPS_BLOCK)
			{
				min_target_bit = max_target_bit = pts[op_index+op_count]->target_bit;

				do
				{
					int op_target_bit = pts[op_index+op_count]->target_bit;
					if (op_target_bit < t_coalesced_bits){
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
			region_start_bit = max(t_coalesced_bits, max_target_bit - extra_region_bits+1);

			is_peer = ((region_start_bit + (block_region_size - t_coalesced_bits)) > (qubits-gpu_count+1));

			for (int batch_index = 0; batch_index < op_count; batch_index++){
				memcpy(operators[batch_index].matrix, pts[op_index+batch_index]->matrix, 4*sizeof(float complex)); error();
				pts[op_index+batch_index]->setArgsGPU(operators[batch_index].arg, region_start_bit, block_region_size, t_coalesced_bits);
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
				ApplyValuesC01<t_block_size, t_repeat_count, t_coalesced_bits> <<<grid,block>>> (region_start_bit, extra_region_bits, op_count, rept_bits, gpu_slice_size, grid.x*device_index); error();
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


//Segundo Wrapper -- tamanho de bloco e número de projeções por bloco
//
// ATENÇÃO: esse switch normalmente cobre block_size em {32,64,128,256,512,1024}
// com vários valores de repeat_count cada (~26 combinações), e GpuExecutionWrapper
// (abaixo) cobre coalesced_bits de 0 a 9 — total de ~260 instanciações do kernel
// ApplyValuesC01. Compilar tudo isso de uma vez em -O3 estava levando horas
// nesta máquina (ver docs/04-gpu-cuda.md e a discussão de build). Reduzido
// temporariamente pra um subconjunto pequeno (inclui o combo default usado
// em lib_grover.cpp/lib_shor.cpp/lib_general.cpp: block_size=64, repeat_count=2,
// coalesced_bits=4) só pra manter o ciclo de compilar/verificar rápido durante o
// trabalho de renomeação. Pra restaurar o conjunto completo antes de um
// build de produção de verdade, é só devolver os cases removidos (mesmo
// padrão dos que sobraram, só repetindo pra cada block_size/repeat_count/coalesced_bits).
template <int t_coalesced_bits>
void GEWrapper2(float* state, PT **pts, int qubits, int gpu_region_bits, int gpu_count, int block_size, int repeat_count, int iterations){
	switch(block_size){
		case 64:
			switch(repeat_count){
				case 2:
					GpuExecution01<64, 2, t_coalesced_bits>(state, pts, qubits, gpu_region_bits, gpu_count, iterations);
					break;
				default:
					printf("Invalid REPT");
			}
			break;
		default:
			printf("Invalid TAM_BLOCK");
	}
}


//Primeiro Wrapper -- Coalescimento (reduzido a 1 única instanciação como
//teste de diagnóstico — ver comentário acima de GEWrapper2)
extern "C" float* GpuExecutionWrapper(float* state, PT **pts, int qubits, int coalesced_bits, int gpu_region_bits, int gpu_count, int block_size, int repeat_count, int iterations){
	switch(coalesced_bits){
		case 4:
			GEWrapper2<4>(state, pts, qubits, gpu_region_bits, gpu_count, block_size, repeat_count, iterations);
			break;
		default:
			printf("Invalid COALESC");
	}

	return state;
}

//Primeiro Wrapper -- Coalescimento
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

bool error(){
	error_check_count++;
	cudaError_t cuda_error;
	cuda_error = cudaGetLastError();
	if (cuda_error == cudaSuccess) return false;
	printf("error_check: %d\nerror: %d - %s\n", error_check_count, cuda_error, cudaGetErrorString (cuda_error));
	exit(1);
	return true;
}
