#include "../../include/lib_shor.h"
#include "../../include/dgm.h"
#include "../../include/common.h"
#include "../../include/gates.h"
#include <cmath>
#include <iostream>
#include <algorithm>

using namespace std;

int revert_bits(int value, int bit_count){
	int reversed = 0;

	for (int bit_index = 0; bit_index < bit_count; bit_index++){
		reversed = (reversed<<1) | (value&1);
		value = value >> 1;
	}
	return reversed;
}

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
		//r = u % v;
		//u = v;
		//v = r;
	}
	return value1;
}


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

string int2str(int number){
	stringstream ss;
	ss << number;

	string text = ss.str();

	return text;
}
