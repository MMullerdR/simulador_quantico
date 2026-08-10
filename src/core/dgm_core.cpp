#include <iostream>
#include "../../include/core/dgm.h"
#include <omp.h>
#include <unistd.h>
#include <cstdio>
#include <iterator>

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
	state = (float complex*) calloc(1L << qubits, sizeof(float complex));
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
	long size = 1L << qubits;

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
	low_bits_mask = (1L << qubit_bit_shift) - 1;
	#pragma omp for
	for (long state_index = 0; state_index < size/2; state_index++){
		long pos = (state_index << 1) - (state_index&low_bits_mask);
		state[pos] = state[pos | (measured_bit << qubit_bit_shift)]/norm;
		state[pos | (1<<qubit_bit_shift)] = 0.0;
	}

	return measured_bit;
}

void DGM::colapse(int qubit_pos, int value){
	long size = 1L << qubits;
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

	long size = 1L << qubits;

	for (long state_index = 0; state_index < size; state_index++) probabilities[state_index&qubit_positions_mask] += pow(crealf(state[state_index]), 2.0) + pow(cimagf(state[state_index]), 2.0);

	return probabilities;
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
