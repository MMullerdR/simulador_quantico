// Teste de execução (não só de string): monta um circuito de 1 porta
// controlada (CNot/Toffoli/ControlledN), roda via DGM::execute() (t_CPU,
// determinístico — sem medição probabilística envolvida) a partir de
// cada estado da base computacional, e confere a tabela-verdade da porta
// contra a amplitude final. Cobertura que test_qft_addf.cpp não tem:
// aquele só compara strings de circuito contra uma cópia congelada da
// implementação antiga, nunca chega a executar nada de verdade via DGM.
//
// Convenção de bit: qubit 0 é o bit mais significativo do índice do
// vetor de estado (ver DGM::measure, dgm_core.cpp) — mesma convenção
// usada aqui pra calcular o índice esperado de cada caso.
#include "../include/core/dgm.h"
#include "../include/core/gates.h"
#include <iostream>
#include <cmath>
using namespace std;

static bool run_case(int qubits, string circuit, int start_index, int expected_index, const string &label){
	DGM dgm;
	dgm.qubits = qubits;
	dgm.allocateMemory();
	dgm.setMemoryValue(start_index);
	dgm.executeFunction(circuit);

	bool ok = true;
	for (long index = 0; index < (1L << qubits); index++){
		float complex expected = (index == expected_index) ? 1.0 : 0.0;
		float complex got = dgm.state[index];
		if (fabs(crealf(got) - crealf(expected)) > 0.0001 || fabs(cimagf(got)) > 0.0001){
			cout << "FAIL " << label << " (start=" << start_index << "): esperado amplitude 1 no índice "
				<< expected_index << ", achou " << crealf(got) << "+" << cimagf(got) << "i no índice " << index << endl;
			ok = false;
		}
	}
	dgm.freeMemory();
	return ok;
}

int main(){
	bool all_ok = true;
	int case_count = 0;

	// CNot(qubits=2, ctrl=0, target=1, control_value=1): AND com 1
	// controle -- flip do target só quando ctrl==1.
	{
		string circuit = CNot(2, 0, 1, 1);
		int expected[4] = {0, 1, 3, 2}; // ctrl=0: intacto; ctrl=1: flip do bit alvo
		for (int start = 0; start < 4; start++){
			all_ok = run_case(2, circuit, start, expected[start], "CNot") && all_ok;
			case_count++;
		}
	}

	// Toffoli(qubits=3, ctrl1=0, ctrl2=1, target=2, control_value=3): AND
	// com 2 controles -- flip só quando os dois == 1.
	{
		string circuit = Toffoli(3, 0, 1, 2, 3);
		int expected[8] = {0, 1, 2, 3, 4, 5, 7, 6}; // só 110/111 trocam
		for (int start = 0; start < 8; start++){
			all_ok = run_case(3, circuit, start, expected[start], "Toffoli") && all_ok;
			case_count++;
		}
	}

	// ControlledN(qubits=4, ctrls={0,1,2}, target=3, control_value=-1):
	// generalização do Toffoli pra 3 controles -- control_value=-1 marca
	// "todos precisam ser 1" (ver gates.cpp).
	{
		vector<int> ctrls = {0, 1, 2};
		string circuit = ControlledN(4, ctrls, 3, "X", -1);
		for (int start = 0; start < 16; start++){
			int expected = ((start >> 1) == 7) ? (start ^ 1) : start; // bits 3..1 (ctrls) == 111 -> flip bit 0 (target)
			all_ok = run_case(4, circuit, start, expected, "ControlledN") && all_ok;
			case_count++;
		}
	}

	// Controlled1/Controlled2 (usadas pelo Shor via genRot/CMultMod etc.)
	// com control_value=0: controle "invertido" -- dispara quando o
	// qubit de controle é 0, não 1. Confere que o token "Control1(0)"
	// (gerado quando control_value é falso) funciona tanto quanto
	// "Control1(1)".
	{
		string circuit = Controlled1(2, 0, 1, "X", 0);
		int expected[4] = {1, 0, 2, 3}; // ctrl==0 (start 0,1) -> flip; ctrl==1 (start 2,3) -> intacto
		for (int start = 0; start < 4; start++){
			all_ok = run_case(2, circuit, start, expected[start], "Controlled1(ctrl=0)") && all_ok;
			case_count++;
		}
	}

	cout << (all_ok ? "OK" : "FAIL") << " test_gates: " << case_count << " casos executados" << endl;
	return all_ok ? 0 : 1;
}
