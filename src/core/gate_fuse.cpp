#include "../../include/core/common.h"
#include <vector>
#include <map>
#include <cmath>

using namespace std;

// Funde corridas de portas de fase consecutivas numa só (ver PhaseTerm em
// common.h e item 27 em docs/07-bugs-e-pontos-de-atencao.md).
//
// Uma porta é elegível quando é diag(1, e^{i*theta}) com theta múltiplo de
// 2*pi/2^FUSED_PHASE_BITS (Z, R1-R3, rotações da QFT e dos somadores do Shor,
// com ou sem controles de valor 0/1): multiplica a amplitude por e^{i*theta}
// onde o alvo e os controles batem, e não faz mais nada. Portas assim
// comutam entre si, então uma corrida delas pode ser aplicada numa passada só.
static bool phase_term_of(PT *term, PhaseTerm &out){
	if (term->phase_terms) return false;
	if (term->matrixType() != DIAG_PRI) return false;

	float complex m00 = term->matrix[0], m11 = term->matrix[3];
	if (crealf(m00) != 1.0f || cimagf(m00) != 0.0f) return false;

	double re = crealf(m11), im = cimagf(m11);
	if (fabs(sqrt(re * re + im * im) - 1.0) > 1e-5) return false;

	double exact = atan2(im, re) / (2.0 * M_PI) * FUSED_PHASE_STEPS;
	long steps = lround(exact);
	// ângulo que não é múltiplo de 2*pi/2^FUSED_PHASE_BITS (com folga pra
	// erro de arredondamento de float): fica como porta normal.
	if (fabs(exact - (double) steps) > 0.02) return false;

	long target_mask = 1L << term->target_bit;
	out.mask = term->control_mask | target_mask;
	out.value = term->control_value | target_mask;
	out.steps = steps & (FUSED_PHASE_STEPS - 1);
	return true;
}

static PT* make_fused(PT *first, const map< pair<long, long>, long > &terms){
	PT *fused = new PT();
	fused->qubits = first->qubits;
	fused->matrix = NULL;
	fused->matrix_size = first->matrix_size;
	fused->span_start_bit = 0;
	fused->target_bit = 0;
	fused->affected = false;
	fused->control_mask = 0;
	fused->control_value = 0;
	fused->control_count = 0;
	fused->phase_terms = new vector<PhaseTerm>();

	for (map< pair<long, long>, long >::const_iterator it = terms.begin(); it != terms.end(); ++it){
		long steps = it->second & (FUSED_PHASE_STEPS - 1);
		if (steps == 0) continue;	// termos que se cancelam não custam nada
		PhaseTerm term;
		term.mask = it->first.first;
		term.value = it->first.second;
		term.steps = steps;
		fused->phase_terms->push_back(term);
	}
	return fused;
}

// Reescreve list NO LUGAR (o índice de escrita nunca passa do de leitura e a
// lista só encolhe): o armazenamento do vector não é realocado, então
// ponteiros para list[0] guardados por quem chamou (DGM::pts, o parâmetro pts
// de HybridExecution) continuam válidos -- uma versão que trocava o vector
// por um novo derrubou o t_HYBRID delegado à GPU por uso de memória liberada.
long FusePhaseRuns(vector<PT*> &list){
	size_t write = 0;
	long absorbed = 0;

	size_t i = 0;
	while (i < list.size() && list[i] != NULL){
		PhaseTerm first_term;
		if (!phase_term_of(list[i], first_term)){
			list[write++] = list[i];
			i++;
			continue;
		}

		// corrida de portas elegíveis a partir de i (no máximo FUSED_MAX_TERMS
		// termos distintos por porta fundida)
		size_t run_start = i;
		map< pair<long, long>, long > terms;
		size_t j = i;
		while (j < list.size() && list[j] != NULL){
			PhaseTerm term;
			if (!phase_term_of(list[j], term)) break;
			pair<long, long> key(term.mask, term.value);
			if (terms.find(key) == terms.end() && terms.size() >= FUSED_MAX_TERMS) break;
			terms[key] += term.steps;
			j++;
		}

		if (j - run_start < 2){
			list[write++] = list[i];	// uma porta sozinha não ganha nada
			i++;
			continue;
		}

		PT *fused = make_fused(list[run_start], terms);
		for (size_t k = run_start; k < j; k++) delete list[k];
		absorbed += (long) (j - run_start);
		list[write++] = fused;
		i = j;
	}

	list[write++] = NULL;
	list.resize(write);
	return absorbed;
}
