// Confere os motores otimizados contra o motor serial t_CPU
// (CpuExecution1_*), que serve de referência: todos precisam produzir o
// mesmo vetor de estado pra qualquer circuito. Três grupos de casos:
//  1. t_PAR_CPU (PCpuExecution1/PCpuExecution1_0, regiões com loops
//     vetorizados), inclusive com "iterations" > 1;
//  2. ScheduleForBatching (reordenação de portas que comutam pra GPU): o
//     t_CPU na ordem reordenada tem que dar o mesmo estado que na ordem
//     original -- confere a regra de comutação numericamente, sem GPU;
//  3. t_GPU contra t_CPU, só quando o build tem kernel.cu de verdade
//     (GpuMinFreeBytes >= 0; o stub devolve -1);
//  4. FusePhaseRuns (portas de fase fundidas): t_CPU e t_PAR_CPU depois da
//     fusão contra o t_CPU na lista original.
//
// Grupo 1 em detalhe: os dois precisam produzir o mesmo vetor de estado
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

// Portas extras registradas em cada DGM do teste (ver comentário de PLAIN_GATES).
static void register_test_gates(DGM &dgm){
	float cos_a = cosf(0.7f), sin_a = sinf(0.7f), cos_b = cosf(0.3f), sin_b = sinf(0.3f);
	float complex phase = cosf(1.1f) + sinf(1.1f) * I;
	dgm.gates.addGate("RX", cos_a, -sin_a * I, -sin_a * I, cos_a);
	dgm.gates.addGate("U", cos_b, sin_b * phase, -sin_b * conjf(phase), cos_b);
	dgm.gates.addGate("DG", phase, 0.0f, 0.0f, conjf(phase) * phase * phase);
	dgm.gates.addGate("AD", 0.0f, phase, conjf(phase) * 0.6f, 0.0f);
	// PH: diag(1, e^{i*2*pi*37/2^16}) -- múltiplo do passo de fase fundida,
	// elegível. PHN: ângulo que NÃO é múltiplo, tem que ficar como porta normal.
	dgm.gates.addGate("PH", 1.0f, 0.0f, 0.0f, cosf(2 * M_PI * 37.0 / 65536.0) + sinf(2 * M_PI * 37.0 / 65536.0) * I);
	dgm.gates.addGate("PHN", 1.0f, 0.0f, 0.0f, cosf(0.123456f) + sinf(0.123456f) * I);
}

// Step rico em portas de fase (corridas longas para a fusão): várias portas de
// fase soltas, uma porta de fase controlada, ou (de vez em quando) uma porta
// que quebra a corrida.
static string random_phase_step(int qubits, mt19937 &rng){
	static const char *PHASES[] = {"Z", "R1", "R2", "R3", "PH", "PH", "PHN"};
	vector <string> tokens(qubits, "ID");
	int kind = rng() % 10;
	if (kind < 4){
		int gate_count = 1 + rng() % 4;
		for (int i = 0; i < gate_count; i++)
			tokens[rng() % qubits] = PHASES[rng() % 7];
	}
	else if (kind < 8){
		int target = rng() % qubits;
		tokens[target] = string("Target1(") + PHASES[rng() % 7] + ")";
		int control_count = 1 + rng() % 3;
		for (int i = 0; i < control_count; i++){
			int control = rng() % qubits;
			if (control == target) continue;
			tokens[control] = (rng() % 4) ? "Control1(1)" : "Control1(0)";
		}
	}
	else{
		tokens[rng() % qubits] = (rng() % 2) ? "H" : "X";
	}
	return concatena(tokens, qubits);
}

static double max_state_diff(DGM &a, DGM &b, long &worst){
	double max_diff = 0;
	worst = 0;
	for (long i = 0; i < (1L << a.qubits); i++){
		double diff = cabsf(a.state[i] - b.state[i]);
		if (diff > max_diff){ max_diff = diff; worst = i; }
	}
	return max_diff;
}

static bool run_case(int qubits, int region_bits, int coalesced_bits, int threads, int step_count, unsigned seed, int iterations = 1){
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

	register_test_gates(ref);
	register_test_gates(par);

	mt19937 state_rng(seed ^ 0x9e3779b9u);
	fill_random_state(ref, state_rng);
	for (long i = 0; i < (1L << qubits); i++) par.state[i] = ref.state[i];

	ref.setFunction(steps);
	par.setFunction(steps);
	ref.execute(iterations);
	par.execute(iterations);

	long worst;
	double max_diff = max_state_diff(ref, par, worst);

	bool ok = max_diff < 2e-6 * iterations;
	if (!ok)
		cout << "FAIL qubits=" << qubits << " region=" << region_bits << " coalesced=" << coalesced_bits
			<< " threads=" << threads << " iterations=" << iterations << " seed=" << seed << ": maior diferença " << max_diff
			<< " no índice " << worst << endl;
	ref.freeMemory();
	par.freeMemory();
	return ok;
}

