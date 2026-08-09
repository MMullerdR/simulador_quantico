# Build tools
#NVCC = nvcc $(ARCH) -ccbin clang++
#CXX = clang++
ARCH = -arch=sm_52

NVCC = nvcc $(ARCH)
CXX = g++
GCC = gcc

#QBS_REGION = 4
#D = -D QBS_REGION=$(QBS_REGION)
OPS_BLOCK=300

INCLUDES = -Iinclude

# here are all the objects
GPUOBJS = kernel.o
OBJS = dgm.o common.o gates.o lib_general.o lib_shor.o lib_grover.o


# make and compile

# executables

shor: shor.o $(OBJS) $(GPUOBJS)
	$(NVCC) -o shor.out shor.o $(OBJS) $(GPUOBJS) -Xcompiler "-fopenmp"

grover: grover.o $(OBJS) $(GPUOBJS)
	$(NVCC) -o grover.out grover.o $(OBJS) $(GPUOBJS) -Xcompiler "-fopenmp"

general: general.o $(OBJS) $(GPUOBJS)
	$(NVCC) -o general.out general.o $(OBJS) $(GPUOBJS) -Xcompiler "-fopenmp"

# objects

dgm.o: src/dgm.cu
	$(NVCC) -c src/dgm.cu $(INCLUDES) -Xcompiler "-fopenmp -O3 -fcx-limited-range"

kernel.o: src/kernel.cu
	$(NVCC) -c -D OPS_BLOCK=$(OPS_BLOCK) src/kernel.cu $(INCLUDES)

gates.o: src/gates.cpp
	$(CXX) -c src/gates.cpp $(INCLUDES)

common.o: src/common.cpp
	$(CXX) -c src/common.cpp $(INCLUDES)

lib_general.o: src/lib_general.cpp
	$(CXX) -c src/lib_general.cpp $(INCLUDES)

lib_shor.o: src/lib_shor.cpp
	$(CXX) -c src/lib_shor.cpp $(INCLUDES)

lib_grover.o: src/lib_grover.cpp
	$(CXX) -c src/lib_grover.cpp $(INCLUDES)

grover.o: src/grover.cpp
	$(CXX) -c src/grover.cpp $(INCLUDES) -fopenmp

shor.o: src/shor.cpp
	$(CXX) -c src/shor.cpp $(INCLUDES) -fopenmp

general.o: src/general.cpp
	$(CXX) -c src/general.cpp $(INCLUDES) -fopenmp

clean:
	rm *.o *.out
