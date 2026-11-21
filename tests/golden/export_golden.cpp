// Exporta os vetores de referência ("golden") do C++ para a migração em Rust
// (Fase 1 de docs/09-plano-migracao-rust.md). Roda os circuitos no t_CPU (o
// oráculo, serial e determinístico) e grava, em tests/golden/data/, tudo o que
// um porte precisa para provar paridade sem ter o C++ por perto: texto dos
// circuitos gerados, lista de portas compiladas (PT), fusão e agendamento,
// estados finais, medição com semente e tabelas de teoria dos números.
//
// Formato: tests/golden/README.md. Uso: make golden (ou
// outputs/export_golden.out [pasta_de_saida]).
//
// Todo número de ponto flutuante de texto sai com %.9g (ida e volta exata de
// float32); estados saem em binário, float32 little-endian, pares (re, im).
#include "../../include/core/dgm.h"
#include "../../include/core/gates.h"
#include "../../include/core/common.h"
#include "../../include/algorithms/lib_shor.h"
#include "../../include/algorithms/lib_grover.h"
#include <cstdio>
#include <cmath>
#include <cstdlib>
#include <fstream>
#include <functional>
#include <iostream>
#include <map>
#include <random>
#include <set>
#include <sstream>
#include <sys/stat.h>
#include <vector>
using namespace std;

typedef vector<float complex> StateVec;

static string g_dir;
static int g_case_count = 0;
static map<string, int> g_kind_count;
static vector<string> g_index;
static long g_state_bytes = 0;

static string fl(float value){
	char buffer[40];
	snprintf(buffer, sizeof buffer, "%.9g", value);
	return buffer;
}

static void make_dir(const string &relative){
	mkdir((g_dir + "/" + relative).c_str(), 0755);
}

static void write_state_file(const string &relative, const float complex *state, long count){
	FILE *file = fopen((g_dir + "/" + relative).c_str(), "wb");
	if (!file){ cerr << "não consegui criar " << relative << endl; exit(1); }
	for (long i = 0; i < count; i++){
		float pair[2] = {crealf(state[i]), cimagf(state[i])};
		fwrite(pair, sizeof(float), 2, file);
	}
	fclose(file);
	g_state_bytes += count * 8;
}

// ---------------------------------------------------------------- estados

static StateVec make_random_state(int qubits, unsigned seed){
	mt19937 rng(seed);
	uniform_real_distribution<float> dist(-1.0f, 1.0f);
	long size = 1L << qubits;
	StateVec state(size);
	double norm = 0;
	for (long i = 0; i < size; i++){
		float re = dist(rng), im = dist(rng);
		state[i] = re + im * 1.0fi;
		norm += (double) re * re + (double) im * im;
	}
	float scale = 1.0f / sqrt(norm);
	for (long i = 0; i < size; i++) state[i] *= scale;
	return state;
}

// Estado aleatório compartilhado entre casos (um arquivo só por qubits/semente).
struct SharedInit{
	string path;
	StateVec state;
};

static SharedInit shared_random(int qubits, unsigned seed){
	SharedInit init;
	char name[64];
	snprintf(name, sizeof name, "shared/rand%d-s%u.f32", qubits, seed);
	init.path = name;
	init.state = make_random_state(qubits, seed);
	static set<string> written;
	if (!written.count(init.path)){
		write_state_file(init.path, &init.state[0], 1L << qubits);
		written.insert(init.path);
	}
	return init;
}

// Estado aleatório só no subespaço em que o Shor vive: controle e registrador
// quaisquer (valor < N), auxiliares em 0.
static StateVec make_valid_shor_state(int qubits, int width, long modulus, unsigned seed){
	mt19937 rng(seed);
	uniform_real_distribution<float> dist(-1.0f, 1.0f);
	StateVec state(1L << qubits, 0);
	double norm = 0;
	for (int ctrl_value = 0; ctrl_value < 2; ctrl_value++){
		for (long x = 0; x < modulus; x++){
			long index = ((long) ctrl_value << (qubits - 1)) | (x << (width + 2));
			float re = dist(rng), im = dist(rng);
			state[index] = re + im * 1.0fi;
			norm += (double) re * re + (double) im * im;
		}
	}
	float scale = 1.0f / sqrt(norm);
	for (size_t i = 0; i < state.size(); i++) state[i] *= scale;
	return state;
}

// ------------------------------------------------------------------ portas

struct ExtraGate{
	string name;
	float complex m[4];
};

static const char *BASE_GATES[] = {"H", "X", "Y", "Z", "R1", "R2", "R3"};

static bool is_base_gate(const string &name){
	for (const char *base : BASE_GATES) if (name == base) return true;
	return false;
}

// As mesmas portas extras de tests/test_cpu_parallel.cpp.
static vector<ExtraGate> test_gates(){
	vector<ExtraGate> gates;
	float cos_a = cosf(0.7f), sin_a = sinf(0.7f), cos_b = cosf(0.3f), sin_b = sinf(0.3f);
	float complex phase = cosf(1.1f) + sinf(1.1f) * I;
	gates.push_back({"RX", {cos_a, -sin_a * I, -sin_a * I, cos_a}});
	gates.push_back({"U", {cos_b, sin_b * phase, -sin_b * conjf(phase), cos_b}});
	gates.push_back({"DG", {phase, 0.0f, 0.0f, conjf(phase) * phase * phase}});
	gates.push_back({"AD", {0.0f, phase, conjf(phase) * 0.6f, 0.0f}});
	float complex ph = cosf(2 * M_PI * 37.0 / 65536.0) + sinf(2 * M_PI * 37.0 / 65536.0) * I;
	gates.push_back({"PH", {1.0f, 0.0f, 0.0f, ph}});
	gates.push_back({"PHN", {1.0f, 0.0f, 0.0f, cosf(0.123456f) + sinf(0.123456f) * I}});
	return gates;
}

