#include <iostream>
#include "../../include/core/dgm.h"
#include <omp.h>
#include <unistd.h>
#include <cstdio>
#include <cstdlib>
#include <iterator>

// Resultado de compute_region: uma "região" (ver docs/01-arquitetura-geral.md,
// seção 3) pronta pra ser processada por PCpuExecution1_0.
struct RegionPlan{
	long region_mask;
	long region_bits;	// pode ser menor que o pedido, se a região não coube inteira
	long region_count;
	long pos_count;
	long op_end;		// primeiro índice de pts[] fora desta região
};

// Agrupa PTs consecutivos, a partir de pts[op_start], numa região de até
// region_bits qubits — a mesma conta que existia repetida (e, por causa
// disso, com o mesmo bug corrigido em mais de um lugar) em PCpuExecution1
// e duas vezes dentro de HybridExecution, ver
// docs/07-bugs-e-pontos-de-atencao.md, item 6.
//
// op_search_end: limite exclusivo de busca em pts[]; -1 significa "sem
// limite, para no NULL terminador" (caso de PCpuExecution1 e da passada
// 'global' do HybridExecution). Usado quando a região precisa ficar
// contida dentro de um lote já delimitado (o sub-lote de CPU dentro de
// HybridExecution, delimitado por global_op_end).
//
// outer_bound_bits: teto usado pra calcular region_count/pos_count —
// qubits pra uma região "solta", ou os bits da região-pai quando esta é
// uma sub-região recortada de dentro de outra.
//
// parent_mask: -1 se a região pode crescer sobre qualquer bit ainda livre;
// caso contrário, só bits dentro de parent_mask entram no preenchimento
// final (caso do sub-lote de CPU, recortado de dentro de global_region_mask).
static RegionPlan compute_region(PT **pts, long op_start, long op_search_end, long coalesced_bits, long region_bits, long outer_bound_bits, long parent_mask){
	long region_qubit_count = coalesced_bits;
	long region_mask = (coalesced_bits)? (1L << coalesced_bits) - 1 : 0;

	long op_index = op_start;
	while (region_qubit_count < region_bits && (op_search_end < 0 || op_index < op_search_end) && pts[op_index] != NULL){
		if (!((region_mask >> pts[op_index]->target_bit) & 1)){
			region_qubit_count++;
		}

		if (region_qubit_count <= region_bits)
			region_mask = region_mask | (1L << pts[op_index]->target_bit);

		op_index++;
	}
	while ((op_search_end < 0 || op_index < op_search_end) && pts[op_index] != NULL){
		if (((region_mask >> pts[op_index]->target_bit) & 1))
			op_index++;
		else
			break;
	}

	for (long bit_mask = 1; region_qubit_count < region_bits; bit_mask = bit_mask << 1){
		if ((bit_mask & ~region_mask) && (parent_mask < 0 || (bit_mask & parent_mask))){
			region_mask = region_mask | bit_mask;
			region_qubit_count++;
		}
	}

	if (region_qubit_count < region_bits)
		region_bits = region_qubit_count;

	RegionPlan plan;
	plan.region_mask = region_mask;
	plan.region_bits = region_bits;
	plan.region_count = (1L << (outer_bound_bits - region_bits)) + 1;
	plan.pos_count = 1L << (region_bits - 1);
	plan.op_end = op_index;
	return plan;
}

// region_bits já chega clampado contra qubits — garantido por
// DGM::validateTuning(), chamado no início de DGM::execute() antes de
// despachar pra cá (ver docs/07-bugs-e-pontos-de-atencao.md, item 6).
void PCpuExecution1(float complex *state, PT **pts, int qubits, long thread_count, int coalesced_bits, int region_bits, int iterations){
	// "iterations" repete a lista de portas inteira (antes o parâmetro era
	// ignorado aqui e quem queria repetir replicava os PT em setFunction --
	// o Grover chegava a guardar centenas de milhares de PT idênticos).
	for (int iteration_index = 0; iteration_index < iterations; iteration_index++){
	long op_index = 0;
	while (pts[op_index] != NULL){
		long pts_start = op_index;

		RegionPlan plan = compute_region(pts, op_index, -1, coalesced_bits, region_bits, qubits, -1);
		region_bits = plan.region_bits;	// ratchet: passadas seguintes partem do valor já reduzido
		long pts_end = plan.op_end;
		long region_mask = plan.region_mask;
		long region_count = plan.region_count;
		long pos_count = plan.pos_count;

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

		op_index = pts_end;
	}
	}
}

