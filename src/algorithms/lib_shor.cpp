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

string genRot(int qubits, int reg, long phase_bits){
	vector <string> step_ops(qubits, "ID");
	string name;

	// Guarda o valor original antes do laço abaixo consumi-lo bit a bit —
	// precisamos dele pra dar um nome único à porta (ver
	// docs/07-bugs-e-pontos-de-atencao.md, item 1: usar 'phase_bits' já
	// zerado aqui fazia toda correção de fase colidir sob o nome
	// "Rot_0", reaproveitando sempre a matriz da primeira correção).
	long phase_bits_orig = phase_bits;

	int k = 2;
	float complex rot, euler_e;
	euler_e = M_E;

	rot = 1;
	while (phase_bits){
		if (phase_bits&1) rot *= cpowf(euler_e, -2*M_PI*I/pow(2.0, k));
		phase_bits = phase_bits >> 1;
		k++;
	}

	if (rot != 1){
		Gates g;
		name = "Rot_" + int2str(phase_bits_orig);
		g.addGate(name, 1.0, 0.0, 0.0, rot);
		step_ops[reg] = name;

		return concatena(step_ops, qubits);
	}

	return "";
}

vector <string> CU(int qubits, int ctrl, int reg1, int reg2, int width, long base_value, long number_to_factor){
	vector <string> m, rm, sw, u;
/*
	m = CMultMod(qubits, ctrl, reg1, reg2, width, a, N);
	rm = CRMultMod(qubits, ctrl, reg1, reg2, width, mul_inv(a,N), N);
	sw = CSwapR(qubits, ctrl, reg1, reg2+1, width);


	u = m;
	u.insert(u.end(), sw.begin(), sw.end());
	u.insert(u.end(), rm.begin(), rm.end());
*/
	return u;
}

vector<string> CMultMod(int qubits, int ctrl, int reg1, int reg2, int over, int over_bool, int width, long base_value, long number_to_factor){
//        cout << "MULT" << endl;

	int ctrl2;
	vector <string> qft = QFT(qubits, reg2, over, width);
//        cout << "MULT" << endl;


	string hadamard_step = Hadamard(qubits, reg2, width);

//        cout << "MULT" << endl;

	vector <string> rqft = RQFT(qubits, reg2, over, width);

//        cout << "MULT" << endl;

	//////////////////////////////////////////////////////////////

//        cout << "MULT" << endl;


	vector <string> mult_mod;
	vector <string> add_mod_steps;
	mult_mod.push_back(Hadamard(qubits, over, 1));
	mult_mod.push_back(hadamard_step);

	ctrl2 = reg1 + width - 1;
	for (int bit_index = 0; bit_index < width; bit_index++){
		add_mod_steps = C2AddMod(qubits, ctrl, ctrl2-bit_index, reg2, over, over_bool, width, base_value, number_to_factor);
		//for (int j = 0; j < add_mod_steps.size(); j++) cout << add_mod_steps[j] << endl;
		//exit(1);
		mult_mod.insert(mult_mod.end(), add_mod_steps.begin(), add_mod_steps.end());

		base_value = (base_value*2)%number_to_factor;
	}

//        cout << "MULT" << endl;


	mult_mod.insert(mult_mod.end(), rqft.begin(), rqft.end());

	return mult_mod;

}

vector<string> CRMultMod(int qubits, int ctrl, int reg1, int reg2, int over, int over_bool, int width, long base_value, long number_to_factor){
	int ctrl2;
	vector <string> qft = QFT(qubits, reg2, over, width);
	vector <string> rqft = RQFT(qubits, reg2, over, width);

	//////////////////////////////////////////////////////////////

	vector <string> mult_mod;
	vector <string> sub_mod_steps;

	ctrl2 = reg1 + width - 1;
	for (int bit_index = 0; bit_index < width; bit_index++){
		sub_mod_steps = C2SubMod(qubits, ctrl, ctrl2-bit_index, reg2, over, over_bool, width, base_value, number_to_factor);
		mult_mod.insert(mult_mod.begin(), sub_mod_steps.begin(), sub_mod_steps.end());

		base_value = (base_value*2)%number_to_factor;
	}

	mult_mod.insert(mult_mod.begin(), qft.begin(), qft.end());
	mult_mod.insert(mult_mod.end(), rqft.begin(), rqft.end());

	return mult_mod;
}

