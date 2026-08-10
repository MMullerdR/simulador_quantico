#include "../../include/algorithms/lib_grover.h"
#include "../../include/core/dgm.h"
#include "../../include/cli/cli_common.h"
#include <vector>
#include <iostream>
#include <sys/time.h>

using namespace std;

int main(int argc, char **argv){
	srand(time(NULL));

	int exec_type = t_CPU, thread_count = 1, cpu_region_bits = 14, cpu_coalesced_bits = 11, gpu_count = 1, gpu_region_bits = 8, gpu_coalesced_bits = 4, block_size = 64, repeat_count = 2;

	if (argc < 2){
		cout << "You need to define the execution parameters" << endl;
		return 0;
	}

	int qubits = atoi(argv[1]);


  if (argc > 2) {
    exec_type = atoi(argv[2]);
  }

	if (exec_type < t_CPU || exec_type > t_HYBRID){
		cout << "Invalid execution type: " << exec_type << endl;
		return 0;
	}

	parse_backend_arg(argc, argv, exec_type, thread_count, gpu_count);

	int search_value = 10;

	float elapsed = Grover(qubits, search_value, t_CPU, thread_count, cpu_region_bits, cpu_coalesced_bits, gpu_count, gpu_region_bits, gpu_coalesced_bits, block_size, repeat_count);

  cout << elapsed << endl;

}
