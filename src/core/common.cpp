#include "../../include/core/common.h"
#include <math.h>

float round_precision = 0.000000001;

// Sem seed_rng() explícito, std::mt19937 default-constrói com uma seed
// fixa (determinístico) -- aceitável pros testes (test_gates.cpp não
// precisa de aleatoriedade de verdade), mas os CLIs de fato (grover.cpp/
// shor.cpp) sempre chamam seed_rng() no início.
std::mt19937 g_rng;

void seed_rng(unsigned long seed){
	g_rng.seed(seed);
}

PT::PT(){
	matrix = NULL;
	control_bit_positions = control_rest = NULL;
}

PT::~PT(){
	// matrix é emprestado de Gates::list (cache de matrizes de porta da
	// execução atual — um por DGM, não mais compartilhado entre execuções
	// desde o item 5 em docs/07-bugs-e-pontos-de-atencao.md) — PT nunca é
	// dono desse ponteiro, então nunca deve liberá-lo; quem libera é
	// Gates::~Gates() (ver docs/07, item 3).
	if (control_bit_positions) free(control_bit_positions);
	if (control_rest) free(control_rest);
}

// Classifica a matriz 2x2 da porta pra escolher o caminho de execução
// mais barato (ver enum DENSE/DIAG_PRI/DIAG_SEC em common.h).
long PT::matrixType(){
	if ((matrix[1] == 0.0) && (matrix[2] == 0.0)) return DIAG_PRI;
	else if ((matrix[0] == 0.0) && (matrix[3] == 0.0)) return DIAG_SEC;

	return DENSE;
}

// Preenche "arg" com a máscara/valor de controle já separados em duas
// partes: a que cai dentro da região coalescida da GPU e o restante —
// ver docs/04-gpu-cuda.md.
void PT::setArgsGPU(long *arg, int region_start, int region_size, int coalesced_bits){
	long coalesced_mask = (1 << coalesced_bits) - 1;
	long region_mask = ((1<<(region_size-coalesced_bits)) - 1) << region_start;
	long rest_mask = ~(coalesced_mask | region_mask);



	arg[SHIFT] = target_bit;
	if (target_bit >= coalesced_bits) arg[SHIFT] = target_bit - region_start + coalesced_bits;

	if (control_mask){
		arg[CTRL_MASK] = control_mask & rest_mask;
		arg[CTRL_VALUE] = control_value & rest_mask;

		arg[CTRL_REG_MASK] = (control_mask & coalesced_mask) | ((control_mask & region_mask) >> (region_start - coalesced_bits));


		arg[CTRL_REG_VALUE] = (control_value & coalesced_mask) | ((control_value & region_mask) >> (region_start - coalesced_bits));
	}
	else
	{
		arg[CTRL_MASK] = arg[CTRL_VALUE] = arg[CTRL_REG_MASK] = arg[CTRL_REG_VALUE] = 0;
	}
}

// Imprime os campos do PT (depuração).
void PT::print(){
	printf("qubits: %d\nmatrix_size: %d\nspan_start_bit: %d\ntarget_bit: %d\nAffect: %d\nCtrl-(value: %ld, mask: %ld, count: %ld)\n", qubits, matrix_size, span_start_bit, target_bit, affected, control_value, control_mask, control_count);

	for (int control_index = 0; control_index < control_count; control_index++)
		printf("%d: %ld\n", control_index, control_bit_positions[control_index]);
}

// Imprime a matriz 2x2 da porta (depuração).
void PT::printMatrix(){
	for (int row = 0; row < matrix_size; row++){
		for (int col = 0; col < matrix_size; col++)
			printf("%d: %.4f, %.4f  \t", row*matrix_size+col, crealf(matrix[row*matrix_size+col]), cimagf(matrix[row*matrix_size+col]));
		printf("\n");
	}

}

// Imprime o vetor de estado inteiro, uma amplitude por linha.
void printMem(float complex* mem, int qubits){
	long size = 1L << qubits;
	float range = 1.0/(1L << 21);

	float real, imag, magnitude;
	for (long state_index = 0; state_index < size; state_index++){
		real = imag = 0;
		magnitude = fabs(crealf(mem[state_index]));
		real = crealf(mem[state_index]);

		magnitude = fabs(cimagf(mem[state_index]));
		imag = cimagf(mem[state_index]);
		printf("%ld:\t%.6f %.6f\n", state_index, real, imag);
	}
}

