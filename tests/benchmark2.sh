#!/bin/bash
set -u

# ==================================================
# Configuração
# ==================================================
LOG="benchmark2.log"
BIN_DIR="outputs"      # pasta onde ficam os executáveis (general.out, shor.out, grover.out)
TIMEOUT="20m"        # tempo máximo por teste; aceita "30s", "1h", etc.
KILL_AFTER="10s"      # tolerância extra antes de forçar SIGKILL

THREADS=16
GPUS=1
BLOCK_SIZE=64
REPEAT_COUNT=2
GPU_REGION_BITS=8

# Ranges dos testes
GENERAL_MIN=15
GENERAL_MAX=30

SHOR_MIN=17
SHOR_MAX=27            # ímpares

GROVER_MIN=15
GROVER_MAX=27

# ==================================================
# Auto-daemonização: se não estiver rodando "destacado" ainda,
# relança a si mesmo com setsid+nohup e sai — assim o benchmark
# continua rodando mesmo se a conexão SSH cair.
# ==================================================
if [ -z "${BENCHMARK_DAEMONIZED:-}" ]; then
    export BENCHMARK_DAEMONIZED=1
    setsid nohup "$0" "$@" > /dev/null 2>&1 < /dev/null &
    disown
    echo "Benchmark iniciado em segundo plano (PID $!)."
    echo "Não depende mais da sua conexão — pode fechar o terminal."
    echo "Acompanhe com: tail -f $LOG"
    exit 0
fi

# A partir daqui já estamos no processo destacado.
START_TS=$(date +%s)
> "$LOG"

log() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" >> "$LOG"
}

TOTAL_TESTS=0
SKIPPED_TESTS=0
FAILED_TESTS=0

run_test() {
    local algorithm="$1"
    local executable="$2"
    local qubits="$3"
    local exec_type="$4"

    TOTAL_TESTS=$((TOTAL_TESTS + 1))

    echo "==================================================" >> "$LOG"
    log "Algoritmo: $algorithm | Qubits: $qubits"

    local bin="$BIN_DIR/$executable"
    local cmd=()
    case "$exec_type" in
        0)
            log "Execução: CPU_SERIAL"
            cmd=("$bin" "$qubits" 0)
            ;;
        1)
            log "Execução: CPU_PARALLEL (threads=$THREADS)"
            cmd=("$bin" "$qubits" 1 "$THREADS")
            ;;
        2)
            log "Execução: GPU (gpus=$GPUS)"
            cmd=("$bin" "$qubits" 2 "$GPUS" "$BLOCK_SIZE" "$REPEAT_COUNT" "$GPU_REGION_BITS")
            ;;
        3)
            log "Execução: HYBRID (threads=$THREADS)"
            cmd=("$bin" "$qubits" 3 "$THREADS" "$BLOCK_SIZE" "$REPEAT_COUNT" "$GPU_REGION_BITS")
            ;;
        *)
            log "exec_type desconhecido: $exec_type — pulando."
            SKIPPED_TESTS=$((SKIPPED_TESTS + 1))
            echo >> "$LOG"
            return
            ;;
    esac

    echo "Comando: timeout -k $KILL_AFTER $TIMEOUT ${cmd[*]}" >> "$LOG"

    local t0=$(date +%s)
    timeout -k "$KILL_AFTER" "$TIMEOUT" "${cmd[@]}" >> "$LOG" 2>&1
    local status=$?
    local t1=$(date +%s)
    local elapsed=$((t1 - t0))

    if [ "$status" -eq 124 ] || [ "$status" -eq 137 ]; then
        log "⚠ PULADO: excedeu $TIMEOUT (rodou ${elapsed}s antes de ser cancelado)"
        SKIPPED_TESTS=$((SKIPPED_TESTS + 1))
    elif [ "$status" -ne 0 ]; then
        log "✗ FALHOU: código de saída $status (${elapsed}s)"
        FAILED_TESTS=$((FAILED_TESTS + 1))
    else
        log "✓ OK (${elapsed}s)"
    fi

    echo >> "$LOG"
}

log "===== Benchmark iniciado ====="
log "Timeout por teste: $TIMEOUT (kill forçado após +$KILL_AFTER)"

# Checagem rápida: os binários existem antes de começar a bateria inteira?
for exe in general.out shor.out grover.out; do
    if [ ! -x "$BIN_DIR/$exe" ]; then
        log "✗ ERRO FATAL: $BIN_DIR/$exe não encontrado ou não executável. Rode 'make' antes. Abortando."
        exit 1
    fi
done

# ==================================================
# GENERAL — CPU paralela, GPU e híbrido
# ==================================================
log "### Bateria GENERAL ($GENERAL_MIN a $GENERAL_MAX qubits) ###"
for ((q=GENERAL_MIN; q<=GENERAL_MAX; q++)); do
    run_test "GENERAL" "general.out" "$q" 1
    run_test "GENERAL" "general.out" "$q" 2
    run_test "GENERAL" "general.out" "$q" 3
done

# ==================================================
# SHOR — CPU serial, CPU paralela, GPU e híbrido (só ímpares)
# ==================================================
log "### Bateria SHOR ($SHOR_MIN a $SHOR_MAX qubits, ímpares) ###"
for ((q=SHOR_MIN; q<=SHOR_MAX; q+=2)); do
    run_test "SHOR" "shor.out" "$q" 0
    run_test "SHOR" "shor.out" "$q" 1
    run_test "SHOR" "shor.out" "$q" 2
    run_test "SHOR" "shor.out" "$q" 3
done

# ==================================================
# GROVER — CPU serial, CPU paralela, GPU e híbrido
# ==================================================
log "### Bateria GROVER ($GROVER_MIN a $GROVER_MAX qubits) ###"
for ((q=GROVER_MIN; q<=GROVER_MAX; q++)); do
    run_test "GROVER" "grover.out" "$q" 0
    run_test "GROVER" "grover.out" "$q" 1
    run_test "GROVER" "grover.out" "$q" 2
    run_test "GROVER" "grover.out" "$q" 3
done

END_TS=$(date +%s)
TOTAL_ELAPSED=$((END_TS - START_TS))

echo "==================================================" >> "$LOG"
log "Benchmark concluído."
log "Total de testes: $TOTAL_TESTS | Pulados (timeout): $SKIPPED_TESTS | Falharam: $FAILED_TESTS"
log "Tempo total: $((TOTAL_ELAPSED / 3600))h $(((TOTAL_ELAPSED % 3600) / 60))m $((TOTAL_ELAPSED % 60))s"