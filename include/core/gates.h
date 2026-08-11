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

// Cada uma monta um "step" (string com um token por qubit — ver
// docs/02-linguagem-de-circuitos.md) para a porta correspondente.
string CNot(int qubits, int ctrl, int target, int control_value = 1);
string Toffoli(int qubits, int ctrl1, int ctrl2, int target, int control_value = 3);
string Controlled1(int qubits, int ctrl, int target, string op, int control_value = 1);
string Controlled2(int qubits, int ctrl1, int ctrl2, int target, string op, int control_value = 3);
string ControlledN(int qubits, vector<int> ctrls, int target, string op, int control_value = -1);
// H/X/Z aplicado em "width" qubits seguidos a partir de "reg".
string Pauli_X(int qubits, int reg, int width = 1);
string Pauli_Z(int qubits, int reg, int width = 1);
string Hadamard(int qubits, int reg, int width = 1);

// Junta os tokens de um step (um por qubit) numa única string separada
// por vírgula — ver docs/02-linguagem-de-circuitos.md.
string concatena(vector <string> step_ops, int qubits, bool reverse = false);


// Catálogo de matrizes de porta (H, X, Y, Z, R1..R3, e as geradas
// dinamicamente por lib_shor_circuits.cpp). Cada DGM tem sua própria
// instância de Gates (campo DGM::gates) — o cache dura a vida de uma
// execução (Shor()/Grover()/etc), não do processo inteiro, evitando
// colisão de nomes entre execuções diferentes (ver
// docs/07-bugs-e-pontos-de-atencao.md, item 1) sem perder o reaproveitamento
// de matrizes dentro de uma mesma execução (a QFT e os somadores de valor
// fixo, como SubF(N) no Shor, são chamados repetidamente com o mesmo nome).
class Gates{
public:
	map <string, float complex*> list;
	Gates();
	~Gates();
	// Registra as portas base (H, X, Y, Z, R1, R2, R3) — chamado pelo
	// construtor.
	void init();
	// Devolve a matriz cadastrada sob "gate_name", ou 0 se não existir.
	float complex* getMatrix(string gate_name);
	// Registra uma matriz nova sob "name"; não sobrescreve um nome já
	// existente (devolve false nesse caso).
	bool addGate(string name, float complex* matrix);
	bool addGate(string name, float complex a0, float complex a1, float complex a2, float complex a3);
};

#endif
