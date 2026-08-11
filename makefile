# Build tools
ARCH = -arch=sm_89
NVCC = nvcc $(ARCH)
CXX  = g++

# Build parameters
OPS_BLOCK = 300
INCLUDES  = -Iinclude

# Histórico: kernel.cu chegou a ter ~260 instanciações de template (uma
# por combinação de block_size/repeat_count/coalesced_bits — ver item 06
# em docs/07-bugs-e-pontos-de-atencao.md), e compilar isso em -O3 travava
# por horas numa máquina sem GPU (nvcc/cicc anormalmente lento nela, causa
# nunca diagnosticada). KERNEL_OPT ficava em -O0 por causa disso.
#
# Os templates foram removidos (kernel.cu não instancia mais nada — um
# único kernel, parâmetros de runtime). Com uma GPU NVIDIA real
# disponível, -O3 foi medido (2026-08-11): build em ~6.3s (vs ~6.5s em
# -O0, sem diferença real) e nenhuma diferença mensurável de performance
# de execução no benchmark testado (general.out 22 qubits, t_GPU) — a
# suspeita de lentidão nunca foi sobre volume de templates nem nível de
# otimização nesta máquina. -O3 virou o default. Numa máquina onde o
# `nvcc` for anormalmente lento por qualquer outro motivo (ver item 7 do
# mesmo doc), use "make KERNEL_OPT=-O0" pra manter o ciclo de
# compilar/verificar rápido enquanto investiga.
KERNEL_OPT = -O3

# -MMD -MP: gera um outputs/<arquivo>.d por .o compilado, listando os
# headers que ele incluiu, no formato que o -include no fim deste arquivo
# entende. Sem isso, "make"/"make test" não recompilava nada quando só um
# .h mudava (make dizia "Nothing to be done") -- um jeito fácil de testar
# um binário desatualizado sem perceber.
CXXFLAGS  = $(INCLUDES) -MMD -MP
NVCCFLAGS = $(INCLUDES) -MMD -MP
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

.PHONY: all clean shor grover general test

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

# tests/ não entra no VPATH de propósito (é só um arquivo, e mistura-lo
# na busca de src/core:src/algorithms:src/cli não ganha nada) — regra
# explícita em vez disso. Ver tests/test_qft_addf.cpp, tests/test_gates.cpp
# e tests/smoke_test.sh.
TEST_OBJS = $(addprefix $(OUT)/, gates.o lib_shor_number_theory.o lib_shor_circuits.o)

$(OUT)/test_qft_addf.o: tests/test_qft_addf.cpp | $(OUT)
	$(CXX) $(CXXFLAGS) -c $< -o $@

$(OUT)/test_qft_addf.out: $(OUT)/test_qft_addf.o $(TEST_OBJS)
	$(CXX) -o $@ $^

# test_gates.cpp roda circuitos de verdade via DGM::execute() (t_CPU) --
# só usa o backend t_CPU, mas DGM::execute() tem um switch cobrindo os 4
# backends num mesmo corpo de função, então o linker exige símbolos de
# todos eles (PCpuExecution1/HybridExecution de dgm_par_exec.cpp,
# GpuExecutionWrapper de kernel.o/kernel_stub.o) mesmo sem alcançar esses
# caminhos em runtime.
TEST_GATES_OBJS = $(addprefix $(OUT)/, dgm_core.o dgm_parser.o dgm_cpu_exec.o dgm_par_exec.o common.o gates.o)

$(OUT)/test_gates.o: tests/test_gates.cpp | $(OUT)
	$(CXX) $(CXXFLAGS) -fopenmp -c $< -o $@

$(OUT)/test_gates.out: $(OUT)/test_gates.o $(TEST_GATES_OBJS) $(GPU_OBJS)
	$(LINKER) -o $@ $^ $(LDFLAGS)

test: all $(OUT)/test_qft_addf.out $(OUT)/test_gates.out
	$(OUT)/test_qft_addf.out
	$(OUT)/test_gates.out
	bash tests/smoke_test.sh

clean:
	rm -rf $(OUT)

# Puxa os .d gerados por -MMD -MP (silenciosamente ignorado se ainda não
# existirem, ex: build do zero) -- precisa vir depois de todo o resto pra
# adicionar aos pré-requisitos das regras já definidas, não substituí-las.
-include $(wildcard $(OUT)/*.d)
