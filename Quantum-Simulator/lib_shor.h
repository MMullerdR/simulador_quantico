#ifndef _LIBSHOR_H_
#define _LIBSHOR_H_

#include <vector>
#include <string>

using namespace std;

#define complex _Complex

//N - Number to ne factored
//type - Execution Type
//threads - Number of threads to be used in case of a parallel execution on CPU
vector<int> Shor(long N, int type, int n_threads, int cpu_region, int cpu_coalesc, int multi_gpu, int gpu_region, int gpu_coalesc, int tam_block, int rept);

void ApplyQFT(int qubits, int type, int multi_gpu, int qbs_region, int coalesc, int tam_block, int rept);

//////////////////////////////////////////////////////////////////////////

vector <string> QFT(int qubits, int reg, int over, int width);
vector <string> QFT2(int qubits, int reg, int width);
vector <string> RQFT(int qubits, int reg, int over, int width);
vector <string> CSwapR(int qubits, int ctrl, int reg1, int reg2, int width);
vector <string> SwapOver(int qubits, int reg, int width);
string genRot(int qubits, int reg, long value);

vector <string> CU(int qubits, int ctrl, int reg1, int reg2, int width, long a, long N);

vector <string> CMultMod(int qubits, int ctrl, int reg1, int reg2, int over, int over_bool, int width, long a, long N);
vector <string> CRMultMod(int qubits, int ctrl, int reg1, int reg2, int over, int over_bool, int width, long a, long N);
vector <string> C2AddMod(int qubits, int ctrl1, int ctrl2, int reg, int over, int over_bool, int width, long a, long N);
vector <string> C2SubMod(int qubits, int ctrl1, int ctrl2, int reg, int over, int over_bool, int width, long a, long N);

string C2AddF(int qubits, int ctrl1, int ctrl2, int reg, int over, long num, int width);
string CAddF(int qubits, int ctrl1, int reg, int over, long num, int width);
string AddF(int qubits, int reg, int over, long num, int width);
vector <string> AddF(int qubits, int reg, int over, long num, int width, bool controlled);

string C2SubF(int qubits, int ctrl1, int ctrl2, int reg, int over, long num, int width);
string CSubF(int qubits, int ctrl1, int reg, int over, long num, int width);
string SubF(int qubits, int reg, int over, long num, int width);
vector <string> SubF(int qubits, int reg, int over, long num, int width, bool controlled);

//////////////////////////////////////////////////////////////////////////

string int2str(int number);

long mul_inv(long a, long b);
int revert_bits(int res, int n);
int quantum_ipow(int a, int b);

/* Calculate the greatest common divisor with Euclid's algorithm */
int quantum_gcd(int u, int v);
void quantum_frac_approx(int *a, int *b, int width);

//////////////////////////////////////////////////////////////////////////

#endif
