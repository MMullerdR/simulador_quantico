#include "lib_grover.h"
#include "dgm.h"
#include <vector>
#include <iostream>
#include <sys/time.h>

using namespace std;

int main(int argc, char **argv){
	srand(time(NULL));

	int execType = t_CPU, n_threads = 1, cpu_region = 14, cpu_coalesc = 11, multi_gpu = 1, gpu_region = 8, gpu_coalesc = 4, tam_block = 64, rept = 2;

	if (argc < 2){
		cout << "You need to define the execution parameters" << endl;
		return 0;
	}

	int qubits = atoi(argv[1]);


  if (argc > 2) {
    execType = atoi(argv[2]);
  }

	if (execType < t_CPU || execType > t_HYBRID){
		cout << "Invalid execution type: " << execType << endl; 
		return 0;
	}

	if (execType == t_PAR_CPU) {
		if (argc > 3) n_threads = atoi(argv[3]);
	}
	else if (execType == t_GPU) {
		if (argc > 3) multi_gpu = atoi(argv[3]);
	}
	else if (execType == t_HYBRID) {
		if (argc > 3) n_threads = atoi(argv[3]);
	}
 
	int value = 10;

	float t = Grover(qubits, value, t_CPU, n_threads, cpu_region, cpu_coalesc, multi_gpu, gpu_region, gpu_coalesc, tam_block, rept);
 
  cout << t << endl;
	
}
