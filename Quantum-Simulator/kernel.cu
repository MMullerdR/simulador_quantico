#include <cuComplex.h>
#include <stdio.h>
#include <stdlib.h>
#include <math.h>
#include "common.h"

#define M_RANGE 512
#define M_PREC 10000

bool error();
static int inst = 0;
static int call_count = 0;
static int call_peer_count = 0;

///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

struct DEV_OP{
	long arg[TAM_ARG];
	cuFloatComplex matrix[4];
};

extern "C" bool setDevice(int num = 0){
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


inline int GET_BLOCK_ID(PT *pt, int coalesc, int qbs_region){
	return (pt->end - coalesc)/(qbs_region-coalesc);
}

__device__ long OPEN_SPACE(long value, int from_bit, int n){
	return ((value >> from_bit) << (from_bit + n)) | (value & ((1 << from_bit) - 1));
}

///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

//extern "C"
template <int t_TAM_BLOCK, int t_REPT, int t_COALESC>
__global__ void ApplyValuesC01(int const b_pos, int const n_bits, int const count, int const rept_bits, int const shift, int const block_shift){
	long p, g_pos1, g_pos2, block = (blockIdx.x + block_shift);
	
	int i, c, thId = threadIdx.x;

	__shared__ cuFloatComplex s[t_REPT*t_TAM_BLOCK*2];

	long block_base;

	block_base = block << t_COALESC;
	block_base = OPEN_SPACE(block_base, b_pos, n_bits);

	long g_pos2_mask = (1 << (b_pos+n_bits-rept_bits-1));

	// copy amplitudes from global memory to shared memory
	for (i = 0; i < t_REPT; i++){
		p = thId + i*t_TAM_BLOCK*2; //another start
		
		g_pos1 = block_base | ((p >> t_COALESC) << b_pos) | (p & ((1 << t_COALESC)-1));
		g_pos2 = g_pos1 | g_pos2_mask;
		
		s[p] = gpu_pointer[g_pos1/shift][g_pos1%shift];
		s[p+t_TAM_BLOCK] = gpu_pointer[g_pos2/shift][g_pos2%shift];
	}

	int pos0, pos1, op_bit;
	cuFloatComplex tmp;
	
	// compute the operators for the amplitudes on the shared memory
	for (c = 0; c < count; c++){
		__syncthreads();

		op_bit = 1 << op[c].arg[SHIFT];

		if (((block_base & op[c].arg[CTRL_MASK]) == op[c].arg[CTRL_VALUE])){		
			for (i = 0; i < t_REPT; i++){
				p = thId + i*t_TAM_BLOCK;

				pos0 = (p * 2) - (p & (op_bit - 1));
				pos1 = pos0 | op_bit;
				if ((pos0 & op[c].arg[CTRL_REG_MASK]) == op[c].arg[CTRL_REG_VALUE]){
					tmp = cuCaddf(cuCmulf(s[pos0], op[c].matrix[0]), cuCmulf(s[pos1], op[c].matrix[1]));
					s[pos1] = cuCaddf(cuCmulf(s[pos0], op[c].matrix[2]), cuCmulf(s[pos1], op[c].matrix[3]));			
					s[pos0] = tmp;
				}
			}
		}
	}
	__syncthreads();

	// copy results from shared memory to global memory
	for (i = 0; i < t_REPT; i++){
		p = thId + i*t_TAM_BLOCK*2; //another start

		g_pos1 = block_base | ((p >> t_COALESC) << b_pos) | (p & ((1 << t_COALESC)-1));
		g_pos2 = g_pos1 | g_pos2_mask;
	
		gpu_pointer[g_pos1/shift][g_pos1%shift] = s[p];
		gpu_pointer[g_pos2/shift][g_pos2%shift] = s[p+t_TAM_BLOCK];
	}
}

//Kernel para execução com múltiplas GPUs se comunicando usando DMA (Direct Memory Access)
template <int t_TAM_BLOCK, int t_REPT, int t_COALESC>	
void GpuExecution01(float* state, PT **pts, int qubits, int qbs_region, int multi_gpu, int num_it){
	DEV_OP operators[OPS_BLOCK];

	inst = 0;

	dim3 block, dim;

	long mem_size = pow(2.0, qubits);
	long mem_desloc = mem_size/multi_gpu;

	int rept_bits = log2(t_REPT);
	
	long nth = mem_size/multi_gpu/t_REPT/2;	// /2 porque cada thread fica responsável por duas posições & /2 pelas 2 GPUS

	long malloc_size = (mem_size * (sizeof(float complex)))/multi_gpu;


	block.x = t_TAM_BLOCK;
	(nth > block.x)? dim.x = nth/block.x : block.x = nth;

	int block_region_size = qbs_region;

	if (block_region_size < qbs_region){
		printf("ERRO: Região do bloco menor que a região de qubits\n");
		exit(1);
	}

	if (multi_gpu > 1){
		for (int d = 0; d < multi_gpu; d++){
			cudaSetDevice(d);
			for (int j = 0; j < multi_gpu; j++)
				if (d!=j) cudaDeviceEnablePeerAccess(j, 0);
		}
		cudaGetLastError();
	}

	// NULL state means it should already be on the gpu's memory (projection)
	if (state != NULL){ 
		for (int d = 0; d < multi_gpu; d++){
			cudaSetDevice(d);
			cudaMalloc(&gpu_mem[d], malloc_size); error();
			cudaMemcpy(gpu_mem[d], state + (mem_desloc*2)*d, malloc_size, cudaMemcpyHostToDevice); error();
		}
		for (int d = 0; d < multi_gpu; d++){
			cudaSetDevice(d);
			cudaMemcpyToSymbol(gpu_pointer, gpu_mem, multi_gpu*sizeof(cuFloatComplex*)); error();
		}
	}

	int i;
	for (int it = 0; it < num_it; it++){
		i = 0;

		while (pts[i]!= NULL){
			int region_start, is_peer, max_end, c = 0; //, qbs_block_id
			is_peer = 0;

			while (pts[i+c] != NULL &&
				pts[i+c]->end < t_COALESC &&
				c < OPS_BLOCK)
			{
				c++;
			}

			max_end = t_COALESC;

			int s_max, s_min = t_COALESC;

			int extra_region = (block_region_size - t_COALESC);

			if (pts[i+c] != NULL &&
				c < OPS_BLOCK)
			{
				s_min = s_max = pts[i+c]->end;

				do
				{
					int e = pts[i+c]->end;
					if (e < t_COALESC){
					}
					else if ((e >= s_min) && ((e - s_min) < extra_region)){
						s_max = max(s_max, e);
					}
					else if ((e <= s_max) && ((s_max - e) < extra_region)){
						s_min = min(s_min, e);
					}
					else{
						break;
					}
					
					c++;
				}
				while (pts[i+c] != NULL &&
					c < OPS_BLOCK);
			}
			region_start = max(t_COALESC, s_max - extra_region+1);

			is_peer = ((region_start + (block_region_size - t_COALESC)) > (qubits-multi_gpu+1));

			for (int j = 0; j < c; j++){
				memcpy(operators[j].matrix, pts[i+j]->matrix, 4*sizeof(float complex)); error();
				pts[i+j]->setArgsGPU(operators[j].arg, region_start, block_region_size, t_COALESC);
			}

			if (is_peer){
				for (int d = 0; d < multi_gpu; d++){
					cudaSetDevice(d);
					cudaDeviceSynchronize();
				}
			}

			for (int d = 0; d < multi_gpu; d++){
				cudaSetDevice(d); error();
				cudaMemcpyToSymbol(op, operators, c*sizeof(DEV_OP)); error();
			}
			
			for (int d = 0; d < multi_gpu; d++){
				cudaSetDevice(d); error();
				ApplyValuesC01<t_TAM_BLOCK, t_REPT, t_COALESC> <<<dim,block>>> (region_start, extra_region, c, rept_bits, mem_desloc, dim.x*d); error();
			}
			cudaDeviceSynchronize(); error();
						
			for (int d = 0; d < multi_gpu; d++){
				cudaSetDevice(d); error();
				cudaDeviceSynchronize(); error();			
			}
			
			call_count++;
			
			if (is_peer) call_peer_count++;

			i += c;
		}
	}	

	if (state != NULL){ 
		for (int d = 0; d < multi_gpu; d++){
			cudaMemcpy(state + (mem_desloc*2)*d, gpu_mem[d], malloc_size, cudaMemcpyDeviceToHost); error();
			cudaFree(gpu_mem[d]); error();
		}
	}
}


//Segundo Wrapper -- tamanho de bloco e número de projeções por bloco
template <int t_COALESC>
void GEWrapper2(float* state, PT **pts, int qubits, int qbs_region, int multi_gpu, int tam_block, int rept, int num_it){
	switch(tam_block){
		case 32:
			switch(rept){
				case 1:
					GpuExecution01<32, 1, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				case 2:
					GpuExecution01<32, 2, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				case 4:
					GpuExecution01<32, 4, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				case 8:
					GpuExecution01<32, 8, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				case 16:
					GpuExecution01<32, 16, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				case 32:
					GpuExecution01<32, 32, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				default:
					printf("Invalid REPT");
			}
			break;
		case 64:
			switch(rept){
				case 1:
					GpuExecution01<64, 1, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				case 2:
					GpuExecution01<64, 2, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				case 4:
					GpuExecution01<64, 4, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				case 8:
					GpuExecution01<64, 8, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				case 16:
					GpuExecution01<64, 16, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				case 32:
					GpuExecution01<64, 32, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				default:
					printf("Invalid REPT");
			}
			break;
		case 128:
			switch(rept){
				case 1:
					GpuExecution01<128, 1, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				case 2:
					GpuExecution01<128, 2, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				case 4:
					GpuExecution01<128, 4, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				case 8:
					GpuExecution01<128, 8, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				case 16:
					GpuExecution01<128, 16, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				default:
					printf("Invalid REPT");
			}
			break;
		case 256:
			switch(rept){
				case 1:
					GpuExecution01<256, 1, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				case 2:
					GpuExecution01<256, 2, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				case 4:
					GpuExecution01<256, 4, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				case 8:
					GpuExecution01<256, 8, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				default:
					printf("Invalid REPT");
			}
			break;
		case 512:
			switch(rept){
				case 1:
					GpuExecution01<512, 1, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				case 2:
					GpuExecution01<512, 2, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				case 4:
					GpuExecution01<512, 4, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				default:
					printf("Invalid REPT");
			}
			break;
		case 1024:
			switch(rept){
				case 1:
					GpuExecution01<1024, 1, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				case 2:
					GpuExecution01<1024, 2, t_COALESC>(state, pts, qubits, qbs_region, multi_gpu, num_it);
					break;
				default:
					printf("Invalid REPT");
			}
			break;
		default:
			printf("Invalid TAM_BLOCK");
	}
}


//Primeiro Wrapper -- Coalescimento
extern "C" float* GpuExecutionWrapper(float* state, PT **pts, int qubits, int coalesc, int qbs_region, int multi_gpu, int tam_block, int rept, int num_it){
	switch(coalesc){
		case 0:
			GEWrapper2<0>(state, pts, qubits, qbs_region, multi_gpu, tam_block, rept, num_it);
			break;
		case 1:
			GEWrapper2<1>(state, pts, qubits, qbs_region, multi_gpu, tam_block, rept, num_it);
			break;
		case 2:
			GEWrapper2<2>(state, pts, qubits, qbs_region, multi_gpu, tam_block, rept, num_it);
			break;
		case 3:
			GEWrapper2<3>(state, pts, qubits, qbs_region, multi_gpu, tam_block, rept, num_it);
			break;
		case 4:
			GEWrapper2<4>(state, pts, qubits, qbs_region, multi_gpu, tam_block, rept, num_it);
			break;
		case 5:
			GEWrapper2<5>(state, pts, qubits, qbs_region, multi_gpu, tam_block, rept, num_it);
			break;
		case 6:
			GEWrapper2<6>(state, pts, qubits, qbs_region, multi_gpu, tam_block, rept, num_it);
			break;
		case 7:
			GEWrapper2<7>(state, pts, qubits, qbs_region, multi_gpu, tam_block, rept, num_it);
			break;
		case 8:
			GEWrapper2<8>(state, pts, qubits, qbs_region, multi_gpu, tam_block, rept, num_it);
			break;
		case 9:
			GEWrapper2<9>(state, pts, qubits, qbs_region, multi_gpu, tam_block, rept, num_it);
			break;
		default:
			printf("Invalid COALESC");
	}

	return state;
}

//Primeiro Wrapper -- Coalescimento
extern "C" bool ProjectState(float* state, int qubits, int proj_qubits, long reg_id, long reg_mask, int multi_gpu){
	int qbs_coales = 0;
	for (int i = 0; i < qubits; i++){
		if ((reg_mask >> i) & 1)
			qbs_coales++;
		else
			break;
	}

	int mem_portions = pow(2.0, proj_qubits - qbs_coales);
	int portion_size = 1 << qbs_coales;

	float malloc_size = (1 << proj_qubits)/multi_gpu * sizeof(float)*2;
	long inc = ~(reg_mask >> qbs_coales);

	long dev_pos, pos, base = 0;
	for (int d = 0; d < multi_gpu; d++){
		cudaSetDevice(d);
		cudaMalloc(&gpu_mem[d], malloc_size); error();
	
		dev_pos = 0;
		for (int b = mem_portions/multi_gpu*d; b < mem_portions/multi_gpu*(d+1); b++){
			pos = (base << qbs_coales) | reg_id;

			cudaMemcpy(gpu_mem[d]+dev_pos, state + pos*2, portion_size*2*sizeof(float), cudaMemcpyHostToDevice);
	
			base = (base + inc + 1) & ~inc;
			dev_pos += portion_size; 
		}
	}


	for (int d = 0; d < multi_gpu; d++){
		cudaSetDevice(d);
		cudaDeviceSynchronize();
		cudaMemcpyToSymbol(gpu_pointer, gpu_mem, multi_gpu*sizeof(cuFloatComplex*)); error();
	}

	return true;
}

extern "C" bool GetState(float* state, int qubits, int proj_qubits, long reg_id, long reg_mask, int multi_gpu){
	int qbs_coales = 0;
	for (int i = 0; i < qubits; i++){
		if ((reg_mask >> i) & 1)
			qbs_coales++;
		else
			break;
	}

	int mem_portions = pow(2.0, proj_qubits - qbs_coales);
	int portion_size = 1 << qbs_coales;

	long inc = ~(reg_mask >> qbs_coales);	

	long dev_pos, pos, base = 0;
	for (int d = 0; d < multi_gpu; d++){
		cudaSetDevice(d);
		
		dev_pos = 0;
		for (int b = mem_portions/multi_gpu*d; b < mem_portions/multi_gpu*(d+1); b++){
			pos = (base << qbs_coales) | reg_id;
	
			cudaMemcpy(state + pos*2, gpu_mem[d] + dev_pos, portion_size*2*sizeof(float), cudaMemcpyDeviceToHost); error();
			cudaDeviceSynchronize(); error();
	
			base = (base + inc + 1) & ~inc;
			dev_pos += portion_size; 
		}
	}

	for (int d = 0; d < multi_gpu; d++){
		cudaFree(gpu_mem[d]); error();
	}

	return true;
}

bool error(){
	inst++;
	cudaError_t e;
	e = cudaGetLastError();
	if (e == cudaSuccess) return false;
	printf("inst: %d\nerror: %d - %s\n", inst, e, cudaGetErrorString (e));
	exit(1);
	return true;
}
