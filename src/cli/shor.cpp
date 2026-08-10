#include "../../include/lib_shor.h"
#include "../../include/dgm.h"
#include <vector>
#include <iostream>
#include <sys/time.h>
#include <map>

using namespace std;

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

	int exec_type = t_CPU, thread_count = 1, cpu_region_bits = 14, cpu_coalesced_bits = 11, gpu_count = 1, gpu_region_bits = 8, gpu_coalesced_bits = 4, block_size = 64, repeat_count = 2;

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

	if (exec_type == t_PAR_CPU) {
		if (argc > 3) thread_count = atoi(argv[3]);
	}
	else if (exec_type == t_GPU) {
		if (argc > 3) gpu_count = atoi(argv[3]);
	}
	else if (exec_type == t_HYBRID) {
		if (argc > 3) thread_count = atoi(argv[3]);
	}

	vector<int> factors;

	cout << "Executing Shor: " << qubits << " qubits" << endl;

	gettimeofday(&tvBegin, NULL);
	factors = Shor(factor_target_by_qubit_count[qubits], t_CPU, thread_count, cpu_region_bits, cpu_coalesced_bits, gpu_count, gpu_region_bits, gpu_coalesced_bits, block_size, repeat_count);
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
