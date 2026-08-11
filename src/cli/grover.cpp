#include "../../include/algorithms/lib_grover.h"
#include "../../include/core/dgm.h"
#include "../../include/cli/cli_common.h"
#include <vector>
#include <iostream>
#include <sys/time.h>
#include <unistd.h>

using namespace std;

// Busca de Grover: outputs/grover.out <qubits> [exec_type] [threads|gpus] [block_size] [repeat_count] [gpu_region_bits] [search_value]
// block_size/repeat_count/gpu_region_bits só valem pra t_GPU/t_HYBRID.
// search_value precisa caber no registrador de busca (qubits-1 bits).
int main(int argc, char **argv){
	// time(NULL) sozinho repete a mesma semente pra processos lançados
	// dentro do mesmo segundo (comum em scripts/testes chamando o binário
	// várias vezes seguidas) -- DGM::measure() usa rand() pra amostrar a
	// medição, então duas execuções nesse caso reproduziriam exatamente a
	// mesma sequência "aleatória". getpid() garante seeds diferentes entre
	// processos concorrentes mesmo no mesmo segundo (ver mesmo fix em
	// shor.cpp).
	srand(time(NULL) ^ getpid());

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
	parse_gpu_tuning_args(argc, argv, tuning);

	int search_value = 10;
	if (argc > 7) search_value = atoi(argv[7]);

	// O oráculo (Oracle1) só olha os qubits-1 bits do registrador de
	// busca — um search_value fora desse alcance é silenciosamente
	// truncado (bits altos ignorados), então a busca "funcionaria" mas
	// pra um valor diferente do pedido, sem aviso nenhum. Melhor recusar
	// de cara.
	long search_space = 1L << (qubits - 1);
	if (search_value < 0 || search_value >= search_space){
		cout << "Invalid search_value: " << search_value << " (precisa estar entre 0 e " << (search_space - 1) << " pra " << qubits << " qubits)" << endl;
		return 0;
	}

	struct timeval timev, tvBegin, tvEnd;
	float elapsed;

	gettimeofday(&tvBegin, NULL);
	long result = Grover(qubits, search_value, exec_type, tuning.thread_count, tuning.cpu_region_bits, tuning.cpu_coalesced_bits, tuning.gpu_count, tuning.gpu_region_bits, tuning.gpu_coalesced_bits, tuning.block_size, tuning.repeat_count);
	gettimeofday(&tvEnd, NULL);
	timeval_subtract(&timev, &tvEnd, &tvBegin);
	elapsed = timev.tv_sec + (timev.tv_usec / 1000000.0);

	cout << elapsed << endl;

	if (result == search_value) {
		cout << "Found value: " << result << endl;
	}
	else {
		cout << "Failed to find value (measured " << result << ", expected " << search_value << ")" << endl;
	}

	return 0;
}
