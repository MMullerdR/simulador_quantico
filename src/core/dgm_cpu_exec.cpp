#include <iostream>
#include "../../include/dgm.h"
#include <omp.h>
#include <unistd.h>
#include <cstdio>
#include <iterator>

void DGM::CpuExecution1(int iterations){
	long mem_size = 1L << qubits;

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
