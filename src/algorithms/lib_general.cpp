#include "../../include/algorithms/lib_general.h"
#include "../../include/core/dgm.h"
#include "../../include/core/common.h"
#include "../../include/core/gates.h"
#include <cmath>
#include <iostream>
#include <algorithm>
#include <vector>
#include <string>

using namespace std;

// Benchmark simples: aplica H em todos os qubits (repetido "iterations"
// vezes) e mede o tempo — usado por general.cpp pra comparar backends.
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
