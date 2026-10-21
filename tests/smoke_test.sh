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
	# A saída tem 6 casas decimais, então o erro de arredondamento é <= 5e-7;
	# 2e-6 deixa folga pro float e continua distinguindo amplitudes a partir
	# de ~22 qubits (1/sqrt(2^22) ~ 4,9e-4) de zero ou do valor vizinho.
	awk -v g="$got" -v e="$expected" 'BEGIN{ d=g-e; if (d<0) d=-d; exit !(d<0.000002) }'
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
	# Menos qubits que a região de GPU padrão (12 bits): validateTuning()
	# precisa encolher block_size/repeat_count junto, senão abortava com
	# "Erro de tuning" (item 22 em docs/07-bugs-e-pontos-de-atencao.md).
	for pair in "4 2 1:0.25" "6 2 1:0.125" "8 2 1:0.0625" "11 2 1:0.02209709"; do
		combo="${pair%%:*}"
		expected="${pair##*:}"
		amp=$("$BIN/general.out" $combo 2>/dev/null | head -1 | awk '{print $2}')
		check_amplitude "general.out $combo (t_GPU, menos qubits que a região)" "$amp" "$expected"
	done
	check_success_rate "grover.out 6 2 1 (t_GPU, menos qubits que a região)" "^Found value:" 10 7 "$BIN/grover.out" 6 2 1
	for pair in "10 3 2:0.03125" "14 3 2:0.0078125"; do
		combo="${pair%%:*}"
		expected="${pair##*:}"
		amp=$("$BIN/general.out" $combo 2>/dev/null | head -1 | awk '{print $2}')
		check_amplitude "general.out $combo (t_HYBRID)" "$amp" "$expected"
	done

	# Os casos acima (<= 20 qubits) caem no desvio "região única -> GPU"
	# de DGM::HybridExecution e nunca passam pelo modo de regiões CPU+GPU.
	# Estes cobrem os outros dois caminhos (item 17, tentativas 5-7 em
	# docs/07-bugs-e-pontos-de-atencao.md):
	#  - 22 e 24 qubits sem variável: o estado cabe na GPU, é delegado ao t_GPU
	#  - HYBRID_FORCE_REGIONS=1: força o modo de regiões (ProjectState/
	#    GetState, memória travada, regra de custo da CPU)
	echo "-- t_HYBRID acima de 20 qubits: delegação (estado cabe na GPU) e modo de regiões forçado --"
	for pair in "22 3 4:0.00048828125" "24 3 4:0.000244140625"; do
		combo="${pair%%:*}"
		expected="${pair##*:}"
		amp=$("$BIN/general.out" $combo 2>/dev/null | head -1 | awk '{print $2}')
		check_amplitude "general.out $combo (t_HYBRID, delegado à GPU)" "$amp" "$expected"
		amp=$(HYBRID_FORCE_REGIONS=1 "$BIN/general.out" $combo 2>/dev/null | head -1 | awk '{print $2}')
		check_amplitude "general.out $combo (t_HYBRID, modo de regiões)" "$amp" "$expected"
	done
fi

echo "-- grover.out: taxa de sucesso (achar search_value) em vários runs, não só 'sem crash' --"
check_success_rate "grover.out 12 0 (t_CPU)" "^Found value:" 10 7 "$BIN/grover.out" 12 0
check_success_rate "grover.out 12 1 2 (t_PAR_CPU)" "^Found value:" 10 7 "$BIN/grover.out" 12 1 2

echo "-- shor.out: pelo menos 1 sucesso em 16 runs -- fraco de propósito (algoritmo probabilístico, taxa historicamente baixa e variável, ver docs/07-bugs-e-pontos-de-atencao.md itens 1 e 21), só pra pegar uma GPU calculando errado sistematicamente (0/16). 16 runs (não 8) porque com taxa de sucesso baixa até uma sequência de tentativas genuinamente independentes pode dar 0 sucessos por acaso — 16 reduz esse risco de falso alarme sem alongar demais o teste (ver srand(time(NULL)^getpid()) em shor.cpp: sem o getpid(), chamadas rápidas em sequência dentro do mesmo segundo repetiam a mesma semente, o que fazia os N runs não serem realmente independentes) --"
check_success_rate "shor.out 15 0 (t_CPU)" "^Found factors:" 16 1 "$BIN/shor.out" 15 0

