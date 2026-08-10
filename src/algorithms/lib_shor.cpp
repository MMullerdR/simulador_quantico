#include "../../include/lib_shor.h"
#include "../../include/dgm.h"
#include "../../include/common.h"
#include "../../include/gates.h"
#include <cmath>
#include <iostream>
#include <algorithm>

using namespace std;

void ApplyQFT(int qubits, int type, int gpu_count, int gpu_region_bits, int coalesced_bits, int block_size, int repeat_count){
	DGM dgm;
	dgm.exec_type = type;
	dgm.gpu_count = gpu_count;

	dgm.cpu_region_bits = gpu_region_bits;
	dgm.cpu_coalesced_bits = coalesced_bits;
	dgm.block_size = block_size;
	dgm.repeat_count = repeat_count;

	dgm.qubits = qubits;
	dgm.allocateMemory();
	dgm.setMemoryValue(0);

	vector<string> qft = QFT2(qubits,0,qubits);

	dgm.executeFunction(qft);
}

//////////////////////////////////////////////////////

//number_to_factor - número a ser fatorado
//type - tipo de execução
//thread_count - número de threads usadas na execução paralela em CPU
vector<int> Shor(long number_to_factor, int type, int thread_count, int cpu_region_bits, int cpu_coalesced_bits, int gpu_count, int gpu_region_bits, int gpu_coalesced_bits, int block_size, int repeat_count){
	long base_value, bit_width, base_pow_mod, base_inv_pow_mod, remaining_value, measured_bit, measured_phase_bits;

	int qubits, qft_qb, reg1, reg2, over, over_bool;
	int gcd_candidate1, gcd_candidate2, found_factor;

	//////////////////////////////////
	DGM dgm;
	dgm.exec_type = type;
	dgm.gpu_count = gpu_count;
	dgm.thread_count = thread_count;

	dgm.cpu_region_bits = cpu_region_bits;
	dgm.cpu_coalesced_bits = cpu_coalesced_bits;

	dgm.gpu_region_bits = gpu_region_bits;
	dgm.gpu_coalesced_bits = gpu_coalesced_bits;
	dgm.block_size = block_size;
	dgm.repeat_count = repeat_count;
	//-----------------------------//

	remaining_value = number_to_factor;
	base_value = bit_width = 0;
	while (remaining_value){
		bit_width++;
		remaining_value = remaining_value >> 1;
	}
	qubits = 2*bit_width+3;

	//////////////////////////////////////
	dgm.qubits = qubits;
	dgm.allocateMemory();
	dgm.setMemoryValue((1<<(bit_width+2)));
	//----------------------------------//

	qft_qb = 0;
	reg1 = 1;
	reg2 = bit_width+2;
	over = bit_width+1;
	over_bool = qubits - 1;

	while((quantum_gcd(number_to_factor, base_value) > 1) || (base_value < 2)){
		base_value = rand() % number_to_factor;
	}

	string x_step0 = Pauli_X(qubits, 0, 1);
	string hadamard_step0 = Hadamard(qubits, qft_qb, 1);

	measured_phase_bits = 0;
	int top_round_index = 2*bit_width-1;
	long base_inverse = mul_inv(base_value,number_to_factor);

	vector <string> round_steps, sub_steps;

	for (int round_index = top_round_index; round_index >= 0; round_index--){
		base_pow_mod = modular_pow(base_value, 1L << round_index, number_to_factor);
		base_inv_pow_mod = modular_pow(base_inverse, 1L << round_index, number_to_factor);

		round_steps.clear();

		round_steps.push_back(hadamard_step0);

		sub_steps = CMultMod(qubits, qft_qb, reg1, reg2, over, over_bool, bit_width, base_pow_mod, number_to_factor);

		round_steps.insert(round_steps.end(), sub_steps.begin(), sub_steps.end());
		sub_steps = CSwapR(qubits, qft_qb, reg1, reg2, bit_width);
		round_steps.insert(round_steps.end(), sub_steps.begin(), sub_steps.end());

		sub_steps = CRMultMod(qubits, qft_qb, reg1, reg2, over, over_bool, bit_width, base_pow_mod, number_to_factor);
		round_steps.insert(round_steps.end(), sub_steps.begin(), sub_steps.end());
		round_steps.push_back(hadamard_step0);

		if (measured_phase_bits) round_steps.push_back(genRot(qubits, qft_qb, measured_phase_bits));

		dgm.executeFunction(round_steps);

		measured_bit = dgm.measure(qft_qb);

		measured_phase_bits = (measured_phase_bits << 1) | measured_bit;
	}

/*
	dgm.setFunction(round_steps);
	//cout << "Aqui" << endl;
	dgm.CountOps();


	cout << "Shor " << qubits << " qubits" << endl;
	cout << "Dense: " << dgm.dense << endl;
	cout << "Main Diagonal: " << dgm.main_diag << endl;
	cout << "Secondary Diagonal: " << dgm.sec_diag << endl;
	cout << "C-Dense: " << dgm.c_dense << endl;
	cout << "C-Main Diagonal: " << dgm.c_main_diag << endl;
	cout << "C-Secondary Diagonal: " << dgm.c_sec_diag << endl;
	cout << "Total: " << dgm.total_op << endl << endl;

	return;
*/

	// Result check
	vector<int> factors;

	int numerator = revert_bits(measured_phase_bits, 2*bit_width);

	//cout << numerator << "   " << measured_phase_bits << endl;

	if(numerator==0)
	{
		//printf("Fail - Measured Zero.\n");
		return factors;
	}

	int denominator = 1<<(2*bit_width);

	//printf("Measured %i (%f), ", numerator, (float)numerator/denominator);

	quantum_frac_approx(&numerator, &denominator, bit_width);

	//printf("fractional approximation is %i/%i.\n", numerator, denominator);

	int cf_denominator = denominator;
	int multiple_index = 1;
	while ((cf_denominator*multiple_index) < (1<<bit_width)){
		if (modular_pow(base_value, cf_denominator*multiple_index, number_to_factor) == 1){
			denominator = cf_denominator * multiple_index;
			break;
		}
		multiple_index++;
	}
	if (denominator >= number_to_factor) denominator = cf_denominator;

	/*
	if((denominator % 2 == 1) && (2*denominator<(1<<bit_width)))
	{
		//printf("Odd denominator, trying to expand by 2.\n");
		denominator *= 2;
	}

	if(denominator % 2 == 1)
	{
		//printf("Odd period, try again.\n");
		return;
	}
	*/

	//printf("Possible period is %i.\n", denominator);

	int half_period_pow_mod = modular_pow(base_value, denominator/2, number_to_factor);
	gcd_candidate1 = quantum_gcd(number_to_factor, half_period_pow_mod+1);
	gcd_candidate2 = quantum_gcd(number_to_factor, half_period_pow_mod-1);

	if(gcd_candidate1>gcd_candidate2)
		found_factor=gcd_candidate1;
	else
		found_factor=gcd_candidate2;

	if((found_factor < number_to_factor) && (found_factor > 1))
	{
		factors.push_back(found_factor);
		factors.push_back((int)number_to_factor/found_factor);
		return factors;
	}

	if (cf_denominator!=denominator){
		half_period_pow_mod = modular_pow(base_value, cf_denominator/2, number_to_factor);
		gcd_candidate1 = quantum_gcd(number_to_factor, half_period_pow_mod+1);
		gcd_candidate2 = quantum_gcd(number_to_factor, half_period_pow_mod-1);

		if(gcd_candidate1>gcd_candidate2)
			found_factor=gcd_candidate1;
		else
			found_factor=gcd_candidate2;

		if((found_factor < number_to_factor) && (found_factor > 1)){
			factors.push_back(found_factor);
			factors.push_back((int)number_to_factor/found_factor);
			return factors;
		}
	}

	//printf("Fail - Try Again.\n");
	return factors;
}
