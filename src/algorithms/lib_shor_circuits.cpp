#include "../../include/algorithms/lib_shor.h"
#include "../../include/core/dgm.h"
#include "../../include/core/common.h"
#include "../../include/core/gates.h"
#include <cmath>
#include <iostream>
#include <algorithm>

using namespace std;

string genRot(int qubits, int reg, long phase_bits, Gates &g){
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
		name = "Rot_" + int2str(phase_bits_orig);
		g.addGate(name, 1.0, 0.0, 0.0, rot);
		step_ops[reg] = name;

		return concatena(step_ops, qubits);
	}

	return "";
}

vector<string> CMultMod(int qubits, int ctrl, int reg1, int reg2, int over, int over_bool, int width, long base_value, long number_to_factor, Gates &g){
	int ctrl2;
	vector <string> qft = QFT(qubits, reg2, over, width, g);

	string hadamard_step = Hadamard(qubits, reg2, width);

	vector <string> rqft = RQFT(qubits, reg2, over, width, g);

	//////////////////////////////////////////////////////////////

	vector <string> mult_mod;
	vector <string> add_mod_steps;
	mult_mod.push_back(Hadamard(qubits, over, 1));
	mult_mod.push_back(hadamard_step);

	ctrl2 = reg1 + width - 1;
	for (int bit_index = 0; bit_index < width; bit_index++){
		add_mod_steps = C2AddMod(qubits, ctrl, ctrl2-bit_index, reg2, over, over_bool, width, base_value, number_to_factor, g);
		mult_mod.insert(mult_mod.end(), add_mod_steps.begin(), add_mod_steps.end());

		base_value = (base_value*2)%number_to_factor;
	}

	mult_mod.insert(mult_mod.end(), rqft.begin(), rqft.end());

	return mult_mod;

}

vector<string> CRMultMod(int qubits, int ctrl, int reg1, int reg2, int over, int over_bool, int width, long base_value, long number_to_factor, Gates &g){
	int ctrl2;
	vector <string> qft = QFT(qubits, reg2, over, width, g);
	vector <string> rqft = RQFT(qubits, reg2, over, width, g);

	//////////////////////////////////////////////////////////////

	vector <string> mult_mod;
	vector <string> sub_mod_steps;

	ctrl2 = reg1 + width - 1;
	for (int bit_index = 0; bit_index < width; bit_index++){
		sub_mod_steps = C2SubMod(qubits, ctrl, ctrl2-bit_index, reg2, over, over_bool, width, base_value, number_to_factor, g);
		mult_mod.insert(mult_mod.begin(), sub_mod_steps.begin(), sub_mod_steps.end());

		base_value = (base_value*2)%number_to_factor;
	}

	mult_mod.insert(mult_mod.begin(), qft.begin(), qft.end());
	mult_mod.insert(mult_mod.end(), rqft.begin(), rqft.end());

	return mult_mod;
}