// Grupo 2: o t_CPU na ordem que ScheduleForBatching devolve precisa dar o
// mesmo estado que na ordem original.
static bool run_schedule_case(int qubits, int coalesced_bits, int extra_bits, int step_count, unsigned seed){
	mt19937 rng(seed);
	vector <string> steps;
	for (int i = 0; i < step_count; i++) steps.push_back(random_step(qubits, rng));

	DGM original, scheduled;
	original.qubits = scheduled.qubits = qubits;
	original.exec_type = scheduled.exec_type = t_CPU;
	original.allocateMemory();
	scheduled.allocateMemory();
	register_test_gates(original);
	register_test_gates(scheduled);

	mt19937 state_rng(seed ^ 0x85ebca6bu);
	fill_random_state(original, state_rng);
	for (long i = 0; i < (1L << qubits); i++) scheduled.state[i] = original.state[i];

	original.setFunction(steps);
	scheduled.setFunction(steps);

	long op_count = 0;
	while (scheduled.pts[op_count] != NULL) op_count++;
	vector <PT*> before(scheduled.pts, scheduled.pts + op_count);

	ScheduleForBatching(scheduled.pts, coalesced_bits, extra_bits, GpuMaxBatchOps());

	// tem que ser uma permutação dos mesmos PT
	vector <PT*> after(scheduled.pts, scheduled.pts + op_count);
	vector <PT*> sorted_before(before), sorted_after(after);
	sort(sorted_before.begin(), sorted_before.end());
	sort(sorted_after.begin(), sorted_after.end());
	bool ok = (sorted_before == sorted_after) && scheduled.pts[op_count] == NULL;
	if (!ok) cout << "FAIL schedule: a saída não é uma permutação da entrada (seed=" << seed << ")" << endl;

	original.execute(1);
	scheduled.execute(1);

	long worst;
	double max_diff = max_state_diff(original, scheduled, worst);
	if (max_diff >= 1e-5){
		cout << "FAIL schedule qubits=" << qubits << " coalesced=" << coalesced_bits << " extra=" << extra_bits
			<< " seed=" << seed << ": maior diferença " << max_diff << " no índice " << worst << endl;
		ok = false;
	}
	original.freeMemory();
	scheduled.freeMemory();
	return ok;
}

// Grupo 3: t_GPU (com a reordenação ligada, que é o padrão) contra t_CPU.
static bool run_gpu_case(int qubits, int coalesced_bits, int region_bits, int block_size, int repeat_count, int step_count, unsigned seed, bool phase_rich = false){
	mt19937 rng(seed);
	vector <string> steps;
	for (int i = 0; i < step_count; i++) steps.push_back(phase_rich ? random_phase_step(qubits, rng) : random_step(qubits, rng));

	DGM ref, gpu;
	ref.qubits = gpu.qubits = qubits;
	ref.exec_type = t_CPU;
	gpu.exec_type = t_GPU;
	gpu.gpu_coalesced_bits = coalesced_bits;
	gpu.gpu_region_bits = region_bits;
	gpu.block_size = block_size;
	gpu.repeat_count = repeat_count;
	ref.allocateMemory();
	gpu.allocateMemory();
	register_test_gates(ref);
	register_test_gates(gpu);

	mt19937 state_rng(seed ^ 0xc2b2ae35u);
	fill_random_state(ref, state_rng);
	for (long i = 0; i < (1L << qubits); i++) gpu.state[i] = ref.state[i];

	ref.setFunction(steps);
	gpu.setFunction(steps);
	ref.execute(1);
	gpu.execute(1);

	long worst;
	double max_diff = max_state_diff(ref, gpu, worst);
	bool ok = max_diff < 1e-5;
	if (!ok)
		cout << "FAIL gpu qubits=" << qubits << " coalesced=" << coalesced_bits << " region=" << region_bits
			<< " block=" << block_size << " repeat=" << repeat_count << " seed=" << seed
			<< ": maior diferença " << max_diff << " no índice " << worst << endl;
	ref.freeMemory();
	gpu.freeMemory();
	return ok;
}

