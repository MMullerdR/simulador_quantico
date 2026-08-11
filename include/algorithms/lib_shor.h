#ifndef _LIBSHOR_H_
#define _LIBSHOR_H_

#include <vector>
#include <string>
// <complex.h> precisa ser incluído antes da define abaixo, senão a
// definição de "complex" daqui contamina a inicialização do <complex.h>
// do sistema quando ele for incluído depois (por common.h/dgm.h).
#include <complex.h>
#include "../core/gates.h"

using namespace std;

#define complex __complex__

// number_to_factor - número a ser fatorado
// type - tipo de execução (t_CPU, t_PAR_CPU, t_GPU, t_HYBRID)
// thread_count - número de threads usadas na execução paralela em CPU
vector<int> Shor(long number_to_factor, int type, int thread_count, int cpu_region_bits, int cpu_coalesced_bits, int gpu_count, int gpu_region_bits, int gpu_coalesced_bits, int block_size, int repeat_count);

void ApplyQFT(int qubits, int type, int gpu_count, int gpu_region_bits, int coalesced_bits, int block_size, int repeat_count);

//////////////////////////////////////////////////////////////////////////

// g: cache de portas da execução atual (DGM::gates) — os nomes das
// portas geradas dinamicamente aqui (R/R'/ADD_/SUB_/Rot_) são chaves
// nesse cache, ver gates.h.
vector <string> QFT(int qubits, int reg, int over, int width, Gates &g);
vector <string> QFT2(int qubits, int reg, int width, Gates &g);
vector <string> RQFT(int qubits, int reg, int over, int width, Gates &g);
vector <string> CSwapR(int qubits, int ctrl, int reg1, int reg2, int width);
vector <string> SwapOver(int qubits, int reg, int width);
string genRot(int qubits, int reg, long phase_bits, Gates &g);

vector <string> CMultMod(int qubits, int ctrl, int reg1, int reg2, int over, int over_bool, int width, long base_value, long number_to_factor, Gates &g);
vector <string> CRMultMod(int qubits, int ctrl, int reg1, int reg2, int over, int over_bool, int width, long base_value, long number_to_factor, Gates &g);
vector <string> C2AddMod(int qubits, int ctrl1, int ctrl2, int reg, int over, int over_bool, int width, long base_value, long number_to_factor, Gates &g);
vector <string> C2SubMod(int qubits, int ctrl1, int ctrl2, int reg, int over, int over_bool, int width, long base_value, long number_to_factor, Gates &g);

string C2AddF(int qubits, int ctrl1, int ctrl2, int reg, int over, long value_to_add, int width, Gates &g);
string CAddF(int qubits, int ctrl1, int reg, int over, long value_to_add, int width, Gates &g);
string AddF(int qubits, int reg, int over, long value_to_add, int width, Gates &g);
vector <string> AddF(int qubits, int reg, int over, long value_to_add, int width, bool controlled, Gates &g);

string C2SubF(int qubits, int ctrl1, int ctrl2, int reg, int over, long value_to_sub, int width, Gates &g);
string CSubF(int qubits, int ctrl1, int reg, int over, long value_to_sub, int width, Gates &g);
string SubF(int qubits, int reg, int over, long value_to_sub, int width, Gates &g);
vector <string> SubF(int qubits, int reg, int over, long value_to_sub, int width, bool controlled, Gates &g);

//////////////////////////////////////////////////////////////////////////

string int2str(int number);

long mul_inv(long value, long modulus);
int revert_bits(int value, int bit_count);
int quantum_ipow(int base, int exponent);

/* Calculate the greatest common divisor with Euclid's algorithm */
int quantum_gcd(int value1, int value2);
void quantum_frac_approx(int *numerator, int *denominator, int width);

//////////////////////////////////////////////////////////////////////////

#endif
