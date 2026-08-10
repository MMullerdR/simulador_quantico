#include "../../include/cli/cli_common.h"
#include "../../include/core/dgm.h"
#include <cstdlib>

void parse_backend_arg(int argc, char **argv, int exec_type, int &thread_count, int &gpu_count){
	if (exec_type == t_PAR_CPU || exec_type == t_HYBRID) {
		if (argc > 3) thread_count = atoi(argv[3]);
	}
	else if (exec_type == t_GPU) {
		if (argc > 3) gpu_count = atoi(argv[3]);
	}
}
