#include "../../include/algorithms/lib_general.h"
#include "../../include/core/dgm.h"
#include "../../include/cli/cli_common.h"
#include <vector>
#include <iostream>
#include <sys/time.h>

using namespace std;

// Benchmark de Hadamard: outputs/general.out <qubits> <exec_type> [threads|gpus]
// exec_type aqui exige t_PAR_CPU/t_GPU/t_HYBRID (t_CPU não é aceito).
int main(int argc, char **argv){
	TuningDefaults tuning;

	if (argc < 3){
		cout << "You need to define the execution parameters" << endl;
		return 0;
	}

	int qubits = atoi(argv[1]);

	int exec_type = atoi(argv[2]);
	if (exec_type < t_PAR_CPU || exec_type > t_HYBRID){
		cout << "Invalid execution type: " << exec_type << endl;
		return 0;
	}

	parse_backend_arg(argc, argv, exec_type, tuning);

	vector <float> samples;

	struct timeval timev, tvBegin, tvEnd;
	float elapsed;
	long iterations = 3;

	gettimeofday(&tvBegin, NULL);
	HadamardNQubits(qubits, iterations, exec_type, tuning.thread_count, tuning.cpu_region_bits, tuning.cpu_coalesced_bits, tuning.gpu_count, tuning.gpu_region_bits, tuning.gpu_coalesced_bits, tuning.block_size, tuning.repeat_count);
	gettimeofday(&tvEnd, NULL);

	timeval_subtract(&timev, &tvEnd, &tvBegin);
	elapsed = timev.tv_sec + (timev.tv_usec / 1000000.0);

	cout << elapsed << endl;

	return 0;
}