// Grupo 4: portas de fase fundidas (CPU). fuse_exec = backend que roda a lista
// fundida (t_CPU ou t_PAR_CPU); a referência é sempre o t_CPU sem fusão.
static bool run_fuse_case(int qubits, int fuse_exec, int region_bits, int coalesced_bits, int step_count, unsigned seed, long *absorbed_total){
	mt19937 rng(seed);
	vector <string> steps;
	for (int i = 0; i < step_count; i++) steps.push_back(random_phase_step(qubits, rng));

	DGM ref, fused;
	ref.qubits = fused.qubits = qubits;
	ref.exec_type = t_CPU;
	fused.exec_type = fuse_exec;
	fused.thread_count = 2;
	fused.cpu_region_bits = region_bits;
	fused.cpu_coalesced_bits = coalesced_bits;
	ref.allocateMemory();
	fused.allocateMemory();
	register_test_gates(ref);
	register_test_gates(fused);

	mt19937 state_rng(seed ^ 0x27d4eb2fu);
	fill_random_state(ref, state_rng);
	for (long i = 0; i < (1L << qubits); i++) fused.state[i] = ref.state[i];

	ref.setFunction(steps);
	fused.setFunction(steps);
	*absorbed_total += FusePhaseRuns(fused.vec_pts);
	fused.pts = &fused.vec_pts[0];

	ref.execute(1);
	fused.execute(1);

	long worst;
	double max_diff = max_state_diff(ref, fused, worst);
	bool ok = max_diff < 1e-5;
	if (!ok)
		cout << "FAIL fusao qubits=" << qubits << " exec=" << fuse_exec << " region=" << region_bits << " coalesced=" << coalesced_bits
			<< " seed=" << seed << ": maior diferença " << max_diff << " no índice " << worst << endl;
	ref.freeMemory();
	fused.freeMemory();
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

	// iterations > 1 (o Grover repete o passo por execute(iterations))
	for (unsigned seed = 1; seed <= 4; seed++){
		all_ok = run_case(12, 8, 4, 2, 25, seed * 104729, 3) && all_ok;
		all_ok = run_case(10, 14, 11, 3, 25, seed * 104729 + 1, 4) && all_ok;
		case_count += 2;
	}
	cout << (all_ok ? "OK" : "FAIL") << " test_cpu_parallel: " << case_count << " circuitos aleatórios, t_PAR_CPU vs t_CPU" << endl;

	bool schedule_ok = true;
	int schedule_cases = 0;
	struct SchedCombo{ int qubits, coalesced, extra; };
	SchedCombo sched_combos[] = {{10, 2, 3}, {10, 3, 4}, {12, 4, 8}, {12, 1, 2}, {11, 0, 3}, {13, 4, 8}};
	for (const SchedCombo &combo : sched_combos){
		for (unsigned seed = 1; seed <= 5; seed++){
			schedule_ok = run_schedule_case(combo.qubits, combo.coalesced, combo.extra, 150, seed * 6007 + combo.qubits) && schedule_ok;
			schedule_cases++;
		}
	}
	cout << (schedule_ok ? "OK" : "FAIL") << " test_cpu_parallel: " << schedule_cases << " circuitos, ScheduleForBatching preserva o resultado" << endl;
	all_ok = all_ok && schedule_ok;

	bool fuse_ok = true;
	int fuse_cases = 0;
	long absorbed_total = 0;
	for (unsigned seed = 1; seed <= 6; seed++){
		fuse_ok = run_fuse_case(10, t_CPU, 14, 11, 120, seed * 5003, &absorbed_total) && fuse_ok;
		fuse_ok = run_fuse_case(12, t_PAR_CPU, 8, 4, 120, seed * 5003 + 1, &absorbed_total) && fuse_ok;
		fuse_ok = run_fuse_case(11, t_PAR_CPU, 6, 3, 120, seed * 5003 + 2, &absorbed_total) && fuse_ok;
		fuse_cases += 3;
	}
	if (absorbed_total < 100){
		cout << "FAIL fusao: poucas portas absorvidas (" << absorbed_total << "), o teste não está exercitando a fusão" << endl;
		fuse_ok = false;
	}
	cout << (fuse_ok ? "OK" : "FAIL") << " test_cpu_parallel: " << fuse_cases << " circuitos, portas de fase fundidas (" << absorbed_total << " portas absorvidas)" << endl;
	all_ok = all_ok && fuse_ok;

	if (GpuMinFreeBytes(1) >= 0){
		bool gpu_ok = true;
		int gpu_cases = 0;
		struct GpuCombo{ int qubits, coalesced, region, block, repeat; };
		GpuCombo gpu_combos[] = {{10, 2, 5, 8, 2}, {12, 3, 6, 16, 2}, {12, 4, 8, 64, 2}, {13, 4, 12, 512, 4}, {14, 4, 10, 128, 4}, {14, 2, 7, 16, 4}};
		for (const GpuCombo &combo : gpu_combos){
			for (unsigned seed = 1; seed <= 4; seed++){
				gpu_ok = run_gpu_case(combo.qubits, combo.coalesced, combo.region, combo.block, combo.repeat, 120, seed * 7001 + combo.qubits) && gpu_ok;
				gpu_ok = run_gpu_case(combo.qubits, combo.coalesced, combo.region, combo.block, combo.repeat, 120, seed * 7001 + combo.qubits + 1, true) && gpu_ok;
				gpu_cases += 2;
			}
		}
		cout << (gpu_ok ? "OK" : "FAIL") << " test_cpu_parallel: " << gpu_cases << " circuitos aleatórios, t_GPU vs t_CPU" << endl;
		all_ok = all_ok && gpu_ok;
	}
	else{
		cout << "(sem GPU de verdade neste build: casos de t_GPU pulados)" << endl;
	}

	return all_ok ? 0 : 1;
}
