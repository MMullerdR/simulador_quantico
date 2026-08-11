#include "../../include/algorithms/lib_shor.h"
#include "../../include/core/dgm.h"
#include "../../include/cli/cli_common.h"
#include <vector>
#include <iostream>
#include <sys/time.h>
#include <map>
#include <unistd.h>

using namespace std;

// Fatoração de Shor: outputs/shor.out <qubits> [exec_type] [threads|gpus] [block_size] [repeat_count] [gpu_region_bits]
// qubits precisa ser um dos valores mapeados abaixo (determina o número
// a ser fatorado). block_size/repeat_count/gpu_region_bits só valem pra
// t_GPU/t_HYBRID.
int main(int argc, char** argv){
	map <int, int> factor_target_by_qubit_count;
	factor_target_by_qubit_count[15] = 57;
	factor_target_by_qubit_count[17] = 119;
	factor_target_by_qubit_count[19] = 253;
	factor_target_by_qubit_count[21] = 485;
	factor_target_by_qubit_count[23] = 1017;
	factor_target_by_qubit_count[25] = 2045;
	factor_target_by_qubit_count[27] = 2863;


	// time(NULL) sozinho repete a mesma semente pra processos lançados
	// dentro do mesmo segundo (comum em scripts/testes chamando o binário
	// várias vezes seguidas) — Shor() escolhe base_value via g_rng, então
	// duas execuções "aleatórias" nesse caso reproduziriam exatamente a
	// mesma sequência de tentativas. getpid() garante seeds diferentes
	// entre processos concorrentes mesmo no mesmo segundo. seed_rng() em
	// vez de srand(): ver g_rng em common.h/common.cpp e item 18 em
	// docs/07-bugs-e-pontos-de-atencao.md.
	seed_rng(time(NULL) ^ getpid());

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
	// Já implicitamente limitado a valores seguros (15-27, todos < QB_LIMIT)
	// pelo map acima — diferente de general.cpp/grover.cpp, não precisa
	// de uma checagem de QB_LIMIT separada.
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
	parse_gpu_tuning_args(argc, argv, tuning);

	vector<int> factors;

	cout << "Executing Shor: " << qubits << " qubits" << endl;

	gettimeofday(&tvBegin, NULL);
	factors = Shor(factor_target_by_qubit_count[qubits], exec_type, tuning.thread_count, tuning.cpu_region_bits, tuning.cpu_coalesced_bits, tuning.gpu_count, tuning.gpu_region_bits, tuning.gpu_coalesced_bits, tuning.block_size, tuning.repeat_count);
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