static void register_gates(Gates &gates, const vector<ExtraGate> &extra){
	for (const ExtraGate &gate : extra) gates.addGate(gate.name, gate.m[0], gate.m[1], gate.m[2], gate.m[3]);
}

// Escreve toda porta que não é do catálogo base (as registradas à mão e as
// criadas pelos geradores de circuito, como Rot_N).
static int dump_gates(ofstream &out, Gates &gates){
	int count = 0;
	for (map<string, float complex*>::iterator it = gates.list.begin(); it != gates.list.end(); ++it){
		if (is_base_gate(it->first)) continue;
		out << "gate " << it->first;
		for (int i = 0; i < 4; i++) out << " " << fl(crealf(it->second[i])) << " " << fl(cimagf(it->second[i]));
		out << "\n";
		count++;
	}
	return count;
}

// ------------------------------------------------------------ arquivos .case

struct CaseFile{
	string id, path;
	ofstream out;
	CaseFile(const string &kind, const string &name){
		char number[16];
		snprintf(number, sizeof number, "%04d", ++g_case_count);
		id = string(number) + "-" + name;
		path = "cases/" + id + ".case";
		out.open((g_dir + "/" + path).c_str());
		if (!out){ cerr << "não consegui criar " << path << endl; exit(1); }
		out << "format 1\nname " << id << "\nkind " << kind << "\n";
		g_kind_count[kind]++;
		g_index.push_back(path);
	}
};

static vector<string> as_lines(const string &line){ return vector<string>(1, line); }
static vector<string> as_lines(const vector<string> &lines){ return lines; }

// -------------------------------------------------------------- caso "state"

struct StateSpec{
	string name;
	int qubits;
	int iterations;
	int qft_degree;			// -1: não altera
	long basis;				// >= 0: estado inicial |basis>
	const StateVec *init;	// senão: este vetor
	string init_path;		// arquivo compartilhado do vetor (vazio: grava um próprio)
	vector<ExtraGate> gates;
	function<vector<string>(Gates&)> make_steps;
	StateSpec(): qubits(0), iterations(1), qft_degree(-1), basis(-1), init(NULL){}
};

static void load_into(DGM &dgm, const StateVec &init){
	for (size_t i = 0; i < init.size(); i++) dgm.state[i] = init[i];
}

static void export_state(StateSpec &spec){
	DGM dgm;
	dgm.qubits = spec.qubits;
	dgm.exec_type = t_CPU;
	dgm.allocateMemory();
	register_gates(dgm.gates, spec.gates);
	if (spec.qft_degree >= 0) set_qft_degree(spec.qft_degree);
	vector<string> steps = spec.make_steps(dgm.gates);
	if (spec.basis >= 0) dgm.setMemoryValue(spec.basis);
	else load_into(dgm, *spec.init);

	CaseFile c("state", spec.name);
	string init_path = spec.init_path;
	if (spec.basis < 0 && init_path.empty()){
		init_path = "cases/" + c.id + ".init.f32";
		write_state_file(init_path, &(*spec.init)[0], 1L << spec.qubits);
	}

	dgm.executeFunction(steps, spec.iterations);

	c.out << "qubits " << spec.qubits << "\niterations " << spec.iterations << "\n";
	if (spec.qft_degree >= 0) c.out << "qft_degree " << spec.qft_degree << "\n";
	if (spec.basis >= 0) c.out << "init basis " << spec.basis << "\n";
	else c.out << "init file " << init_path << "\n";
	dump_gates(c.out, dgm.gates);
	c.out << "steps " << steps.size() << "\n";
	for (size_t i = 0; i < steps.size(); i++) c.out << steps[i] << "\n";
	string final_path = "cases/" + c.id + ".final.f32";
	write_state_file(final_path, dgm.state, 1L << spec.qubits);
	c.out << "final file " << final_path << "\n";
	dgm.freeMemory();
}

// -------------------------------------------------------------- caso "text"

static void export_text(const string &name, int qft_degree, const string &call, function<vector<string>(Gates&)> generate){
	Gates gates;
	set_qft_degree(qft_degree);
	vector<string> lines = generate(gates);
	CaseFile c("text", name);
	c.out << "qft_degree " << qft_degree << "\ncall " << call << "\n";
	dump_gates(c.out, gates);
	c.out << "lines " << lines.size() << "\n";
	for (size_t i = 0; i < lines.size(); i++) c.out << lines[i] << "\n";
}

#define TEXT_CASE(name, degree, call, expr) export_text(name, degree, call, [&](Gates &g){ return as_lines(expr); })

// --------------------------------------------------------------- caso "pts"

static void dump_pt(ofstream &out, const PT *term){
	if (term->phase_terms){
		out << "fused " << term->phase_terms->size();
		for (size_t i = 0; i < term->phase_terms->size(); i++){
			const PhaseTerm &phase = (*term->phase_terms)[i];
			out << " " << phase.mask << " " << phase.value << " " << phase.steps;
		}
		out << "\n";
		return;
	}
	PT *mutable_term = const_cast<PT*>(term);
	out << "pt " << term->target_bit << " " << term->control_mask << " " << term->control_value << " "
		<< term->control_count << " " << mutable_term->matrixType();
	for (int i = 0; i < 4; i++) out << " " << fl(crealf(term->matrix[i])) << " " << fl(cimagf(term->matrix[i]));
	for (long i = 0; i < term->control_count; i++) out << " " << term->control_bit_positions[i];
	out << "\n";
}

