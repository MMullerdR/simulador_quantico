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
// AllocGpuState/FreeGpuState: alocam/liberam o buffer de GPU usado por
// ProjectState/GetState uma única vez por lote de regiões (não mais a
// cada região individual) — ver DGM::HybridExecution e item 17 em
// docs/07-bugs-e-pontos-de-atencao.md. region_size precisa ser o mesmo
// valor passado a ProjectState/GetState durante o lote inteiro (o
// tamanho do buffer depende só dele, não muda região a região).
extern "C" bool AllocGpuState(int region_size, int gpu_count);
extern "C" bool FreeGpuState(int gpu_count);
// PinHostState/UnpinHostState: travam (page-lock) o vetor de estado do host
// enquanto durar o modo híbrido, pra acelerar ProjectState/GetState (item 17,
// tentativa 6). Devolvem false se não conseguiram -- nesse caso o híbrido
// continua funcionando, só sem a aceleração.
extern "C" bool PinHostState(void* ptr, size_t bytes);
// GpuMinFreeBytes: menor memória livre (em bytes) entre as gpu_count GPUs,
// ou -1 se não for possível consultar (item 17, tentativa 7).
extern "C" long GpuMinFreeBytes(int gpu_count);
// GpuMaxBatchOps: máximo de portas por lote de kernel (OPS_BLOCK de kernel.cu).
extern "C" int GpuMaxBatchOps();
// Sessão de GPU (item 28 em docs/07-bugs-e-pontos-de-atencao.md): o vetor de
// estado inteiro fica residente na GPU entre execuções (sem cópia host<->GPU a
// cada execute()) e o measure() roda lá. Só com 1 GPU. GpuSessionBegin devolve
// false (nada alocado) se não couber ou não houver GPU de verdade; depois dele,
// GpuExecutionWrapper(NULL, ...) opera sobre o estado residente.
extern "C" bool GpuSessionBegin(float complex *state, int qubits, int gpu_count);
// Libera o estado da GPU; copy_back copia antes para state (host).
extern "C" void GpuSessionEnd(float complex *state, int qubits, bool copy_back);
// Soma de |amplitude|^2 com o bit "bit_shift" do índice em 0 (zero_sum) e em 1
// (one_sum), em double, sobre o estado residente.
extern "C" void GpuMeasureSums(int qubits, int bit_shift, double *zero_sum, double *one_sum);
// Colapso de DGM::measure: a metade com o bit == measured_bit, dividida por
// norm, vai para a metade com o bit == 0; a metade com o bit == 1 zera.
extern "C" void GpuCollapse(int qubits, int bit_shift, int measured_bit, double norm);
// Permutação do estado residente (DGM::applyModMult): out[d] = in[src(d)], com
// src(d) = d fora do caso controlado; no caso controlado o valor x do campo de
// "width" bits em reg_shift vira gather_table[x] (x < modulus). Usa um buffer
// auxiliar do tamanho do estado, alocado na primeira chamada; devolve false (sem
// alterar nada) se não houver sessão ou memória, e o chamador volta ao host.
extern "C" bool GpuModMult(int qubits, int ctrl_shift, int reg_shift, int width, const int *gather_table, long modulus);
extern "C" bool UnpinHostState(void* ptr);
extern "C" bool ProjectState(float complex* state, int qubits, int region_size, long region_id, long region_mask, int gpu_count);
extern "C" bool GetState(float complex* state, int qubits, int region_size, long region_id, long region_mask, int gpu_count);

// Backend t_PAR_CPU: divide o vetor de estado em regiões e processa
// várias em paralelo via OpenMP (ver docs/03-motor-de-execucao-cpu.md).
void PCpuExecution1(float complex *state, PT **pts, int qubits, long thread_count, int coalesced_bits, int region_bits, int iterations);
// Aplica os operadores de uma região específica — chamada por
// PCpuExecution1 e por DGM::HybridExecution (lado CPU do modo híbrido).
void PCpuExecution1_0(float complex *state, PT **pts, int qubits, long pts_start, long pts_end, long pos_count, long region_id, long region_mask);

// Backends de execução (DGM::exec_type).
enum {
	t_CPU,
	t_PAR_CPU,
	t_GPU,
	t_HYBRID
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

	// Buffer auxiliar de applyModMult (mesmo tamanho de state), alocado na
	// primeira chamada e trocado de lugar com state a cada permutação.
	float complex *scratch_state;

	// true: o vetor de estado vive na GPU (sessão de GPU) e state (host) está
	// desatualizado; execute() e measure() operam lá. Ver beginGpuSession().
	bool gpu_resident;

	DGM();
	~DGM();

	bool print_enabled;

	void printPTs();
	// Libera todos os PT já compilados (vec_pts) e zera pts.
	void erase();
	void setExecType(int type);

	void setCpuStructure(long cpu_region_bits, long cpu_coalesced_bits);
	void setGpuStructure(long gpu_coalesced_bits, long gpu_region_bits, int repeat_count = 1);

	// Aloca/libera o vetor de estado (state) -- freeMemory() também é
	// chamado automaticamente pelo destrutor (RAII), então só precisa ser
	// chamado manualmente se quiser liberar antes do fim do escopo.
	void allocateMemory();
	void freeMemory();
	// Seta state[pos] = 1 — estado inicial |pos>.
	void setMemoryValue(long pos);

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
	// Sessão de GPU: copia state pra GPU uma vez e passa execute()/measure()
	// a operar sobre ele sem cópias por execução. Só para t_GPU (ou t_HYBRID que
	// delegaria à GPU) com 1 GPU; devolve false e não faz nada nos outros casos
	// (o caminho com cópia por execução segue valendo). GPU_RESIDENT=0 desliga.
	bool beginGpuSession();
	// Encerra a sessão; copy_back traz o estado de volta para state (host).
	void endGpuSession(bool copy_back);
	// Reordena pts[] pra GPU (ScheduleForBatching); GPU_SCHEDULE=0 desliga.
	void scheduleForGpu();
	// Emulação (não é um circuito): se o qubit ctrl_qubit vale 1, o registrador
	// de "width" qubits que começa em reg_first_qubit (o último é o bit menos
	// significativo) passa de x para (multiplier * x) mod modulus, para
	// x < modulus; os demais valores e o caso de controle 0 ficam como estão.
	// Uma permutação dos índices do estado: uma passada, no lugar das centenas
	// de portas do circuito de Beauregard. Exige multiplier coprimo com modulus,
	// modulus <= 2^width <= 65536 e o controle acima do registrador no índice.
	void applyModMult(int ctrl_qubit, int reg_first_qubit, int width, long multiplier, long modulus);

	// Backend t_HYBRID: um thread OpenMP comanda a GPU enquanto os
	// demais processam regiões em CPU ao mesmo tempo.
	void HybridExecution(PT **pts, int iterations);
	// true se o t_HYBRID rodaria este circuito inteiro na GPU (mesma regra do
	// desvio "delegado à GPU" de HybridExecution).
	bool hybridDelegatesToGpu();

	// Backend t_CPU: aplica cada PT sobre o vetor de estado inteiro,
	// numa só thread, escolhendo CpuExecution1_1/2/3 conforme o tipo de
	// matriz (DENSE/DIAG_PRI/DIAG_SEC).
	void CpuExecution1(int iterations);
	void CpuExecution1_1(PT *term, long mem_size);
	void CpuExecution1_2(PT *term, long mem_size);
	void CpuExecution1_3(PT *term, long mem_size);

};

#endif
