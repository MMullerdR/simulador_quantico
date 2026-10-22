// DGM::applyModMult (a multiplicação modular controlada emulada como
// permutação dos índices do estado, SHOR_EMULATE=1) contra o circuito exato de
// Beauregard que ela substitui (CMultMod + CSwapR + CRMultMod com a QFT exata).
// O circuito é o oráculo: os dois têm que dar o mesmo vetor de estado. Três
// grupos:
//  1. circuito x emulação em t_CPU, a partir de um estado aleatório em que o
//     qubit de controle e o registrador valem qualquer coisa (valor < N) e os
//     auxiliares (over, reg2, over_bool) valem 0 -- o subespaço em que o
//     circuito é a multiplicação modular, e o único que o Shor alcança;
//  2. emulação em t_PAR_CPU contra t_CPU (é só uma permutação: igualdade exata);
//  3. só com kernel de verdade: sessão de GPU (a permutação roda lá e o estado
//     residente muda de buffer) contra t_CPU, com portas entre as permutações
//     para conferir que os kernels seguintes enxergam o buffer novo.
//
// Convenção de bit e formato de step: ver test_gates.cpp e
// docs/02-linguagem-de-circuitos.md.
#include "../include/algorithms/lib_shor.h"
#include "../include/core/dgm.h"
#include "../include/core/gates.h"
#include <iostream>
#include <cmath>
#include <random>
using namespace std;

struct Layout{
	long modulus;
	int width, qubits, ctrl, reg1, over, reg2, over_bool;
};

static Layout layout_for(long modulus){
	Layout l;
	l.modulus = modulus;
	l.width = 0;
	for (long rest = modulus; rest; rest >>= 1) l.width++;
	l.qubits = 2 * l.width + 3;
	l.ctrl = 0;
	l.reg1 = 1;
	l.over = l.width + 1;
	l.reg2 = l.width + 2;
	l.over_bool = l.qubits - 1;
	return l;
}

// Amplitudes aleatórias nos estados com o controle em 0 ou 1, o registrador em
// qualquer valor < N e os auxiliares em 0; o resto do vetor fica em 0.
static void fill_valid_state(DGM &dgm, const Layout &l, mt19937 &rng){
	uniform_real_distribution<float> dist(-1.0f, 1.0f);
	long size = 1L << l.qubits;
	for (long i = 0; i < size; i++) dgm.state[i] = 0;
	double norm = 0;
	for (int ctrl_value = 0; ctrl_value < 2; ctrl_value++){
		for (long x = 0; x < l.modulus; x++){
			long index = ((long) ctrl_value << (l.qubits - 1)) | (x << (l.width + 2));
			float re = dist(rng), im = dist(rng);
			dgm.state[index] = re + im * 1.0fi;
			norm += (double) re * re + (double) im * im;
		}
	}
	float scale = 1.0f / sqrt(norm);
	for (long i = 0; i < size; i++) dgm.state[i] *= scale;
}

