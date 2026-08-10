#ifndef _GATES_H_
#define _GATES_H_

#include "common.h"
#include <vector>
#include <map>
#include <string>
#include <complex.h>
#include <math.h>

#define complex __complex__

using namespace std;

string CNot(int qubits, int ctrl, int target, int control_value = 1);
string Toffoli(int qubits, int ctrl1, int ctrl2, int target, int control_value = 3);
string Controlled1(int qubits, int ctrl, int target, string op, int control_value = 1);
string Controlled2(int qubits, int ctrl1, int ctrl2, int target, string op, int control_value = 3);
string ControlledN(int qubits, vector<int> ctrls, int target, string op, int control_value = -1);
string Pauli_X(int qubits, int reg, int width = 1);
string Pauli_Z(int qubits, int reg, int width = 1);
string Hadamard(int qubits, int reg, int width = 1);

// Junta os tokens de um step (um por qubit) numa única string separada
// por vírgula — ver docs/02-linguagem-de-circuitos.md.
string concatena(vector <string> step_ops, int qubits, bool reverse = false);


// Catálogo de matrizes de porta (H, X, Y, Z, R1..R3, e as geradas
// dinamicamente por lib_shor.cpp). Gates::list é estático: compartilhado
// por toda instância de Gates no processo.
class Gates{
public:
	static map <string, float complex*> list;
	Gates();
	~Gates();
	void init();
	float complex* getMatrix(string gate_name);
	bool addGate(string name, float complex* matrix);
	bool addGate(string name, float complex a0, float complex a1, float complex a2, float complex a3);
};

#endif
