#include "../../include/algorithms/lib_shor.h"
#include "../../include/core/dgm.h"
#include "../../include/core/common.h"
#include "../../include/core/gates.h"
#include <cmath>
#include <iostream>
#include <algorithm>

using namespace std;

// Inverte a ordem dos bit_count bits de "value" (bit 0 vira o mais
// significativo e vice-versa) — desfaz a ordem em que a QFT semiclássica
// do Shor mede os bits da fase.
int revert_bits(int value, int bit_count){
	int reversed = 0;

	for (int bit_index = 0; bit_index < bit_count; bit_index++){
		reversed = (reversed<<1) | (value&1);
		value = value >> 1;
	}
	return reversed;
}

// Potência inteira (base^exponent); sem uso no código atual.
int quantum_ipow(int base, int exponent)
{
	int exp_index;
	int result=1;

	for(exp_index=0; exp_index<exponent ;exp_index++)
		result*=base;

	return result;
}

/* Calculate the greatest common divisor with Euclid's algorithm */

int quantum_gcd(int value1, int value2)
{
	int remainder;

	while(value2)
	{
		remainder = value2;
		value2 = value1 % value2;
		value1 = remainder;
	}
	return value1;
}

// Aproxima *numerator/*denominator pela fração contínua mais simples com
// denominador < 2^width — usado no Shor pra extrair o período candidato
// a partir da fase medida (ver docs/06-algoritmo-shor.md).
void quantum_frac_approx(int *numerator, int *denominator, int width)
{
	float target_ratio = (float) *numerator / *denominator;
	float residual=target_ratio;
	int term, num_prev2=0, den_prev2=1, num_prev1=1, den_prev1=0, num_curr=0, den_curr=0;

	do
		{
		term = (int) (residual+0.000005);

		residual -= term-0.000005;
		residual = 1.0/residual;

		if (term * den_prev1 + den_prev2 > 1<<width)
		break;

		num_curr = term * num_prev1 + num_prev2;
		den_curr = term * den_prev1 + den_prev2;

		num_prev2 = num_prev1;
		den_prev2 = den_prev1;
		num_prev1 = num_curr;
		den_prev1 = den_curr;

		} while(fabs(((double) num_curr / den_curr) - target_ratio) > 1.0 / (2 * (1 << width)));

	*numerator = num_curr;
	*denominator = den_curr;

	return;
}

// Inverso multiplicativo de "value" módulo "modulus" (algoritmo de
// Euclides estendido, iterativo).
long mul_inv(long value, long modulus){
	long modulus_orig = modulus, temp, quotient;
	long coeff_prev = 0, coeff_curr = 1;
	if (modulus == 1) return 1;
	while (value > 1) {
		quotient = value / modulus;
		temp = modulus, modulus = value % modulus, value = temp;
		temp = coeff_prev, coeff_prev = coeff_curr - quotient * coeff_prev, coeff_curr = temp;
	}
	if (coeff_curr < 0) coeff_curr += modulus_orig;

	return coeff_curr;
}

// Converte um inteiro em string — usado pra montar nomes únicos de porta
// (ex: "ADD_" + int2str(value) + "_" + int2str(digit)).
string int2str(int number){
	stringstream ss;
	ss << number;

	string text = ss.str();

	return text;
}
