#!/usr/bin/env bash
# Baseline do C++/CUDA (tag v1.1.0) numa máquina NVIDIA: roda os tempos de
# docs/08 e grava um CSV, que depois serve de referência para comparar com o
# Rust (critérios 2 e 3 de docs/09). Não serve para GPU de outro fornecedor: o
# C++ só tem CUDA.
#
# Pré-requisito: um checkout do tag v1.1.0 compilado com "make GPU=real" (o C++ não está
# mais na main); aponte para ele com CPP_DIR (usa $CPP_DIR/outputs). Uso, a partir da raiz:
#   tests/gpu-battery/run-baseline-cpp.sh [pasta_de_saida]
# Variáveis: REPS (padrão 3 repetições por caso), QUICK=1 (só casos pequenos,
# para conferir que o script funciona), THREADS (padrão: nproc).
set -u
cd "$(dirname "$0")/../.."

BIN="${CPP_DIR:-.}/outputs"
OUT="${1:-tests/gpu-battery/resultados/baseline-cpp-$(date +%Y%m%d-%H%M%S)}"
REPS="${REPS:-3}"
THREADS="${THREADS:-$(nproc 2>/dev/null || echo 4)}"
QUICK="${QUICK:-0}"
mkdir -p "$OUT"
CSV="$OUT/tempos.csv"
echo "caso,backend,repeticao,segundos,saida_ok" > "$CSV"

tests/gpu-battery/coleta-ambiente.sh > "$OUT/ambiente.md" 2>&1

for exe in general shor grover; do
	if [ ! -x "$BIN/$exe.out" ]; then
		echo "ERRO: $BIN/$exe.out não existe; rode 'make GPU=real' no checkout do tag (CPP_DIR)." >&2
		exit 1
	fi
done

# Memória livre da GPU em MiB (vazio se não der para saber).
free_mib(){ nvidia-smi --query-gpu=memory.free --format=csv,noheader,nounits 2>/dev/null | head -1 | tr -d ' \r'; }

# fits <qubits> <fator_de_buffers>: 0 se o estado (2^q * 8 bytes * fator) + 256 MiB
# cabe na memória livre; se não der para consultar, assume que cabe.
fits(){
	local q="$1" factor="$2" free need
	[ "$factor" -eq 0 ] && return 0	# caso de CPU: não depende da memória da GPU
	free="$(free_mib)"
	[ -z "$free" ] && return 0
	need=$(( (1 << q) * 8 * factor / 1048576 + 256 ))
	[ "$free" -ge "$need" ]
}

# run_case <caso> <backend> <qubits> <fator> <VAR=valor ou -> <comando...>
# O tempo vem da linha "Time: x" (shor) ou da primeira linha só com número
# (general, grover). O caso é marcado como ok se o programa saiu com 0 e deu tempo.
run_case(){
	local label="$1" backend="$2" q="$3" factor="$4" envset="$5"; shift 5
	if ! fits "$q" "$factor"; then
		echo "$label,$backend,-,PULADO (memoria),0" >> "$CSV"
		echo "pulado: $label ($backend), não cabe na memória livre da GPU"
		return
	fi
	local rep out secs status
	for rep in $(seq 1 "$REPS"); do
		if [ "$envset" = "-" ]; then
			out="$("$@" 2>&1)"; status=$?
		else
			out="$(env "$envset" "$@" 2>&1)"; status=$?
		fi
		secs="$(printf '%s\n' "$out" | awk '/^Time:/{print $2; exit} /^[0-9]+(\.[0-9]+)?([eE][-+]?[0-9]+)?$/{print $1; exit}')"
		if [ "$status" -eq 0 ] && [ -n "$secs" ]; then ok=1; else ok=0; fi
		echo "$label,$backend,$rep,${secs:--},$ok" >> "$CSV"
		echo "$label ($backend) #$rep: ${secs:--} s (saída $status)"
	done
}

# 1. Corretude: a mesma suíte que o CI roda com GPU real.
echo "== make test GPU=real =="
make test GPU=real > "$OUT/make-test.log" 2>&1
echo "make test: saída $? (ver $OUT/make-test.log)"
grep -c '^OK' "$OUT/make-test.log" | sed 's/^/linhas OK: /'
grep '^FAIL' "$OUT/make-test.log" | head

# 2. Tempos. t_GPU = 2, t_PAR_CPU = 1. general.out e grover.out: <qubits> <tipo> <threads|gpus>.
if [ "$QUICK" = "1" ]; then
	run_case general24 gpu 24 1 - $BIN/general.out 24 2 1
	run_case shor21 gpu 21 1 - $BIN/shor.out 21 2 1
	run_case grover22 gpu 22 1 - $BIN/grover.out 22 2 1
else
	run_case general24 cpu_par 24 0 - $BIN/general.out 24 1 "$THREADS"
	run_case general26 cpu_par 26 0 - $BIN/general.out 26 1 "$THREADS"
	run_case general26 gpu 26 1 - $BIN/general.out 26 2 1
	run_case general26 hibrido 26 1 - $BIN/general.out 26 3 1
	run_case grover24 gpu 24 1 - $BIN/grover.out 24 2 1
	run_case shor21 cpu_par 21 0 - $BIN/shor.out 21 1 "$THREADS"
	run_case shor21 gpu 21 1 - $BIN/shor.out 21 2 1
	run_case shor25 gpu 25 1 - $BIN/shor.out 25 2 1
	run_case shor27 gpu 27 1 - $BIN/shor.out 27 2 1
	# emulação da multiplicação modular (o buffer auxiliar dobra a memória)
	run_case shor25_emul gpu 25 2 SHOR_EMULATE=1 $BIN/shor.out 25 2 1
	run_case shor27_emul gpu 27 2 SHOR_EMULATE=1 $BIN/shor.out 27 2 1
fi

echo
echo "Resultados em $OUT (tempos.csv, ambiente.md, make-test.log)"
