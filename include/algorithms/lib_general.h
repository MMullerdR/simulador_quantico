#ifndef _LIBGENERAL_H_
#define _LIBGENERAL_H_

// <complex.h> precisa ser incluído antes da define abaixo, senão a
// definição de "complex" daqui contamina a inicialização do <complex.h>
// do sistema quando ele for incluído depois (por common.h/dgm.h).
#include <complex.h>
#define complex __complex__

// Benchmark: aplica H em todos os qubits, "iterations" vezes seguidas
// (usado por general.cpp).
float HadamardNQubits(long qubits, long iterations, int type, int thread_count, int cpu_region_bits, int cpu_coalesced_bits, int gpu_count, int gpu_region_bits, int gpu_coalesced_bits, int block_size, int repeat_count);

#endif
