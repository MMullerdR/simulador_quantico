#include "../../include/core/common.h"
#include <vector>
#include <set>
#include <cstring>

using namespace std;

// Reordena pts[] (terminado em NULL) para a GPU gastar menos passadas sobre
// o vetor de estado -- ver ScheduleForBatching em common.h e item 24 em
// docs/07-bugs-e-pontos-de-atencao.md.
//
// Duas portas comutam quando nenhum qubit compartilhado é alvo "não
// diagonal" de uma delas: controles e alvos de matriz diagonal só
// multiplicam fases por projetores da base computacional, e isso comuta com
// qualquer outra coisa que também seja diagonal naquele qubit. Cada porta
// vira dois conjuntos de qubits:
//   Z = controles + alvo (se a matriz é diagonal)
//   N = alvo (se a matriz não é diagonal)
// e duas portas só NÃO comutam se algum qubit está em N de uma e em Z ou N
// da outra. As dependências abaixo guardam, por qubit, a última porta N e as
// portas Z desde então -- o mesmo que o grafo completo, sem O(n^2).
//
// Em seguida, lista de prontos em ordem original, enchendo um lote por vez
// com o mesmo critério de janela do laço de GpuExecution01 (kernel.cu):
// alvo abaixo de coalesced_bits entra sempre; os demais precisam caber numa
// janela de extra_bits bits consecutivos.
void ScheduleForBatching(PT **pts, int coalesced_bits, int extra_bits, int max_batch){
	long n = 0;
	while (pts[n] != NULL) n++;
	if (n < 3 || max_batch < 1) return;

	vector< vector<int> > successors(n);
	vector<int> pending(n, 0);
	vector<int> stamp(n, -1);

	const int MAX_BITS = 64;
	vector<int> last_n(MAX_BITS, -1);
	vector< vector<int> > z_since(MAX_BITS);

	for (long i = 0; i < n; i++){
		PT *term = pts[i];
		long target = 1L << term->target_bit;
		bool diagonal;
		long z_bits;
		if (term->phase_terms){
			// porta de fase fundida: diagonal em todos os bits que algum termo toca
			diagonal = true;
			z_bits = 0;
			for (size_t t = 0; t < term->phase_terms->size(); t++) z_bits |= (*term->phase_terms)[t].mask;
		}
		else{
			diagonal = (term->matrixType() == DIAG_PRI);
			z_bits = (long) term->control_mask | (diagonal ? target : 0L);
		}
		long n_bits = diagonal ? 0L : target;

		for (int bit = 0; bit < MAX_BITS; bit++){
			long mask = 1L << bit;
			if (!((z_bits | n_bits) & mask)) continue;

			// quem esta porta precisa esperar neste qubit
			if (last_n[bit] >= 0 && stamp[last_n[bit]] != i){
				stamp[last_n[bit]] = i;
				successors[last_n[bit]].push_back(i);
				pending[i]++;
			}
			if (n_bits & mask){
				for (size_t k = 0; k < z_since[bit].size(); k++){
					int earlier = z_since[bit][k];
					if (stamp[earlier] == i) continue;
					stamp[earlier] = i;
					successors[earlier].push_back(i);
					pending[i]++;
				}
			}
		}

		for (int bit = 0; bit < MAX_BITS; bit++){
			long mask = 1L << bit;
			if (z_bits & mask) z_since[bit].push_back(i);
			if (n_bits & mask){
				last_n[bit] = i;
				z_since[bit].clear();
			}
		}
	}

	set<int> ready;
	for (long i = 0; i < n; i++)
		if (pending[i] == 0) ready.insert(i);

	vector<PT*> ordered;
	ordered.reserve(n);

	while ((long) ordered.size() < n){
		int batch_size = 0;
		int window_min = -1, window_max = -1;
		bool progress = true;

		while (progress && batch_size < max_batch){
			progress = false;
			set<int>::iterator it = ready.begin();
			while (it != ready.end() && batch_size < max_batch){
				int i = *it;
				int bit = pts[i]->target_bit;
				bool fits = false;
				int new_min = window_min, new_max = window_max;

				if (bit < coalesced_bits){
					fits = true;
				}
				else if (window_min < 0){
					fits = true;
					new_min = new_max = bit;
				}
				else{
					int low = (bit < window_min) ? bit : window_min;
					int high = (bit > window_max) ? bit : window_max;
					if (high - low < extra_bits){
						fits = true;
						new_min = low;
						new_max = high;
					}
				}

				if (!fits){
					++it;
					continue;
				}

				window_min = new_min;
				window_max = new_max;
				ordered.push_back(pts[i]);
				batch_size++;
				progress = true;

				it = ready.erase(it);
				for (size_t k = 0; k < successors[i].size(); k++){
					int next = successors[i][k];
					if (--pending[next] == 0) ready.insert(next);
				}
			}
		}
	}

	// ordered tem n ponteiros: é uma permutação de pts[0..n)
	memcpy(pts, &ordered[0], n * sizeof(PT*));
}