// ---------------------------------------------------------------------------
// Kernels de CPU vetorizáveis (item 23 em docs/07-bugs-e-pontos-de-atencao.md)
//
// O laço antigo de PCpuExecution1_0 caminhava pelos pares (pos0, pos1) com a
// recorrência `pos = (pos + inc) & pos_mask`: cada índice depende do
// anterior, então o compilador não vetoriza e cada par custa ~9 ciclos. Os
// bits baixos da região são sempre contíguos (cpu_coalesced_bits), então
// dá pra enumerar só os blocos "externos" com a recorrência e processar
// cada bloco contíguo com um laço simples, que o compilador vetoriza.
//
// target_clones gera uma versão AVX2 e uma genérica e escolhe em tempo de
// execução, então o binário continua rodando em qualquer x86-64 sem
// precisar de -march=native. Fora de x86-64/Linux/GCC>=6 (ou clang) o
// atributo some e fica só a versão genérica.
//
// "avx2" e não "arch=haswell": o dispatcher de "arch=<cpu>" testa o modelo
// exato do processador (cpu_is), não as instruções, e num Zen 3 (Threadripper
// PRO 5955WX da qflex) caía na versão genérica sem avisar -- 0,575 ns por
// amplitude-porta contra 0,15 ns com a versão AVX2 de fato escolhida.
// ---------------------------------------------------------------------------
#if (defined(__clang__) || (defined(__GNUC__) && __GNUC__ >= 6)) && defined(__x86_64__) && defined(__linux__)
#define CPU_SIMD_CLONES __attribute__((target_clones("avx2", "default")))
#else
#define CPU_SIMD_CLONES
#endif
#define CPU_INLINE static inline __attribute__((always_inline))

// m = matriz 2x2 como 8 floats (re, im de m00, m01, m10, m11). a[j] e b[j]
// são os pares (pos0, pos1) de n amplitudes consecutivas.
CPU_INLINE void dense_run(float *__restrict a, float *__restrict b, long n, const float *m){
	const float m0r = m[0], m0i = m[1], m1r = m[2], m1i = m[3];
	const float m2r = m[4], m2i = m[5], m3r = m[6], m3i = m[7];
	#pragma omp simd
	for (long j = 0; j < n; j++){
		float ar = a[2*j], ai = a[2*j+1], br = b[2*j], bi = b[2*j+1];
		a[2*j]   = m0r*ar - m0i*ai + m1r*br - m1i*bi;
		a[2*j+1] = m0r*ai + m0i*ar + m1r*bi + m1i*br;
		b[2*j]   = m2r*ar - m2i*ai + m3r*br - m3i*bi;
		b[2*j+1] = m2r*ai + m2i*ar + m3r*bi + m3i*br;
	}
}

// Alvo fora da faixa baixa contígua: a e b são dois blocos separados de n
// amplitudes (distância tmask entre eles).
CPU_SIMD_CLONES static void dense_pairs_far(float *__restrict a, float *__restrict b, long n, const float *m){
	dense_run(a, b, n, m);
}

// Alvo dentro da faixa baixa contígua: o bloco de n amplitudes contém os
// dois lados de cada par, a tmask de distância (blocos alternados de tmask).
CPU_INLINE void dense_pair(float *a, float *b, const float *m){
	float ar = a[0], ai = a[1], br = b[0], bi = b[1];
	a[0] = m[0]*ar - m[1]*ai + m[2]*br - m[3]*bi;
	a[1] = m[0]*ai + m[1]*ar + m[2]*bi + m[3]*br;
	b[0] = m[4]*ar - m[5]*ai + m[6]*br - m[7]*bi;
	b[1] = m[4]*ai + m[5]*ar + m[6]*bi + m[7]*br;
}

// tmask = 1 e 2 têm blocos curtos demais pra dense_run: laço próprio sobre
// os blocos, que o compilador vetoriza entre blocos.
// m é copiado pra uma matriz local: por ser float* sem restrict, as escritas em s
// obrigariam o compilador a reler m a cada iteração e impediriam a vetorização.
CPU_SIMD_CLONES static void dense_pairs_low(float *s, long n, long tmask, const float *mat){
	const float m[8] = {mat[0], mat[1], mat[2], mat[3], mat[4], mat[5], mat[6], mat[7]};
	if (tmask == 1){
		#pragma omp simd
		for (long k = 0; k < n/2; k++)
			dense_pair(s + 4*k, s + 4*k + 2, m);
	}
	else if (tmask == 2){
		#pragma omp simd
		for (long k = 0; k < n/4; k++){
			dense_pair(s + 8*k,     s + 8*k + 4, m);
			dense_pair(s + 8*k + 2, s + 8*k + 6, m);
		}
	}
	else{
		for (long base = 0; base < n; base += 2*tmask)
			dense_run(s + 2*base, s + 2*(base + tmask), tmask, m);
	}
}

