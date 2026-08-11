#include "../../include/cli/cli_common.h"
#include "../../include/core/dgm.h"
#include <cstdlib>

// Bloco repetido nos 3 CLIs: depois de exec_type já validado, decide se
// argv[3] é thread_count (CPU paralela/híbrida) ou gpu_count (GPU).
void parse_backend_arg(int argc, char **argv, int exec_type, TuningDefaults &tuning){
	if (exec_type == t_PAR_CPU || exec_type == t_HYBRID) {
		if (argc > 3) tuning.thread_count = atoi(argv[3]);
	}
	else if (exec_type == t_GPU) {
		if (argc > 3) tuning.gpu_count = atoi(argv[3]);
	}
}