vector <string> C2AddMod(int qubits, int ctrl1, int ctrl2, int reg, int over, int over_bool, int width, long base_value, long number_to_factor, Gates &g){
	vector<string> qft = QFT(qubits, reg, over, width, g);
	vector<string> rqft = RQFT(qubits, reg, over, width, g);

	string c2_add_a = C2AddF(qubits, ctrl1, ctrl2, reg, over, base_value, width, g);
	string c2_sub_a = C2SubF(qubits, ctrl1, ctrl2, reg, over, base_value, width, g);

	string sub_N = SubF(qubits, reg, over, number_to_factor, width, g);
	string c_add_N = CAddF(qubits, over_bool, reg, over, number_to_factor, width, g);

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

vector <string> C2SubMod(int qubits, int ctrl1, int ctrl2, int reg, int over, int over_bool, int width, long base_value, long number_to_factor, Gates &g){
	vector <string> qft = QFT(qubits, reg, over, width, g);
	vector <string> rqft = RQFT(qubits, reg, over, width, g);

	string c2_add_a = C2AddF(qubits, ctrl1, ctrl2, reg, over, base_value, width, g);
	string c2_sub_a = C2SubF(qubits, ctrl1, ctrl2, reg, over, base_value, width, g);

	string add_N = AddF(qubits, reg, over, number_to_factor, width, g);
	string c_add_N = CAddF(qubits, over_bool, reg, over, number_to_factor, width, g);
	string c_sub_N = CSubF(qubits, over_bool, reg, over, number_to_factor, width, g);

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


string CAddF(int qubits, int ctrl1, int reg, int over, long value_to_add, int width, Gates &g){
	vector <string> step_ops = AddF(qubits, reg, over, value_to_add, width, true, g);

	step_ops[ctrl1] = "Control1(1)";

	return concatena(step_ops, qubits);
}


string C2AddF(int qubits, int ctrl1, int ctrl2, int reg, int over, long value_to_add, int width, Gates &g){
	vector <string> step_ops = AddF(qubits, reg, over, value_to_add, width, true, g);

	step_ops[ctrl1] = "Control1(1)";
	step_ops[ctrl2] = "Control1(1)";

	return concatena(step_ops, qubits);
}

// AddF e SubF são espelhos exatos (mesma lógica de decompor 'value' em
// rotações de fase), diferindo só no sinal da fase e no prefixo do nome
// da porta ("ADD_" vs "SUB_" — precisa ser diferente pelo mesmo motivo
// do QFT/RQFT acima: nomes são chaves no cache de portas da execução).
vector <string> AddSubF_impl(int qubits, int reg, int over, long value, int width, bool controlled, bool subtract, Gates &g){
	int digit_count = width+1;
	vector <float complex> rot (digit_count, 1);
	float complex  identity;

	long remaining_value = value;

	float complex euler_e = M_E;
	float phase_sign = subtract ? -1.0 : 1.0;

	for (int digit_index = 0; digit_index < digit_count; digit_index++){
		if (remaining_value&1)
			for (int higher_digit_index = digit_index; higher_digit_index < digit_count; higher_digit_index++)
				rot[higher_digit_index] *= cpowf(euler_e, phase_sign*2*M_PI*I/pow(2.0, higher_digit_index-digit_index+1));
		remaining_value = remaining_value >> 1;
	}

	vector<string> step_ops(qubits, "ID");
	string name;
	string prefix = subtract ? "SUB_" : "ADD_";

	int msb_pos = reg+width-1;
	identity = 1;
	for (int digit_index = 0; digit_index < digit_count; digit_index++){
		if (rot[digit_index] != identity){
			name = prefix + int2str(value) + "_" + int2str(digit_index);
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

string AddF(int qubits, int reg, int over, long value_to_add, int width, Gates &g){
	return concatena(AddF(qubits, reg, over, value_to_add, width, false, g), qubits);
}

vector <string> AddF(int qubits, int reg, int over, long value_to_add, int width, bool controlled, Gates &g){
	return AddSubF_impl(qubits, reg, over, value_to_add, width, controlled, false, g);
}

string CSubF(int qubits, int ctrl1, int reg, int over, long value_to_sub, int width, Gates &g){
	vector <string> step_ops = SubF(qubits, reg, over, value_to_sub, width, true, g);

	step_ops[ctrl1] = "Control1(1)";

	return concatena(step_ops, qubits);
}

string C2SubF(int qubits, int ctrl1, int ctrl2, int reg, int over, long value_to_sub, int width, Gates &g){
	vector <string> step_ops = SubF(qubits, reg, over, value_to_sub, width, true, g);

	step_ops[ctrl1] = "Control1(1)";
	step_ops[ctrl2] = "Control1(1)";

	return concatena(step_ops, qubits);
}

string SubF(int qubits, int reg, int over, long value_to_sub, int width, Gates &g){
	return concatena(SubF(qubits, reg, over, value_to_sub, width, false, g), qubits);
}

vector <string> SubF(int qubits, int reg, int over, long value_to_sub, int width, bool controlled, Gates &g){
	return AddSubF_impl(qubits, reg, over, value_to_sub, width, controlled, true, g);
}


// QFT e RQFT são espelhos exatos um do outro (mesma estrutura de dois
// laços de rotações controladas), diferindo só no sinal da fase, no
// prefixo do nome da porta ("R" vs "R'" — precisa ser diferente porque
// os nomes viram chaves no cache de portas da execução, e addGate não
// sobrescreve, ver docs/07 item 1) e no reverse() final do inverso.
vector <string> QFT_impl(int qubits, int reg, int over, int width, bool inverse, Gates &g){
	string joined_step, name;
	vector <string> steps;
	string prefix = inverse ? "R'" : "R";
	float phase_sign = inverse ? -1.0 : 1.0;

	float complex euler_e = M_E;
	float complex rotation_value;
	for (int level = 1; level <= width+1; level++){
		name = prefix + int2str(level);
		rotation_value = cpowf(euler_e, phase_sign*2*M_PI*I/pow(2.0, level));
		g.addGate(name, 1.0, 0.0, 0.0, rotation_value);
	}

	vector <string> step_ops (qubits, "ID");

	steps.push_back(Hadamard(qubits, over, 1));
	for (int control_qubit_index = 0; control_qubit_index < width; control_qubit_index++){
		step_ops[control_qubit_index+reg] = "Control1(1)";
		step_ops[over] = "Target1(" + prefix + int2str(control_qubit_index+2) + ")";

		joined_step = concatena(step_ops, qubits);
		steps.push_back(joined_step);
		step_ops[control_qubit_index+reg] = "ID";
	}
	step_ops[over] = "ID";

	for (int target_qubit_index = 0; target_qubit_index < width; target_qubit_index++){
		steps.push_back(Hadamard(qubits, target_qubit_index+reg, 1));

		for (int control_qubit_index = target_qubit_index+1; control_qubit_index < width; control_qubit_index++){
			step_ops[control_qubit_index+reg] = "Control1(1)";
			step_ops[target_qubit_index+reg] = "Target1(" + prefix + int2str(control_qubit_index-target_qubit_index+1) + ")";

			joined_step = concatena(step_ops, qubits);
			steps.push_back(joined_step);

			step_ops[control_qubit_index+reg] = "ID";
		}
		step_ops[target_qubit_index+reg] = "ID";
	}

	if (inverse) reverse(steps.begin(), steps.end());

	return steps;
}

vector <string> QFT(int qubits, int reg, int over, int width, Gates &g){
	return QFT_impl(qubits, reg, over, width, false, g);
}

vector <string> QFT2(int qubits, int reg, int width, Gates &g){
	string joined_step;
	vector <string> qft;

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

vector <string> RQFT(int qubits, int reg, int over, int width, Gates &g){
	return QFT_impl(qubits, reg, over, width, true, g);
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
