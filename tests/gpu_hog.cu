// Ocupa memória da GPU deixando livres só <leave_mb> MB, e segura até ser
// morto (ou 300 s, pra nunca ficar preso numa GPU compartilhada). Usado
// por tests/smoke_test.sh pra testar a decisão "o estado cabe na GPU?" do
// t_HYBRID (DGM::HybridExecution) com a GPU parcialmente cheia.
//
// Imprime "pronto <bytes livres>" quando terminou de ocupar.
#include <cuda_runtime.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

int main(int argc, char **argv){
	if (argc < 2){
		printf("uso: gpu_hog.out <leave_mb>\n");
		return 1;
	}

	size_t leave_bytes = (size_t) atol(argv[1]) << 20;
	size_t free_bytes = 0, total_bytes = 0;

	if (cudaMemGetInfo(&free_bytes, &total_bytes) != cudaSuccess){
		printf("erro: cudaMemGetInfo falhou\n");
		return 1;
	}

	if (free_bytes > leave_bytes){
		void *hogged = NULL;
		if (cudaMalloc(&hogged, free_bytes - leave_bytes) != cudaSuccess){
			printf("erro: cudaMalloc de %zu bytes falhou\n", free_bytes - leave_bytes);
			return 1;
		}
	}

	cudaMemGetInfo(&free_bytes, &total_bytes);
	printf("pronto %zu\n", free_bytes);
	fflush(stdout);

	sleep(300);
	return 0;
}
