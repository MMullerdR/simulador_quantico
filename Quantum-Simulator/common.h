#ifndef _COMMON_H_
#define _COMMON_H_

#include <stdio.h>
#include <stdlib.h>
#include <complex.h>
#include <sys/time.h>

#define complex _Complex

#define CHUNCK_SIZE 262144

#define PT_TAM 1
#define QB_LIMIT 30

#define DIM_BLOCK 2048

#define TAM_ARG 5

#define SHIFT 0
#define CTRL_MASK 1
#define CTRL_VALUE 2
#define CTRL_REG_MASK 3
#define CTRL_REG_VALUE 4

#define ACUMM 0
#define SHIFT_READ 0
#define SHIFT_WRITE 0
#define MAT_START 0
#define MAT_SIZE 0
#define MAT_END 0

enum {
	DENSE,
	DIAG_PRI,
	DIAG_SEC
};

struct PT{
	int qubits;
	float complex *matrix;
	int mat_size;
	int start, end;
	bool affected;
	
	long ctrl_value, ctrl_mask;
	long *ctrl_pos, ctrl_count;
	long *ctrl_rest, ctrl_rest_count;

	PT();

	void destructor();
	long ctrlAffect(long qubit);
	long matrixType();
	void setArgs(long *arg, long affect);
	void setArgs_soft(long *arg, long affect);
	void setArgsGPU(long *arg, int region_start, int region_size, int coalesc);
	void print();
	void printMatrix();

};

void printMem(float complex* mem, int qubits);
void printMemExp(float complex* mem, int qubits, int reg1, int reg2, long n);
void printMemCheckExp(float complex* mem, int qubits, long n, long a, long N);

long modular_pow(long base, long exponent, long modulus);

bool increasing(const PT *pt1, const PT *pt2);
bool decreasing(const PT *pt1, const PT *pt2);

void swap_value(int *v1, int* v2);
void swap_ptr(float **ptr1, float **ptr2);
void swap_ptr(float complex **ptr1, float complex **ptr2);

int timeval_subtract(struct timeval *result, struct timeval *t2, struct timeval *t1);

#endif

