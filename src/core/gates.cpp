#include "../../include/gates.h"
#include <iostream>
#include <cmath>

using namespace std;

map <string, float complex*> Gates::list;

Gates::Gates(){
	init();
}

Gates::~Gates(){}

void Gates::init(){
	if (Gates::list.size() == 0){
		addGate("H", 1.0/sqrt(2), 1.0/sqrt(2), 1.0/sqrt(2), -1.0/sqrt(2));
		addGate("X", 0.0, 1.0, 1.0, 0.0);
		addGate("Z", 1.0, 0.0, 0.0, -1.0);
		addGate("Y", 0.0, 1.0*I, -1.0*I, 0.0);
		addGate("R1", 1.0, 0, 0.0, cpowf(M_E, M_PI*I));
		addGate("R2", 1.0, 0, 0.0, cpowf(M_E, M_PI*I/2.0));
		addGate("R3", 1.0, 0, 0.0, cpowf(M_E, M_PI*I/4.0));
	}
}

float complex* Gates::getMatrix(string gate_name){
	if (Gates::list.find(gate_name) == Gates::list.end()) return 0;

	return Gates::list[gate_name];
}

bool Gates::addGate(string name, float complex* matrix){
	if (Gates::list.find(name) != Gates::list.end()) return false;

	Gates::list[name] = matrix;
	return true;
}

bool Gates::addGate(string name, float complex a0, float complex a1, float complex a2, float complex a3){
//        cout << "GATES" << endl;

	if (Gates::list.find(name) != Gates::list.end()) return false;

//        cout << "GATES" << endl;

	float complex* matrix = new float complex[4];
	matrix[0] = a0;
	matrix[1] = a1;
	matrix[2] = a2;
	matrix[3] = a3;

	/*
	cout << "Gate Adicionado " << name << endl;
	cout << "Matrix" << endl;
	cout << crealf(matrix[0]) << " , " << cimagf(matrix[0]) << "\t" << crealf(matrix[1]) << " , " << cimagf(matrix[1]) << endl;
	cout << crealf(matrix[2]) << " , " << cimagf(matrix[2]) << "\t" << crealf(matrix[3]) << " , " << cimagf(matrix[3]) << endl << endl;
	*/

	Gates::list[name] = matrix;
	return true;
}

/////////////////////////////////////////////////////////////////////
// Cada função abaixo monta um "step_ops": um vetor com um token por
// qubit ("ID"/"Control1(v)"/"Target1(porta)") que descreve uma camada
// do circuito, e devolve isso já concatenado numa string (ver
// docs/02-linguagem-de-circuitos.md).
/////////////////////////////////////////////////////////////////////

string CNot(int qubits, int ctrl, int target, int control_value){
	vector <string> step_ops (qubits, "ID");
	step_ops[ctrl] = "Control1(0)";
	if (control_value) step_ops[ctrl] = "Control1(1)";
	step_ops[target] = "Target1(X)";

	return concatena(step_ops, qubits);
}

string Toffoli(int qubits, int ctrl1, int ctrl2, int target, int control_value){
	vector <string> step_ops (qubits, "ID");
	step_ops[ctrl1] = "Control1(0)";
	if (control_value>>1) step_ops[ctrl1] = "Control1(1)";
	step_ops[ctrl2] = "Control1(0)";
	if (control_value&1) step_ops[ctrl2] = "Control1(1)";
	step_ops[target] = "Target1(X)";

	return concatena(step_ops, qubits);
}

string Controlled1(int qubits, int ctrl, int target, string op, int control_value){
	vector <string> step_ops (qubits, "ID");
	step_ops[ctrl] = "Control1(0)";
	if (control_value) step_ops[ctrl] = "Control1(1)";
	step_ops[target] = "Target1(" + op + ")";

	return concatena(step_ops, qubits);
}


string Controlled2(int qubits, int ctrl1, int ctrl2, int target, string op, int control_value){
	vector <string> step_ops (qubits, "ID");
	step_ops[ctrl1] = "Control1(0)";
	if (control_value&2) step_ops[ctrl1] = "Control1(1)";
	step_ops[ctrl2] = "Control1(0)";
	if (control_value&1) step_ops[ctrl2] = "Control1(1)";
	step_ops[target] = "Target1(" + op + ")";

	return concatena(step_ops, qubits);
}

string ControlledN(int qubits, vector <int> ctrls, int target, string op, int control_value){
	if (control_value == -1) control_value = pow(2,ctrls.size()) - 1;
	vector <string> step_ops (qubits, "ID");

	for (int ctrl_index = ctrls.size() - 1; ctrl_index >=0; ctrl_index--){
		step_ops[ctrls[ctrl_index]] = "Control1(0)";
		if (control_value & 1) step_ops[ctrls[ctrl_index]] = "Control1(1)";
		control_value = control_value >> 1;
	}

	step_ops[target] = "Target1(" + op + ")";

	return concatena(step_ops, qubits);
}


string Pauli_X(int qubits, int reg, int width){
	vector <string> step_ops (qubits, "ID");
	for (int qubit_index = 0; qubit_index < width; qubit_index++) step_ops[qubit_index+reg] = "X";

	return concatena(step_ops, qubits);
}

string Pauli_Z(int qubits, int reg, int width){
	vector <string> step_ops (qubits, "ID");
	for (int qubit_index = 0; qubit_index < width; qubit_index++) step_ops[qubit_index+reg] = "Z";

	return concatena(step_ops, qubits);
}


string Hadamard(int qubits, int reg, int width){
	vector <string> step_ops (qubits, "ID");
	for (int qubit_index = 0; qubit_index < width; qubit_index++) step_ops[qubit_index+reg] = "H";

	return concatena(step_ops, qubits);
}


string concatena(vector <string> step_ops, int qubits, bool reverse){
	string joined_step;
	if (!reverse){
		joined_step = step_ops[0];
		for (int qubit_index = 1; qubit_index < qubits; qubit_index++)
			joined_step += "," + step_ops[qubit_index];
	}
	else{
		joined_step = step_ops[qubits-1];
		for (int qubit_index = qubits - 2; qubit_index >= 0; qubit_index--)
			joined_step += "," + step_ops[qubit_index];
	}

	return joined_step;
}