// Matriz diagonal principal (m01 = m10 = 0): a *= m00, b *= m11. Portas de
// fase (Z, R1-R3) têm m00 = 1, e aí só o lado b precisa ser tocado --
// metade das leituras/escritas e das multiplicações.
CPU_INLINE void diag_run(float *__restrict a, float *__restrict b, long n, const float *m){
	const float p0r = m[0], p0i = m[1], p3r = m[6], p3i = m[7];
	if (p0r == 1.0f && p0i == 0.0f){
		#pragma omp simd
		for (long j = 0; j < n; j++){
			float br = b[2*j], bi = b[2*j+1];
			b[2*j]   = p3r*br - p3i*bi;
			b[2*j+1] = p3r*bi + p3i*br;
		}
	}
	else{
		#pragma omp simd
		for (long j = 0; j < n; j++){
			float ar = a[2*j], ai = a[2*j+1], br = b[2*j], bi = b[2*j+1];
			a[2*j]   = p0r*ar - p0i*ai;
			a[2*j+1] = p0r*ai + p0i*ar;
			b[2*j]   = p3r*br - p3i*bi;
			b[2*j+1] = p3r*bi + p3i*br;
		}
	}
}

CPU_SIMD_CLONES static void diag_pairs_far(float *__restrict a, float *__restrict b, long n, const float *m){
	diag_run(a, b, n, m);
}

CPU_INLINE void diag_pair(float *a, float *b, const float *m, bool skip_a){
	if (!skip_a){
		float ar = a[0], ai = a[1];
		a[0] = m[0]*ar - m[1]*ai;
		a[1] = m[0]*ai + m[1]*ar;
	}
	float br = b[0], bi = b[1];
	b[0] = m[6]*br - m[7]*bi;
	b[1] = m[6]*bi + m[7]*br;
}

// m é copiado pra uma matriz local: por ser float* sem restrict, as escritas em s
// obrigariam o compilador a reler m a cada iteração e impediriam a vetorização.
CPU_SIMD_CLONES static void diag_pairs_low(float *s, long n, long tmask, const float *mat){
	const float m[8] = {mat[0], mat[1], mat[2], mat[3], mat[4], mat[5], mat[6], mat[7]};
	if (tmask == 1 || tmask == 2){
		const bool skip_a = (m[0] == 1.0f && m[1] == 0.0f);
		if (tmask == 1){
			#pragma omp simd
			for (long k = 0; k < n/2; k++)
				diag_pair(s + 4*k, s + 4*k + 2, m, skip_a);
		}
		else{
			#pragma omp simd
			for (long k = 0; k < n/4; k++){
				diag_pair(s + 8*k,     s + 8*k + 4, m, skip_a);
				diag_pair(s + 8*k + 2, s + 8*k + 6, m, skip_a);
			}
		}
	}
	else{
		for (long base = 0; base < n; base += 2*tmask)
			diag_run(s + 2*base, s + 2*(base + tmask), tmask, m);
	}
}

// Matriz anti-diagonal (m00 = m11 = 0): a' = m01 * b, b' = m10 * a. Quando
// m01 = m10 = 1 (X) é só uma troca de memória, sem nenhuma multiplicação.
CPU_INLINE void sec_run(float *__restrict a, float *__restrict b, long n, const float *m){
	const float q1r = m[2], q1i = m[3], q2r = m[4], q2i = m[5];
	if (q1r == 1.0f && q1i == 0.0f && q2r == 1.0f && q2i == 0.0f){
		#pragma omp simd
		for (long j = 0; j < 2*n; j++){
			float t = a[j];
			a[j] = b[j];
			b[j] = t;
		}
	}
	else{
		#pragma omp simd
		for (long j = 0; j < n; j++){
			float ar = a[2*j], ai = a[2*j+1], br = b[2*j], bi = b[2*j+1];
			a[2*j]   = q1r*br - q1i*bi;
			a[2*j+1] = q1r*bi + q1i*br;
			b[2*j]   = q2r*ar - q2i*ai;
			b[2*j+1] = q2r*ai + q2i*ar;
		}
	}
}

CPU_SIMD_CLONES static void sec_pairs_far(float *__restrict a, float *__restrict b, long n, const float *m){
	sec_run(a, b, n, m);
}

