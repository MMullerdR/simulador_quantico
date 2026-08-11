#include "../../include/algorithms/lib_grover.h"
#include "../../include/core/dgm.h"
#include "../../include/cli/cli_common.h"
#include <vector>
#include <iostream>
#include <sys/time.h>

using namespace std;

// Busca de Grover: outputs/grover.out <qubits> [exec_type] [threads|gpus]
int main(int argc, char **argv){
	srand(time(NULL));

	int exec_type = t_CPU;
	TuningDefaults tuning;

	if (argc < 2){
		cout << "You need to define the execution parameters" << endl;
		return 0;
	}

	int qubits = atoi(argv[1]);
	// Sem isso, um qubits absurdo (ou negativo) tenta alocar 2^qubits
	// complexos sem aviso nenhum — mesmo problema de general.cpp/shor.cpp.
	if (qubits < 1 || qubits > QB_LIMIT){
		cout << "Invalid qubit count: " << qubits << " (precisa estar entre 1 e " << QB_LIMIT << ")" << endl;
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

	int search_value = 10;

	float elapsed = Grover(qubits, search_value, exec_type, tuning.thread_count, tuning.cpu_region_bits, tuning.cpu_coalesced_bits, tuning.gpu_count, tuning.gpu_region_bits, tuning.gpu_coalesced_bits, tuning.block_size, tuning.repeat_count);

  cout << elapsed << endl;

}
