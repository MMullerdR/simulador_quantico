#include "../../include/lib_general.h"
#include "../../include/dgm.h"
#include <vector>
#include <iostream>
#include <sys/time.h>

using namespace std;

int main(int argc, char **argv){
	int thread_count = 1, cpu_region_bits = 14, cpu_coalesced_bits = 11, gpu_count = 1, gpu_region_bits = 8, gpu_coalesced_bits = 4, block_size = 64, repeat_count = 2;

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

	if (exec_type == t_PAR_CPU) {
		if (argc > 3) thread_count = atoi(argv[3]);
	}
	else if (exec_type == t_GPU) {
		if (argc > 3) gpu_count = atoi(argv[3]);
	}
	else if (exec_type == t_HYBRID) {
		if (argc > 3) thread_count = atoi(argv[3]);
	}

	vector <float> samples;

	struct timeval timev, tvBegin, tvEnd;
	float elapsed;
	long iterations = 3;

	gettimeofday(&tvBegin, NULL);
	HadamardNQubits(qubits, iterations, exec_type, thread_count, cpu_region_bits, cpu_coalesced_bits, gpu_count, gpu_region_bits, gpu_coalesced_bits, block_size, repeat_count);
	gettimeofday(&tvEnd, NULL);

	timeval_subtract(&timev, &tvEnd, &tvBegin);
	elapsed = timev.tv_sec + (timev.tv_usec / 1000000.0);

	cout << elapsed << endl;

	return 0;
}
