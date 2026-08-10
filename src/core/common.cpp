#include "../../include/common.h"
#include <math.h>

float round_precision = 0.000000001;

PT::PT(){
	matrix = NULL;
	control_bit_positions = control_rest = NULL;
}

void PT::destructor(){
	if ((matrix_size != 1) && !matrix) free(matrix);
	if (!control_bit_positions) free(control_bit_positions);
	if (!control_rest) free(control_rest);
}

// Retorna quantos controles têm posição de bit menor que "qubit" — parte
// de uma otimização de controle parcial nunca finalizada (ver setArgs).
long PT::ctrlAffect(long qubit){
	long control_index;
	for (control_index = 0; control_index < control_count; control_index++)
		if (control_bit_positions[control_index] < qubit) return control_index;

	return control_count;
}

long PT::matrixType(){
	if ((matrix[1] == 0.0) && (matrix[2] == 0.0)) return DIAG_PRI;
	else if ((matrix[0] == 0.0) && (matrix[3] == 0.0)) return DIAG_SEC;

	return DENSE;
}

/*
void PT::ctrlRest(long up_to_bit){
	long p, v, aux;
	long n = pow(2,control_count-up_to_bit);
	control_rest_count = n - 1;
	//if (control_rest != NULL) free(control_rest);
	if (n) control_rest = (long*)malloc(sizeof(long)*n - 1);

	p = 0;
	for (long i = 0; i < n; i++){
		v = 0;
		aux = i;
		for (long c = control_count - 1; c >= up_to_bit; c--){
			v = v | ((aux & 1) << control_bit_positions[c]);
			aux = aux >> 1;
		}
		if (v != control_value){
			control_rest[p] = v;
			p++;
		}
	}
}
*/
void PT::setArgs(long *arg, long up_to_bit){
	long affected_control_count = ctrlAffect(up_to_bit);
	//ctrlRest(up_to_bit);

	//arg[MAT_SIZE] = matrix_size;
	arg[SHIFT] = target_bit;
	arg[CTRL_MASK] = control_mask;
	arg[CTRL_VALUE] = control_value;
	//arg[CTRL_COUNT] = control_count - affected_control_count;
	//arg[CTRL_REST] = control_rest_count;

	//arg[MAT_START] = 0;
	//arg[MAT_END] = arg[MAT_SIZE];
	//arg[ACUMM] = 0;
}

void PT::setArgs_soft(long *arg, long up_to_bit){
	//long affected_control_count = ctrlAffect(up_to_bit);
	//ctrlRest(up_to_bit);

	//arg[MAT_SIZE] = matrix_size;
	arg[SHIFT] = target_bit;
	arg[CTRL_MASK] = control_mask;
	arg[CTRL_VALUE] = control_value;
	//arg[CTRL_COUNT] = control_count - affected_control_count;
	//arg[CTRL_REST] = control_rest_count;

	//arg[MAT_START] = 0;
	//arg[MAT_END] = arg[MAT_SIZE];
	//arg[ACUMM] = 0;
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
	/*
	if (control_mask){
		printf("\n\ntarget_bit: %d\nregion_start: %ld\nregion_size: %ld\ncoalesced_bits: %ld\nCtrl-(value: %ld, mask: %ld, count: %ld)\n**mask**:%ld\n", target_bit, region_start, region_size, coalesced_bits, control_value, control_mask, control_count, mask);
		printf("Global Ctrl-(value: %ld, mask: %ld)\n", arg[CTRL_VALUE], arg[CTRL_MASK]);
		printf("Region Ctrl-(value: %ld, mask: %ld)\n----------------------------------------\n\n", arg[CTRL_REG_VALUE], arg[CTRL_REG_MASK]);
	}
	*/
}

void PT::print(){
	printf("qubits: %d\nmatrix_size: %d\nspan_start_bit: %d\ntarget_bit: %d\nAffect: %d\nCtrl-(value: %ld, mask: %ld, count: %ld)\n", qubits, matrix_size, span_start_bit, target_bit, affected, control_value, control_mask, control_count);

	for (int control_index = 0; control_index < control_count; control_index++)
		printf("%d: %ld\n", control_index, control_bit_positions[control_index]);

	/*
	printf("Rest:\n");
	for (int i = 0; i < control_rest_count; i++){
		printf("%ld\n", control_rest[i]);
	}
	*/
}

void PT::printMatrix(){
	for (int row = 0; row < matrix_size; row++){
		for (int col = 0; col < matrix_size; col++)
			printf("%d: %.4f, %.4f  \t", row*matrix_size+col, crealf(matrix[row*matrix_size+col]), cimagf(matrix[row*matrix_size+col]));
		printf("\n");
	}

}

void printMem(float complex* mem, int qubits){
	long size = 1L << qubits;
	float range = 1.0/(1L << 21);

	float real, imag, magnitude;
	for (long state_index = 0; state_index < size; state_index++){
		real = imag = 0;
		magnitude = fabs(crealf(mem[state_index]));
		//if (magnitude > round_precision)
			real = crealf(mem[state_index]);

		magnitude = fabs(cimagf(mem[state_index]));
		//if (magnitude > round_precision)
			imag = cimagf(mem[state_index]);
		//if (real != 0 || imag != 0)
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
			//printf("%ld >>> X: %ld\tB: %ld\tOver: b%ld %ld\tV: %f %f\n", i, (i>>(n+2))&mask, (i>>1)&(mask), (i&1), ((i>>(n+1))&1), crealf(mem[i]), cimagf(mem[i]));
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

			//printf("%ld >>> X: %ld\tB: %ld\tOver: b%ld %ld\tV: %f %f\n", i, (i>>(n+2))&mask, (i>>1)&(mask), (i&1), ((i>>(n+1))&1), crealf(mem[i]), cimagf(mem[i]));
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