// mode: "parse" (só o parser), "fuse" (parser + FusePhaseRuns),
// "fuse_schedule" (+ ScheduleForBatching), "schedule" (parser + agendamento).
static void export_pts(const string &name, int qubits, const string &mode, int coalesced_bits, int extra_bits, int max_batch,
	const vector<ExtraGate> &gates, int qft_degree, function<vector<string>(Gates&)> make_steps){
	DGM dgm;
	dgm.qubits = qubits;
	dgm.exec_type = t_CPU;
	register_gates(dgm.gates, gates);
	set_qft_degree(qft_degree);
	vector<string> steps = make_steps(dgm.gates);
	dgm.setFunction(steps);

	long absorbed = 0;
	if (mode == "fuse" || mode == "fuse_schedule"){
		absorbed = FusePhaseRuns(dgm.vec_pts);
		dgm.pts = &dgm.vec_pts[0];
	}
	if (mode == "schedule" || mode == "fuse_schedule")
		ScheduleForBatching(dgm.pts, coalesced_bits, extra_bits, max_batch);

	CaseFile c("pts", name);
	c.out << "qubits " << qubits << "\nmode " << mode << "\n";
	if (mode == "schedule" || mode == "fuse_schedule")
		c.out << "schedule " << coalesced_bits << " " << extra_bits << " " << max_batch << "\n";
	if (mode == "fuse" || mode == "fuse_schedule") c.out << "absorbed " << absorbed << "\n";
	dump_gates(c.out, dgm.gates);
	c.out << "steps " << steps.size() << "\n";
	for (size_t i = 0; i < steps.size(); i++) c.out << steps[i] << "\n";
	long count = 0;
	while (dgm.pts[count] != NULL) count++;
	c.out << "pts " << count << "\n";
	for (long i = 0; i < count; i++) dump_pt(c.out, dgm.pts[i]);
}

// ------------------------------------------------------------ casos de medição

static void export_measure(const string &name, int qubits, unsigned seed, const vector<int> &order, const SharedInit &init){
	DGM dgm;
	dgm.qubits = qubits;
	dgm.exec_type = t_CPU;
	dgm.allocateMemory();
	load_into(dgm, init.state);
	seed_rng(seed);
	vector<int> bits;
	vector<float> values;
	for (size_t i = 0; i < order.size(); i++){
		bits.push_back(dgm.measure(order[i]));
		values.push_back(dgm.measure_value);
	}
	CaseFile c("measure", name);
	c.out << "qubits " << qubits << "\ninit file " << init.path << "\nseed " << seed << "\nmeasure " << order.size();
	for (size_t i = 0; i < order.size(); i++) c.out << " " << order[i];
	c.out << "\nbits";
	for (size_t i = 0; i < bits.size(); i++) c.out << " " << bits[i];
	c.out << "\nvalues";
	for (size_t i = 0; i < values.size(); i++) c.out << " " << fl(values[i]);
	c.out << "\n";
	string final_path = "cases/" + c.id + ".final.f32";
	write_state_file(final_path, dgm.state, 1L << qubits);
	c.out << "final file " << final_path << "\n";
	dgm.freeMemory();
}

static void export_colapse(const string &name, int qubits, int qubit, int value, const SharedInit &init){
	DGM dgm;
	dgm.qubits = qubits;
	dgm.exec_type = t_CPU;
	dgm.allocateMemory();
	load_into(dgm, init.state);
	ostringstream silence;			// colapse() imprime a soma no cout
	streambuf *old = cout.rdbuf(silence.rdbuf());
	dgm.colapse(qubit, value);
	cout.rdbuf(old);
	CaseFile c("colapse", name);
	c.out << "qubits " << qubits << "\ninit file " << init.path << "\ncolapse " << qubit << " " << value << "\n";
	string final_path = "cases/" + c.id + ".final.f32";
	write_state_file(final_path, dgm.state, 1L << qubits);
	c.out << "final file " << final_path << "\n";
	dgm.freeMemory();
}

// ----------------------------------------------------------- caso "modmult"

static void export_modmult(const string &name, int qubits, int ctrl, int reg_first, int width, long multiplier, long modulus, const StateVec &init){
	DGM dgm;
	dgm.qubits = qubits;
	dgm.exec_type = t_CPU;
	dgm.allocateMemory();
	load_into(dgm, init);
	CaseFile c("modmult", name);
	string init_path = "cases/" + c.id + ".init.f32";
	write_state_file(init_path, &init[0], 1L << qubits);
	dgm.applyModMult(ctrl, reg_first, width, multiplier, modulus);
	c.out << "qubits " << qubits << "\nmodmult " << ctrl << " " << reg_first << " " << width << " " << multiplier << " " << modulus
		<< "\ninit file " << init_path << "\n";
	string final_path = "cases/" + c.id + ".final.f32";
	write_state_file(final_path, dgm.state, 1L << qubits);
	c.out << "final file " << final_path << "\n";
	dgm.freeMemory();
}

// -------------------------------------------------------- circuitos aleatórios

