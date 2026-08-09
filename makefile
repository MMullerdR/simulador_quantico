# Build tools
ARCH = -arch=sm_52
NVCC = nvcc $(ARCH)
CXX  = g++

# Build parameters
OPS_BLOCK = 300
INCLUDES  = -Iinclude

CXXFLAGS  = $(INCLUDES)
NVCCFLAGS = $(INCLUDES)
LDFLAGS   = -Xcompiler "-fopenmp"

SRC = src

# Object groups
CORE_OBJS = dgm.o common.o gates.o lib_general.o lib_shor.o lib_grover.o
GPU_OBJS  = kernel.o

.PHONY: all clean

all: shor grover general

# executables

shor: shor.o $(CORE_OBJS) $(GPU_OBJS)
	$(NVCC) -o shor.out $^ $(LDFLAGS)

grover: grover.o $(CORE_OBJS) $(GPU_OBJS)
	$(NVCC) -o grover.out $^ $(LDFLAGS)

general: general.o $(CORE_OBJS) $(GPU_OBJS)
	$(NVCC) -o general.out $^ $(LDFLAGS)

# entry points also need OpenMP at compile time
shor.o general.o grover.o: CXXFLAGS += -fopenmp

# per-file extra flags
dgm.o: NVCCFLAGS += -Xcompiler "-fopenmp -O3 -fcx-limited-range"
kernel.o: NVCCFLAGS += -D OPS_BLOCK=$(OPS_BLOCK)

# pattern rules

%.o: $(SRC)/%.cpp
	$(CXX) $(CXXFLAGS) -c $< -o $@

%.o: $(SRC)/%.cu
	$(NVCC) $(NVCCFLAGS) -c $< -o $@

clean:
	rm -f *.o *.out
