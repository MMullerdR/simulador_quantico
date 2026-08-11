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

// Inicializa a GPU escolhida (implementado em kernel.cu/kernel_stub.cpp).
extern "C" bool setDevice(int device_id = 0);

// Wrapper de execução em GPU (implementado em kernel.cu) — o único
// efetivamente usado por DGM::execute(). Atenção: a ordem dos parâmetros
// aqui precisa bater com a definição real em kernel.cu (state, pts,
// qubits, coalesced_bits, gpu_region_bits, gpu_count, block_size,
// repeat_count, iterations).
extern "C" float complex* GpuExecutionWrapper(float complex* read_memory, PT **pts, int qubits, int coalesced_bits, int gpu_region_bits, int gpu_count, int block_size, int repeat_count, int iterations);
extern "C" bool ProjectState(float complex* state, int qubits, int region_size, long region_id, long region_mask, int gpu_count);
extern "C" bool GetState(float complex* state, int qubits, int region_size, long region_id, long region_mask, int gpu_count);

// Backend t_PAR_CPU: divide o vetor de estado em regiões e processa
// várias em paralelo via OpenMP (ver docs/03-motor-de-execucao-cpu.md).
void PCpuExecution1(float complex *state, PT **pts, int qubits, long thread_count, int coalesced_bits, int region_bits, int iterations);
// Aplica os operadores de uma região específica — chamada por
// PCpuExecution1 e por DGM::HybridExecution (lado CPU do modo híbrido).
void PCpuExecution1_0(float complex *state, PT **pts, int qubits, int pts_start, int pts_end, int pos_count, int region_id, int region_mask);

// Funções auxiliares de indexação de bit; sem uso no código atual.
inline long LINE (long pos, long shift){
	return ((pos >> shift) & 1) * 2;
}
inline long BASE (long pos, long shift){
	return pos & (~(1 << shift));
}

// Backends de execução (DGM::exec_type); t_SPEC nunca chegou a ser usado.
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

	// Cache de matrizes de porta desta execução — ver docs/03 e o
	// comentário em gates.h. Vive e morre com a instância de DGM.
	Gates gates;

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
	// Libera todos os PT já compilados (vec_pts) e zera pts.
	void erase();
	void setExecType(int type);

	void setCpuStructure(long cpu_region_bits, long cpu_coalesced_bits);
	void setGpuStructure(long gpu_coalesced_bits, long gpu_region_bits, int repeat_count = 1);

	// Aloca/associa/libera o vetor de estado (state).
	void allocateMemory();
	void setMemory(float complex *mem);
	void freeMemory();
	// Seta state[pos] = 1 — estado inicial |pos>.
	void setMemoryValue(int pos);

	// Mede um qubit (ou vários) e colapsa o estado de acordo com o resultado.
	int measure(int qubit_pos);
	map <long, float> measure(vector<int> qubit_positions);
	void colapse(int qubit_pos, int value);

	// Faz o parsing de um circuito (string ou vector<string>) e monta
	// vec_pts/pts — ver docs/02-linguagem-de-circuitos.md.
	void setFunction(string function, int iterations = 1, bool reset = true);
	void setFunction(vector<string> steps, int iterations = 1, bool reset = true);
	map <long, Group> genGroups(string step);
	void genPTs(map<long, Group> &groups, vector <PT*> &step_pts);
	// Montaria a matriz combinada de um step via produto de Kronecker das
	// matrizes individuais de cada qubit; não é chamada de lugar nenhum
	// hoje (sem uso).
	void genMatrix(float complex* matrix, vector<float complex*> &matrices, long qubit_count, long current_qubit, long line, long column, float complex value);

	// Conta quantos PT de cada tipo (DENSE/DIAG_PRI/DIAG_SEC, com/sem
	// controle) o circuito atual tem — só estatística, não afeta execução.
	void CountOps(int iterations = 1);

	// Clampa os parâmetros de região contra qubits antes do despacho —
	// centraliza uma validação que antes só existia (e só foi corrigida)
	// dentro de cada backend separadamente, ver
	// docs/07-bugs-e-pontos-de-atencao.md, item 6.
	void validateTuning();

	// setFunction() + execute() num só passo.
	void executeFunction(string function, int iterations = 1);
	void executeFunction(vector<string> steps, int iterations = 1);
	// Despacha pts[] pro backend escolhido em exec_type.
	float complex* execute(int iterations);

	// Backend t_HYBRID: um thread OpenMP comanda a GPU enquanto os
	// demais processam regiões em CPU ao mesmo tempo.
	void HybridExecution(PT **pts);

	// Backend t_CPU: aplica cada PT sobre o vetor de estado inteiro,
	// numa só thread, escolhendo CpuExecution1_1/2/3 conforme o tipo de
	// matriz (DENSE/DIAG_PRI/DIAG_SEC).
	void CpuExecution1(int iterations);
	void CpuExecution1_1(PT *term, long mem_size);
	void CpuExecution1_2(PT *term, long mem_size);
	void CpuExecution1_3(PT *term, long mem_size);

};

#endif
