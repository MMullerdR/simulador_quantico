#include "lib_general.h"
#include "dgm.h"
#include "common.h"
#include "gates.h"
#include <cmath>
#include <iostream>
#include <algorithm>
#include <vector>
#include <string>

using namespace std;

float HadamardNQubits(long qubits, long num_of_it, int type, int n_threads, int cpu_region, int cpu_coales, int multi_gpu, int gpu_region, int gpu_coales, int tam_block, int rept){
	DGM dgm;
	dgm.qubits = qubits;
	dgm.exec_type = type;
 
	dgm.n_threads = n_threads;
	dgm.cpu_region = cpu_region;
	dgm.cpu_coales = cpu_coales;
	
	dgm.multi_gpu = multi_gpu;
	dgm.gpu_region = gpu_region;
	dgm.gpu_coales = gpu_coales;
	dgm.tam_block = tam_block;
	dgm.rept = rept;

	dgm.allocateMemory();
	dgm.setMemoryValue(0);

	string hadamardN = Hadamard(qubits, 0, qubits);
	dgm.setFunction(hadamardN, num_of_it);

	dgm.execute(1);

	printMem(dgm.state, 4);

	dgm.freeMemory();

	return 0;
}


float HadamardNQubits_PAR_CPU(long qubits, long num_of_it, int n_threads, int cpu_region, int cpu_coales){
	return HadamardNQubits(qubits, num_of_it, t_PAR_CPU, n_threads, cpu_region, cpu_coales, 1, 1, 1, 1, 1);
}

float HadamardNQubits_GPU(long qubits, long num_of_it, int multi_gpu, int gpu_region, int gpu_coales, int tam_block, int rept){
	return HadamardNQubits(qubits, num_of_it, t_GPU, 1, 1, 1, multi_gpu, gpu_region, gpu_coales, tam_block, rept);
}
