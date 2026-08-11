#!/usr/bin/env bash
# Testes de regressão ponta a ponta: roda os executáveis já buildados em
# outputs/ com poucos qubits e confere o resultado esperado. Pega a
# maioria das regressões de lógica/paralelismo (região, RAII, tuning) sem
# precisar de GPU real. Detecta em runtime se o build tem kernel.cu de
# verdade por trás ("make GPU=real") ou o stub ("make GPU=stub", padrão) —
# sob GPU=real, os backends t_GPU/t_HYBRID de general.out são conferidos
# com o mesmo valor exato do t_PAR_CPU, e grover.out/shor.out por taxa de
# sucesso ao longo de vários runs (não só "não crasha" — isso sozinho não
# pegaria uma GPU calculando resultado errado de forma consistente).
#
# Uso: make test (builda antes de rodar) ou ./tests/smoke_test.sh direto
# se outputs/ já estiver atualizado.
set -uo pipefail
cd "$(dirname "$0")/.."
BIN=outputs
FAIL=0

check_amplitude(){
	local label="$1" got="$2" expected="$3"
	if [ -z "$got" ]; then
		echo "FAIL $label (sem saída)"
		FAIL=1
		return
	fi
	awk -v g="$got" -v e="$expected" 'BEGIN{ d=g-e; if (d<0) d=-d; exit !(d<0.0001) }'
	if [ $? -eq 0 ]; then
		echo "OK   $label ($got ~ $expected)"
	else
		echo "FAIL $label ($got != $expected)"
		FAIL=1
	fi
}

check_no_crash(){
	local label="$1"; shift
	if "$@" >/dev/null 2>&1; then
		echo "OK   $label (sem crash)"
	else
		echo "FAIL $label (saiu com erro)"
		FAIL=1
	fi
}

# Roda "$@" $runs vezes, conta quantas vezes a saída bate com $pattern
# (grep -q), exige pelo menos $min_successes acertos. Pensado pra
# algoritmos probabilísticos (Grover/Shor) onde uma única rodada não
# prova nada — "sem crash" sozinho também não pega uma GPU calculando
# bobagem sistematicamente (correria "sem crash" 100% das vezes mesmo
# assim). $min_successes bem abaixo da taxa observada manualmente evita
# flakiness: Grover deu 30/30 em testes manuais (t_CPU/t_GPU/t_HYBRID,
# ver docs/07-bugs-e-pontos-de-atencao.md item 12), Shor deu bem menos
# (2-6 de 8, ver item 7) -- os limiares abaixo refletem essa diferença.
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

echo "-- general.out (t_PAR_CPU): H em N qubits, amplitude uniforme 1/sqrt(2^N) --"
for pair in "10 1 2:0.03125" "10 1 4:0.03125" "14 1 2:0.0078125" "18 1 4:0.001953125"; do
	combo="${pair%%:*}"
	expected="${pair##*:}"
	amp=$("$BIN/general.out" $combo 2>/dev/null | head -1 | awk '{print $2}')
	check_amplitude "general.out $combo" "$amp" "$expected"
done

# kernel_stub.cpp (usado por "make GPU=stub") avisa em stderr toda vez que
# um backend t_GPU/t_HYBRID é chamado, sem tocar o estado -- é assim que
# detectamos, em runtime, se este build tem um kernel.cu de verdade por
# trás (GPU=real) ou o stub (GPU=stub), sem precisar de nenhum artefato
# de build à parte. Ver src/core/kernel_stub.cpp.
gpu_stderr=$("$BIN/general.out" 10 2 1 2>&1 1>/dev/null)
if [[ "$gpu_stderr" == *"kernel_stub.cpp"* ]]; then
	echo "-- GPU=stub detectado: t_GPU/t_HYBRID só confere que não crasham --"
	check_no_crash "general.out 10 2 1 (t_GPU)" "$BIN/general.out" 10 2 1
	check_no_crash "general.out 10 3 2 (t_HYBRID)" "$BIN/general.out" 10 3 2
else
	echo "-- GPU=real detectado: t_GPU/t_HYBRID conferidos com o mesmo valor exato do t_PAR_CPU --"
	for pair in "10 2 1:0.03125" "14 2 1:0.0078125" "18 2 1:0.001953125"; do
		combo="${pair%%:*}"
		expected="${pair##*:}"
		amp=$("$BIN/general.out" $combo 2>/dev/null | head -1 | awk '{print $2}')
		check_amplitude "general.out $combo (t_GPU)" "$amp" "$expected"
	done
	for pair in "10 3 2:0.03125" "14 3 2:0.0078125"; do
		combo="${pair%%:*}"
		expected="${pair##*:}"
		amp=$("$BIN/general.out" $combo 2>/dev/null | head -1 | awk '{print $2}')
		check_amplitude "general.out $combo (t_HYBRID)" "$amp" "$expected"
	done
fi

echo "-- grover.out: taxa de sucesso (achar search_value) em vários runs, não só 'sem crash' --"
check_success_rate "grover.out 12 0 (t_CPU)" "^Found value:" 10 7 "$BIN/grover.out" 12 0
check_success_rate "grover.out 12 1 2 (t_PAR_CPU)" "^Found value:" 10 7 "$BIN/grover.out" 12 1 2

echo "-- shor.out: pelo menos 1 sucesso em 8 runs -- fraco de propósito (algoritmo probabilístico, taxa historicamente baixa), só pra pegar uma GPU calculando errado sistematicamente (0/8) --"
check_success_rate "shor.out 15 0 (t_CPU)" "^Found factors:" 8 1 "$BIN/shor.out" 15 0

if [[ "$gpu_stderr" == *"kernel_stub.cpp"* ]]; then
	check_no_crash "grover.out 12 2 1 (t_GPU)" "$BIN/grover.out" 12 2 1
	check_no_crash "grover.out 12 3 2 (t_HYBRID)" "$BIN/grover.out" 12 3 2
	check_no_crash "shor.out 15 2 1 (t_GPU)" "$BIN/shor.out" 15 2 1
	check_no_crash "shor.out 15 3 2 (t_HYBRID)" "$BIN/shor.out" 15 3 2
else
	check_success_rate "grover.out 12 2 1 (t_GPU)" "^Found value:" 10 7 "$BIN/grover.out" 12 2 1
	check_success_rate "grover.out 12 3 2 (t_HYBRID)" "^Found value:" 10 7 "$BIN/grover.out" 12 3 2
	check_success_rate "shor.out 15 2 1 (t_GPU)" "^Found factors:" 8 1 "$BIN/shor.out" 15 2 1
	check_success_rate "shor.out 15 3 2 (t_HYBRID)" "^Found factors:" 8 1 "$BIN/shor.out" 15 3 2
fi

exit $FAIL
