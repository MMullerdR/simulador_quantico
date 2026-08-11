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

// block_size/repeat_count/gpu_region_bits eram fixos em TuningDefaults
// (item 06 do design de arquitetura removeu a limitação de kernel.cu que
// só permitia essa única combinação em tempo de execução — ver
// docs/04-gpu-cuda.md). Expostos aqui pra quem quiser explorar tuning de
// GPU por linha de comando.
void parse_gpu_tuning_args(int argc, char **argv, TuningDefaults &tuning){
	if (argc > 4) tuning.block_size = atoi(argv[4]);
	if (argc > 5) tuning.repeat_count = atoi(argv[5]);
	if (argc > 6) tuning.gpu_region_bits = atoi(argv[6]);
}