vector <string> C2AddMod(int qubits, int ctrl1, int ctrl2, int reg, int over, int over_bool, int width, long base_value, long number_to_factor){
	vector<string> qft = QFT(qubits, reg, over, width);
	vector<string> rqft = RQFT(qubits, reg, over, width);

	string c2_add_a = C2AddF(qubits, ctrl1, ctrl2, reg, over, base_value, width);
	string c2_sub_a = C2SubF(qubits, ctrl1, ctrl2, reg, over, base_value, width);

	string sub_N = SubF(qubits, reg, over, number_to_factor, width);
	string c_add_N = CAddF(qubits, over_bool, reg, over, number_to_factor, width);

	string n_over = Pauli_X(qubits, over, 1);
	string c_over = CNot(qubits, over, over_bool);

	vector <string> circuit_steps;

	circuit_steps.push_back(c2_add_a);
	circuit_steps.push_back(sub_N);
	circuit_steps.insert(circuit_steps.end(), rqft.begin(), rqft.end());
	circuit_steps.push_back(c_over);
	circuit_steps.insert(circuit_steps.end(), qft.begin(), qft.end());
	circuit_steps.push_back(c_add_N);
	circuit_steps.push_back(c2_sub_a);
	circuit_steps.insert(circuit_steps.end(), rqft.begin(), rqft.end());
	circuit_steps.push_back(n_over);
	circuit_steps.push_back(c_over);
	circuit_steps.push_back(n_over);
	circuit_steps.insert(circuit_steps.end(), qft.begin(), qft.end());
	circuit_steps.push_back(c2_add_a);

	return circuit_steps;
}

vector <string> C2SubMod(int qubits, int ctrl1, int ctrl2, int reg, int over, int over_bool, int width, long base_value, long number_to_factor){
	vector <string> qft = QFT(qubits, reg, over, width);
	vector <string> rqft = RQFT(qubits, reg, over, width);

	string c2_add_a = C2AddF(qubits, ctrl1, ctrl2, reg, over, base_value, width);
	string c2_sub_a = C2SubF(qubits, ctrl1, ctrl2, reg, over, base_value, width);

	string add_N = AddF(qubits, reg, over, number_to_factor, width);
	string c_add_N = CAddF(qubits, over_bool, reg, over, number_to_factor, width);
	string c_sub_N = CSubF(qubits, over_bool, reg, over, number_to_factor, width);

	string n_over = Pauli_X(qubits, over, 1);
	string c_over = CNot(qubits, over, over_bool);

	vector <string> circuit_steps;

	circuit_steps.push_back(c2_sub_a);
	circuit_steps.insert(circuit_steps.end(), rqft.begin(), rqft.end());
	circuit_steps.push_back(n_over);
	circuit_steps.push_back(c_over);
	circuit_steps.push_back(n_over);
	circuit_steps.insert(circuit_steps.end(), qft.begin(), qft.end());
	circuit_steps.push_back(c2_add_a);
	circuit_steps.push_back(c_sub_N);
	circuit_steps.insert(circuit_steps.end(), rqft.begin(), rqft.end());
	circuit_steps.push_back(c_over);
	circuit_steps.insert(circuit_steps.end(), qft.begin(), qft.end());
	circuit_steps.push_back(add_N);
	circuit_steps.push_back(c2_sub_a);

	return circuit_steps;
}

//////////////////////////////////////////////////////////////////////////


string CAddF(int qubits, int ctrl1, int reg, int over, long value_to_add, int width){
	vector <string> step_ops = AddF(qubits, reg, over, value_to_add, width, true);

	step_ops[ctrl1] = "Control1(1)";

	return concatena(step_ops, qubits);
}


string C2AddF(int qubits, int ctrl1, int ctrl2, int reg, int over, long value_to_add, int width){
	vector <string> step_ops = AddF(qubits, reg, over, value_to_add, width, true);

	step_ops[ctrl1] = "Control1(1)";
	step_ops[ctrl2] = "Control1(1)";

	return concatena(step_ops, qubits);
}

string AddF(int qubits, int reg, int over, long value_to_add, int width){
	return concatena(AddF(qubits, reg, over, value_to_add, width, false), qubits);
}

vector <string> AddF(int qubits, int reg, int over, long value_to_add, int width, bool controlled){
	int digit_count = width+1;
	vector <float complex> rot (digit_count, 1);
	float complex  identity;

	Gates g;

	long remaining_value = value_to_add;

	float complex euler_e = M_E;

	for (int digit_index = 0; digit_index < digit_count; digit_index++){
		if (remaining_value&1)
			for (int higher_digit_index = digit_index; higher_digit_index < digit_count; higher_digit_index++)
				rot[higher_digit_index] *= cpowf(euler_e, 2*M_PI*I/pow(2.0, higher_digit_index-digit_index+1));
		remaining_value = remaining_value >> 1;
	}

	vector<string> step_ops(qubits, "ID");
	string name;

	int msb_pos = reg+width-1;
	identity = 1;
	for (int digit_index = 0; digit_index < digit_count; digit_index++){
		if (rot[digit_index] != identity){
			name = "ADD_" + int2str(value_to_add) + "_" + int2str(digit_index);
			g.addGate(name, 1.0, 0.0, 0.0, rot[digit_index]);
			if (controlled) name = "Target1(" + name + ")";
			step_ops[msb_pos-digit_index] = name;
		}
	}

	name = step_ops[reg-1];
	step_ops[reg-1] = "ID";
	step_ops[over] = name;

	return step_ops;
}