CPU_INLINE void sec_pair(float *a, float *b, const float *m){
	float ar = a[0], ai = a[1], br = b[0], bi = b[1];
	a[0] = m[2]*br - m[3]*bi;
	a[1] = m[2]*bi + m[3]*br;
	b[0] = m[4]*ar - m[5]*ai;
	b[1] = m[4]*ai + m[5]*ar;
}

// m é copiado pra uma matriz local: por ser float* sem restrict, as escritas em s
// obrigariam o compilador a reler m a cada iteração e impediriam a vetorização.
CPU_SIMD_CLONES static void sec_pairs_low(float *s, long n, long tmask, const float *mat){
	const float m[8] = {mat[0], mat[1], mat[2], mat[3], mat[4], mat[5], mat[6], mat[7]};
	if (tmask == 1){
		#pragma omp simd
		for (long k = 0; k < n/2; k++)
			sec_pair(s + 4*k, s + 4*k + 2, m);
	}
	else if (tmask == 2){
		#pragma omp simd
		for (long k = 0; k < n/4; k++){
			sec_pair(s + 8*k,     s + 8*k + 4, m);
			sec_pair(s + 8*k + 2, s + 8*k + 6, m);
		}
	}
	else{
		for (long base = 0; base < n; base += 2*tmask)
			sec_run(s + 2*base, s + 2*(base + tmask), tmask, m);
	}
}