static void copy_state(DGM &from, DGM &to){
	for (long i = 0; i < (1L << from.qubits); i++) to.state[i] = from.state[i];
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

static void setup(DGM &dgm, const Layout &l, int type){
	dgm.qubits = l.qubits;
	dgm.exec_type = type;
	dgm.thread_count = 4;
	dgm.cpu_region_bits = 8;
	dgm.cpu_coalesced_bits = 3;
	dgm.allocateMemory();
}

// Grupo 1 e 2: circuito exato contra emulação (t_CPU) e emulação t_PAR_CPU
// contra t_CPU.
static long modular_inverse(long value, long modulus){
	for (long candidate = 1; candidate < modulus; candidate++)
		if ((value * candidate) % modulus == 1) return candidate;
	return 0;
}

static bool run_case(long modulus, long multiplier, unsigned seed){
	Layout l = layout_for(modulus);
	mt19937 rng(seed);

	DGM circuit, emulated, parallel;
	setup(circuit, l, t_CPU);
	setup(emulated, l, t_CPU);
	setup(parallel, l, t_PAR_CPU);
	fill_valid_state(circuit, l, rng);
	copy_state(circuit, emulated);
	copy_state(circuit, parallel);

	set_qft_degree(0);	// QFT exata: o circuito é o oráculo
	vector <string> steps = CMultMod(l.qubits, l.ctrl, l.reg1, l.reg2, l.over, l.over_bool, l.width, multiplier, modulus, circuit.gates);
	vector <string> swap_steps = CSwapR(l.qubits, l.ctrl, l.reg1, l.reg2, l.width);
	steps.insert(steps.end(), swap_steps.begin(), swap_steps.end());
	// desfazer a soma exige o inverso: depois da troca reg1 = a*x e reg2 = x, tem
	// que subtrair a^-1 * (a*x) de reg2 para ele voltar a 0. (Shor() chama
	// CRMultMod com o próprio a e não com a^-1: com isso reg2 fica com
	// x*(1 - a^2) em vez de 0 -- ver item 29 em docs/07-bugs-e-pontos-de-atencao.md.)
	long undo_multiplier = modular_inverse(multiplier, modulus);
	vector <string> undo_steps = CRMultMod(l.qubits, l.ctrl, l.reg1, l.reg2, l.over, l.over_bool, l.width, undo_multiplier, modulus, circuit.gates);
	steps.insert(steps.end(), undo_steps.begin(), undo_steps.end());
	circuit.executeFunction(steps);

	emulated.applyModMult(l.ctrl, l.reg1, l.width, multiplier, modulus);
	parallel.applyModMult(l.ctrl, l.reg1, l.width, multiplier, modulus);

	bool ok = true;
	long worst;
	double diff = max_state_diff(circuit, emulated, worst);
	if (diff >= 1e-3){
		cout << "FAIL emulacao N=" << modulus << " a=" << multiplier << " seed=" << seed
			<< ": circuito e emulação diferem " << diff << " no índice " << worst << endl;
		ok = false;
	}
	diff = max_state_diff(emulated, parallel, worst);
	if (diff != 0){
		cout << "FAIL emulacao N=" << modulus << " a=" << multiplier << " seed=" << seed
			<< ": t_PAR_CPU difere de t_CPU " << diff << " no índice " << worst << endl;
		ok = false;
	}
	circuit.freeMemory();
	emulated.freeMemory();
	parallel.freeMemory();
	return ok;
}

// Grupo 3: sessão de GPU. Várias permutações com uma porta H no qubit de
// controle entre elas, contra o mesmo roteiro em t_CPU.
static bool run_gpu_case(long modulus, unsigned seed){
	Layout l = layout_for(modulus);
	mt19937 rng(seed);

	DGM reference, gpu;
	setup(reference, l, t_CPU);
	setup(gpu, l, t_GPU);
	gpu.gpu_coalesced_bits = 4;
	gpu.gpu_region_bits = 8;
	gpu.block_size = 64;
	gpu.repeat_count = 2;
	fill_valid_state(reference, l, rng);
	copy_state(reference, gpu);

	if (!gpu.beginGpuSession()){
		cout << "FAIL emulacao GPU: beginGpuSession() não abriu a sessão (N=" << modulus << ")" << endl;
		return false;
	}

	string hadamard = Hadamard(l.qubits, l.ctrl, 1);
	long multipliers[3] = {2, modulus - 1, 4};
	for (int round = 0; round < 3; round++){
		long multiplier = multipliers[round];
		for (DGM *dgm : {&reference, &gpu}){
			dgm->executeFunction(hadamard);
			dgm->applyModMult(l.ctrl, l.reg1, l.width, multiplier, modulus);
			dgm->executeFunction(hadamard);
		}
	}

	gpu.endGpuSession(true);
	long worst;
	double diff = max_state_diff(reference, gpu, worst);
	bool ok = true;
	if (diff >= 1e-5){
		cout << "FAIL emulacao GPU N=" << modulus << " seed=" << seed << ": difere " << diff << " no índice " << worst << endl;
		ok = false;
	}
	reference.freeMemory();
	gpu.freeMemory();
	return ok;
}

int main(){
	// (N, a) com a coprimo com N; de N pequeno (width 2) a N com 6 bits, incluindo
	// N perto de 2^width (valores >= N do registrador ficam como estão).
	struct Case{ long modulus, multiplier; };
	Case cases[] = {{3, 2}, {5, 2}, {7, 3}, {9, 2}, {15, 2}, {15, 7}, {15, 11}, {21, 5}, {21, 2}, {21, 13}, {33, 5}, {35, 3}, {55, 2}};

	bool all_ok = true;
	int count = 0;
	bool emulation_ok = true;
	for (const Case &c : cases){
		for (unsigned seed = 1; seed <= 2; seed++){
			emulation_ok = run_case(c.modulus, c.multiplier, seed * 7919 + c.modulus) && emulation_ok;
			count++;
		}
	}
	cout << (emulation_ok ? "OK" : "FAIL") << " test_shor_emulate: " << count << " casos, circuito exato x emulação (t_CPU) e t_PAR_CPU x t_CPU" << endl;
	all_ok = all_ok && emulation_ok;

	if (GpuMinFreeBytes(1) >= 0){
		bool gpu_ok = true;
		int gpu_count = 0;
		long gpu_moduli[4] = {15, 21, 35, 55};
		for (long modulus : gpu_moduli){
			for (unsigned seed = 1; seed <= 3; seed++){
				gpu_ok = run_gpu_case(modulus, seed * 104729 + modulus) && gpu_ok;
				gpu_count++;
			}
		}
		cout << (gpu_ok ? "OK" : "FAIL") << " test_shor_emulate: " << gpu_count << " sessões de GPU (3 permutações com H entre elas), t_GPU residente x t_CPU" << endl;
		all_ok = all_ok && gpu_ok;
	}
	else{
		cout << "(sem kernel de GPU nesta build: grupo de sessão de GPU não rodou)" << endl;
	}

	return all_ok ? 0 : 1;
}
