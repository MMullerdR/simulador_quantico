#include <iostream>
#include "../../include/core/dgm.h"
#include <omp.h>
#include <unistd.h>
#include <cstdio>
#include <iterator>

void Tokenize(const string& text, vector<string>& tokens, const string& delimiters = ",")
{
	// Skip delimiters at beginning.
	string::size_type lastPos = text.find_first_not_of(delimiters, 0);
	// Find first "non-delimiter".
	string::size_type pos = text.find_first_of(delimiters, lastPos);

	while (string::npos != pos || string::npos != lastPos)
	{
		// Found a token, add it to the vector.
		tokens.push_back(text.substr(lastPos, pos - lastPos));
		// Skip delimiters.  Note the "not_of"
		lastPos = text.find_first_not_of(delimiters, pos);
		// Find next "non-delimiter"
		pos = text.find_first_of(delimiters, lastPos);
	}
}

void DGM::setFunction(string function, int iterations, bool reset){
	vector <string> steps;

	Tokenize(function, steps, ";");

	setFunction(steps, iterations, reset);
}

void DGM::setFunction(vector <string> steps, int iterations, bool reset){
	if (reset) erase();
	else vec_pts.pop_back();


	vector <PT*> step_pts, vec_tmp;
	map<long, Group> groups;

	for (long iteration_index = 0; iteration_index < iterations; iteration_index++)
	for (long step_index = 0; step_index < steps.size(); step_index++){
		groups = genGroups(steps[step_index]);
		genPTs(groups, step_pts);

		if (step_index%2)
			sort(step_pts.begin(), step_pts.end(), increasing);
		else
			sort(step_pts.begin(), step_pts.end(), decreasing);

		vec_pts.insert(vec_pts.end(), step_pts.begin(), step_pts.end());
	}

	vec_pts.push_back(NULL);

	pts = &vec_pts[0];
}

map <long, Group> DGM::genGroups(string step){
	vector <string> ops;
	Tokenize(step, ops); //separa os operadores usando "," como delimitador
	qubits = ops.size();

	size_t control_keyword_pos, target_keyword_pos, paren_pos;
	string token;
	long qubit_pos, ctrl_value, ctrl_num;

	map<long, Group> groups;

	char * pEnd;
	qubit_pos = 0;
	vector<string>::iterator it;
	for (it = ops.begin() ; it != ops.end(); ++it){ //percorre os operadores
		token = *it;
		//cout << token << endl;
		control_keyword_pos = token.find("Control"); //tamanho 7
		target_keyword_pos = token.find("Target");  //tamanho 6
		paren_pos = token.find("(") + 1;

		if (control_keyword_pos != string::npos){ //Controle
			ctrl_num = strtol(token.c_str()+7, &pEnd, 10);
			ctrl_value = strtol(token.c_str()+paren_pos, &pEnd, 10);

			groups[ctrl_num].ctrl.push_back(ctrl_value); //adicona o valor do controle
			groups[ctrl_num].pos_ctrl.push_back(qubit_pos);  //e a sua posição ao map relacionado ao controle
		}
		else if(target_keyword_pos != string::npos){ //Target
			ctrl_num = strtol(token.c_str()+6, &pEnd, 10);
			token = token.substr(paren_pos, token.size()-paren_pos-1);

			groups[ctrl_num].ops.push_back(token);     //adicona o operador
			groups[ctrl_num].pos_ops.push_back(qubit_pos); //e a sua posição ao map relacionado ao target
		}
		else{ //operador normal
			if (token != "ID"){ //se for ID ignora
				groups[0].ops.push_back(token);     //adiciona o operador
				groups[0].pos_ops.push_back(qubit_pos); //e a sua posição ao map '0'
			}
		}
		qubit_pos++;
	}

	return groups;
}

void DGM::genPTs(map<long, Group> &groups, vector <PT*> &step_pts){
	step_pts.clear();
	Gates gates;

	map<long,Group>::iterator it;
	Group group;
	PT* term;
	long group_control_mask, group_control_value, group_control_count;
	long op_count;

	for (it = groups.begin(); it != groups.end(); ++it){ //percorre os grupos
		group = it->second;
		op_count = group.ops.size();

		group_control_count = group.ctrl.size();
		group_control_value = group_control_mask = 0;

		for (long ctrl_index = 0; ctrl_index < group_control_count; ctrl_index++){ //gera a mascara e o valor do controle (em binario)
			group.pos_ctrl[ctrl_index] =  qubits - group.pos_ctrl[ctrl_index] - 1;
			group_control_mask += (1 << group.pos_ctrl[ctrl_index]);
			if (group.ctrl[ctrl_index]) group_control_value += (1 << group.pos_ctrl[ctrl_index]);
		}

		for (int op_index = 0; op_index < op_count; op_index++){

			term = (PT*) malloc(sizeof(PT));
			// malloc() puro não chama PT::PT(), então control_bit_positions/
			// control_rest não nascem NULL sozinhos (ficam com lixo de
			// memória) — precisam ser inicializados aqui explicitamente,
			// não só dentro do "if (group_control_count)" abaixo. Sem isso,
			// PT::destructor() acaba chamando free() num ponteiro de lixo
			// para toda porta sem controle (a maioria dos circuitos).
			term->control_bit_positions = NULL;
			term->control_rest = NULL;
			term->control_rest_count = 0;

			term->affected = false;

			term->qubits = 1;
			term->span_start_bit = qubits - group.pos_ops[op_index];
			term->target_bit = term->span_start_bit - 1;
			term->matrix_size = 2;

			term->matrix = gates.getMatrix(group.ops[op_index]);

			term->control_value = group_control_value;
			term->control_mask = group_control_mask;
			term->control_count = group_control_count;

			if (group_control_count){
				term->control_bit_positions = (long*)malloc(sizeof(long) * group_control_count);
				copy(group.pos_ctrl.begin(), group.pos_ctrl.end(), term->control_bit_positions);
			}

			step_pts.push_back(term);
		}
	}
}

void DGM::genMatrix(float complex* matrix, vector<float complex*> &matrices, long qubit_count, long current_qubit, long line, long column, float complex value){
	if (value == 0.0) return;

	if (current_qubit == qubit_count){ //percorreu até a ultima matriz
		matrix[line*(1<<qubit_count) + column] = value;
		return;
	}

	for (long row_bit = 0; row_bit < 2; row_bit++)
		for (long col_bit = 0; col_bit < 2; col_bit++)
			genMatrix(matrix, matrices, qubit_count, current_qubit+1, (line<<1)|row_bit, (column<<1)|col_bit, value * matrices[current_qubit][row_bit*2+col_bit]);
}
