#ifndef _CLI_COMMON_H_
#define _CLI_COMMON_H_

// Bloco repetido nos 3 CLIs (general/grover/shor): depois que exec_type
// já foi validado, decide se argv[3] é thread_count (CPU paralela ou
// híbrida) ou gpu_count (GPU).
void parse_backend_arg(int argc, char **argv, int exec_type, int &thread_count, int &gpu_count);

#endif
