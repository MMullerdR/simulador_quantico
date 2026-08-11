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

// Busca "search_value" num espaço de qubits-1 qubits (o qubit 0 é
// ancilla) via amplificação de amplitude — ver docs/05-algoritmo-grover.md.
// Devolve o valor medido do registrador de busca (com alta probabilidade
// igual a search_value, mas não garantido — Grover é probabilístico).
long Grover(long qubits, long search_value, int type, int thread_count, int cpu_region_bits, int cpu_coalesced_bits, int gpu_count, int gpu_region_bits, int gpu_coalesced_bits, int block_size, int repeat_count);

// Z multi-controlado sobre todos os qubits de busca — parte do difusor.
string ControledZ(int qubits);
// Oráculo: inverte a fase do estado onde o registrador de busca == search_value.
string Oracle1(long qubits, long search_value);

#endif
