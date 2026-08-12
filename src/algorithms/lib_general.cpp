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

	// H em |0...0> em todos os qubits produz uma superposição uniforme --
	// toda amplitude do vetor de estado vale 1/sqrt(2^qubits), inclusive a
	// do estado 0. Imprime só essa (custo O(1), seguro em qualquer qubits)
	// em vez do vetor inteiro (2^qubits linhas, inviável a partir de ~20
	// qubits) -- é o suficiente pra tests/smoke_test.sh conferir a
	// amplitude exata contra o valor esperado.
	printf("%ld:\t%.6f %.6f\n", 0L, crealf(dgm.state[0]), cimagf(dgm.state[0]));

	return 0;
}