static const char *PLAIN_GATES[] = {"H", "X", "Y", "Z", "R1", "R2", "R3", "RX", "U", "DG", "AD"};
static const int PLAIN_GATE_COUNT = 11;

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

static vector<string> random_circuit(int qubits, int step_count, unsigned seed, bool phase_heavy){
	mt19937 rng(seed);
	vector<string> steps;
	for (int i = 0; i < step_count; i++) steps.push_back(phase_heavy ? random_phase_step(qubits, rng) : random_step(qubits, rng));
	return steps;
}

// --------------------------------------------------------------- layout do Shor

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

static long modular_inverse(long value, long modulus){
	for (long candidate = 1; candidate < modulus; candidate++)
		if ((value * candidate) % modulus == 1) return candidate;
	return 0;
}

static long shor_basis(const Layout &l, int ctrl_value, long x){
	return ((long) ctrl_value << (l.qubits - 1)) | (x << (l.width + 2));
}

// Uma rodada do Shor sem a medição: CMultMod + CSwapR + CRMultMod com o inverso
// modular (item 30 de docs/07).
static vector<string> shor_round_steps(const Layout &l, long multiplier, Gates &g){
	vector<string> steps = CMultMod(l.qubits, l.ctrl, l.reg1, l.reg2, l.over, l.over_bool, l.width, multiplier, l.modulus, g);
	vector<string> swap_steps = CSwapR(l.qubits, l.ctrl, l.reg1, l.reg2, l.width);
	steps.insert(steps.end(), swap_steps.begin(), swap_steps.end());
	vector<string> undo = CRMultMod(l.qubits, l.ctrl, l.reg1, l.reg2, l.over, l.over_bool, l.width, modular_inverse(multiplier, l.modulus), l.modulus, g);
	steps.insert(steps.end(), undo.begin(), undo.end());
	return steps;
}

// ------------------------------------------------------------------ as tabelas

static void export_numbers(){
	CaseFile c("table", "number-theory");
	ostream &out = c.out;
	long bases[] = {2, 3, 5, 7, 11, 13};
	long exponents[] = {0, 1, 2, 3, 10, 31, 64, 1000, 65535};
	long moduli[] = {15, 21, 57, 119, 1017, 2863};
	for (long m : moduli) for (long b : bases) for (long e : exponents)
		out << "modular_pow " << b << " " << e << " " << m << " = " << modular_pow(b, e, m) << "\n";
	long inverse_moduli[] = {15, 21, 57, 119};
	for (long m : inverse_moduli)
		for (long a = 1; a < m; a++)
			if (quantum_gcd((int) a, (int) m) == 1) out << "mul_inv " << a << " " << m << " = " << mul_inv(a, m) << "\n";
	long gcd_values[] = {1, 2, 6, 12, 15, 21, 35, 57, 64, 119, 1000};
	for (long a : gcd_values) for (long b : gcd_values)
		out << "quantum_gcd " << a << " " << b << " = " << quantum_gcd((int) a, (int) b) << "\n";
	for (int bits : {3, 4, 5, 8, 12, 16})
		for (long value : {1L, 2L, 5L, 11L, 100L})
			if (value < (1L << bits)) out << "revert_bits " << value << " " << bits << " = " << revert_bits((int) value, bits) << "\n";
	for (int width = 1; width <= 16; width++)
		out << "qft_degree_for_width " << width << " = " << qft_degree_for_width(width) << "\n";
	// frações contínuas: numeradores perto de k/r para vários períodos r
	for (int width : {4, 5, 6}){
		int denominator_value = 1 << (2 * width);
		int periods[] = {2, 3, 4, 5, 6, 7, 8, 12};
		for (int r : periods){
			for (int k = 1; k < r; k++){
				int numerator = (int) llround((double) k * denominator_value / r);
				int n = numerator, d = denominator_value;
				quantum_frac_approx(&n, &d, width);
				out << "frac_approx " << numerator << " " << denominator_value << " " << width << " = " << n << " " << d << "\n";
			}
		}
	}
}

// ------------------------------------------------------------------------- main