// Aplica o PT "term" sobre todos os pares da região descrita por
// (mask, id): mask = bits que a região enumera (inclui o alvo; já sem os
// controles que caem dentro da região), id = valor dos bits fora de mask
// (inclui o valor dos controles). Mesmo resultado do laço genérico no fim
// da função, só que em blocos contíguos quando dá.
static void apply_gate(float complex *state, PT *term, long matrix_type, long mask, long id){
	long target_bit_mask = 1L << term->target_bit;
	long region_size = __builtin_popcountl(mask);
	long low_bits = __builtin_ctzl(~mask);	// bits baixos contíguos dentro da região

	if (low_bits >= 2){
		void (*far_kernel)(float *, float *, long, const float *);
		void (*low_kernel)(float *, long, long, const float *);
		switch (matrix_type){
			case DENSE:    far_kernel = dense_pairs_far; low_kernel = dense_pairs_low; break;
			case DIAG_PRI: far_kernel = diag_pairs_far;  low_kernel = diag_pairs_low;  break;
			default:       far_kernel = sec_pairs_far;   low_kernel = sec_pairs_low;   break;	// DIAG_SEC
		}
		const float *m = (const float *) term->matrix;
		float *s = (float *) state;
		long run_len = 1L << low_bits;
		long pos = 0;

		if (term->target_bit >= low_bits){
			long outer_mask = mask & ~target_bit_mask & ~(run_len - 1);
			long outer_count = 1L << (region_size - 1 - low_bits);
			long inc = ~outer_mask + 1;
			for (long outer_index = 0; outer_index < outer_count; outer_index++){
				long base = pos | id;
				far_kernel(s + 2*base, s + 2*(base + target_bit_mask), run_len, m);
				pos = (pos + inc) & outer_mask;
			}
		}
		else{
			long outer_mask = mask & ~(run_len - 1);
			long outer_count = 1L << (region_size - low_bits);
			long inc = ~outer_mask + 1;
			for (long outer_index = 0; outer_index < outer_count; outer_index++){
				low_kernel(s + 2*(pos | id), run_len, target_bit_mask, m);
				pos = (pos + inc) & outer_mask;
			}
		}
		return;
	}

	// Caminho genérico (um par por iteração).
	long pos_mask = mask & ~target_bit_mask;
	long inc = ~pos_mask + 1;
	long pos = 0;
	long pos_count = 1L << (region_size - 1);
	long pos0, pos1;
	float complex tmp;

	switch (matrix_type){
		case DENSE:
			for (long pos_index = 0; pos_index < pos_count; pos_index++){
				pos0 = pos | id;
				pos1 = pos0 | target_bit_mask;
				pos = (pos+inc) & pos_mask;

				tmp 		= term->matrix[2] * state[pos0] + term->matrix[3] * state[pos1];
				state[pos0] = term->matrix[0] * state[pos0] + term->matrix[1] * state[pos1];
				state[pos1] = tmp;
			}
			break;
		case DIAG_PRI:
			for (long pos_index = 0; pos_index < pos_count; pos_index++){
				pos0 = pos | id;
				pos1 = pos0 | target_bit_mask;
				pos = (pos+inc) & pos_mask;

				tmp			= term->matrix[3] * state[pos1];
				state[pos0] *= term->matrix[0];
				state[pos1] = tmp;
			}
			break;
		case DIAG_SEC:
			for (long pos_index = 0; pos_index < pos_count; pos_index++){
				pos0 = pos | id;
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

// Aplica os PT de pts[pts_start..pts_end) restritos a uma única região
// (region_id/region_mask) — mesma lógica de pares (pos0, pos1) e tipos de
// matriz de CpuExecution1_1/2/3, só que iterando apenas sobre a fatia de
// memória dessa região (ver docs/03-motor-de-execucao-cpu.md, seção 5).
// pos_count (2^(bits da região - 1)) segue na assinatura por compatibilidade
// com dgm.h, mas agora sai da própria máscara em apply_gate().
void PCpuExecution1_0(float complex *state, PT **pts, int qubits, long pts_start, long pts_end, long pos_count, long region_id, long region_mask){
	for (long op_index = pts_start; op_index < pts_end; op_index++){
		PT *term = pts[op_index];
		long matrix_type = term->matrixType();

		if (!term->control_count){
			apply_gate(state, term, matrix_type, region_mask, region_id);
		}
		//Importante: region_id é o identificador da região e corresponde ao valor dos qubits externos à região de operação (region_mask)
		else if ((term->control_mask & region_id & ~region_mask) == (term->control_value & ~region_mask)){		//Verifica se a parte 'global' do controle satisfaz a região (region_id)
			// Os controles que caem dentro da região saem da máscara (não se
			// itera sobre eles) e o valor deles entra no region_id.
			apply_gate(state, term, matrix_type, region_mask & ~term->control_mask, region_id | term->control_value);
		}
	}
}

// Item 17, tentativa 6: modelo de custo por região usado pra decidir se uma
// thread de CPU deve ou não pegar mais uma região. Sem isso a CPU pegava
// regiões na mesma taxa que a GPU mesmo sendo dezenas de vezes mais lenta
// por região em circuitos pesados -- Grover a 21 qubits: uma região de
// CPU leva ~65s, a GPU faz o circuito inteiro em ~1,4s (docs/07, item 17).
//
// Custo de uma região de 2^bits amplitudes com `ops` portas:
//   CPU (1 thread) = ops * 2^bits * cpu_ns_per_amp_op
//   GPU            = escala * 2^bits * (ns_cópia + ops * ns_porta)
// Valores iniciais medidos no qflex (RTX A4500 + Threadripper PRO 5955WX,
// 1,4 ns por amplitude-porta na CPU; GPU ~3,8 ns/amplitude de cópia com
// memória travada e ~0,012 ns por amplitude-porta). São só o ponto de
// partida: cada região concluída refina cpu_ns_per_amp_op e a escala da
// GPU (média móvel), então outra máquina converge sozinha.
static double hybrid_cpu_ns_per_amp_op = 1.4;
static double hybrid_gpu_scale = 1.0;
static long hybrid_gpu_samples = 0;
static const double HYBRID_GPU_NS_COPY = 3.8;
static const double HYBRID_GPU_NS_OP = 0.012;

static double hybrid_est_cpu_ns(long ops, long region_bits){
	return (double)ops * (double)(1L << region_bits) * hybrid_cpu_ns_per_amp_op;
}

static double hybrid_gpu_prior_ns(long ops, long region_bits){
	return (double)(1L << region_bits) * (HYBRID_GPU_NS_COPY + (double)ops * HYBRID_GPU_NS_OP);
}

// Regra: uma thread de CPU só pega mais uma região se ela terminar antes de
// a GPU, sozinha, limpar todas as que sobraram. Chamar dentro da seção
// crítica global_teste (lê/escreve as estimativas acima).
static bool hybrid_cpu_may_claim(long unclaimed, long ops, long region_bits){
	if (unclaimed < 1) return false;
	double est_gpu = hybrid_gpu_scale * hybrid_gpu_prior_ns(ops, region_bits);
	return (double)unclaimed * est_gpu >= hybrid_est_cpu_ns(ops, region_bits);
}

// Backend t_HYBRID: a thread OpenMP 0 comanda a GPU (projeta o estado,
// dispara o kernel, recolhe o resultado) enquanto as demais processam
// regiões em CPU ao mesmo tempo, região "global" por região "global" —
// ver docs/01-arquitetura-geral.md e docs/04-gpu-cuda.md.
void DGM::HybridExecution(PT **pts, int iterations){
	// Instrumentação opt-in (item 17 em docs/07-bugs-e-pontos-de-atencao.md)
	// -- ligada com HYBRID_DEBUG=1 no ambiente, silenciosa por padrão. Só
	// imprime quem processou cada região "global" (CPU de qual thread, ou
	// GPU) e quantas cada lado pegou no total; não muda nada do cálculo.
	static bool hybrid_debug = (getenv("HYBRID_DEBUG") != NULL);
	long debug_cpu_regions = 0, debug_gpu_regions = 0;

	long mem_size = 1L << qubits;
	long qubits_limit = 20;

	// Item 17 em docs/07-bugs-e-pontos-de-atencao.md, tentativa 5: com
	// qubits <= qubits_limit existe uma única região "global", e a corrida por
	// ela (#pragma omp critical) virava loteria -- se a GPU ganhava, o tempo
	// era o da GPU; se uma thread de CPU ganhava, ela processava o estado
	// inteiro sozinha, no tempo do backend serial (Grover a 20 qubits: 0,59s
	// numa execução, 43,6s em outra idêntica, ver benchmark.log/
	// benchmark2.log). Sem mais de uma região não há o que dividir entre CPU
	// e GPU, então o resultado deste caso é decidido aqui de forma
	// determinística: roda inteiro na GPU, pelo mesmo caminho do t_GPU.
	//
	// Tentativa 7: o mesmo desvio vale quando o vetor de estado inteiro cabe
	// na memória livre da GPU. Medido no qflex (RTX A4500, 20 GB): o modo de
	// regiões copia o estado a cada passada do circuito e fica de 1x a 3,4x
	// mais lento que o caminho residente do t_GPU, que copia uma vez só.
	// O modo de regiões com CPU+GPU só é necessário quando o estado NÃO cabe
	// na GPU (32 qubits = 32 GB: t_GPU dá "out of memory", o híbrido roda e
	// é ~2x mais rápido que a CPU paralela). HYBRID_FORCE_REGIONS=1 desliga
	// este desvio de "cabe na GPU" (pra medir/comparar o modo de regiões).
	bool fits_in_gpu = false;
	static bool force_regions = (getenv("HYBRID_FORCE_REGIONS") != NULL && getenv("HYBRID_FORCE_REGIONS")[0] == '1');
	if (qubits > qubits_limit && !force_regions){
		long free_bytes = GpuMinFreeBytes(gpu_count);
		long need_bytes = (mem_size * (long)sizeof(float complex)) / gpu_count + (512L << 20);	// + folga pro contexto/kernels
		fits_in_gpu = (free_bytes >= 0 && free_bytes >= need_bytes);
		if (hybrid_debug) fprintf(stderr, "[HYBRID] memória livre da GPU=%ld bytes, estado precisa de %ld: %s\n", free_bytes, need_bytes, fits_in_gpu ? "cabe" : "não cabe");
	}
	if (qubits <= qubits_limit || fits_in_gpu){
		scheduleForGpu();	// aqui nenhuma thread de CPU lê pts[], então pode reordenar
		GpuExecutionWrapper(state, pts, qubits, gpu_coalesced_bits, gpu_region_bits, gpu_count, block_size, repeat_count, iterations);
		if (hybrid_debug){
			fprintf(stderr, "[HYBRID] qubits=%ld: %s, delegado inteiro à GPU\n", qubits, (qubits <= qubits_limit) ? "região única (<= qubits_limit)" : "estado cabe na GPU");
		}
		return;
	}
	// Fixo em 15 (em vez de derivado de cpu_coalesced_bits/gpu_coalesced_bits)
	// — nunca chegou a ser parametrizado; ver item 6 do design de
	// arquitetura em docs/07-bugs-e-pontos-de-atencao.md, item 7.
	long global_coalesced_bits = 15;

	long global_region_bits = qubits_limit;
	// Mesmo bug de PCpuExecution1 (docs/07-bugs-e-pontos-de-atencao.md,
	// item 6): sem este clamp, region_bits > qubits faz
	// "1 << (qubits - global_region_bits)" mais abaixo deslocar por um
	// expoente negativo. Desde a tentativa 5 do item 17 o retorno antecipado
	// acima já cobre qubits <= qubits_limit, então o clamp virou só defesa.
	if (global_region_bits > qubits) global_region_bits = qubits;
	long global_op_start, global_op_end;
	long next_proj_id;

	// Item 17, tentativa 6: o vetor de estado é travado em memória (pinned)
	// uma vez por chamada, pela thread de GPU na primeira região que ela
	// processar (assim o custo de inicializar o contexto CUDA continua
	// sobreposto ao trabalho das threads de CPU), e destravado no fim.
	// HYBRID_PIN=0 desliga, pra comparar A/B sem recompilar.
	static bool pin_enabled = !(getenv("HYBRID_PIN") != NULL && getenv("HYBRID_PIN")[0] == '0');
	bool state_pinned = false, pin_attempted = false;

	// HYBRID_GATING=0 desliga a regra hybrid_cpu_may_claim (comparação A/B).
	static bool gating_enabled = !(getenv("HYBRID_GATING") != NULL && getenv("HYBRID_GATING")[0] == '0');
	long debug_cpu_refusals = 0;

	omp_set_num_threads(thread_count);

	// Modo de regiões: cada iteração percorre a lista de portas inteira
	// (passada por passada), com o vetor de estado travado em memória uma
	// vez só para todas elas.
	for (int iteration_index = 0; iteration_index < iterations; iteration_index++){
	int op_index = 0;
	while (pts[op_index] != NULL){
		global_op_start = op_index;

		RegionPlan plan = compute_region(pts, op_index, -1, global_coalesced_bits, global_region_bits, qubits, -1);
		global_region_bits = plan.region_bits;	// ratchet: passadas seguintes partem do valor já reduzido
		global_op_end = plan.op_end;
		long global_region_mask = plan.region_mask;
		long global_region_count = plan.region_count;
		long global_pos_count = plan.pos_count;
		long pass_ops = global_op_end - global_op_start;
		if (pass_ops < 1) pass_ops = 1;

		/////////////////////////////////////////////////////////////////////////////////////////////////////

		next_proj_id = 0;	//contador 'global' do número de regiões já computadas

		//Define a primeira região (region_id) da thread

		#pragma omp parallel num_threads(thread_count)
		{
			if (omp_get_thread_num()!=0){  //CPU EXECUTION
				long cpu_proj_id;

				#pragma omp critical (global_teste)
				{
					if (!gating_enabled || hybrid_cpu_may_claim(global_region_count - 1, pass_ops, global_region_bits)){
						cpu_proj_id = next_proj_id;
						next_proj_id = (next_proj_id + global_region_mask + 1) & ~global_region_mask;
						global_region_count--;
						if (global_region_count <= 0)
							cpu_proj_id = -1;
					}
					else{
						cpu_proj_id = -1;
						debug_cpu_refusals++;
					}
				}

				double cpu_t0 = omp_get_wtime();
				while (cpu_proj_id != -1){
					if (hybrid_debug){
						#pragma omp atomic
						debug_cpu_regions++;
						fprintf(stderr, "[HYBRID] CPU (thread %d) global_region=%ld qubits=%ld global_region_bits=%ld\n", omp_get_thread_num(), cpu_proj_id, qubits, global_region_bits);
					}
					long cpu_op_start = global_op_start;

					while (cpu_op_start < global_op_end){
						// Mesmo clamp de global_region_bits acima, aplicado à
						// sub-região de CPU: ela é recortada de dentro da
						// região global, então precisa ficar <= global_region_bits
						// (não <= qubits), senão o mesmo deslocamento por
						// expoente negativo acontece logo abaixo em cpu_region_count.
						long local_cpu_region_bits = cpu_region_bits;
						if (local_cpu_region_bits > global_region_bits) local_cpu_region_bits = global_region_bits;

						RegionPlan cpu_plan = compute_region(pts, cpu_op_start, global_op_end, cpu_coalesced_bits, local_cpu_region_bits, global_region_bits, global_region_mask);
						long cpu_op_end = cpu_plan.op_end;
						long cpu_region_mask = cpu_plan.region_mask;
						long cpu_region_count = cpu_plan.region_count;
						long cpu_pos_count = cpu_plan.pos_count;

						long cpu_next_proj_id = 0;
						long proj_id_increment = ~(cpu_region_mask ^ global_region_mask) & ((1L << qubits) - 1);

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

					double cpu_dur_ns = (omp_get_wtime() - cpu_t0) * 1e9;

					#pragma omp critical (global_teste)
					{
						// refina a estimativa de custo da CPU com a região que acabou de terminar
						hybrid_cpu_ns_per_amp_op = 0.7 * hybrid_cpu_ns_per_amp_op + 0.3 * (cpu_dur_ns / ((double)pass_ops * (double)(1L << global_region_bits)));

						if (!gating_enabled || hybrid_cpu_may_claim(global_region_count - 1, pass_ops, global_region_bits)){
							cpu_proj_id = next_proj_id;
							next_proj_id = (next_proj_id + global_region_mask + 1) & ~global_region_mask;
							global_region_count--;
							if (global_region_count <= 0)
								cpu_proj_id = -1;
						}
						else{
							cpu_proj_id = -1;
							debug_cpu_refusals++;
						}
					}
					cpu_t0 = omp_get_wtime();
				}

			}
			else{  // thread 0: GPU
				long gpu_proj_id;

				#pragma omp critical (global_teste)
				{
					gpu_proj_id = next_proj_id;
					next_proj_id = (next_proj_id + global_region_mask + 1) & ~global_region_mask;
					global_region_count--;
					if (global_region_count <= 0)
						gpu_proj_id = -1;
				}

				// Item 17 em docs/07-bugs-e-pontos-de-atencao.md: o buffer
				// de GPU usado por ProjectState/GetState agora é alocado
				// uma única vez por lote (não mais a cada região), já que
				// global_region_bits (o que determina o tamanho do buffer)
				// não muda enquanto este laço roda. Só aloca se a thread
				// realmente vai processar pelo menos uma região.
				bool gpu_state_allocated = (gpu_proj_id != -1);
				if (gpu_state_allocated) AllocGpuState(global_region_bits, gpu_count);
				if (gpu_state_allocated && pin_enabled && !pin_attempted){
					pin_attempted = true;
					state_pinned = PinHostState(state, mem_size * sizeof(float complex));
					if (hybrid_debug) fprintf(stderr, "[HYBRID] pin do vetor de estado (%ld bytes): %s\n", mem_size * (long)sizeof(float complex), state_pinned ? "ok" : "falhou, segue com memória comum");
				}

				while (gpu_proj_id != -1){
					double gpu_t0 = omp_get_wtime();
					if (hybrid_debug){
						#pragma omp atomic
						debug_gpu_regions++;
						fprintf(stderr, "[HYBRID] GPU global_region=%ld qubits=%ld global_region_bits=%ld\n", gpu_proj_id, qubits, global_region_bits);
					}
					// Reconstrói, só pra essa região, uma lista de PT com
					// qubits renumerados (0..region_bits-1) — a GPU só
					// enxerga a fatia projetada, não o registrador inteiro.
					vector <PT*> gpu_pts;

					int qubit_index;

					int qubit_map[qubits];
					memset(qubit_map, -1, qubits * sizeof(int));

					int mapped_qubit_index = 0;
					for (qubit_index = 0; qubit_index < qubits; qubit_index++){
						if ((1L << qubit_index) & global_region_mask){
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
								if (projected_term->control_mask & (1L <<qubit_index)){
									projected_term->control_count++;

									projected_term->control_mask &= ~(1L <<qubit_index);			//retira da mascara o controle do qubit atual (qubit_index)
									projected_term->control_mask |= (1L << qubit_map[qubit_index]);	//e coloca o qubit que ele mapeia (qubit_map[qubit_index])

									if (projected_term->control_value & (1L <<qubit_index)){ 		//se o valor do controle for zero faz a mesma coisa para control_value;
										projected_term->control_mask &= ~(1L <<qubit_index);
										projected_term->control_mask |= (1L << qubit_map[qubit_index]);
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

					double gpu_dur_ns = (omp_get_wtime() - gpu_t0) * 1e9;

					#pragma omp critical (global_teste)
					{
						// A primeira região do processo paga o cold-start do contexto
						// CUDA (~0,2s, docs/07 item 17) e não representa o custo normal.
						if (hybrid_gpu_samples++ > 0)
							hybrid_gpu_scale = 0.7 * hybrid_gpu_scale + 0.3 * (gpu_dur_ns / hybrid_gpu_prior_ns(pass_ops, global_region_bits));

						gpu_proj_id = next_proj_id;
						next_proj_id = (next_proj_id + global_region_mask + 1) & ~global_region_mask;
						global_region_count--;
						if (global_region_count <= 0)
							gpu_proj_id = -1;
					}
				}

				if (gpu_state_allocated) FreeGpuState(gpu_count);
			}

		}

		op_index = global_op_end;
	}
	}

	if (state_pinned) UnpinHostState(state);

	if (hybrid_debug){
		fprintf(stderr, "[HYBRID] total: %ld região(ões) CPU, %ld região(ões) GPU, %ld recusa(s) de CPU pelo modelo de custo (cpu_ns_por_amp_porta=%.3f gpu_escala=%.3f)\n", debug_cpu_regions, debug_gpu_regions, debug_cpu_refusals, hybrid_cpu_ns_per_amp_op, hybrid_gpu_scale);
	}
}
