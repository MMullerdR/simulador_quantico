#ifndef _LIBGENERAL_H_
#define _LIBGENERAL_H_

#define complex _Complex

float HadamardNQubits(long qubits, long num_of_it, int type, int n_threads, int cpu_region, int cpu_coales, int multi_gpu, int gpu_region, int gpu_coales, int tam_block, int rept);

float HadamardNQubits_PAR_CPU(long qubits, long num_of_it, int n_threads = 1, int cpu_region = 13, int cpu_coales = 9);
float HadamardNQubits_GPU(long qubits, long num_of_it, int multi_gpu = 1, int gpu_region = 8, int gpu_coales = 4, int tam_block = 64, int rept = 2);

#endif