string CSubF(int qubits, int ctrl1, int reg, int over, long value_to_sub, int width){
	vector <string> step_ops = SubF(qubits, reg, over, value_to_sub, width, true);

	step_ops[ctrl1] = "Control1(1)";

	return concatena(step_ops, qubits);
}

string C2SubF(int qubits, int ctrl1, int ctrl2, int reg, int over, long value_to_sub, int width){
	vector <string> step_ops = SubF(qubits, reg, over, value_to_sub, width, true);

	step_ops[ctrl1] = "Control1(1)";
	step_ops[ctrl2] = "Control1(1)";

	return concatena(step_ops, qubits);
}

string SubF(int qubits, int reg, int over, long value_to_sub, int width){
	return concatena(SubF(qubits, reg, over, value_to_sub, width, false), qubits);
}

vector <string> SubF(int qubits, int reg, int over, long value_to_sub, int width, bool controlled){
	long digit_count = width+1;
	vector <float complex> rot (digit_count, 1);
	float complex  identity;

	Gates g;

	long remaining_value = value_to_sub;

	float complex euler_e = M_E;
	for (int digit_index = 0; digit_index < digit_count; digit_index++){
		if (remaining_value&1)
			for (int higher_digit_index = digit_index; higher_digit_index < digit_count; higher_digit_index++)
				rot[higher_digit_index] *= cpowf(euler_e, -2*M_PI*I/pow(2.0, higher_digit_index-digit_index+1));
		remaining_value = remaining_value >> 1;
	}

	vector<string> step_ops(qubits, "ID");
	string name;

	int msb_pos = reg+width-1;
	identity = 1;
	for (int digit_index = 0; digit_index < digit_count; digit_index++){
		if (rot[digit_index] != identity){
			name = "SUB_" + int2str(value_to_sub) + "_" + int2str(digit_index);
			g.addGate(name, 1.0, 0.0, 0.0, rot[digit_index]);
			if (controlled) name = "Target1(" + name + ")";
			step_ops[msb_pos-digit_index] = name;
		}
	}

	name = step_ops[reg-1];
	step_ops[reg-1] = "ID";
	step_ops[over] = name;

	return step_ops;
}


vector <string> QFT(int qubits, int reg, int over, int width){

//        cout << "QFT" << endl;

	string joined_step, name;
	vector <string> qft;

	Gates g;
	float complex rotation_value;
	for (int level = 1; level <= width+1; level++){
                name = "R" + int2str(level);

//		cout << "QFT " << level << endl;
		rotation_value = M_E;
		rotation_value = cpowf(rotation_value, 2*M_PI*I/pow(2.0, level));

//                cout << "QFT " << level << endl;
		g.addGate(name, 1.0, 0.0, 0.0, rotation_value);
//                cout << "QFT " << level << endl;
	}

//        cout << "QFT" << endl;


	vector <string> step_ops (qubits, "ID");

	qft.push_back(Hadamard(qubits, over, 1));
	for (int control_qubit_index = 0; control_qubit_index < width; control_qubit_index++){
		step_ops[control_qubit_index+reg] = "Control1(1)";
		step_ops[over] = "Target1(R" + int2str(control_qubit_index+2) + ")";

		joined_step = concatena(step_ops, qubits);
		qft.push_back(joined_step);
		step_ops[control_qubit_index+reg] = "ID";
	}
	step_ops[over] = "ID";

	for (int target_qubit_index = 0; target_qubit_index < width; target_qubit_index++){
		qft.push_back(Hadamard(qubits, target_qubit_index+reg, 1));

		for (int control_qubit_index = target_qubit_index+1; control_qubit_index < width; control_qubit_index++){
			step_ops[control_qubit_index+reg] = "Control1(1)";
			step_ops[target_qubit_index+reg] = "Target1(R" + int2str(control_qubit_index-target_qubit_index+1) + ")";

			joined_step = concatena(step_ops, qubits);
			qft.push_back(joined_step);

			step_ops[control_qubit_index+reg] = "ID";
		}
		step_ops[target_qubit_index+reg] = "ID";
	}

//        cout << "QFT" << endl;


	return qft;
}

