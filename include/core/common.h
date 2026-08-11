#ifndef _COMMON_H_
#define _COMMON_H_

#include <stdio.h>
#include <stdlib.h>
#include <complex.h>
#include <sys/time.h>

#define complex __complex__

#define CHUNCK_SIZE 262144

#define PT_TAM 1
#define QB_LIMIT 30

#define DIM_BLOCK 2048

#define TAM_ARG 5

// Índices usados dentro do array "arg" de PT::setArgs/setArgsGPU e do
// array "arg" de DEV_OP (kernel.cu) — os dois lados precisam concordar
// com essa ordem.
#define SHIFT 0
#define CTRL_MASK 1
#define CTRL_VALUE 2
#define CTRL_REG_MASK 3
#define CTRL_REG_VALUE 4

// Constantes de um esquema de indexação anterior, hoje sem uso (mantidas
// sem alteração de comportamento).
#define ACUMM 0
#define SHIFT_READ 0
#define SHIFT_WRITE 0
#define MAT_START 0
#define MAT_SIZE 0
#define MAT_END 0

// Classificação da matriz 2x2 de uma porta, usada para escolher o
// caminho de execução mais barato (ver docs/03-motor-de-execucao-cpu.md).
enum {
	DENSE,
	DIAG_PRI,
	DIAG_SEC
};

// PT ("Pauli Term"): uma porta de 1 qubit já compilada a partir da
// linguagem de circuitos (ver docs/02-linguagem-de-circuitos.md),
// pronta para ser aplicada sobre o vetor de estado.
struct PT{
	int qubits;
	float complex *matrix;
	int matrix_size;

	// span_start_bit/target_bit: posições de bit (no índice do vetor de
	// estado) do qubit alvo. target_bit é o campo realmente usado hoje;
	// span_start_bit faz parte de uma feature de portas multi-qubit que
	// nunca chegou a ser implementada (mat_size/qubits sempre valem 2/1
	// na prática) — mantido sem uso, só com nome mais claro.
	int span_start_bit, target_bit;

	bool affected;

	long control_value, control_mask;
	long *control_bit_positions, control_count;

	// Parte de uma otimização de controle parcial nunca finalizada
	// (ver PT::ctrlAffect e o corpo comentado de setArgs) — mantida.
	long *control_rest, control_rest_count;

	PT();
	~PT();

	// Quantos controles têm posição de bit menor que "qubit" (parte de
	// uma otimização de controle parcial nunca finalizada).
	long ctrlAffect(long qubit);
	// Classifica a matriz em DENSE/DIAG_PRI/DIAG_SEC (ver enum acima).
	long matrixType();
	// Empacota deslocamento/máscara/valor de controle em "arg" (formato
	// usado pelo motor de CPU).
	void setArgs(long *arg, long up_to_bit);
	void setArgs_soft(long *arg, long up_to_bit);
	// Mesma ideia de setArgs, mas separando a parte do controle que cai
	// dentro da região coalescida da GPU do restante.
	void setArgsGPU(long *arg, int region_start, int region_size, int coalesced_bits);
	void print();
	void printMatrix();

};

// Imprime o vetor de estado inteiro (índice: parte real, parte imaginária).
void printMem(float complex* mem, int qubits);
// Como printMem, mas decompondo o índice em dois sub-registradores
// (útil para depurar exponenciação modular no Shor).
void printMemExp(float complex* mem, int qubits, int reg1, int reg2, long width);
// Como printMemExp, mas também conferindo se o valor medido bate com
// modular_pow(base_value, expoente, modulus) — usado só em depuração.
void printMemCheckExp(float complex* mem, int qubits, long width, long base_value, long modulus);

// Exponenciação modular rápida (base^exponent mod modulus).
long modular_pow(long base, long exponent, long modulus);

// Critérios de ordenação usados ao montar vec_pts em DGM::setFunction.
bool increasing(const PT *term1, const PT *term2);
bool decreasing(const PT *term1, const PT *term2);

void swap_value(int *value1, int *value2);
void swap_ptr(float **ptr1, float **ptr2);
void swap_ptr(float complex **ptr1, float complex **ptr2);

// Diferença entre dois "timeval" (fim - início), pra medir tempo de execução.
int timeval_subtract(struct timeval *result, struct timeval *end, struct timeval *start);

#endif