int main(int argc, char **argv){
	unsigned int one = 1;
	if (*(unsigned char*) &one != 1){ cerr << "este exportador assume host little-endian" << endl; return 1; }

	g_dir = (argc > 1) ? argv[1] : "tests/golden/data";
	mkdir(g_dir.c_str(), 0755);
	make_dir("cases");
	make_dir("shared");
	vector<ExtraGate> extra = test_gates();
	vector<ExtraGate> none;

	// ---- 1. portas de 1 qubit, em cada alvo, sobre um estado aleatório de 4 qubits
	{
		SharedInit init = shared_random(4, 11);
		for (int g = 0; g < PLAIN_GATE_COUNT; g++){
			for (int target = 0; target < 4; target++){
				StateSpec spec;
				spec.name = string("gate1-") + PLAIN_GATES[g] + "-q" + to_string(target);
				spec.qubits = 4;
				spec.init = &init.state;
				spec.init_path = init.path;
				spec.gates = extra;
				string gate_name = PLAIN_GATES[g];
				spec.make_steps = [=](Gates&){
					vector<string> tokens(4, "ID");
					tokens[target] = gate_name;
					return as_lines(concatena(tokens, 4));
				};
				export_state(spec);
			}
		}
	}

	// ---- 2. portas controladas (5 qubits)
	{
		SharedInit init = shared_random(5, 12);
		struct Controlled{ string label; string step; };
		vector<Controlled> list;
		const char *ops[] = {"X", "Z", "H", "R2", "U"};
		for (const char *op : ops){
			list.push_back({string("c1-") + op + "-0to3-v1", Controlled1(5, 0, 3, op, 1)});
			list.push_back({string("c1-") + op + "-4to1-v0", Controlled1(5, 4, 1, op, 0)});
			list.push_back({string("c2-") + op + "-v3", Controlled2(5, 1, 3, 0, op, 3)});
			list.push_back({string("c2-") + op + "-v1", Controlled2(5, 2, 4, 1, op, 1)});
			list.push_back({string("cn3-") + op + "-v5", ControlledN(5, {0, 2, 4}, 3, op, 5)});
		}
		for (int value = 0; value < 4; value++) list.push_back({"toffoli-v" + to_string(value), Toffoli(5, 0, 3, 2, value)});
		list.push_back({"cnot-v1", CNot(5, 1, 4, 1)});
		list.push_back({"cnot-v0", CNot(5, 3, 0, 0)});
		list.push_back({"cn4-X-all", ControlledN(5, {0, 1, 2, 3}, 4, "X", -1)});
		list.push_back({"cn4-Z-v6", ControlledN(5, {1, 2, 3, 4}, 0, "Z", 6)});
		for (size_t i = 0; i < list.size(); i++){
			StateSpec spec;
			spec.name = "ctrl-" + list[i].label;
			spec.qubits = 5;
			spec.init = &init.state;
			spec.init_path = init.path;
			spec.gates = extra;
			string step = list[i].step;
			spec.make_steps = [=](Gates&){ return as_lines(step); };
			export_state(spec);
		}
	}

	// ---- 3. várias portas e vários grupos no mesmo step, vários steps
	{
		SharedInit init = shared_random(4, 13);
		const char *steps_list[] = {
			"H,Control2(1),Target2(Z),H",
			"Control1(1),Target1(X),Control2(0),Target2(H)",
			"H,H,H,H",
			"Target1(X),Control1(1),Control2(1),Target2(Y)"};
		for (int i = 0; i < 4; i++){
			StateSpec spec;
			spec.name = "multi-group-" + to_string(i);
			spec.qubits = 4;
			spec.init = &init.state;
			spec.init_path = init.path;
			spec.gates = extra;
			string step = steps_list[i];
			spec.make_steps = [=](Gates&){ return as_lines(step); };
			export_state(spec);
		}
	}

	// ---- 4. circuitos aleatórios (denso/diagonal/anti-diagonal, com e sem controles)
	{
		int sizes[] = {6, 8, 10};
		for (int s = 0; s < 3; s++){
			int qubits = sizes[s];
			for (unsigned seed = 1; seed <= 3; seed++){
				StateSpec spec;
				spec.name = "random-q" + to_string(qubits) + "-s" + to_string(seed);
				spec.qubits = qubits;
				spec.gates = extra;
				spec.iterations = (seed == 3) ? 2 : 1;
				SharedInit init;
				if (qubits < 10){
					init = shared_random(qubits, 20 + seed);
					spec.init = &init.state;
					spec.init_path = init.path;
				}
				else{
					spec.basis = 0;
				}
				vector<string> steps = random_circuit(qubits, 40, 1000 * qubits + seed, false);
				spec.make_steps = [=](Gates&){ return steps; };
				export_state(spec);
			}
		}
	}

	// ---- 5. circuitos ricos em portas de fase (as corridas que a fusão junta)
	{
		for (unsigned seed = 1; seed <= 3; seed++){
			SharedInit init = shared_random(8, 30 + seed);
			StateSpec spec;
			spec.name = "phase-q8-s" + to_string(seed);
			spec.qubits = 8;
			spec.gates = extra;
			spec.init = &init.state;
			spec.init_path = init.path;
			vector<string> steps = random_circuit(8, 60, 7000 + seed, true);
			spec.make_steps = [=](Gates&){ return steps; };
			export_state(spec);
		}
	}

	// ---- 6. Hadamard em todos os qubits (o benchmark general.out) e Grover
	{
		for (int qubits : {6, 8}){
			StateSpec spec;
			spec.name = "general-hadamard-q" + to_string(qubits);
			spec.qubits = qubits;
			spec.iterations = 3;
			spec.basis = 0;
			spec.make_steps = [=](Gates&){ return as_lines(Hadamard(qubits, 0, qubits)); };
			export_state(spec);
		}
		struct GroverCase{ int qubits; long search; };
		GroverCase grover_cases[] = {{4, 5}, {6, 21}, {8, 100}};
		for (const GroverCase &gc : grover_cases){
			StateSpec spec;
			spec.name = "grover-q" + to_string(gc.qubits) + "-v" + to_string(gc.search);
			spec.qubits = gc.qubits;
			spec.basis = 1L << (gc.qubits - 1);
			int qubits = gc.qubits;
			long search = gc.search;
			spec.make_steps = [=](Gates&){
				// o mesmo circuito de Grover() em lib_grover.cpp
				vector<string> steps;
				steps.push_back(Hadamard(qubits, 0, qubits));
				vector<string> grover_step;
				grover_step.push_back(Oracle1(qubits, search));
				for (int q = 1; q < qubits; q++) grover_step.push_back(Hadamard(qubits, q, 1));
				grover_step.push_back(Pauli_X(qubits, qubits - 1, 1));
				grover_step.push_back(ControledZ(qubits));
				grover_step.push_back(Pauli_X(qubits, qubits - 1, 1));
				for (int q = qubits - 1; q >= 1; q--) grover_step.push_back(Hadamard(qubits, q, 1));
				int iterations = (int) (M_PI / 4.0 * sqrt((double) (1L << (qubits - 1))));
				for (int i = 0; i < iterations; i++) steps.insert(steps.end(), grover_step.begin(), grover_step.end());
				return steps;
			};
			export_state(spec);
		}
	}

	// ---- 7. circuitos do Shor (estado): QFT, somador de Fourier, uma rodada
	{
		Layout l15 = layout_for(15);
		for (int degree : {0, qft_degree_for_width(l15.width)}){
			// reg2 ocupa os qubits reg2..reg2+w-1; o último deles é o bit 1 do índice
			// (o bit 0 é over_bool), então reg2 = x é o índice x << 1.
			for (long x : {1L, 5L, 11L}){
				StateSpec spec;
				spec.name = "shor-qft-N15-deg" + to_string(degree) + "-x" + to_string(x);
				spec.qubits = l15.qubits;
				spec.qft_degree = degree;
				spec.basis = x << 1;
				Layout l = l15;
				spec.make_steps = [=](Gates &g){
					return QFT(l.qubits, l.reg2, l.over, l.width, g);
				};
				export_state(spec);

				StateSpec round_trip = spec;
				round_trip.name = "shor-qft-rqft-N15-deg" + to_string(degree) + "-x" + to_string(x);
				round_trip.make_steps = [=](Gates &g){
					vector<string> steps = QFT(l.qubits, l.reg2, l.over, l.width, g);
					vector<string> back = RQFT(l.qubits, l.reg2, l.over, l.width, g);
					steps.insert(steps.end(), back.begin(), back.end());
					return steps;
				};
				export_state(round_trip);

				StateSpec adder = spec;
				adder.name = "shor-draper-add-N15-deg" + to_string(degree) + "-x" + to_string(x);
				adder.make_steps = [=](Gates &g){
					vector<string> steps = QFT(l.qubits, l.reg2, l.over, l.width, g);
					steps.push_back(AddF(l.qubits, l.reg2, l.over, 7, l.width, g));
					vector<string> back = RQFT(l.qubits, l.reg2, l.over, l.width, g);
					steps.insert(steps.end(), back.begin(), back.end());
					return steps;
				};
				export_state(adder);
			}
		}

		struct RoundCase{ long modulus, multiplier; };
		RoundCase rounds[] = {{15, 7}, {15, 2}, {21, 5}};
		for (const RoundCase &rc : rounds){
			Layout l = layout_for(rc.modulus);
			for (int degree : {0, qft_degree_for_width(l.width)}){
				// N=21 tem 13 qubits (64 KB por arquivo): só um x
				vector<long> xs;
				if (rc.modulus == 21) xs.push_back(4);
				else { xs.push_back(1); xs.push_back(4); xs.push_back(rc.modulus - 2); }
				for (int ctrl_value = 0; ctrl_value < 2; ctrl_value++){
					for (long x : xs){
						StateSpec spec;
						spec.name = "shor-round-N" + to_string(rc.modulus) + "-a" + to_string(rc.multiplier) + "-deg" + to_string(degree)
							+ "-c" + to_string(ctrl_value) + "-x" + to_string(x);
						spec.qubits = l.qubits;
						spec.qft_degree = degree;
						spec.basis = shor_basis(l, ctrl_value, x);
						long multiplier = rc.multiplier;
						spec.make_steps = [=](Gates &g){ return shor_round_steps(l, multiplier, g); };
						export_state(spec);
					}
				}
			}
		}
	}

	// ---- 8. emulação da multiplicação modular (permutação do estado)
	{
		struct ModCase{ long modulus, multiplier; };
		ModCase mods[] = {{15, 7}, {15, 11}, {21, 5}};
		for (const ModCase &mc : mods){
			Layout l = layout_for(mc.modulus);
			StateVec valid = make_valid_shor_state(l.qubits, l.width, mc.modulus, 40 + mc.modulus);
			export_modmult("modmult-valid-N" + to_string(mc.modulus) + "-a" + to_string(mc.multiplier), l.qubits, l.ctrl, l.reg1, l.width, mc.multiplier, mc.modulus, valid);
		}
		Layout l = layout_for(15);
		StateVec full = make_random_state(l.qubits, 77);
		export_modmult("modmult-fullrandom-N15-a2", l.qubits, l.ctrl, l.reg1, l.width, 2, 15, full);
	}

	// ---- 9. medição com semente e colapso
	{
		SharedInit init6 = shared_random(6, 51);
		export_measure("measure-q6-all-s1", 6, 1, {0, 1, 2, 3, 4, 5}, init6);
		export_measure("measure-q6-all-s2", 6, 2, {5, 4, 3, 2, 1, 0}, init6);
		export_measure("measure-q6-all-s3", 6, 123456789, {3, 0, 5, 1, 4, 2}, init6);
		export_measure("measure-q6-partial-s4", 6, 4, {2, 4}, init6);
		export_measure("measure-q6-repeat-s5", 6, 5, {1, 1, 1}, init6);
		SharedInit init8 = shared_random(8, 52);
		export_measure("measure-q8-partial-s6", 8, 6, {7, 0, 3}, init8);
		export_colapse("colapse-q6-q0-v0", 6, 0, 0, init6);
		export_colapse("colapse-q6-q3-v1", 6, 3, 1, init6);
		export_colapse("colapse-q8-q7-v1", 8, 7, 1, init8);
	}

	// ---- 10. texto dos geradores de circuito
	{
		TEXT_CASE("gen-hadamard-q5-r1-w3", 0, "Hadamard qubits=5 reg=1 width=3", Hadamard(5, 1, 3));
		TEXT_CASE("gen-paulix-q6-r2-w2", 0, "Pauli_X qubits=6 reg=2 width=2", Pauli_X(6, 2, 2));
		TEXT_CASE("gen-pauliz-q6-r0-w4", 0, "Pauli_Z qubits=6 reg=0 width=4", Pauli_Z(6, 0, 4));
		TEXT_CASE("gen-cnot", 0, "CNot qubits=4 ctrl=3 target=1 cv=1", CNot(4, 3, 1, 1));
		TEXT_CASE("gen-cnot-cv0", 0, "CNot qubits=4 ctrl=0 target=2 cv=0", CNot(4, 0, 2, 0));
		for (int v = 0; v < 4; v++)
			TEXT_CASE("gen-toffoli-v" + to_string(v), 0, "Toffoli qubits=5 c1=4 c2=1 target=0 cv=" + to_string(v), Toffoli(5, 4, 1, 0, v));
		for (int v = 0; v < 4; v++)
			TEXT_CASE("gen-controlled2-v" + to_string(v), 0, "Controlled2 qubits=5 c1=0 c2=2 target=4 op=R2 cv=" + to_string(v), Controlled2(5, 0, 2, 4, "R2", v));
		TEXT_CASE("gen-controlled1", 0, "Controlled1 qubits=3 ctrl=2 target=0 op=H cv=1", Controlled1(3, 2, 0, "H", 1));
		TEXT_CASE("gen-controlledn-all", 0, "ControlledN qubits=6 ctrls=0,1,2,3 target=5 op=X cv=-1", ControlledN(6, {0, 1, 2, 3}, 5, "X", -1));
		TEXT_CASE("gen-controlledn-v5", 0, "ControlledN qubits=6 ctrls=5,3,1 target=0 op=Z cv=5", ControlledN(6, {5, 3, 1}, 0, "Z", 5));
		TEXT_CASE("gen-concatena", 0, "concatena a,b,c,d reverse=false", concatena({"a", "b", "c", "d"}, 4, false));
		TEXT_CASE("gen-concatena-reverse", 0, "concatena a,b,c,d reverse=true", concatena({"a", "b", "c", "d"}, 4, true));
		for (int q = 3; q <= 8; q++){
			long value = (1L << (q - 1)) - 3;
			TEXT_CASE("gen-oracle1-q" + to_string(q), 0, "Oracle1 qubits=" + to_string(q) + " value=" + to_string(value), Oracle1(q, value));
			TEXT_CASE("gen-controledz-q" + to_string(q), 0, "ControledZ qubits=" + to_string(q), ControledZ(q));
		}
		TEXT_CASE("gen-oracle1-q10-v0", 0, "Oracle1 qubits=10 value=0", Oracle1(10, 0));
		TEXT_CASE("gen-oracle1-q10-v511", 0, "Oracle1 qubits=10 value=511", Oracle1(10, 511));

		long phase_values[] = {1, 2, 5, 12, 255, 1000};
		for (long bits : phase_values)
			TEXT_CASE("gen-genrot-q11-r0-p" + to_string(bits), 0, "genRot qubits=11 reg=0 phase_bits=" + to_string(bits), genRot(11, 0, bits, g));

		Layout l15 = layout_for(15), l21 = layout_for(21);
		for (const Layout &l : {l15, l21}){
			string tag = "N" + to_string(l.modulus);
			for (int degree : {0, qft_degree_for_width(l.width)}){
				string suffix = tag + "-deg" + to_string(degree);
				TEXT_CASE("gen-qft-" + suffix, degree, "QFT " + tag + " reg=reg2", QFT(l.qubits, l.reg2, l.over, l.width, g));
				TEXT_CASE("gen-rqft-" + suffix, degree, "RQFT " + tag + " reg=reg2", RQFT(l.qubits, l.reg2, l.over, l.width, g));
				TEXT_CASE("gen-addf-" + suffix, degree, "AddF " + tag + " value=7", AddF(l.qubits, l.reg2, l.over, 7, l.width, g));
				TEXT_CASE("gen-addf-ctrl-" + suffix, degree, "AddF(controlled) " + tag + " value=9", AddF(l.qubits, l.reg2, l.over, 9, l.width, true, g));
				TEXT_CASE("gen-subf-" + suffix, degree, "SubF " + tag + " value=3", SubF(l.qubits, l.reg2, l.over, 3, l.width, g));
				TEXT_CASE("gen-caddf-" + suffix, degree, "CAddF " + tag + " ctrl=0 value=5", CAddF(l.qubits, 0, l.reg2, l.over, 5, l.width, g));
				TEXT_CASE("gen-csubf-" + suffix, degree, "CSubF " + tag + " ctrl=0 value=5", CSubF(l.qubits, 0, l.reg2, l.over, 5, l.width, g));
				TEXT_CASE("gen-c2addf-" + suffix, degree, "C2AddF " + tag + " ctrl1=0 ctrl2=1 value=6", C2AddF(l.qubits, 0, 1, l.reg2, l.over, 6, l.width, g));
				TEXT_CASE("gen-c2subf-" + suffix, degree, "C2SubF " + tag + " ctrl1=0 ctrl2=1 value=6", C2SubF(l.qubits, 0, 1, l.reg2, l.over, 6, l.width, g));
			}
			TEXT_CASE("gen-cswapr-" + tag, 0, "CSwapR " + tag, CSwapR(l.qubits, l.ctrl, l.reg1, l.reg2, l.width));
			TEXT_CASE("gen-swapover-" + tag, 0, "SwapOver " + tag + " reg=reg1", SwapOver(l.qubits, l.reg1, l.width));
		}
		for (int degree : {0, qft_degree_for_width(l15.width)}){
			string suffix = "N15-deg" + to_string(degree);
			TEXT_CASE("gen-c2addmod-" + suffix, degree, "C2AddMod N15 a=7", C2AddMod(l15.qubits, 0, 4, l15.reg2, l15.over, l15.over_bool, l15.width, 7, 15, g));
			TEXT_CASE("gen-c2submod-" + suffix, degree, "C2SubMod N15 a=7", C2SubMod(l15.qubits, 0, 4, l15.reg2, l15.over, l15.over_bool, l15.width, 7, 15, g));
			TEXT_CASE("gen-cmultmod-" + suffix, degree, "CMultMod N15 a=7", CMultMod(l15.qubits, l15.ctrl, l15.reg1, l15.reg2, l15.over, l15.over_bool, l15.width, 7, 15, g));
			TEXT_CASE("gen-crmultmod-" + suffix, degree, "CRMultMod N15 a=13", CRMultMod(l15.qubits, l15.ctrl, l15.reg1, l15.reg2, l15.over, l15.over_bool, l15.width, 13, 15, g));
		}
		TEXT_CASE("gen-cmultmod-N21-deg0", 0, "CMultMod N21 a=5", CMultMod(l21.qubits, l21.ctrl, l21.reg1, l21.reg2, l21.over, l21.over_bool, l21.width, 5, 21, g));
		TEXT_CASE("gen-crmultmod-N21-deg0", 0, "CRMultMod N21 a=17", CRMultMod(l21.qubits, l21.ctrl, l21.reg1, l21.reg2, l21.over, l21.over_bool, l21.width, 17, 21, g));
	}

	// ---- 11. parser, fusão de fase e agendamento
	{
		Layout l15 = layout_for(15);
		struct Source{ string name; int qubits; int degree; function<vector<string>(Gates&)> make; };
		vector<Source> sources;
		sources.push_back({"random-q6", 6, 0, [](Gates&){ return random_circuit(6, 30, 424242, false); }});
		sources.push_back({"random-q8", 8, 0, [](Gates&){ return random_circuit(8, 30, 515151, false); }});
		sources.push_back({"phase-q8", 8, 0, [](Gates&){ return random_circuit(8, 50, 7001, true); }});
		sources.push_back({"phase-q10", 10, 0, [](Gates&){ return random_circuit(10, 60, 7002, true); }});
		sources.push_back({"toffoli-chain-q6", 6, 0, [](Gates&){
			vector<string> steps;
			for (int i = 0; i < 4; i++){ steps.push_back(Toffoli(6, i, i + 1, 5, 3)); steps.push_back(Hadamard(6, i, 1)); }
			return steps; }});
		sources.push_back({"shor-qft-N15-deg0", l15.qubits, 0, [=](Gates &g){
			vector<string> steps = QFT(l15.qubits, l15.reg2, l15.over, l15.width, g);
			vector<string> back = RQFT(l15.qubits, l15.reg2, l15.over, l15.width, g);
			steps.insert(steps.end(), back.begin(), back.end());
			return steps; }});
		sources.push_back({"shor-addmod-N15-deg0", l15.qubits, 0, [=](Gates &g){
			return C2AddMod(l15.qubits, 0, 4, l15.reg2, l15.over, l15.over_bool, l15.width, 7, 15, g); }});
		sources.push_back({"shor-round-N15-deg2", l15.qubits, 2, [=](Gates &g){ return shor_round_steps(l15, 7, g); }});
		struct Schedule{ int coalesced, extra_bits, max_batch; };
		Schedule schedules[] = {{4, 8, 300}, {3, 5, 50}, {0, 6, 300}};
		for (const Source &s : sources){
			export_pts("parse-" + s.name, s.qubits, "parse", 0, 0, 0, extra, s.degree, s.make);
			export_pts("fuse-" + s.name, s.qubits, "fuse", 0, 0, 0, extra, s.degree, s.make);
			for (const Schedule &sc : schedules){
				string tag = "-c" + to_string(sc.coalesced) + "e" + to_string(sc.extra_bits) + "b" + to_string(sc.max_batch);
				export_pts("schedule-" + s.name + tag, s.qubits, "schedule", sc.coalesced, sc.extra_bits, sc.max_batch, extra, s.degree, s.make);
				export_pts("fuse-schedule-" + s.name + tag, s.qubits, "fuse_schedule", sc.coalesced, sc.extra_bits, sc.max_batch, extra, s.degree, s.make);
			}
		}
	}

	// ---- 12. teoria dos números
	export_numbers();

	// ---- índice
	{
		ofstream index((g_dir + "/INDEX.txt").c_str());
		index << "format 1\ncases " << g_index.size() << "\n";
		for (size_t i = 0; i < g_index.size(); i++) index << g_index[i] << "\n";
	}
	cout << "casos: " << g_case_count << "  (";
	for (map<string, int>::iterator it = g_kind_count.begin(); it != g_kind_count.end(); ++it)
		cout << it->first << "=" << it->second << " ";
	cout << ")\nestados binários: " << g_state_bytes / 1024 << " KiB\nsaída: " << g_dir << endl;
	return 0;
}
