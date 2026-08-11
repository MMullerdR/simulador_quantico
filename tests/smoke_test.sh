#!/usr/bin/env bash
# Testes de regressão ponta a ponta: roda os executáveis já buildados em
# outputs/ com poucos qubits e confere o resultado esperado. Pega a
# maioria das regressões de lógica/paralelismo (região, RAII, tuning) sem
# precisar de GPU real. Detecta em runtime se o build tem kernel.cu de
# verdade por trás ("make GPU=real") ou o stub ("make GPU=stub", padrão) —
# sob GPU=real, os backends t_GPU/t_HYBRID de general.out são conferidos
# com o mesmo valor exato do t_PAR_CPU, não só "não crasha".
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

echo "-- grover.out/shor.out em t_GPU/t_HYBRID: saída não é um valor determinístico verificável (tempo/probabilístico) -- só confere que não crasham, nos dois modos de GPU --"
check_no_crash "grover.out 12 1 2 (t_PAR_CPU)" "$BIN/grover.out" 12 1 2
check_no_crash "grover.out 12 2 1 (t_GPU)" "$BIN/grover.out" 12 2 1
check_no_crash "grover.out 12 3 2 (t_HYBRID)" "$BIN/grover.out" 12 3 2
check_no_crash "shor.out 15 0 (t_CPU)" "$BIN/shor.out" 15 0
check_no_crash "shor.out 15 2 1 (t_GPU)" "$BIN/shor.out" 15 2 1
check_no_crash "shor.out 15 3 2 (t_HYBRID)" "$BIN/shor.out" 15 3 2

exit $FAIL
