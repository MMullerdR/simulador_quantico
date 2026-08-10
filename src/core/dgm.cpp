#include <iostream>
#include "../../include/dgm.h"
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

///////////////////////////////////////////////////////////////////////////////////////////////

float complex* GenericExecute(float complex *state, string function, int qubits, int type, int threads, int factor = 0){
	DGM dgm;
	dgm.exec_type = type;
	dgm.thread_count = threads;
	dgm.qubits = qubits;
	dgm.factor = factor;

	dgm.setMemory(state);

	dgm.executeFunction(function);

	state = dgm.state;

	dgm.state = NULL;

	return state;
}

float complex* GenericExecute(float complex *state, vector<string> function, int qubits, int type, int threads, int factor = 0){
	DGM dgm;
	dgm.exec_type = type;
	dgm.thread_count = threads;
	dgm.qubits = qubits;
	dgm.factor = factor;
	dgm.setMemory(state);

	dgm.executeFunction(function);

	dgm.state = NULL;

	return state;
}

///////////////////////////////////////////////////////////////////////////////////////////////

DGM::DGM(){
	max_qubits = QB_LIMIT;
	max_pt = PT_TAM;

	pts = NULL;
	state = NULL;
	print_enabled = false;
	exec_type = t_CPU;
	factor = 1;
	gpu_count = 1;
}

DGM::~DGM(){erase();}

void DGM::setExecType(int type){
	exec_type = type;
}

void DGM::printPTs(){
	for (int pt_index = 0; pt_index < vec_pts.size() -1; pt_index++){
		vec_pts[pt_index]->print();
	}
}

void DGM::erase(){
	if (!pts) return;

	long pt_index = 0;
	while (pts[pt_index] != NULL){
		pts[pt_index]->destructor();
		free(pts[pt_index]);
		pt_index++;
	}

	vec_pts.clear();
	pts = NULL;
}

void DGM::allocateMemory(){
	state = (float complex*) calloc(pow(2, qubits), sizeof(float complex));
}

void DGM::setMemory(float complex* mem){
	freeMemory();
	state = mem;
}

void DGM::freeMemory(){
	if (state) free(state);
	state = NULL;
}

void DGM::setMemoryValue(int pos){
	state[pos] = 1;
}

int DGM::measure(int qubit_pos){
	long size = pow(2.0, qubits);

	long qubit_bit_shift = (qubits - 1 - qubit_pos);

	int count_one, count_zero, sample_count;
	float zero, one, norm, random_sample;
	one = zero = 0;

	//#pragma omp for;
	for (long state_index = 0; state_index < size; state_index++){
		if ((state_index >> qubit_bit_shift) & 1)
			one += pow(crealf(state[state_index]), 2.0) + pow(cimagf(state[state_index]), 2.0);
		else
			zero += pow(crealf(state[state_index]), 2.0) + pow(cimagf(state[state_index]), 2.0);
	}

	long measured_bit;
	srand (time(NULL));
	count_one = 0;
	count_zero = 0;
	sample_count = 1;

	for (int sample_index = 0; sample_index < sample_count; sample_index++){
		random_sample = (double) rand() / RAND_MAX;
		if (zero > random_sample) count_zero++;
		else count_one++;
	}

	if (count_one > count_zero){
		measure_value = one;
		norm = sqrt(one);
		measured_bit = 1;
	}
	else{
		measure_value = zero;
		norm = sqrt(zero);
		measured_bit = 0;
	}

	long low_bits_mask;
	low_bits_mask = pow(2, qubit_bit_shift) - 1;
	#pragma omp for
	for (long state_index = 0; state_index < size/2; state_index++){
		long pos = (state_index << 1) - (state_index&low_bits_mask);
		state[pos] = state[pos | (measured_bit << qubit_bit_shift)]/norm;
		state[pos | (1<<qubit_bit_shift)] = 0.0;
	}

	return measured_bit;
}

void DGM::colapse(int qubit_pos, int value){
	long size = pow(2.0, qubits);
	long qubit_bit_shift = (qubits - 1 - qubit_pos);

	float probability_sum;
	probability_sum = 0;

	for (long state_index = 0; state_index < size; state_index++)
		if (((state_index >> qubit_bit_shift)&1) == value) probability_sum += pow(crealf(state[state_index]), 2.0) + pow(cimagf(state[state_index]), 2.0);

	cout << probability_sum << endl;

	probability_sum = sqrt(probability_sum);
	for (long state_index = 0; state_index < size; state_index++){
		if (((state_index >> qubit_bit_shift)&1) == value) state[state_index] = state[state_index]/probability_sum;
		else state[state_index] = 0.0;
	}
}

