#include "../../include/lib_general.h"
#include "../../include/dgm.h"
#include "../../include/common.h"
#include "../../include/gates.h"
#include <cmath>
#include <iostream>
#include <algorithm>
#include <vector>
#include <string>

using namespace std;

float HadamardNQubits(long qubits, long iterations, int type, int thread_count, int cpu_region_bits, int cpu_coalesced_bits, int gpu_count, int gpu_region_bits, int gpu_coalesced_bits, int block_size, int repeat_count){
	DGM dgm;
	dgm.qubits = qubits;
	dgm.exec_type = type;

	dgm.thread_count = thread_count;
	dgm.cpu_region_bits = cpu_region_bits;
	dgm.cpu_coalesced_bits = cpu_coalesced_bits;

	dgm.gpu_count = gpu_count;
	dgm.gpu_region_bits = gpu_region_bits;
	dgm.gpu_coalesced_bits = gpu_coalesced_bits;
	dgm.block_size = block_size;
	dgm.repeat_count = repeat_count;

	dgm.allocateMemory();
	dgm.setMemoryValue(0);

	string hadamard_step = Hadamard(qubits, 0, qubits);
	dgm.setFunction(hadamard_step, iterations);

	dgm.execute(1);

	printMem(dgm.state, 4);

	dgm.freeMemory();

	return 0;
}


float HadamardNQubits_PAR_CPU(long qubits, long iterations, int thread_count, int cpu_region_bits, int cpu_coalesced_bits){
	return HadamardNQubits(qubits, iterations, t_PAR_CPU, thread_count, cpu_region_bits, cpu_coalesced_bits, 1, 1, 1, 1, 1);
}

float HadamardNQubits_GPU(long qubits, long iterations, int gpu_count, int gpu_region_bits, int gpu_coalesced_bits, int block_size, int repeat_count){
	return HadamardNQubits(qubits, iterations, t_GPU, 1, 1, 1, gpu_count, gpu_region_bits, gpu_coalesced_bits, block_size, repeat_count);
}
