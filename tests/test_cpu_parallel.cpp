// Confere o motor t_PAR_CPU (PCpuExecution1/PCpuExecution1_0, regiões com
// loops vetorizados) contra o motor serial t_CPU (CpuExecution1_*), que
// serve de referência: os dois precisam produzir o mesmo vetor de estado
// pra qualquer circuito. Parte de um estado aleatório (não de |0...0>)
// pra que um erro de índice não passe despercebido por simetria, e
// sorteia circuitos com portas densas (H), diagonais (Z, R1-R3),
// anti-diagonais (X, Y), com e sem controles, em várias combinações de
// qubits / cpu_region_bits / cpu_coalesced_bits / threads -- o que cobre
// alvo dentro e fora da faixa de bits baixos contíguos da região,
// controles dentro e fora da região, e coalesced_bits = 0.
//
// Convenção de bit e formato de step: ver test_gates.cpp e
// docs/02-linguagem-de-circuitos.md.
#include "../include/core/dgm.h"
#include "../include/core/gates.h"
#include <iostream>
#include <cmath>
#include <random>
using namespace std;

// H, X, Y, Z, R1-R3 vêm do catálogo; as outras são registradas em
// run_case(): RX e U são densas com parte imaginária (H é a única densa do
// catálogo e tem a matriz toda real), DG é diagonal com m00 != 1 (Z e
// R1-R3 têm m00 = 1) e AD é anti-diagonal com entradas que não são 1 (X
// troca sem multiplicar).
static const char *PLAIN_GATES[] = {"H", "X", "Y", "Z", "R1", "R2", "R3", "RX", "U", "DG", "AD"};
static const int PLAIN_GATE_COUNT = 11;

// Um step aleatório: ou algumas portas soltas em qubits diferentes, ou uma
// porta controlada por 1 a 3 qubits.
static string random_step(int qubits, mt19937 &rng){
	vector <string> tokens(qubits, "ID");
	if (rng() % 2 == 0){
		int gate_count = 1 + rng() % 3;
		for (int i = 0; i < gate_count; i++)
			tokens[rng() % qubits] = PLAIN_GATES[rng() % PLAIN_GATE_COUNT];
	}
	else{
		int target = rng() % qubits;
		tokens[target] = string("Target1(") + PLAIN_GATES[rng() % PLAIN_GATE_COUNT] + ")";
		int control_count = 1 + rng() % 3;
		for (int i = 0; i < control_count; i++){
			int control = rng() % qubits;
			if (control == target) continue;
			tokens[control] = (rng() % 2) ? "Control1(1)" : "Control1(0)";
		}
	}
	return concatena(tokens, qubits);
}

static void fill_random_state(DGM &dgm, mt19937 &rng){
	uniform_real_distribution<float> dist(-1.0f, 1.0f);
	long size = 1L << dgm.qubits;
	double norm = 0;
	for (long i = 0; i < size; i++){
		float re = dist(rng), im = dist(rng);
		dgm.state[i] = re + im * 1.0fi;
		norm += (double)re * re + (double)im * im;
	}
	float scale = 1.0f / sqrt(norm);
	for (long i = 0; i < size; i++) dgm.state[i] *= scale;
}

static bool run_case(int qubits, int region_bits, int coalesced_bits, int threads, int step_count, unsigned seed){
	// TEST_VERBOSE=1 mostra cada caso antes de rodar -- útil se algum travar.
	if (getenv("TEST_VERBOSE"))
		cerr << "caso q=" << qubits << " r=" << region_bits << " c=" << coalesced_bits << " t=" << threads << " seed=" << seed << endl;
	mt19937 rng(seed);
	vector <string> steps;
	for (int i = 0; i < step_count; i++) steps.push_back(random_step(qubits, rng));

	DGM ref, par;
	ref.qubits = par.qubits = qubits;
	ref.exec_type = t_CPU;
	par.exec_type = t_PAR_CPU;
	par.thread_count = threads;
	par.cpu_region_bits = region_bits;
	par.cpu_coalesced_bits = coalesced_bits;
	ref.allocateMemory();
	par.allocateMemory();

	float cos_a = cosf(0.7f), sin_a = sinf(0.7f), cos_b = cosf(0.3f), sin_b = sinf(0.3f);
	float complex phase = cosf(1.1f) + sinf(1.1f) * I;
	for (DGM *dgm : {&ref, &par}){
		dgm->gates.addGate("RX", cos_a, -sin_a * I, -sin_a * I, cos_a);
		dgm->gates.addGate("U", cos_b, sin_b * phase, -sin_b * conjf(phase), cos_b);
		dgm->gates.addGate("DG", phase, 0.0f, 0.0f, conjf(phase) * phase * phase);
		dgm->gates.addGate("AD", 0.0f, phase, conjf(phase) * 0.6f, 0.0f);
	}

	mt19937 state_rng(seed ^ 0x9e3779b9u);
	fill_random_state(ref, state_rng);
	for (long i = 0; i < (1L << qubits); i++) par.state[i] = ref.state[i];

	ref.setFunction(steps);
	par.setFunction(steps);
	ref.execute(1);
	par.execute(1);

	double max_diff = 0;
	long worst = 0;
	for (long i = 0; i < (1L << qubits); i++){
		double diff = cabsf(ref.state[i] - par.state[i]);
		if (diff > max_diff){ max_diff = diff; worst = i; }
	}

	bool ok = max_diff < 2e-6;
	if (!ok)
		cout << "FAIL qubits=" << qubits << " region=" << region_bits << " coalesced=" << coalesced_bits
			<< " threads=" << threads << " seed=" << seed << ": maior diferença " << max_diff
			<< " no índice " << worst << endl;
	ref.freeMemory();
	par.freeMemory();
	return ok;
}

int main(){
	struct Combo{ int qubits, region_bits, coalesced_bits, threads; };
	// (qubits, region, coalesced, threads): região menor que os qubits
	// (várias regiões globais), região == qubits, coalesced = 0, coalesced
	// == region, 1 e vários threads.
	Combo combos[] = {
		{10, 6, 3, 1}, {10, 6, 3, 3}, {12, 8, 4, 2}, {12, 7, 0, 2}, {12, 10, 5, 4},
		{13, 9, 6, 3}, {11, 11, 4, 2}, {9, 5, 5, 2}, {12, 12, 2, 1}, {13, 8, 1, 4},
		{14, 11, 11, 2}, {14, 10, 3, 3},
		// defaults dos CLIs (region 14, coalesced 11) em circuitos menores
		// que a própria região/faixa coalescida
		{10, 14, 11, 2}, {6, 14, 11, 3}, {12, 14, 11, 4}
	};

	bool all_ok = true;
	int case_count = 0;
	for (const Combo &combo : combos){
		for (unsigned seed = 1; seed <= 6; seed++){
			all_ok = run_case(combo.qubits, combo.region_bits, combo.coalesced_bits, combo.threads, 40, seed * 7919 + combo.qubits) && all_ok;
			case_count++;
		}
	}

	cout << (all_ok ? "OK" : "FAIL") << " test_cpu_parallel: " << case_count << " circuitos aleatórios, t_PAR_CPU vs t_CPU" << endl;
	return all_ok ? 0 : 1;
}