map <long, float> DGM::measure(vector<int> qubit_positions){
	long qubit_positions_mask = 0;

	for (int i = 0; i < qubit_positions.size(); i++) qubit_positions_mask = qubit_positions_mask | (1<<(qubits - 1 - qubit_positions[i]));

	map <long, float> probabilities;

	long size = pow(2.0, qubits);

	for (long state_index = 0; state_index < size; state_index++) probabilities[state_index&qubit_positions_mask] += pow(crealf(state[state_index]), 2.0) + pow(cimagf(state[state_index]), 2.0);

	return probabilities;
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


void DGM::executeFunction(vector <string> function, int iterations){
	setFunction(function);
	execute(iterations);
}

void DGM::executeFunction(string function, int iterations){
	if (function == "") return;

	setFunction(function);
	execute(iterations);
}


float complex* DGM::execute(int iterations){
	float complex* result = state;

	switch (exec_type){
		case t_CPU:
			CpuExecution1(iterations);
			break;
		case t_PAR_CPU:
			PCpuExecution1(state, pts, qubits, thread_count, cpu_coalesced_bits, cpu_region_bits, iterations);
			break;
		case t_GPU:
			result = GpuExecutionWrapper(state, pts, qubits, gpu_coalesced_bits, gpu_region_bits, gpu_count, block_size, repeat_count, iterations);
			break;
		case t_HYBRID:
			HybridExecution(pts);
			break;
		default:
			cout << "Erro exec type" << endl;
			exit(1);
	}

	return result;
}


void DGM::CountOps(int iterations){
	dense = main_diag = sec_diag = c_dense = c_main_diag = c_sec_diag = 0;

	for (int pt_index = 0; pts[pt_index]!=NULL; pt_index++){
		long matrix_type = pts[pt_index]->matrixType();
		switch (matrix_type){
			case DENSE:
				(pts[pt_index]->control_mask) ? c_dense++ : dense++;
				break;
			case DIAG_PRI:
				(pts[pt_index]->control_mask) ? c_main_diag++ : main_diag++;
				break;
			case DIAG_SEC:
				(pts[pt_index]->control_mask) ? c_sec_diag++ : sec_diag++;
				break;
			default:
				cout << "Error on operator type" << endl;
				exit(1);
		}
	}

	dense *= iterations;
	c_dense *= iterations;
	main_diag *= iterations;
	c_main_diag *= iterations;
	sec_diag *= iterations;
	c_sec_diag *= iterations;

	total_op = dense + c_dense + main_diag + c_main_diag + sec_diag + c_sec_diag;
}

void DGM::CpuExecution1(int iterations){
	long mem_size = pow(2.0, qubits);

	for (int iteration_index = 0; iteration_index < iterations; iteration_index++){
		long pt_index = 0;
		while (pts[pt_index] != NULL){
			long matrix_type = pts[pt_index]->matrixType();

			switch (matrix_type){
				case DENSE:
					CpuExecution1_1(pts[pt_index], mem_size);
					break;
				case DIAG_PRI:
					CpuExecution1_2(pts[pt_index], mem_size);
					break;
				case DIAG_SEC:
					CpuExecution1_3(pts[pt_index], mem_size);
					break;
				default:
					exit(1);
			}
			pt_index++;
		}
	}
}

void DGM::CpuExecution1_1(PT *term, long mem_size){ //Denso
	long pos0, pos1, target_bit_mask;

	target_bit_mask = 1 << term->target_bit;

	float complex tmp;

	if (!term->control_count){ 			//operador não controlado
		mem_size /= 2;
		for (long pos = 0; pos < mem_size; pos++){
			pos0 = (pos * 2) - (pos & (target_bit_mask-1));
			pos1 = pos0 | target_bit_mask;

			tmp = term->matrix[0] * state[pos0] + term->matrix[1] * state[pos1];
			state[pos1] = term->matrix[2] * state[pos0] + term->matrix[3] * state[pos1];
			state[pos0] = tmp;
		}
	}
	else{					//operador controlado
		long mask = ~(term->control_mask | target_bit_mask);
		long inc = (~mask) + 1;

		for (long pos = 0; pos < mem_size; pos = (pos+inc) & mask){
			pos0 = pos | term->control_value;
			pos1 = pos0 | target_bit_mask;

			tmp = term->matrix[0] * state[pos0] + term->matrix[1] * state[pos1];
			state[pos1] = term->matrix[2] * state[pos0] + term->matrix[3] * state[pos1];
			state[pos0] = tmp;
		}
	}
}

void DGM::CpuExecution1_2(PT *term, long mem_size){ //Diagonal Principal
	long pos0, target_bit_index = term->target_bit;

	if (!term->control_count)	//operador não controlado
		for (long pos = 0; pos < mem_size; pos++)
			state[pos] = term->matrix[((pos >> target_bit_index) & 1) * 3] * state[pos];
	else{					//operador controlado
		long mask = ~(term->control_mask);
		long inc = (~mask) + 1;

		for (long pos = 0; pos < mem_size; pos = (pos+inc) & mask){
			pos0 = pos | term->control_value;

			state[pos0] = term->matrix[((pos0 >> target_bit_index) & 1) * 3] * state[pos0];
		}
	}
}

void DGM::CpuExecution1_3(PT *term, long mem_size){ //Diagonal Secundária
	long pos0, pos1, target_bit_mask;

	target_bit_mask = 1 << term->target_bit;

	float complex tmp;

	if (!term->control_count){ 	//operador não controlado
		mem_size /= 2;
		for (long pos = 0; pos < mem_size; pos++){
			pos0 = (pos * 2) - (pos & (target_bit_mask-1));
			pos1 = pos0 | target_bit_mask;

			tmp = term->matrix[1] * state[pos1];
			state[pos1] = term->matrix[2] * state[pos0];
			state[pos0] = tmp;
		}
	}
	else{					//operador controlado
		long mask = ~(term->control_mask | target_bit_mask);
		long inc = (~mask) + 1;

		for (long pos = 0; pos < mem_size; pos = (pos+inc) & mask){
			pos0 = pos | term->control_value;
			pos1 = pos0 | target_bit_mask;

			tmp = term->matrix[1] * state[pos1];
			state[pos1] = term->matrix[2] * state[pos0];
			state[pos0] = tmp;
		}
	}
}

///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

void DGM::CpuExecution2_1(PT *term, long mem_size){ //Denso
	long pos0, pos1, target_bit_mask;

	target_bit_mask = 1 << term->target_bit;
	mem_size /= 2;

	float complex tmp;

	if (!term->control_count) 			//operador não controlado
		for (long pos = 0; pos < mem_size; pos++){
			pos0 = (pos * 2) - (pos & (target_bit_mask-1));
			pos1 = pos0 | target_bit_mask;

			tmp = term->matrix[0] * state[pos0] + term->matrix[1] * state[pos1];
			state[pos1] = term->matrix[2] * state[pos0] + term->matrix[3] * state[pos1];
			state[pos0] = tmp;
		}
	else{					//operador controlado
		for (long pos = 0; pos < mem_size; pos++){
			pos0 = (pos * 2) - (pos & (target_bit_mask-1));
			pos1 = pos0 | target_bit_mask;
			if ((pos0 & term->control_mask) == term->control_value){
				tmp = term->matrix[0] * state[pos0] + term->matrix[1] * state[pos1];
				state[pos1] = term->matrix[2] * state[pos0] + term->matrix[3] * state[pos1];
				state[pos0] = tmp;
			}
		}
		cout << endl;
	}
}

void DGM::CpuExecution2_2(PT *term, long mem_size){ //Diagonal Principal
	long target_bit_index = term->target_bit;

	if (!term->control_count)	//operador não controlado
		for (long pos = 0; pos < mem_size; pos++)
			state[pos] = term->matrix[((pos >> target_bit_index) & 1) * 3] * state[pos];
	else					//operador controlado
		for (long pos = 0; pos < mem_size; pos++)
			if ((pos & term->control_mask) == term->control_value)
				state[pos] = term->matrix[((pos >> target_bit_index) & 1) * 3] * state[pos];

}



void DGM::CpuExecution2_3(PT *term, long mem_size){ //Diagonal Secundária
	long pos0, pos1, target_bit_mask;

	target_bit_mask = 1 << term->target_bit;
	mem_size /= 2;

	float complex tmp;

	if (!term->control_count) 	//operador não controlado
		for (long pos = 0; pos < mem_size; pos++){
			pos0 = (pos * 2) - (pos & (target_bit_mask-1));
			pos1 = pos0 | target_bit_mask;

			tmp = term->matrix[1] * state[pos1];
			state[pos1] = term->matrix[2] * state[pos0];
			state[pos0] = tmp;
		}
	else					//operador controlado
		for (long pos = 0; pos < mem_size; pos++){
			pos0 = (pos * 2) - (pos & (target_bit_mask-1));
			pos1 = pos0 | target_bit_mask;
			if ((pos0 & term->control_mask) == term->control_value){
				tmp = term->matrix[1] * state[pos1];
				state[pos1] = term->matrix[2] * state[pos0];
				state[pos0] = tmp;
			}
		}

}

///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

void DGM::CpuExecution3_1(PT *term, long mem_size){ //Denso
	long pos0, pos1, target_bit_mask;

	target_bit_mask = 1 << term->target_bit;

	float complex tmp;

	if (!term->control_count){ 			//operador não controlado
		mem_size /= 2;
		for (long pos = 0; pos < mem_size; pos++){
			pos0 = (pos * 2) - (pos & (target_bit_mask-1));
			pos1 = pos0 | target_bit_mask;

			tmp = term->matrix[0] * state[pos0] + term->matrix[1] * state[pos1];
			state[pos1] = term->matrix[2] * state[pos0] + term->matrix[3] * state[pos1];
			state[pos0] = tmp;
		}
	}
	else{					//operador controlado
		vector <long> free_bit_run_length, free_bit_run_ceiling;
		long qubit_index, free_run_len, mask;

		mask = term->control_mask | target_bit_mask;

		free_run_len = 0;
		for (qubit_index = 0; qubit_index < qubits; qubit_index++){
			if (((mask >> qubit_index) & 1) == 0) free_run_len++;
			else if (free_run_len){
				free_bit_run_length.push_back(1<<(qubit_index-free_run_len));
				free_bit_run_ceiling.push_back(1<<qubit_index);
				free_run_len = 0;
			}
		}
		if (free_run_len){
			free_bit_run_length.push_back(1<<(qubit_index-free_run_len));
			free_bit_run_ceiling.push_back(1<<(qubits+1));
		}
		else{
			free_bit_run_length.push_back(1<<(qubits+1));
			free_bit_run_ceiling.push_back(1<<(qubits+2));
		}

		long pos = 0;

		while (pos < mem_size){
				pos0 = pos | term->control_value;
				pos1 = pos0 | target_bit_mask;

				//cout << pos0 <<  " " << pos1 << endl;

				tmp = term->matrix[0] * state[pos0] + term->matrix[1] * state[pos1];
				state[pos1] = term->matrix[2] * state[pos0] + term->matrix[3] * state[pos1];
				state[pos0] = tmp;

				pos += free_bit_run_length[0];
				qubit_index = 0;
				while (pos & free_bit_run_ceiling[qubit_index]){
					pos ^= free_bit_run_ceiling[qubit_index++];
					pos += free_bit_run_length[qubit_index];
				}

		}
		//cout << endl;
	}
}

void DGM::CpuExecution3_2(PT *term, long mem_size){ //Diagonal Principal
	long pos0, target_bit_index = term->target_bit;

	if (!term->control_count)	//operador não controlado
		for (long pos = 0; pos < mem_size; pos++)
			state[pos] = term->matrix[((pos >> target_bit_index) & 1) * 3] * state[pos];
	else{					//operador controlado
		vector <long> free_bit_run_length, free_bit_run_ceiling;
		long qubit_index, free_run_len, mask;

		mask = term->control_mask;

		free_run_len = 0;
		for (qubit_index = 0; qubit_index < qubits; qubit_index++){
			if (((mask >> qubit_index) & 1) == 0) free_run_len++;
			else if (free_run_len){
				free_bit_run_length.push_back(1<<(qubit_index-free_run_len));
				free_bit_run_ceiling.push_back(1<<qubit_index);
				free_run_len = 0;
			}
		}
		if (free_run_len){
			free_bit_run_length.push_back(1<<(qubit_index-free_run_len));
			free_bit_run_ceiling.push_back(1<<(qubits+1));
		}
		else{
			free_bit_run_length.push_back(1<<(qubits+1));
			free_bit_run_ceiling.push_back(1<<(qubits+2));
		}

		long pos = 0;

		while (pos < mem_size){
				pos0 = pos | term->control_value;

				//cout << pos0 << endl;
				state[pos0] = term->matrix[((pos0 >> target_bit_index) & 1) * 3] * state[pos0];

				pos += free_bit_run_length[0];
				qubit_index = 0;
				while (pos & free_bit_run_ceiling[qubit_index]){
					pos ^= free_bit_run_ceiling[qubit_index++];
					pos += free_bit_run_length[qubit_index];
				}

		}
	}
}

void DGM::CpuExecution3_3(PT *term, long mem_size){ //Diagonal Secundária
	long pos0, pos1, target_bit_mask;

	target_bit_mask = 1 << term->target_bit;


	float complex tmp;

	if (!term->control_count){ 	//operador não controlado
		mem_size /= 2;
		for (long pos = 0; pos < mem_size; pos++){
			pos0 = (pos * 2) - (pos & (target_bit_mask-1));
			pos1 = pos0 | target_bit_mask;

			tmp = term->matrix[1] * state[pos1];
			state[pos1] = term->matrix[2] * state[pos0];
			state[pos0] = tmp;
		}
	}
	else{					//operador controlado
		vector <long> free_bit_run_length, free_bit_run_ceiling;
		long qubit_index, free_run_len, mask;

		mask = term->control_mask | target_bit_mask;

		free_run_len = 0;
		for (qubit_index = 0; qubit_index < qubits; qubit_index++){
			if (((mask >> qubit_index) & 1) == 0) free_run_len++;
			else if (free_run_len){
				free_bit_run_length.push_back(1<<(qubit_index-free_run_len));
				free_bit_run_ceiling.push_back(1<<qubit_index);
				free_run_len = 0;
			}
		}
		if (free_run_len){
			free_bit_run_length.push_back(1<<(qubit_index-free_run_len));
			free_bit_run_ceiling.push_back(1<<(qubits+1));
		}
		else{
			free_bit_run_length.push_back(1<<(qubits+1));
			free_bit_run_ceiling.push_back(1<<(qubits+2));
		}

		long pos = 0;

		while (pos < mem_size){
			pos0 = pos | term->control_value;
			pos1 = pos0 | target_bit_mask;

			tmp = term->matrix[1] * state[pos1];
			state[pos1] = term->matrix[2] * state[pos0];
			state[pos0] = tmp;

			pos += free_bit_run_length[0];
			qubit_index = 0;
			while (pos & free_bit_run_ceiling[qubit_index]){
				pos ^= free_bit_run_ceiling[qubit_index++];
				pos += free_bit_run_length[qubit_index];
			}
		}
	}
}

///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////

void PCpuExecution1(float complex *state, PT **pts, int qubits, long thread_count, int coalesced_bits, int region_bits, int iterations){
	long op_index, pts_start, pts_end;
	op_index = pts_start = 0;
	while (pts[op_index] != NULL){
		long region_qubit_count = coalesced_bits;
		long region_mask = (coalesced_bits)? (1 << coalesced_bits) - 1 : 0;

		//Pega os operadores que estão dentro da região coalescida (region_mask inicial),
		//e acrescenta operadores em qubits fora dela até chegar ao limite da região (region_bits definida)
		pts_start = op_index;
		while (region_qubit_count < region_bits && pts[op_index] != NULL){					//Repete enquanto o número de qubits da região não atingir o limite (region_bits) e houver operadores
			if (//pts[op_index]->matrixType() != DIAG_PRI &&					//O qubit de operadores de diagonal principal não importa para região (sempre podem ser acrescentados)
				!((region_mask >> pts[op_index]->target_bit) & 1)){				//Se o qubit do operador estiver fora da região (region_mask), incrementa o contador de qubits da região
				region_qubit_count++;
			}

			if (region_qubit_count <= region_bits)// && pts[op_index]->matrixType() != DIAG_PRI)
				region_mask = region_mask | (1 << pts[op_index]->target_bit);			//Acrescenta o qubit do operador na região se ainda não tiver atingido o limite (region_bits)

			op_index++;
		}
		//Segue acerscentado até encontrar um operador que não esteja dentro da região
		while (pts[op_index] != NULL){
			if (((region_mask >> pts[op_index]->target_bit) & 1))// || pts[op_index]->matrixType() == DIAG_PRI)
				op_index++;
			else
				break;
		}
		pts_end = op_index;													//Executa até o operador na posiçao 'op_index' (exclusive) nesta iteração


		//Se o número de qubits na região (region_qubit_count) não tiver atingido o limite (region_bits),
		//acrescenta os ultimos qubits (final da mascara) à região até completar
		//for (long bit_mask = 1<<(qubits-1); region_qubit_count < region_bits; bit_mask = bit_mask >> 1){
		for (long bit_mask = 1; region_qubit_count < region_bits; bit_mask = bit_mask << 1){
			if (bit_mask & ~region_mask){
				region_mask = region_mask | bit_mask;
				region_qubit_count++;
			}
		}

		if (region_qubit_count < region_bits)
			region_bits = region_qubit_count;

		long region_count = (1 << (qubits - region_bits)) + 1; 				//Número de regiões 			-	 +1 para a condição de parada incluir todos
		long pos_count = 1 << (region_bits - 1); 						//Número de posições na região 	-	 -1 porque são duas posições por iteração

		omp_set_num_threads(thread_count);

		long next_region_id = 0;	//contador 'global' do número de regiões já computadas

		#pragma omp parallel
		{

			long region_id;		//indentificador local da região

			//Define a primeira região (region_id) da thread
			#pragma omp critical (teste)
			{
				region_id = next_region_id;
				next_region_id = (next_region_id + region_mask + 1) & ~region_mask;
				region_count--;
				if (region_count <= 0)
					region_id = -1;
			}

			int is_main_thread = (omp_get_thread_num()==0);


			while (region_id != -1){
				//Computa os operadores
				PCpuExecution1_0(state, pts, qubits, pts_start, pts_end, pos_count, region_id, region_mask);

				//Define a próxima região (region_id) da thread
				#pragma omp critical (teste)
				{
					region_id = next_region_id;
					next_region_id = (next_region_id + region_mask + 1) & ~region_mask;
					region_count--;
					if (region_count <= 0)
						region_id = -1;
				}
			}

		}
	}
}

void PCpuExecution1_0(float complex *state, PT **pts, int qubits, int pts_start, int pts_end, int pos_count, int region_id, int region_mask){
	PT *term;
	long pos0, pos1;
	float complex tmp;


	for (int op_index = pts_start; op_index < pts_end; op_index++){
		term = pts[op_index];
		long target_bit_mask = (1 << term->target_bit);						//mascara com a posição do qubit do operador
		long matrix_type = term->matrixType();
		//if (matrix_type == DIAG_PRI) target_bit_mask = coalesced_bits;	//se for um operador de diagonal principal, a posição do qubit não é relevante
		long pos_mask = region_mask & ~target_bit_mask;			//mascara da posição --- retira o 'target_bit_mask' da region_mask, para o 'inc pular sobre ' esse bit também
		long inc = ~pos_mask + 1;						  	//usado para calcular a proxima posição de uma região
		long pos = 0;

		if (!term->control_count){
			switch (matrix_type){
				case DENSE:
					for (long pos_index = 0; pos_index < pos_count; pos_index++){
						pos0 = pos | region_id;
						pos1 = pos0 | target_bit_mask;
						pos = (pos+inc) & pos_mask;

						tmp 		= term->matrix[2] * state[pos0] + term->matrix[3] * state[pos1];
						state[pos0] = term->matrix[0] * state[pos0] + term->matrix[1] * state[pos1];
						state[pos1] = tmp;
					}
					break;
				case DIAG_PRI:
					for (long pos_index = 0; pos_index < pos_count; pos_index++){
							pos0 = pos | region_id;
							pos1 = pos0 | target_bit_mask;
							pos = (pos+inc) & pos_mask;

							tmp			= term->matrix[3] * state[pos1];
							state[pos0] *= term->matrix[0];// * state[pos0];
							state[pos1] = tmp;// * state[pos1];tmp;
					}
					break;

				case DIAG_SEC:
					for (long pos_index = 0; pos_index < pos_count; pos_index++){
							pos0 = pos | region_id;
							pos1 = pos0 | target_bit_mask;
							pos = (pos+inc) & pos_mask;

							tmp 		= term->matrix[2] * state[pos0];
							state[pos0] = term->matrix[1] * state[pos1];
							state[pos1] = tmp;
					}
					break;
				default:
					printf("Erro de Tipo\n");
			}
		}
		//Importante: region_id é o identificador da região e corresponde ao valor dos qubits externos à região de operação (region_mask)
		else {
			if ((term->control_mask & region_id & ~region_mask) == (term->control_value & ~region_mask)){		//Verifica se a parte 'global' do controle satisfaz a região (region_id)

				// É preciso arrumar o region_mask retirando os qubits de controle que estão dentro da região e arrumar o region_id para incluir o valor dos controles
				long control_region_id = region_id | term->control_value;				//Esta operação inclui o valor dos controles locais no region_id (funciona pois os valores globais já deram match)
				long control_region_mask = region_mask;							//Valor inicial da mascara da região com controle
				long control_pos_count = pos_count;						//Número inicial de posições a serem calculadas

				for (int qubit_index = 0, bit_mask = 1; qubit_index < qubits; qubit_index++, bit_mask = bit_mask << 1){ 	//percorre os qubits
					if (bit_mask & region_mask & term->control_mask){					//se o qubit pertencer a região e for um controle:
						control_region_mask ^= bit_mask;								//	remove ele da região(region_mask) (para não iterar sobre ele)
						control_pos_count /= 2;							//	diminui a quantidade de posições que é preciso calcular.
					}
				}

				pos_mask = control_region_mask & ~target_bit_mask;						//mascara da posição --- retira o 'target_bit_mask' da region_mask, para o 'inc pular sobre' esse bit também
				inc = ~pos_mask + 1;

				switch (matrix_type){
					case DENSE:
						for (long pos_index = 0; pos_index < control_pos_count; pos_index++){
							pos0 = pos | control_region_id;
							pos1 = pos0 | target_bit_mask;
							pos = (pos+inc) & pos_mask;

							tmp 		= term->matrix[2] * state[pos0] + term->matrix[3] * state[pos1];
							state[pos0] = term->matrix[0] * state[pos0] + term->matrix[1] * state[pos1];
							state[pos1] = tmp;
						}
						break;
					case DIAG_PRI:
						for (long pos_index = 0; pos_index < control_pos_count; pos_index++){
							pos0 = pos | control_region_id;
							pos1 = pos0 | target_bit_mask;
							pos = (pos+inc) & pos_mask;

							tmp			= term->matrix[3] * state[pos1];
							state[pos0] *= term->matrix[0];
							state[pos1] = tmp;
						}
						break;

					case DIAG_SEC:
						for (long pos_index = 0; pos_index < control_pos_count; pos_index++){
							pos0 = pos | control_region_id;
							pos1 = pos0 | target_bit_mask;
							pos = (pos+inc) & pos_mask;

							tmp 		= term->matrix[2] * state[pos0];
							state[pos0] = term->matrix[1] * state[pos1];
							state[pos1] = tmp;
						}
						break;

					default:
						printf("Erro de Tipo");
				}
			}
		}
	}
}

void report_num_threads(int level){
	#pragma omp single
	{
		printf("Level %d: number of threads in the team - %d\n", level, omp_get_num_threads());
	}
}

void DGM::HybridExecution(PT **pts){
	long mem_size = pow(2.0, qubits);
	long qubits_limit = 20;
	long global_coalesced_bits = 15; //(cpu_coalesced_bits > gpu_coalesced_bits) ? cpu_coalesced_bits : gpu_coalesced_bits;

	long global_region_bits = qubits_limit;
	long global_op_start, global_op_end;

	long global_qubit_count, global_region_mask, global_region_count, global_pos_count, next_proj_id;

	omp_set_num_threads(thread_count);

	int op_index = 0;
	while (pts[op_index] != NULL){
		global_qubit_count = global_coalesced_bits;
		global_region_mask = (global_coalesced_bits)? (1 << global_coalesced_bits) - 1 : 0;

		//Realiza a projeção dos operadores de acordo com o limite de qubits que podem ser executados
		global_op_start = op_index;
		while (global_qubit_count < global_region_bits && pts[op_index] != NULL){			//Repete enquanto o número de qubits da região não atingir o limite (region) e houver operadores
			if (//pts[op_index]->matrixType() != DIAG_PRI &&					//O qubit de operadores de diagonal principal não importa para região (sempre podem ser acrescentados)
			!((global_region_mask >> pts[op_index]->target_bit) & 1)){
				global_qubit_count++;
			}

			if (global_qubit_count <= global_region_bits)// && pts[op_index]->matrixType() != DIAG_PRI)
				global_region_mask = global_region_mask | (1 << pts[op_index]->target_bit);			//Acrescenta o qubit do operador na região se ainda não tiver atingido o limite (region)

			op_index++;
		}

		while (pts[op_index] != NULL){
			if (((global_region_mask >> pts[op_index]->target_bit) & 1))// || pts[op_index]->matrixType() == DIAG_PRI)
				op_index++;
			else
				break;
		}
		global_op_end = op_index;

		//Se o número de qubits na região (global_qubit_count) nãoo tiver atingido o limite (region),
		//acrescenta os ultimos qubits (final da mascara) à região até completar
		//for (long bit_mask = 1<<(qubits-1); global_qubit_count < global_region_bits; bit_mask = bit_mask >> 1){
		for (long bit_mask = 1; global_qubit_count < global_region_bits; bit_mask = bit_mask << 1){
			if (bit_mask & ~global_region_mask){
				global_region_mask = global_region_mask | bit_mask;
				global_qubit_count++;
			}
		}

		if (global_qubit_count < global_region_bits)
			global_region_bits = global_qubit_count;

		global_region_count = (1 << (qubits - global_region_bits)) + 1; 				//Número de regiões	- +1 para a condição de parada incluir todos
		global_pos_count = 1 << (global_region_bits - 1);

		/////////////////////////////////////////////////////////////////////////////////////////////////////

		next_proj_id = 0;	//contador 'global' do número de regiões já computadas

		//Define a primeira região (region_id) da thread

		#pragma omp parallel num_threads(thread_count)
		{
			if (omp_get_thread_num()!=0){  //CPU EXECUTION
				long cpu_proj_id;

				#pragma omp critical (global_teste)
				{
					cpu_proj_id = next_proj_id;
					next_proj_id = (next_proj_id + global_region_mask + 1) & ~global_region_mask;
					global_region_count--;
					if (global_region_count <= 0)
						cpu_proj_id = -1;
				}

				while (cpu_proj_id != -1){
					long cpu_op_index, cpu_op_start, cpu_op_end;

					cpu_op_start = global_op_start;

					cpu_op_index = cpu_op_start;

					while (cpu_op_start < global_op_end){
						long cpu_qubit_count = cpu_coalesced_bits;
						long cpu_region_mask = (cpu_coalesced_bits)? (1 << cpu_coalesced_bits) - 1 : 0;

						while ((cpu_qubit_count < cpu_region_bits) && (cpu_op_index < global_op_end)){	//Tem que pertencer a região 'global'
							if (!((cpu_region_mask >> pts[cpu_op_index]->target_bit) & 1)){			//Se o qubit do operador estiver fora da região (region_mask), incrementa o contador de qubits da região
								cpu_qubit_count++;
							}

							if (cpu_qubit_count <= cpu_region_bits)// && pts[op_index]->matrixType() != DIAG_PRI)
								cpu_region_mask = cpu_region_mask | (1 << pts[cpu_op_index]->target_bit);	//Acrescenta o qubit do operador na região se ainda não tiver atingido o limite (region)

							cpu_op_index++;
						}

						while (cpu_op_index < global_op_end){
							if (((cpu_region_mask >> pts[cpu_op_index]->target_bit) & 1))// || pts[op_index]->matrixType() == DIAG_PRI)
								cpu_op_index++;
							else
								break;
						}
						cpu_op_end = cpu_op_index;

						for (long bit_mask = 1; cpu_qubit_count < cpu_region_bits; bit_mask = bit_mask << 1){
							if ((bit_mask & global_region_mask) && (bit_mask & ~cpu_region_mask)){ //tem que não estar na região da cpu e estar na global
								cpu_region_mask = cpu_region_mask | bit_mask;
								cpu_qubit_count++;
							}
						}

						long cpu_region_count = (1 << (global_region_bits - cpu_region_bits)) + 1; 		//Número de regiões 			      -	 +1 para a condição de parada incluir todos
						long cpu_pos_count = 1 << (cpu_region_bits - 1); 						//Número de posições na região 	-	 -1 porque são duas posições por iteração


						long cpu_next_proj_id = 0;
						long proj_id_increment = ~(cpu_region_mask ^ global_region_mask) & ((1 << qubits) - 1);

						long proj_id;		//indentificador local da região
						proj_id = cpu_next_proj_id | cpu_proj_id;
						cpu_next_proj_id = (cpu_next_proj_id + proj_id_increment + 1) & ~proj_id_increment;
						cpu_region_count--;

						while (proj_id != -1){
							//Computa os operadores
							PCpuExecution1_0(state, pts, qubits, cpu_op_start, cpu_op_end, cpu_pos_count, proj_id, cpu_region_mask);

							proj_id = cpu_next_proj_id | cpu_proj_id;
							cpu_next_proj_id = (cpu_next_proj_id + proj_id_increment + 1) & ~proj_id_increment;
							cpu_region_count--;
							if (cpu_region_count <= 0)
								proj_id = -1;
						}

						cpu_op_start = cpu_op_end;
					}

					#pragma omp critical (global_teste)
					{
						cpu_proj_id = next_proj_id;
						next_proj_id = (next_proj_id + global_region_mask + 1) & ~global_region_mask;
						global_region_count--;
							if (global_region_count <= 0)
						cpu_proj_id = -1;
					}
				}

			}
			//#pragma omp section          //GPU EXECUTION
			else{
				long gpu_proj_id;

				#pragma omp critical (global_teste)
				{
					gpu_proj_id = next_proj_id;
					next_proj_id = (next_proj_id + global_region_mask + 1) & ~global_region_mask;
					global_region_count--;
					if (global_region_count <= 0)
						gpu_proj_id = -1;
				}

				while (gpu_proj_id != -1){
					//Project Gates
					vector <PT*> gpu_pts;

					int qubit_index;

					int qubit_map[qubits];
					memset(qubit_map, -1, qubits * sizeof(int));

					int mapped_qubit_index = 0;
					for (qubit_index = 0; qubit_index < qubits; qubit_index++){
						if ((1 << qubit_index) & global_region_mask){
							qubit_map[qubit_index] = mapped_qubit_index++;
						}
					}

					PT *projected_term;
					gpu_pts.clear();
					for (int op_index = global_op_start; op_index < global_op_end; op_index++){

						//verifica se o controle do operador satisfaz a parte global da região
						if ((pts[op_index]->control_mask & gpu_proj_id & ~global_region_mask) == (pts[op_index]->control_value & ~global_region_mask)){
							projected_term = new PT();

							projected_term->qubits = pts[op_index]->qubits;

							projected_term->matrix = pts[op_index]->matrix;
							projected_term->matrix_size = pts[op_index]->matrix_size;
							projected_term->control_mask = pts[op_index]->control_mask & global_region_mask;
							projected_term->control_value = pts[op_index]->control_value & global_region_mask;

							projected_term->target_bit = qubit_map[pts[op_index]->target_bit];
							projected_term->span_start_bit = projected_term->target_bit - log2(projected_term->matrix_size);

							projected_term->control_count = 0;
							for (int qubit_index = global_coalesced_bits; qubit_index < qubits; qubit_index++){
								if (projected_term->control_mask & (1<<qubit_index)){
									projected_term->control_count++;

									projected_term->control_mask &= ~(1<<qubit_index);			//retira da mascara o controle do qubit atual (qubit_index)
									projected_term->control_mask |= (1 << qubit_map[qubit_index]);	//e coloca o qubit que ele mapeia (qubit_map[qubit_index])

									if (projected_term->control_value & (1<<qubit_index)){ 		//se o valor do controle for zero faz a mesma coisa para control_value;
										projected_term->control_mask &= ~(1<<qubit_index);
										projected_term->control_mask |= (1 << qubit_map[qubit_index]);
									}
								}
							}

							gpu_pts.push_back(projected_term);
						}
					}
					gpu_pts.push_back(NULL);
					////////////////

					ProjectState(state, qubits, global_region_bits, gpu_proj_id, global_region_mask, gpu_count);

					GpuExecutionWrapper(NULL, &gpu_pts[0], global_region_bits, gpu_coalesced_bits, gpu_region_bits, gpu_count, block_size, repeat_count, 1);

					GetState(state, qubits, global_region_bits, gpu_proj_id, global_region_mask, gpu_count);

					for (int cleanup_index = 0; cleanup_index < gpu_pts.size() - 1; cleanup_index++){
						delete gpu_pts[cleanup_index];
					}

					#pragma omp critical (global_teste)
					{
						gpu_proj_id = next_proj_id;
						next_proj_id = (next_proj_id + global_region_mask + 1) & ~global_region_mask;
						global_region_count--;
						if (global_region_count <= 0)
							gpu_proj_id = -1;
					}
				}
			}

		//}
		}
	}
}

void DGM::setCpuStructure(long cpu_region_bits, long cpu_coalesced_bits){
	this->cpu_region_bits = cpu_region_bits;
	this->cpu_coalesced_bits = cpu_coalesced_bits;
}

// Atenção: a ordem dos parâmetros aqui foi corrigida para bater com a
// declaração em dgm.h (gpu_coalesced_bits, gpu_region_bits, repeat_count)
// — a definição antiga usava (gpu_region, gpu_coales, rept), em ordem
// diferente da declaração; inofensivo hoje porque este método nunca é
// chamado em lugar nenhum do projeto, mas corrigido para não virar uma
// armadilha assim que alguém passar a usá-lo.
void DGM::setGpuStructure(long gpu_coalesced_bits, long gpu_region_bits, int repeat_count){
	this->gpu_coalesced_bits = gpu_coalesced_bits;
	this->gpu_region_bits = gpu_region_bits;
	this->repeat_count = repeat_count;
	this->block_size = 1 << gpu_region_bits / 2 / repeat_count;
}
