#include "lib_grover.h"
#include "dgm.h"
#include "common.h"
#include "gates.h"
#include <cmath>
#include <iostream>
#include <algorithm>

using namespace std;


float Grover(long qubits, long value, int type, int n_threads, int cpu_region, int cpu_coales, int multi_gpu, int gpu_region, int gpu_coales, int tam_block, int rept){
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
	dgm.setMemoryValue(1<<(qubits-1));
	
	string H = Hadamard(qubits, 0, qubits);
	string orcl = Oracle1(qubits, value);
	string CZ = ControledZ(qubits);

	vector <string> grover_step;

	grover_step.push_back(orcl);
	
	for (int i = 1; i < qubits; i++){
		grover_step.push_back(Hadamard(qubits, i, 1));
		grover_step.push_back(Pauli_X(qubits, i, 1));
	}
	
	grover_step.push_back(CZ);

	for (int i = qubits-1; i >= 1; i--){
		grover_step.push_back(Pauli_X(qubits, i, 1));
		grover_step.push_back(Hadamard(qubits, i, 1));
	}

	int num_of_it = (int) (M_PI/4.0*sqrt(1<<(qubits-1)));
	long result = 0;

	dgm.setFunction(H);
	dgm.setFunction(grover_step, num_of_it, false);

	
	struct timeval timev, tvBegin, tvEnd;
	gettimeofday(&tvBegin, NULL);
	
	dgm.execute(1);

	for (int i = 1; i < qubits; i++) {
		result = (result << 1) | dgm.measure(i);
	}

	gettimeofday(&tvEnd, NULL);
	timeval_subtract(&timev, &tvEnd, &tvBegin);
	float t = timev.tv_sec + (timev.tv_usec / 1000000.0);

	dgm.freeMemory();

	return t;
}

string Oracle1(long qubits, long int value){
	vector <string> t(qubits);
	int ctrl_v;

	for (int i = qubits - 1; i >= 1; i--){
		ctrl_v = value&1;
		value = value >> 1;
		if (ctrl_v) t[i] = "Control1(1)";
		else t[i] = "Control1(0)";
	}
	t[0] = "Target1(X)";

	return concatena(t, qubits);

}

string ControledZ(int qubits){
	vector <string> cz;
	cz.push_back("ID");
	for (int i = 0; i < qubits-2; i++) cz.push_back("Control1(1)");
	cz.push_back("Target1(Z)");

	return concatena(cz, qubits);
}
