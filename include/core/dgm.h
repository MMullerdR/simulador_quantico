#ifndef _DGM_H_
#define _DGM_H_

#include <string>
#include <fstream>
#include <vector>
#include <map>
#include <algorithm>
#include <sstream>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <math.h>
#include <sys/time.h>
#include "common.h"
#include "gates.h"

#define complex __complex__

using namespace std;

float complex* GenericExecute(float complex *state, string function, int qubits, int type, int threads, int factor);
float complex*  GenericExecute(float complex *state, vector<string> function, int qubits, int type, int threads, int factor);

extern "C" bool setDevice(int device_id = 0);

// Wrappers de execução em GPU (implementados em kernel.cu). GpuExecutionWrapper
// é o único efetivamente usado por DGM::execute(); os demais (GpuExecution,
// GpuExecution2, GpuExecution3) são assinaturas de versões alternativas sem
// implementação atual — mantidas como estão.
// Atenção: a ordem dos parâmetros aqui precisa bater com a definição
// real em kernel.cu (state, pts, qubits, coalesced_bits, gpu_region_bits,
// gpu_count, block_size, repeat_count, iterations) — a declaração antiga
// tinha os nomes multi_gpu/coalesc/qbs_region fora de ordem (inofensivo
// em C/extern "C", já que só a posição importa para o linker, mas
// enganoso para quem lê).
extern "C" float complex* GpuExecutionWrapper(float complex* read_memory, PT **pts, int qubits, int coalesced_bits, int gpu_region_bits, int gpu_count, int block_size, int repeat_count, int iterations);
extern "C" float complex* GpuExecution(float complex* read_memory, float complex* write_memory, PT **pts, int qubits, float *total_time, long max_pt, long max_qubits, int iterations);
extern "C" float complex* GpuExecution2(float complex* read_memory, PT **pts, int pts_size, int qubits, long max_pt, int iterations);
extern "C" float complex* GpuExecution3(float complex* read_memory, float complex* write_memory, int sub_size, int shift_write, PT *pt, int qubits, long max_pt, long max_qubits, int iterations);
extern "C" bool ProjectState(float complex* state, int qubits, int region_size, long region_id, long region_mask, int gpu_count);
extern "C" bool GetState(float complex* state, int qubits, int region_size, long region_id, long region_mask, int gpu_count);

void PCpuExecution1(float complex *state, PT **pts, int qubits, long thread_count, int coalesced_bits, int region_bits, int iterations);
void PCpuExecution1_0(float complex *state, PT **pts, int qubits, int pts_start, int pts_end, int pos_count, int region_id, int region_mask);

// Insere um bit 0 na posição "shift" de "pos" (ver docs/03-motor-de-execucao-cpu.md).
inline long LINE (long pos, long shift){
	return ((pos >> shift) & 1) * 2;
}
inline long BASE (long pos, long shift){
	return pos & (~(1 << shift));
}

enum {
	t_CPU,
	t_PAR_CPU,
	t_GPU,
	t_HYBRID,
	t_SPEC
};

// Agrupa os controles e alvos de um mesmo grupo dentro de um step do
// circuito, antes de virarem PTs (ver docs/02-linguagem-de-circuitos.md).
class Group{
public:
	vector <string> ops;
	vector <long> pos_ops;
	vector <bool> ctrl;
	vector <long> pos_ctrl;

	Group(){};
	bool isAfected(int pos, int afect);
};

// Motor central do simulador: guarda o vetor de estado e despacha a
// execução dos circuitos para um dos backends (CPU serial, CPU paralela,
// GPU ou híbrido) — ver docs/01-arquitetura-geral.md.
class DGM{
public:
	long total_op, dense, main_diag, sec_diag, c_dense, c_main_diag, c_sec_diag; //contadores de operações, ver CountOps()

	vector <string> diag;
	long max_qubits, max_pt, affected_qubit_count;

	long factor;

	int exec_type;

	long thread_count;
	long cpu_coalesced_bits;
	long cpu_region_bits;

	int gpu_count;
	long gpu_coalesced_bits;
	long gpu_region_bits;

	int block_size;
	int repeat_count;

	vector <PT*> vec_pts;
	PT** pts;
	long qubits;

	float measure_value;

	float elapsed_time;
	struct timeval timev;

	float complex *state;

	DGM();
	~DGM();

	bool print_enabled;

	void printPTs();
	void erase();
	void setExecType(int type);

	void setCpuStructure(long cpu_region_bits, long cpu_coalesced_bits);
	void setGpuStructure(long gpu_coalesced_bits, long gpu_region_bits, int repeat_count = 1);

	void allocateMemory();
	void setMemory(float complex *mem);
	void freeMemory();
	void setMemoryValue(int pos);

	int measure(int qubit_pos);
	map <long, float> measure(vector<int> qubit_positions);
	void colapse(int qubit_pos, int value);

	void setFunction(string function, int iterations = 1, bool reset = true);
	void setFunction(vector<string> steps, int iterations = 1, bool reset = true);
	map <long, Group> genGroups(string step);
	void genPTs(map<long, Group> &groups, vector <PT*> &step_pts);
	void genMatrix(float complex* matrix, vector<float complex*> &matrices, long qubit_count, long current_qubit, long line, long column, float complex value);

	void CountOps(int iterations = 1);

	void executeFunction(string function, int iterations = 1);
	void executeFunction(vector<string> steps, int iterations = 1);
	float complex* execute(int iterations);

	void HybridExecution(PT **pts);

	void CpuExecution1(int iterations);
	void CpuExecution1_1(PT *term, long mem_size);
	void CpuExecution1_2(PT *term, long mem_size);
	void CpuExecution1_3(PT *term, long mem_size);

};

#endif
