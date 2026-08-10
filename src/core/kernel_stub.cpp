#include "../../include/core/common.h"
#include <cstdio>

// Stub temporário no lugar de kernel.cu: implementa as mesmas funções
// extern "C" declaradas em dgm.h, mas compilado com g++ (não nvcc) e sem
// nenhuma GPU de verdade por trás. Existe só porque o nvcc está
// anormalmente lento nesta máquina (investigar depois com calma) e o
// projeto precisa compilar agora pra testar os backends de CPU
// (t_CPU/t_PAR_CPU), que são os únicos usáveis sem uma GPU NVIDIA real.
//
// Não chame o backend t_GPU/t_HYBRID enquanto este stub estiver em uso —
// vai imprimir o aviso abaixo e sair. Pra voltar a usar o kernel.cu real,
// compile com "make GPU=real" (ver makefile).

extern "C" bool setDevice(int device_id){
	fprintf(stderr, "kernel_stub.cpp: GPU indisponível nesta build.\n");
	return false;
}

extern "C" float complex* GpuExecutionWrapper(float complex* read_memory, PT **pts, int qubits, int coalesced_bits, int gpu_region_bits, int gpu_count, int block_size, int repeat_count, int iterations){
	fprintf(stderr, "kernel_stub.cpp: GPU indisponível nesta build (compile com \"make GPU=real\" pra usar o kernel.cu de verdade).\n");
	return read_memory;
}

extern "C" bool ProjectState(float complex* state, int qubits, int region_size, long region_id, long region_mask, int gpu_count){
	fprintf(stderr, "kernel_stub.cpp: GPU indisponível nesta build.\n");
	return false;
}

extern "C" bool GetState(float complex* state, int qubits, int region_size, long region_id, long region_mask, int gpu_count){
	fprintf(stderr, "kernel_stub.cpp: GPU indisponível nesta build.\n");
	return false;
}