vector <string> QFT2(int qubits, int reg, int width){
	string joined_step;
	vector <string> qft;

	Gates g;
	float complex rotation_value;
	for (int level = 1; level <= width+1; level++){
		rotation_value = M_E;
		rotation_value = cpowf(rotation_value, 2*M_PI*I/pow(2.0, level));
		g.addGate("R-" + int2str(level), 1.0, 0.0, 0.0, rotation_value);
	}

	vector <string> step_ops (qubits, "ID");

	for (int target_qubit_index = 0; target_qubit_index < width; target_qubit_index++){
		step_ops[target_qubit_index+reg] = "H";
		joined_step = concatena(step_ops, qubits);
		qft.push_back(joined_step);

		for (int control_qubit_index = target_qubit_index+1; control_qubit_index < width; control_qubit_index++){
			step_ops[control_qubit_index+reg] = "Control1(1)";
			step_ops[target_qubit_index+reg] = "Target1(R-" + int2str(control_qubit_index-target_qubit_index+1) + ")";

			joined_step = concatena(step_ops, qubits);
			qft.push_back(joined_step);

			step_ops[control_qubit_index+reg] = "ID";
		}
		step_ops[target_qubit_index+reg] = "ID";
	}

	return qft;
}

vector <string> RQFT(int qubits, int reg, int over, int width){
	string joined_step;
	vector <string> rqft;

	Gates g;
	float complex rotation_value;
	for (int level = 1; level <= width+1; level++){
		rotation_value = M_E;
		rotation_value = cpowf(rotation_value, -2*M_PI*I/pow(2.0, level));
		g.addGate("R'" + int2str(level), 1.0, 0.0, 0.0, rotation_value);
	}

	vector <string> step_ops (qubits, "ID");


	rqft.push_back(Hadamard(qubits, over, 1));
	for (int control_qubit_index = 0; control_qubit_index < width; control_qubit_index++){
		step_ops[control_qubit_index+reg] = "Control1(1)";
		step_ops[over] = "Target1(R'" + int2str(control_qubit_index+2) + ")";

		joined_step = concatena(step_ops, qubits);
		rqft.push_back(joined_step);
		step_ops[control_qubit_index+reg] = "ID";
	}
	step_ops[over] = "ID";

	for (int target_qubit_index = 0; target_qubit_index < width; target_qubit_index++){
		step_ops[target_qubit_index+reg] = "H";
		joined_step = concatena(step_ops, qubits);
		rqft.push_back(joined_step);

		for (int control_qubit_index = target_qubit_index+1; control_qubit_index < width; control_qubit_index++){
			step_ops[control_qubit_index+reg] = "Control1(1)";
			step_ops[target_qubit_index+reg] = "Target1(R'" + int2str(control_qubit_index-target_qubit_index+1) + ")";

			joined_step = concatena(step_ops, qubits);
			rqft.push_back(joined_step);

			step_ops[control_qubit_index+reg] = "ID";
		}
		step_ops[target_qubit_index+reg] = "ID";
	}
	reverse(rqft.begin(), rqft.end());

	return rqft;
}

vector <string> CSwapR(int qubits, int ctrl, int reg1, int reg2, int width){
	vector <string> swap_steps;
	vector <string>	step_ops (qubits, "ID");
	string joined_step1, joined_step2;

	for (int qubit_index = 0; qubit_index < width; qubit_index++){
		step_ops[ctrl] = "Control1(1)";
		step_ops[qubit_index+reg1] = "Target1(X)";
		step_ops[qubit_index+reg2] = "Control1(1)";
		joined_step1 = concatena(step_ops, qubits);

		step_ops[ctrl] = "ID";
		step_ops[qubit_index+reg1] = "Control1(1)";
		step_ops[qubit_index+reg2] = "Target1(X)";
		joined_step2 = concatena(step_ops, qubits);

		step_ops[qubit_index+reg1] = step_ops[qubit_index+reg2] = "ID";

		swap_steps.push_back(joined_step2);
		swap_steps.push_back(joined_step1);
		swap_steps.push_back(joined_step2);
	}

	return swap_steps;
}

vector <string> SwapOver(int qubits, int reg, int width){
	vector <string> swap_steps;

	for(int qubit_index=0; qubit_index<width/2; qubit_index++){
		swap_steps.push_back(CNot(qubits, reg+width-qubit_index-1, reg+qubit_index));
		swap_steps.push_back(CNot(qubits, reg+qubit_index, reg+width-qubit_index-1));
		swap_steps.push_back(CNot(qubits, reg+width-qubit_index-1, reg+qubit_index));
    }

	return swap_steps;
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
		base_pow_mod = modular_pow(base_value, pow(2,round_index), number_to_factor);
		base_inv_pow_mod = modular_pow(base_inverse, pow(2,round_index), number_to_factor);

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
