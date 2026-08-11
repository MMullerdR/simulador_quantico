#include "../../include/algorithms/lib_shor.h"
#include "../../include/core/dgm.h"
#include "../../include/cli/cli_common.h"
#include <vector>
#include <iostream>
#include <sys/time.h>
#include <map>

using namespace std;

// Fatoração de Shor: outputs/shor.out <qubits> [exec_type] [threads|gpus]
// qubits precisa ser um dos valores mapeados abaixo (determina o número
// a ser fatorado).
int main(int argc, char** argv){
	map <int, int> factor_target_by_qubit_count;
	factor_target_by_qubit_count[15] = 57;
	factor_target_by_qubit_count[17] = 119;
	factor_target_by_qubit_count[19] = 253;
	factor_target_by_qubit_count[21] = 485;
	factor_target_by_qubit_count[23] = 1017;
	factor_target_by_qubit_count[25] = 2045;
	factor_target_by_qubit_count[27] = 2863;


	srand (time(NULL));

	struct timeval timev, tvBegin, tvEnd;
	float elapsed;
	vector <float> samples;

	int exec_type = t_CPU;
	TuningDefaults tuning;

	if (argc < 2){
		cout << "You need to define the execution parameters" << endl;
		return 0;
	}

	int qubits = atoi(argv[1]);
	if (factor_target_by_qubit_count.count(qubits) == 0){
		cout << "The amount of qubits does not map to a valid number to be factored: " << qubits << endl;
		return 0;
	}

	if (argc > 2) {
		exec_type = atoi(argv[2]);
	}

	if (exec_type < t_CPU || exec_type > t_HYBRID){
		cout << "Invalid execution type: " << exec_type << endl;
		return 0;
	}

	parse_backend_arg(argc, argv, exec_type, tuning);

	vector<int> factors;

	cout << "Executing Shor: " << qubits << " qubits" << endl;

	gettimeofday(&tvBegin, NULL);
	// ATENÇÃO: mesma inconsistência de grover.cpp — passa t_CPU fixo em
	// vez de "exec_type" (parseado/validado acima). Pedir t_PAR_CPU/
	// t_GPU/t_HYBRID na linha de comando não muda o backend realmente
	// usado por Shor(), só os parâmetros de tuning.
	factors = Shor(factor_target_by_qubit_count[qubits], t_CPU, tuning.thread_count, tuning.cpu_region_bits, tuning.cpu_coalesced_bits, tuning.gpu_count, tuning.gpu_region_bits, tuning.gpu_coalesced_bits, tuning.block_size, tuning.repeat_count);
	gettimeofday(&tvEnd, NULL);
	timeval_subtract(&timev, &tvEnd, &tvBegin);
	elapsed = timev.tv_sec + (timev.tv_usec / 1000000.0);

	cout << "Time: " << elapsed << endl;

	if (factors.size() == 2) {
		cout << "Found factors: " << factors[0] << " -- " << factors[1] << endl;
	}
	else {
		cout << "Failed to find factors" << endl;
	}

	return 0;
}
