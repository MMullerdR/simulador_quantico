#ifndef _CLI_COMMON_H_
#define _CLI_COMMON_H_

// Defaults de tuning repetidos, idênticos, nos 3 CLIs — thread/gpu count e
// os parâmetros de região/coalescimento de CPU e GPU (ver
// docs/01-arquitetura-geral.md). region_bits é validado contra qubits
// centralmente em DGM::validateTuning(), não aqui.
struct TuningDefaults{
	int thread_count = 1;
	int cpu_region_bits = 14;
	int cpu_coalesced_bits = 11;
	int gpu_count = 1;
	int gpu_region_bits = 8;
	int gpu_coalesced_bits = 4;
	int block_size = 64;
	int repeat_count = 2;
};

// Depois que exec_type já foi validado, decide se argv[3] é thread_count
// (CPU paralela ou híbrida) ou gpu_count (GPU).
void parse_backend_arg(int argc, char **argv, int exec_type, TuningDefaults &tuning);

// argv[4]/[5]/[6] opcionais — block_size/repeat_count/gpu_region_bits, só
// fazem sentido pra t_GPU/t_HYBRID (ignorados, mas inofensivos, nos outros
// exec_type). DGM::validateTuning() valida a combinação (item 06 do design
// de arquitetura, ver docs/04-gpu-cuda.md) antes de qualquer execução —
// não precisa repetir a validação aqui.
void parse_gpu_tuning_args(int argc, char **argv, TuningDefaults &tuning);

#endif
