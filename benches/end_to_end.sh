#!/usr/bin/env bash
# Benchmark ponta a ponta: C++ (tag v1.1.0) x Rust, com as mesmas linhas de comando do
# "Resumo atual" de docs/08. Cada célula é UMA execução, medida por relógio de parede
# (inclui abrir o dispositivo de GPU); Grover e Shor são probabilísticos, então a coluna
# "ok" diz se a execução achou o valor/os fatores. O Shor escolhe o `a` ao acaso: uma
# execução que acha os fatores logo (gcd(a, N) > 1) ou erra cedo é bem mais curta que
# uma que roda a ordem inteira, então os tempos do Shor só se comparam entre execuções
# do mesmo tipo; no circuito (sem emulação) os casos longos são os que valem.
#
# Uso: benches/end_to_end.sh DIR_CPP DIR_RUST [FILTRO]
#   DIR_CPP   raiz do C++ compilado com `make GPU=real` (usa outputs/*.out)
#   DIR_RUST  raiz do workspace, já com `cargo build --release` (usa target/release/*)
#   FILTRO    só roda os casos cujo nome contém este texto (ex.: "grover")
set -uo pipefail

CPP="${1:?uso: end_to_end.sh DIR_CPP DIR_RUST [FILTRO]}"
RUST="${2:?uso: end_to_end.sh DIR_CPP DIR_RUST [FILTRO]}"
FILTER="${3:-}"

# nome | executável | argumentos | variáveis de ambiente
CASES=(
	"shor 17, serial|shor|17 0|"
	"shor 19, serial|shor|19 0|"
	"grover 18, serial|grover|18 0|"
	"general 24, CPU paralela 16 thr|general|24 1 16|"
	"general 26, CPU paralela 16 thr|general|26 1 16|"
	"general 26, GPU|general|26 2 1|"
	"general 26, híbrido|general|26 3 16|"
	"grover 22, CPU paralela 16 thr|grover|22 1 16|"
	"grover 24, GPU|grover|24 2 1|"
	"grover 24, híbrido|grover|24 3 16|"
	"shor 21, CPU paralela 16 thr|shor|21 1 16|"
	"shor 21, GPU|shor|21 2 1|"
	"shor 25, GPU (circuito)|shor|25 2 1|"
	"shor 27, GPU (circuito)|shor|27 2 1|"
	"shor 27, GPU (emulação)|shor|27 2 1|SHOR_EMULATE=1"
	"general 32, CPU paralela 32 thr|general|32 1 32|"
	"general 32, híbrido (não cabe na GPU)|general|32 3 32|"
)

# Roda um comando; imprime "segundos ok" (ok = 1 se saiu com 0 e achou o resultado).
timed() {
	local exe="$1" envs="$2"; shift 2
	local start end out status
	start=$(date +%s.%N)
	if [ -n "$envs" ]; then
		out=$(env "$envs" "$exe" "$@" 2>&1); status=$?
	else
		out=$("$exe" "$@" 2>&1); status=$?
	fi
	end=$(date +%s.%N)
	local ok=1
	[ "$status" -ne 0 ] && ok=0
	case "$exe" in
	*grover*) echo "$out" | grep -q "^Found value:" || ok=0 ;;
	*shor*) echo "$out" | grep -q "^Found factors:" || ok=0 ;;
	esac
	awk -v s="$start" -v e="$end" -v ok="$ok" 'BEGIN { printf "%.2f %s", e - s, ok }'
}

printf "%-40s %10s %4s %10s %4s %7s\n" "caso" "C++ (s)" "ok" "Rust (s)" "ok" "Rust/C++"
for entry in "${CASES[@]}"; do
	IFS='|' read -r name exe args envs <<<"$entry"
	[ -n "$FILTER" ] && [[ "$name" != *"$FILTER"* ]] && continue
	# shellcheck disable=SC2086
	read -r cpp_s cpp_ok <<<"$(timed "$CPP/outputs/$exe.out" "$envs" $args)"
	# shellcheck disable=SC2086
	read -r rs_s rs_ok <<<"$(timed "$RUST/target/release/$exe" "$envs" $args)"
	ratio=$(awk -v c="$cpp_s" -v r="$rs_s" 'BEGIN { if (c > 0) printf "%.2f", r / c; else print "-" }')
	printf "%-40s %10s %4s %10s %4s %7s\n" "$name" "$cpp_s" "$cpp_ok" "$rs_s" "$rs_ok" "$ratio"
done
