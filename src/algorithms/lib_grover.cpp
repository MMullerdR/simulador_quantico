#include "../../include/algorithms/lib_grover.h"
#include "../../include/core/dgm.h"
#include "../../include/core/common.h"
#include "../../include/core/gates.h"
#include <cmath>
#include <iostream>
#include <algorithm>

using namespace std;


// Monta e executa o circuito completo do Grover (preparação + oráculo +
// difusor repetidos ~sqrt(2^(qubits-1)) vezes) e mede o registrador de
// busca — ver docs/05-algoritmo-grover.md.
long Grover(long qubits, long search_value, int type, int thread_count, int cpu_region_bits, int cpu_coalesced_bits, int gpu_count, int gpu_region_bits, int gpu_coalesced_bits, int block_size, int repeat_count){
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
	dgm.setMemoryValue(1L<<(qubits-1));

	string hadamard_step = Hadamard(qubits, 0, qubits);
	string oracle_step = Oracle1(qubits, search_value);
	string controlled_z_step = ControledZ(qubits);
	string flip_target_step = Pauli_X(qubits, qubits-1, 1);

	vector <string> grover_step;

	grover_step.push_back(oracle_step);

	// Difusor: H em todos, inverte a fase de |00...0>, H em todos. O
	// "inverte a fase de |00...0>" usa controles com valor 0 (ControledZ)
	// em vez de um X em cada qubit antes e depois do Z multi-controlado
	// de |11...1>: dá exatamente o mesmo operador, com 2 passadas de X no
	// qubit alvo em vez de 2*(qubits-1) -- ver docs/05-algoritmo-grover.md
	// e item 22 em docs/07-bugs-e-pontos-de-atencao.md.
	for (int qubit_index = 1; qubit_index < qubits; qubit_index++)
		grover_step.push_back(Hadamard(qubits, qubit_index, 1));

	grover_step.push_back(flip_target_step);
	grover_step.push_back(controlled_z_step);
	grover_step.push_back(flip_target_step);

	for (int qubit_index = qubits-1; qubit_index >= 1; qubit_index--)
		grover_step.push_back(Hadamard(qubits, qubit_index, 1));

	int iteration_count = (int) (M_PI/4.0*sqrt((double)(1L<<(qubits-1))));
	long result = 0;

	dgm.setFunction(hadamard_step);
	dgm.setFunction(grover_step, iteration_count, false);

	dgm.execute(1);

	for (int qubit_index = 1; qubit_index < qubits; qubit_index++) {
		result = (result << 1) | dgm.measure(qubit_index);
	}

	return result;
}

string Oracle1(long qubits, long search_value){
	vector <string> step_ops(qubits);
	int ctrl_bit;

	for (int qubit_index = qubits - 1; qubit_index >= 1; qubit_index--){
		ctrl_bit = search_value&1;
		search_value = search_value >> 1;
		if (ctrl_bit) step_ops[qubit_index] = "Control1(1)";
		else step_ops[qubit_index] = "Control1(0)";
	}
	step_ops[0] = "Target1(X)";

	return concatena(step_ops, qubits);

}

string ControledZ(int qubits){
	vector <string> step_ops;
	step_ops.push_back("ID");
	for (int qubit_index = 0; qubit_index < qubits-2; qubit_index++) step_ops.push_back("Control1(0)");
	step_ops.push_back("Target1(Z)");

	return concatena(step_ops, qubits);
}
