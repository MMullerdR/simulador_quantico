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
OUT = outputs

# src/ is split by role (core engine, algorithm libraries, CLI entry
# points); VPATH lets the pattern rules below find a %.cpp/%.cu by name
# without caring which of the three subfolders it actually lives in.
VPATH = $(SRC)/core:$(SRC)/algorithms:$(SRC)/cli

# Object groups
CORE_OBJS = $(addprefix $(OUT)/, dgm.o common.o gates.o lib_general.o lib_shor.o lib_grover.o)
GPU_OBJS  = $(OUT)/kernel.o

.PHONY: all clean shor grover general

all: shor grover general

# executables (phony aliases for the real files under $(OUT))

shor: $(OUT)/shor.out
grover: $(OUT)/grover.out
general: $(OUT)/general.out

$(OUT)/shor.out: $(OUT)/shor.o $(CORE_OBJS) $(GPU_OBJS)
	$(NVCC) -o $@ $^ $(LDFLAGS)

$(OUT)/grover.out: $(OUT)/grover.o $(CORE_OBJS) $(GPU_OBJS)
	$(NVCC) -o $@ $^ $(LDFLAGS)

$(OUT)/general.out: $(OUT)/general.o $(CORE_OBJS) $(GPU_OBJS)
	$(NVCC) -o $@ $^ $(LDFLAGS)

# entry points also need OpenMP at compile time
$(OUT)/shor.o $(OUT)/general.o $(OUT)/grover.o: CXXFLAGS += -fopenmp

# per-file extra flags
$(OUT)/dgm.o: CXXFLAGS += -fopenmp -O3 -fcx-limited-range
$(OUT)/kernel.o: NVCCFLAGS += -D OPS_BLOCK=$(OPS_BLOCK)

# pattern rules (the "| $(OUT)" order-only prerequisite makes sure the
# output folder exists before compiling, without forcing a rebuild every
# time the folder's own timestamp changes)

$(OUT)/%.o: %.cpp | $(OUT)
	$(CXX) $(CXXFLAGS) -c $< -o $@

$(OUT)/%.o: %.cu | $(OUT)
	$(NVCC) $(NVCCFLAGS) -c $< -o $@

$(OUT):
	mkdir -p $(OUT)

clean:
	rm -rf $(OUT)
