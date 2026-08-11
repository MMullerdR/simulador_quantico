// Teste de regressão: QFT/RQFT e AddF/SubF compartilham uma implementação
// interna (QFT_impl/AddSubF_impl em src/algorithms/lib_shor_circuits.cpp).
// Este teste mantém uma cópia literal das versões originais, separadas
// (de antes da unificação), e compara byte a byte com as funções reais
// atuais para os mesmos parâmetros — protege contra uma futura mudança
// acidental na unificação quebrar QFT, RQFT, AddF ou SubF sem se notar.
//
// Roda com "make test" ou compilando/linkando manualmente contra
// gates.o e lib_shor_number_theory.o (ver alvo "test" no makefile).
#include "../include/algorithms/lib_shor.h"
#include "../include/core/gates.h"
#include <vector>
#include <string>
#include <iostream>
#include <cmath>
#include <algorithm>
using namespace std;

static vector<string> OLD_QFT(int qubits, int reg, int over, int width){
	string joined_step, name;
	vector <string> qft;
	Gates g;
	float complex rotation_value;
	for (int level = 1; level <= width+1; level++){
		name = "R" + int2str(level);
		rotation_value = M_E;
		rotation_value = cpowf(rotation_value, 2*M_PI*I/pow(2.0, level));
		g.addGate(name, 1.0, 0.0, 0.0, rotation_value);
	}
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
	return qft;
}

static vector<string> OLD_RQFT(int qubits, int reg, int over, int width){
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

static vector <string> OLD_AddF(int qubits, int reg, int over, long value_to_add, int width, bool controlled){
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

static vector <string> OLD_SubF(int qubits, int reg, int over, long value_to_sub, int width, bool controlled){
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

static bool cmp(const vector<string>&a, const vector<string>&b, const string &label){
	if (a.size() != b.size()){
		cout << "FAIL " << label << ": tamanhos diferentes " << a.size() << " vs " << b.size() << endl;
		return false;
	}
	for (size_t i = 0; i < a.size(); i++){
		if (a[i] != b[i]){
			cout << "FAIL " << label << " idx " << i << ": '" << a[i] << "' vs '" << b[i] << "'" << endl;
			return false;
		}
	}
	return true;
}

int main(){
	bool all_ok = true;
	int case_count = 0;

	int cases[3][4] = { // qubits, reg, over, width
		{10, 1, 6, 4},
		{15, 2, 9, 6},
		{8, 0, 5, 3},
	};

	for (int i = 0; i < 3; i++){
		int qubits=cases[i][0], reg=cases[i][1], over=cases[i][2], width=cases[i][3];
		all_ok = cmp(QFT(qubits,reg,over,width), OLD_QFT(qubits,reg,over,width), "QFT") && all_ok;
		all_ok = cmp(RQFT(qubits,reg,over,width), OLD_RQFT(qubits,reg,over,width), "RQFT") && all_ok;
		case_count += 2;
	}

	long values[5] = {5, 13, 57, 0, 1023};
	int widths[3] = {4, 6, 8};
	for (int vi = 0; vi < 5; vi++){
		for (int wi = 0; wi < 3; wi++){
			long v = values[vi];
			int w = widths[wi];
			int qubits = w+3, reg=1, over=w+1;
			all_ok = cmp(AddF(qubits,reg,over,v,w,true), OLD_AddF(qubits,reg,over,v,w,true), "AddF(controlled)") && all_ok;
			all_ok = cmp(AddF(qubits,reg,over,v,w,false), OLD_AddF(qubits,reg,over,v,w,false), "AddF") && all_ok;
			all_ok = cmp(SubF(qubits,reg,over,v,w,true), OLD_SubF(qubits,reg,over,v,w,true), "SubF(controlled)") && all_ok;
			all_ok = cmp(SubF(qubits,reg,over,v,w,false), OLD_SubF(qubits,reg,over,v,w,false), "SubF") && all_ok;
			case_count += 4;
		}
	}

	cout << (all_ok ? "OK" : "FAIL") << " test_qft_addf: " << case_count << " casos comparados" << endl;
	return all_ok ? 0 : 1;
}
