// Execuções completas do C++ com semente fixa (fase 5 de docs/09-plano-migracao-rust.md).
//
// Roda Grover() e Shor() no t_CPU com g_rng semeado e grava o que cada execução
// devolveu em tests/golden/data/runs.txt. O Rust reproduz a mesma semente (mesmo
// MT19937, mesma uniform_int_distribution do libstdc++, mesma regra de medição) e tem
// que chegar ao mesmo valor: é a prova de ponta a ponta de que os 3 executáveis
// calculam o mesmo que o C++, não só que cada peça confere isolada.
//
// Formato (uma execução por linha; `#` abre comentário):
//   grover qubits=Q value=V seed=S result=R
//   shor n=N seed=S emulate=0|1 aqft=-|D factors=A,B|none
//
// Uso: make golden-runs (ou outputs/export_runs.out tests/golden/data/runs.txt).
#include "../../include/core/dgm.h"
#include "../../include/core/common.h"
#include "../../include/algorithms/lib_shor.h"
#include "../../include/algorithms/lib_grover.h"
#include <cstdio>
#include <cstdlib>
#include <vector>
using namespace std;

int main(int argc, char **argv){
	if (argc < 2){
		fprintf(stderr, "uso: %s ARQUIVO_DE_SAIDA\n", argv[0]);
		return 2;
	}
	FILE *out = fopen(argv[1], "w");
	if (!out){
		perror(argv[1]);
		return 1;
	}
	fprintf(out, "format 1\n");
	fprintf(out, "# Gerado por tests/golden/export_runs.cpp (t_CPU, g_rng semeado com `seed`).\n");

	// Grover: (qubits, valor buscado), sementes 1..8.
	struct GroverRun{ long qubits, value; };
	GroverRun grover_runs[] = {{4, 5}, {6, 21}, {8, 100}, {10, 300}, {12, 1234}};
	for (const GroverRun &run : grover_runs){
		for (int seed = 1; seed <= 8; seed++){
			seed_rng(seed);
			long result = Grover(run.qubits, run.value, t_CPU, 1, 14, 11, 1, 12, 4, 512, 4);
			fprintf(out, "grover qubits=%ld value=%ld seed=%d result=%ld\n", run.qubits, run.value, seed, result);
		}
	}

	// Shor: número, emulação da multiplicação modular, grau da QFT (-1 = padrão por
	// largura) e quantas sementes (1..seeds).
	struct ShorRun{ long number; int emulate; int aqft; int seeds; };
	ShorRun shor_runs[] = {
		{15, 0, -1, 12}, {15, 1, -1, 12}, {15, 0, 0, 6},
		{21, 0, -1, 4}, {21, 1, -1, 8},
		{35, 1, -1, 8}, {57, 1, -1, 8}, {119, 1, -1, 6}, {253, 1, -1, 4},
	};
	for (const ShorRun &run : shor_runs){
		setenv("SHOR_EMULATE", run.emulate ? "1" : "0", 1);
		if (run.aqft >= 0){
			char degree[16];
			snprintf(degree, sizeof degree, "%d", run.aqft);
			setenv("SHOR_AQFT", degree, 1);
		}
		else{
			unsetenv("SHOR_AQFT");
		}
		for (int seed = 1; seed <= run.seeds; seed++){
			seed_rng(seed);
			vector<int> factors = Shor(run.number, t_CPU, 1, 14, 11, 1, 12, 4, 512, 4);
			fprintf(out, "shor n=%ld seed=%d emulate=%d aqft=", run.number, seed, run.emulate);
			if (run.aqft >= 0) fprintf(out, "%d", run.aqft);
			else fprintf(out, "-");
			if (factors.size() == 2) fprintf(out, " factors=%d,%d\n", factors[0], factors[1]);
			else fprintf(out, " factors=none\n");
			fflush(out);
		}
	}

	fclose(out);
	return 0;
}