if [[ "$gpu_stderr" == *"kernel_stub.cpp"* ]]; then
	check_no_crash "grover.out 12 2 1 (t_GPU)" "$BIN/grover.out" 12 2 1
	check_no_crash "grover.out 12 3 2 (t_HYBRID)" "$BIN/grover.out" 12 3 2
	check_no_crash "shor.out 15 2 1 (t_GPU)" "$BIN/shor.out" 15 2 1
	check_no_crash "shor.out 15 3 2 (t_HYBRID)" "$BIN/shor.out" 15 3 2
else
	check_success_rate "grover.out 12 2 1 (t_GPU)" "^Found value:" 10 7 "$BIN/grover.out" 12 2 1
	check_success_rate "grover.out 12 3 2 (t_HYBRID)" "^Found value:" 10 7 "$BIN/grover.out" 12 3 2
	check_success_rate "shor.out 15 2 1 (t_GPU)" "^Found factors:" 16 1 "$BIN/shor.out" 15 2 1
	check_success_rate "shor.out 15 3 2 (t_HYBRID)" "^Found factors:" 16 1 "$BIN/shor.out" 15 3 2

	# Decisão "o estado cabe na GPU?" com a GPU parcialmente ocupada
	# (DGM::HybridExecution: GpuMinFreeBytes + 512 MB de margem). gpu_hog.out
	# ocupa a memória da GPU menos N MB e segura até ser morto (ou 300 s).
	# 24 qubits = 128 MB de estado, precisa de 640 MB livres pra delegar:
	# com 450 MB livres tem que cair no modo de regiões, com 3000 MB tem que
	# delegar à GPU. O contexto CUDA do próprio processo também consome
	# memória, por isso as margens são folgadas.
	if [ -x "$BIN/gpu_hog.out" ]; then
		echo "-- t_HYBRID com a GPU parcialmente ocupada: a decisão cabe/não cabe segue a memória livre --"
		for pair in "450:não cabe" "3000:cabe"; do
			leave_mb="${pair%%:*}"
			expected="${pair##*:}"
			hog_log=$(mktemp)
			"$BIN/gpu_hog.out" "$leave_mb" > "$hog_log" 2>&1 &
			hog_pid=$!
			for _ in $(seq 1 50); do grep -q "^pronto" "$hog_log" && break; sleep 0.2; done
			if ! grep -q "^pronto" "$hog_log"; then
				echo "FAIL gpu_hog.out $leave_mb (não ocupou a memória: $(head -c 200 "$hog_log"))"
				FAIL=1
			else
				out=$(HYBRID_DEBUG=1 "$BIN/general.out" 24 3 4 2>&1)
				decision=$(echo "$out" | grep -a "memória livre da GPU" | head -1 | sed 's/.*precisa de [0-9]*: //')
				amp=$(echo "$out" | grep -a "^0:" | head -1 | awk '{print $2}')
				if [ "$decision" = "$expected" ]; then
					echo "OK   t_HYBRID 24 qubits com ~${leave_mb} MB livres: decisão \"$decision\""
				else
					echo "FAIL t_HYBRID 24 qubits com ~${leave_mb} MB livres: esperava \"$expected\", saiu \"$decision\""
					FAIL=1
				fi
				check_amplitude "general.out 24 3 4 com ~${leave_mb} MB livres na GPU" "$amp" "0.000244140625"
			fi
			kill "$hog_pid" 2>/dev/null
			wait "$hog_pid" 2>/dev/null
			rm -f "$hog_log"
		done
	fi

	# Modo de regiões de verdade (21 qubits = 2 regiões globais por passada):
	# Grover confere o resultado; Shor roda centenas de HybridExecution
	# seguidas (alloc/pin/free de GPU a cada chamada) e só confere que não
	# quebra -- a taxa de sucesso do Shor é baixa demais pra servir de teste
	# em poucas rodadas.
	check_success_rate "grover.out 21 3 4 (t_HYBRID, modo de regiões)" "^Found value:" 3 2 env HYBRID_FORCE_REGIONS=1 "$BIN/grover.out" 21 3 4
	check_no_crash "shor.out 21 3 4 (t_HYBRID, modo de regiões)" env HYBRID_FORCE_REGIONS=1 "$BIN/shor.out" 21 3 4
fi

exit $FAIL
