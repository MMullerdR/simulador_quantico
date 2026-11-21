// Benchmark de CPU do C++ (t_CPU / t_PAR_CPU) com as MESMAS cargas do exemplo Rust
// crates/hgm-cpu/examples/bench_par.rs, para comparar os dois lados (criterio 2 da
// fase 3 de docs/09-plano-migracao-rust.md).
//
// uso: bench_par.out QUBITS CARGA THREADS REGIAO COALESCIDA REPS [serial]
//   CARGA: hadamard | phase | mixed
// saida: uma linha "tempo_s=... a0=... a1=... checksum=..." (amplitudes 0 e 12345 e uma
// soma ponderada do vetor inteiro, para conferir que os dois lados calculam o mesmo estado).
//
// Compilar (ver benches/README.md): g++ -O3 -fopenmp -Iinclude benches/cpp/bench_par.cpp
//   <objetos do core> -o bench_par.out
#include "../../include/core/dgm.h"
#include <cstdio>
#include <cstdlib>
#include <string>
#include <vector>
#include <sys/time.h>
using namespace std;

static string join(const vector<string> &tokens){
	string text;
	for (size_t i = 0; i < tokens.size(); i++){
		if (i) text += ",";
		text += tokens[i];
	}
	return text;
}

// Mesma construcao do Rust (workload_steps em bench_par.rs).
static vector<string> workload_steps(const string &name, int qubits){
	vector<string> steps;
	if (name == "hadamard"){
		steps.push_back(join(vector<string>(qubits, "H")));
	}
	else if (name == "phase"){
		vector<string> tokens;
		const char *gates[] = {"R1", "R2", "R3"};
		for (int i = 0; i < qubits; i++) tokens.push_back(gates[i % 3]);
		steps.push_back(join(tokens));
	}
	else{	// mixed
		steps.push_back(join(vector<string>(qubits, "H")));
		const char *gates[] = {"X", "Z", "Y", "R2"};
		for (int s = 0; s < 8; s++){
			vector<string> tokens(qubits, "ID");
			int control = (5 * s + 2) % qubits;
			int target = (7 * s + 3) % qubits;
			if (target == control) target = (target + 1) % qubits;
			tokens[control] = (s % 2) ? "Control1(1)" : "Control1(0)";
			tokens[target] = string("Target1(") + gates[s % 4] + ")";
			steps.push_back(join(tokens));
		}
	}
	return steps;
}

int main(int argc, char **argv){
	if (argc < 7){
		fprintf(stderr, "uso: %s QUBITS CARGA THREADS REGIAO COALESCIDA REPS [serial]\n", argv[0]);
		return 2;
	}
	int qubits = atoi(argv[1]);
	string workload = argv[2];
	long threads = atol(argv[3]);
	long region = atol(argv[4]);
	long coalesced = atol(argv[5]);
	int reps = atoi(argv[6]);
	bool serial = (argc > 7 && string(argv[7]) == "serial");

	DGM dgm;
	dgm.qubits = qubits;
	dgm.exec_type = serial ? t_CPU : t_PAR_CPU;
	dgm.thread_count = threads;
	dgm.cpu_region_bits = region;
	dgm.cpu_coalesced_bits = coalesced;

	dgm.allocateMemory();
	dgm.setMemoryValue(0);
	dgm.setFunction(workload_steps(workload, qubits), 1);

	struct timeval t0, t1;
	gettimeofday(&t0, NULL);
	dgm.execute(reps);
	gettimeofday(&t1, NULL);
	double seconds = (t1.tv_sec - t0.tv_sec) + (t1.tv_usec - t0.tv_usec) / 1e6;

	long probe = 12345 % (1L << qubits);
	// checksum do vetor inteiro (mesma formula do Rust): soma ponderada em double
	double checksum = 0;
	for (long i = 0; i < (1L << qubits); i++)
		checksum += (double) crealf(dgm.state[i]) * (double) (i % 7 + 1) + (double) cimagf(dgm.state[i]) * (double) (i % 5 + 1);
	printf("tempo_s=%.6f a0=%.7f,%.7f a1=%.7f,%.7f checksum=%.6f\n", seconds,
		crealf(dgm.state[0]), cimagf(dgm.state[0]), crealf(dgm.state[probe]), cimagf(dgm.state[probe]), checksum);
	return 0;
}