void printMemExp(float complex* mem, int qubits, int reg1, int reg2, long width){
	long size = 1L << qubits;
	float range = 1.0/(1L << 21);

	long mask = (1L << width) - 1;

	float real, imag, magnitude;
	long previous_exponent_value = 0;
	for (long state_index = 0; state_index < size; state_index++){
		real = imag = 0;
		magnitude = fabs(crealf(mem[state_index]));
		if (magnitude > round_precision)
			real = crealf(mem[state_index]);

		magnitude = fabs(cimagf(mem[state_index]));
		if (magnitude > round_precision)
			imag = cimagf(mem[state_index]);

		if ((imag != 0) || (real != 0)){
			long exponent_value = (state_index>>(qubits-reg1-width))&mask;
			long modular_result = (state_index>>(qubits-reg2-width))&mask;
			printf("%ld\t>>  X: %ld\tExp: %ld\tDif: %ld\t\t\tV: %f %f\n", state_index, exponent_value, modular_result, exponent_value-previous_exponent_value, crealf(mem[state_index]), cimagf(mem[state_index]));
			previous_exponent_value = exponent_value;
		}
	}
}

void printMemCheckExp(float complex* mem, int qubits, long width, long base_value, long modulus){
	long size = 1L << qubits;
	float range = 1.0/(1L << 21);

	long mask = (1L << width) - 1;

	float real, imag, magnitude;
	long previous_exponent_value = 0;
	for (long state_index = 0; state_index < size; state_index++){
		real = imag = 0;
		magnitude = fabs(crealf(mem[state_index]));
		if (magnitude > round_precision)
			real = crealf(mem[state_index]);

		magnitude = fabs(cimagf(mem[state_index]));
		if (magnitude > round_precision)
			imag = cimagf(mem[state_index]);

		if ((imag != 0) || (real != 0)){
			long exponent_value = (state_index>>(2*width+2));
			long modular_result = (state_index>>(width+2))&mask;
			bool is_valid = true;

			if (modular_pow(base_value,exponent_value,modulus) != modular_result) is_valid = "Erro";

			printf("%ld\t>>  X: %ld\tExp: %ld\tDif: %ld\t\t", state_index, exponent_value, modular_result, exponent_value-previous_exponent_value);
			previous_exponent_value = exponent_value;
			if (modular_pow(base_value,exponent_value,modulus) != modular_result) printf("Errado\n");
			else printf("\n");
		}
	}
}

// Critério de ordenação usado ao montar vec_pts (ver DGM::setFunction):
// operadores não afetados/controlados vêm antes, depois por posição de
// bit crescente.
bool increasing(const PT *term1, const PT *term2){
	if (term1->affected == term2->affected)
		return term1->span_start_bit > term2->span_start_bit;
	else
		return term2->affected;
}

bool decreasing(const PT *term1, const PT *term2){
	if (term1->affected == term2->affected)
		return term1->span_start_bit < term2->span_start_bit;
	else
		return term1->affected;
}

////////////////////////////////////////////////////////////
void swap_value(int *value1, int *value2){
	int temp = *value1;
	*value1 = *value2;
	*value2 = temp;
}

void swap_ptr(float **ptr1, float **ptr2){
	float *temp = *ptr1;
	*ptr1 = *ptr2;
	*ptr2 = temp;
}

void swap_ptr(float complex **ptr1, float complex **ptr2){
	float complex *temp = *ptr1;
	*ptr1 = *ptr2;
	*ptr2 = temp;
}
//////////////////////////////////////////////////////////

int timeval_subtract(struct timeval *result, struct timeval *end, struct timeval *start)
{
    long int diff = (end->tv_usec + 1000000 * end->tv_sec) - (start->tv_usec + 1000000 * start->tv_sec);
    result->tv_sec = diff / 1000000;
    result->tv_usec = diff % 1000000;

    return (diff<0);
}

long modular_pow(long base, long exponent, long modulus){
	long result = 1;
	base = base % modulus;
	exponent = exponent;
	while (exponent){
		if (exponent&1) result = (result * base) % modulus;
		exponent = exponent >> 1;
		base = (base * base) % modulus;
	}
	return result;
}
