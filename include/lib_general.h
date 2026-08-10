#ifndef _LIBGENERAL_H_
#define _LIBGENERAL_H_

// <complex.h> precisa ser incluído antes da define abaixo, senão a
// definição de "complex" daqui contamina a inicialização do <complex.h>
// do sistema quando ele for incluído depois (por common.h/dgm.h).
#include <complex.h>
#define complex __complex__

float HadamardNQubits(long qubits, long iterations, int type, int thread_count, int cpu_region_bits, int cpu_coalesced_bits, int gpu_count, int gpu_region_bits, int gpu_coalesced_bits, int block_size, int repeat_count);

float HadamardNQubits_PAR_CPU(long qubits, long iterations, int thread_count = 1, int cpu_region_bits = 13, int cpu_coalesced_bits = 9);
float HadamardNQubits_GPU(long qubits, long iterations, int gpu_count = 1, int gpu_region_bits = 8, int gpu_coalesced_bits = 4, int block_size = 64, int repeat_count = 2);

#endif
