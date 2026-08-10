# Build tools
ARCH = -arch=sm_52
NVCC = nvcc $(ARCH)
CXX  = g++

# Build parameters
OPS_BLOCK = 300
INCLUDES  = -Iinclude

# kernel.cu instancia ~260 versões do mesmo kernel via template (uma por
# combinação de tam_block/rept/coalesc — ver docs/04-gpu-cuda.md). Em -O3
# (padrão do nvcc) isso pode fazer o back-end (cicc/ptxas) travar por
# muito tempo numa única instanciação mais pesada de otimizar. KERNEL_OPT
# fica em -O0 por padrão pra manter o ciclo de compilar/verificar rápido;
# pra um build de produção de verdade (com performance real), rode
# "make KERNEL_OPT=-O3 kernel.o" (ou "make clean && make KERNEL_OPT=-O3").
KERNEL_OPT = -O0

CXXFLAGS  = $(INCLUDES)
NVCCFLAGS = $(INCLUDES)
LDFLAGS   = -Xcompiler "-fopenmp"

SRC = src
OUT = outputs

# src/ is split by role (core engine, algorithm libraries, CLI entry
# points); VPATH lets the pattern rules below find a %.cpp/%.cu by name
# without caring which of the three subfolders it actually lives in.
VPATH = $(SRC)/core:$(SRC)/algorithms:$(SRC)/cli

# GPU=stub (padrão): usa src/core/kernel_stub.cpp, compilado com g++,
# no lugar de kernel.cu — não usa nvcc em NADA do build, só serve pros
# backends de CPU (t_CPU/t_PAR_CPU). Existe porque o nvcc está
# anormalmente lento nesta máquina (a investigar depois).
# GPU=real: usa o kernel.cu de verdade, compilado com nvcc, com suporte
# a GPU (precisa de nvcc funcionando; roda "make GPU=real").
GPU ?= stub

ifeq ($(GPU),stub)
GPU_OBJS = $(OUT)/kernel_stub.o
LINKER   = $(CXX)
LDFLAGS  = -fopenmp
else
GPU_OBJS = $(OUT)/kernel.o
LINKER   = $(NVCC)
LDFLAGS  = -Xcompiler "-fopenmp"
endif

# Object groups
# dgm.cpp foi dividido por responsabilidade (ver docs/) e lib_shor.cpp
# separou a matemática pura e a construção de circuito da orquestração
# do algoritmo em si.
CORE_OBJS = $(addprefix $(OUT)/, dgm_core.o dgm_parser.o dgm_cpu_exec.o dgm_par_exec.o common.o gates.o lib_general.o lib_shor.o lib_shor_number_theory.o lib_shor_circuits.o lib_grover.o cli_common.o)

.PHONY: all clean shor grover general

all: shor grover general

# executables (phony aliases for the real files under $(OUT))

shor: $(OUT)/shor.out
grover: $(OUT)/grover.out
general: $(OUT)/general.out

$(OUT)/shor.out: $(OUT)/shor.o $(CORE_OBJS) $(GPU_OBJS)
	$(LINKER) -o $@ $^ $(LDFLAGS)

$(OUT)/grover.out: $(OUT)/grover.o $(CORE_OBJS) $(GPU_OBJS)
	$(LINKER) -o $@ $^ $(LDFLAGS)

$(OUT)/general.out: $(OUT)/general.o $(CORE_OBJS) $(GPU_OBJS)
	$(LINKER) -o $@ $^ $(LDFLAGS)

# entry points also need OpenMP at compile time
$(OUT)/shor.o $(OUT)/general.o $(OUT)/grover.o: CXXFLAGS += -fopenmp

# per-file extra flags
$(OUT)/dgm_core.o $(OUT)/dgm_parser.o $(OUT)/dgm_cpu_exec.o $(OUT)/dgm_par_exec.o: CXXFLAGS += -fopenmp -O3 -fcx-limited-range
$(OUT)/kernel.o: NVCCFLAGS += -D OPS_BLOCK=$(OPS_BLOCK) $(KERNEL_OPT)

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
