# Alvos extras do makefile do C++ para regenerar os vetores golden. Não é usado na
# raiz (o C++ saiu da main); `regenerar.sh` o inclui num checkout do tag v1.1.0:
#   make GPU=stub -f makefile -f golden.mk golden golden-runs

# Vetores de referencia para a migracao em Rust (docs/09, fase 1): roda o
# t_CPU e grava tests/golden/data (formato em tests/golden/README.md).
$(OUT)/export_golden.o: tests/golden/export_golden.cpp | $(OUT)
	$(CXX) $(CXXFLAGS) -fopenmp -c $< -o $@

$(OUT)/export_golden.out: $(OUT)/export_golden.o $(CORE_OBJS) $(GPU_OBJS)
	$(LINKER) -o $@ $^ $(LDFLAGS)

golden: $(OUT)/export_golden.out
	$(OUT)/export_golden.out tests/golden/data

# Execucoes completas (Grover/Shor) com semente fixa, para a fase 5: grava
# tests/golden/data/runs.txt (formato no cabecalho de tests/golden/export_runs.cpp).
$(OUT)/export_runs.o: tests/golden/export_runs.cpp | $(OUT)
	$(CXX) $(CXXFLAGS) -fopenmp -c $< -o $@

$(OUT)/export_runs.out: $(OUT)/export_runs.o $(CORE_OBJS) $(GPU_OBJS)
	$(LINKER) -o $@ $^ $(LDFLAGS)

golden-runs: $(OUT)/export_runs.out
	$(OUT)/export_runs.out tests/golden/data/runs.txt
