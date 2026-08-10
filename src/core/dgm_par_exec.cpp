#include <iostream>
#include "../../include/core/dgm.h"
#include <omp.h>
#include <unistd.h>
#include <cstdio>
#include <iterator>

void PCpuExecution1(float complex *state, PT **pts, int qubits, long thread_count, int coalesced_bits, int region_bits, int iterations){
	// Sem isso, pedir uma região maior que o próprio número de qubits
	// (ex: o cpu_region_bits=14 padrão dos CLIs com menos de 14 qubits)
	// faz "1 << (qubits - region_bits)" mais abaixo deslocar por um
	// expoente negativo — comportamento indefinido que na prática vira
	// um reg_count gigante e escreve fora do vetor de estado (segfault
	// confirmado; ver docs/07-bugs-e-pontos-de-atencao.md, item 6).
	if (region_bits > qubits) region_bits = qubits;

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
	long mem_size = 1L << qubits;
	long qubits_limit = 20;
	long global_coalesced_bits = 15; //(cpu_coalesced_bits > gpu_coalesced_bits) ? cpu_coalesced_bits : gpu_coalesced_bits;

	long global_region_bits = qubits_limit;
	// Mesmo bug de PCpuExecution1 (docs/07-bugs-e-pontos-de-atencao.md,
	// item 6): sem este clamp, region_bits > qubits faz
	// "1 << (qubits - global_region_bits)" mais abaixo deslocar por um
	// expoente negativo. Aqui dispara sempre que qubits < qubits_limit
	// (20), não só em casos extremos de linha de comando.
	if (global_region_bits > qubits) global_region_bits = qubits;
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
						// Mesmo clamp de global_region_bits acima, aplicado à
						// sub-região de CPU: ela é recortada de dentro da
						// região global, então precisa ficar <= global_region_bits
						// (não <= qubits), senão o mesmo deslocamento por
						// expoente negativo acontece logo abaixo em cpu_region_count.
						long local_cpu_region_bits = cpu_region_bits;
						if (local_cpu_region_bits > global_region_bits) local_cpu_region_bits = global_region_bits;

						long cpu_qubit_count = cpu_coalesced_bits;
						long cpu_region_mask = (cpu_coalesced_bits)? (1 << cpu_coalesced_bits) - 1 : 0;

						while ((cpu_qubit_count < local_cpu_region_bits) && (cpu_op_index < global_op_end)){	//Tem que pertencer a região 'global'
							if (!((cpu_region_mask >> pts[cpu_op_index]->target_bit) & 1)){			//Se o qubit do operador estiver fora da região (region_mask), incrementa o contador de qubits da região
								cpu_qubit_count++;
							}

							if (cpu_qubit_count <= local_cpu_region_bits)// && pts[op_index]->matrixType() != DIAG_PRI)
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

						for (long bit_mask = 1; cpu_qubit_count < local_cpu_region_bits; bit_mask = bit_mask << 1){
							if ((bit_mask & global_region_mask) && (bit_mask & ~cpu_region_mask)){ //tem que não estar na região da cpu e estar na global
								cpu_region_mask = cpu_region_mask | bit_mask;
								cpu_qubit_count++;
							}
						}

						long cpu_region_count = (1 << (global_region_bits - local_cpu_region_bits)) + 1; 		//Número de regiões 			      -	 +1 para a condição de parada incluir todos
						long cpu_pos_count = 1 << (local_cpu_region_bits - 1); 						//Número de posições na região 	-	 -1 porque são duas posições por iteração


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
