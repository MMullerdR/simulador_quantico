#ifndef _LIBGROVER_H_
#define _LIBGROVER_H_

#include <vector>
#include <string>
// <complex.h> precisa ser incluído antes da define abaixo, senão a
// definição de "complex" daqui contamina a inicialização do <complex.h>
// do sistema quando ele for incluído depois (por common.h/dgm.h).
#include <complex.h>

using namespace std;

#define complex __complex__

float Grover(long qubits, long search_value, int type, int thread_count, int cpu_region_bits, int cpu_coalesced_bits, int gpu_count, int gpu_region_bits, int gpu_coalesced_bits, int block_size, int repeat_count);

string ControledZ(int qubits);
string Oracle1(long qubits, long search_value);

#endif
