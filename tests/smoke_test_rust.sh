#!/usr/bin/env bash
# Smoke test dos executáveis Rust (fase 5 de docs/09): a parte de CPU de
# tests/smoke_test.sh, com as mesmas amplitudes e os mesmos limiares de taxa de
# sucesso, aplicada a general/grover/shor de crates/hgm-cli. A parte de t_GPU só
# roda se houver adaptador (HGM_REQUIRE_GPU=1 transforma a falta dele em falha). O
# t_HYBRID roda sempre: sem adaptador cai na CPU paralela, e o resultado é o mesmo.
#
# Uso: tests/smoke_test_rust.sh   (compila em --release antes de rodar)
set -uo pipefail
cd "$(dirname "$0")/.."
FAIL=0

cargo build --release -p hgm-cli || { echo "FAIL cargo build"; exit 1; }
BIN=target/release

check_amplitude(){
	local label="$1" got="$2" expected="$3"
	if [ -z "$got" ]; then
		echo "FAIL $label (sem saída)"
		FAIL=1
		return
	fi
	# 6 casas decimais na saída: erro de arredondamento <= 5e-7; 2e-6 dá folga.
	awk -v g="$got" -v e="$expected" 'BEGIN{ d=g-e; if (d<0) d=-d; exit !(d<0.000002) }'
	if [ $? -eq 0 ]; then
		echo "OK   $label ($got ~ $expected)"
	else
		echo "FAIL $label ($got != $expected)"
		FAIL=1
	fi
}

# Roda "$@" $runs vezes e exige pelo menos $min_successes saídas que casem com
# $pattern. Grover/Shor são probabilísticos: uma rodada não prova nada.
check_success_rate(){
	local label="$1" pattern="$2" runs="$3" min_successes="$4"; shift 4
	local successes=0
	for i in $(seq 1 "$runs"); do
		if "$@" 2>/dev/null | grep -q "$pattern"; then
			successes=$((successes+1))
		fi
	done
	if [ "$successes" -ge "$min_successes" ]; then
		echo "OK   $label ($successes/$runs >= $min_successes/$runs esperado)"
	else
		echo "FAIL $label ($successes/$runs < $min_successes/$runs esperado)"
		FAIL=1
	fi
}

check_refused(){
	local label="$1"; shift
	if "$@" >/dev/null 2>&1; then
		echo "FAIL $label (deveria recusar)"
		FAIL=1
	else
		echo "OK   $label (recusado, como esperado)"
	fi
}

echo "-- general: H em N qubits, amplitude uniforme 1/sqrt(2^N) --"
for pair in "10 1 2:0.03125" "10 1 4:0.03125" "14 1 2:0.0078125" "18 1 4:0.001953125" "10 0:0.03125"; do
	combo="${pair%%:*}"
	expected="${pair##*:}"
	amp=$("$BIN/general" $combo 2>/dev/null | head -1 | awk '{print $2}')
	check_amplitude "general $combo" "$amp" "$expected"
done

echo "-- grover: taxa de sucesso (achar search_value) em vários runs --"
check_success_rate "grover 12 0 (CPU serial)" "^Found value:" 10 7 "$BIN/grover" 12 0
check_success_rate "grover 12 1 2 (CPU paralela)" "^Found value:" 10 7 "$BIN/grover" 12 1 2

echo "-- shor: pelo menos 1 sucesso em 16 runs (fraco de propósito: algoritmo probabilístico) --"
check_success_rate "shor 15 0 (CPU serial)" "^Found factors:" 16 1 "$BIN/shor" 15 0
check_success_rate "shor 15 0 (CPU serial, SHOR_EMULATE=1)" "^Found factors:" 16 1 env SHOR_EMULATE=1 "$BIN/shor" 15 0
check_success_rate "shor 15 1 2 (CPU paralela, SHOR_EMULATE=1)" "^Found factors:" 16 1 env SHOR_EMULATE=1 "$BIN/shor" 15 1 2

echo "-- GPU (t_GPU): só com adaptador; HGM_REQUIRE_GPU=1 exige --"
if "$BIN/general" 10 2 1 >/dev/null 2>&1; then
	for pair in "10 2 1:0.03125" "14 2 1:0.0078125" "18 2 1:0.001953125"; do
		combo="${pair%%:*}"
		expected="${pair##*:}"
		amp=$("$BIN/general" $combo 2>/dev/null | head -1 | awk '{print $2}')
		check_amplitude "general $combo (t_GPU)" "$amp" "$expected"
	done
	# Menos rodadas que na CPU: cada processo abre o dispositivo (em adaptador de software
	# isso leva segundos).
	check_success_rate "grover 12 2 (t_GPU)" "^Found value:" 8 5 "$BIN/grover" 12 2
	check_success_rate "shor 15 2 (t_GPU)" "^Found factors:" 8 1 "$BIN/shor" 15 2
elif [ -n "${HGM_REQUIRE_GPU:-}" ]; then
	echo "FAIL t_GPU (HGM_REQUIRE_GPU definida, mas general 10 2 1 não rodou)"
	FAIL=1
else
	echo "SKIP t_GPU (sem adaptador de GPU utilizável; não é verde de verdade, ver HGM_REQUIRE_GPU)"
fi

echo "-- híbrido (t_HYBRID): a GPU se há adaptador e o estado cabe, senão a CPU paralela --"
for pair in "10 3 2:0.03125" "14 3 2:0.0078125"; do
	combo="${pair%%:*}"
	expected="${pair##*:}"
	amp=$("$BIN/general" $combo 2>/dev/null | head -1 | awk '{print $2}')
	check_amplitude "general $combo (t_HYBRID)" "$amp" "$expected"
done
check_success_rate "grover 12 3 2 (t_HYBRID)" "^Found value:" 8 5 "$BIN/grover" 12 3 2

echo "-- entradas inválidas: código de saída diferente de zero --"
check_refused "grover 8 0 1 1 1 1 999 (search_value fora do espaço)" "$BIN/grover" 8 0 1 1 1 1 999
check_refused "shor 16 (qubits sem número mapeado)" "$BIN/shor" 16
check_refused "general 40 1 (qubits acima do limite)" "$BIN/general" 40 1

exit $FAIL
