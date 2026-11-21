//! Paridade dos backends de CPU com o C++ (`tests/golden`, casos `kind state`).
//!
//! Aplica os passos do caso (repetidos `iterations` vezes) ao estado inicial e
//! compara com o estado final que o `t_CPU` do tag v1.1.0 produziu. Tolerância de
//! 1e-5 no módulo da diferença, a mesma sugerida em `tests/golden/README.md` (o
//! C++ e o simulador independente em Python já diferem de até ~5e-6).
//!
//! Roda o backend serial e o paralelo (com vários ajustes de região/threads), e
//! cada um também com o programa otimizado (fusão de fase + agendamento).

use hgm_core::circuit::compile_steps;
use hgm_core::optimize::{fuse_phase_runs, schedule_for_batching};
use hgm_core::{Backend, GateCatalog, StateVector};
use hgm_cpu::{ParallelBackend, SerialBackend, Tuning};
use hgm_golden::{cases_of_kind, read_bytes, Case};

const STATE_TOLERANCE: f32 = 1e-5;
/// Com fusão de fase, as rotações passam a vir da tabela de `2^16` passos em vez
/// das matrizes em `float`: cada porta fundida difere em ~1e-7; o erro soma-se.
const OPTIMIZED_TOLERANCE: f32 = 1e-4;

#[derive(Clone, Copy)]
enum Mode {
    Plain,
    /// `FusePhaseRuns` + `ScheduleForBatching(3, 5, 50)`.
    Optimized,
}

fn initial_state(case: &Case, qubits: usize) -> Result<StateVector, String> {
    let init = case
        .get("init")
        .ok_or_else(|| format!("{}: falta `init`", case.name))?;
    if let Some(index) = init.strip_prefix("basis ") {
        let index: usize = index
            .trim()
            .parse()
            .map_err(|e| format!("{}: `init basis` inválido: {e}", case.name))?;
        StateVector::basis(qubits, index).map_err(|e| e.to_string())
    } else {
        let bytes = read_bytes(&case.file_field("init")?)?;
        StateVector::from_f32_le_bytes(qubits, &bytes).map_err(|e| e.to_string())
    }
}

fn check_case(case: &Case, mode: Mode, backend: &mut dyn Backend) -> Result<f32, String> {
    let qubits = case.get_usize("qubits")?;
    let iterations = case.get_usize("iterations")?;

    let mut catalog = GateCatalog::with_base_gates();
    for (name, matrix) in &case.gates {
        catalog.add_gate(name, *matrix);
    }

    let mut program = compile_steps(case.block("steps"), 1, &catalog).map_err(|e| e.to_string())?;
    let tolerance = match mode {
        Mode::Plain => STATE_TOLERANCE,
        Mode::Optimized => {
            let (fused, _) = fuse_phase_runs(program.ops);
            program.ops = schedule_for_batching(fused, 3, 5, 50);
            OPTIMIZED_TOLERANCE
        }
    };
    let mut state = initial_state(case, qubits)?;
    backend
        .execute(&mut state, &program, iterations as u32)
        .map_err(|e| e.to_string())?;

    let expected_bytes = read_bytes(&case.file_field("final")?)?;
    let expected =
        StateVector::from_f32_le_bytes(qubits, &expected_bytes).map_err(|e| e.to_string())?;

    let diff = state.max_abs_diff(&expected);
    if diff < tolerance {
        Ok(diff)
    } else {
        Err(format!("máx. |diferença| {diff:e} >= {tolerance:e}"))
    }
}

fn run_all(mode: Mode, label: &str, backend: &mut dyn Backend) {
    let cases = cases_of_kind("state").expect("ler os casos state");
    assert_eq!(cases.len(), 144, "o INDEX deveria ter 144 casos state");

    let mut worst = 0.0f32;
    let mut failures = Vec::new();
    for case in &cases {
        match check_case(case, mode, backend) {
            Ok(diff) => worst = worst.max(diff),
            Err(message) => failures.push(format!("{}: {message}", case.name)),
        }
    }

    println!(
        "state ({label}): {} casos, pior diferença {worst:e}",
        cases.len()
    );
    assert!(failures.is_empty(), "falhas:\n{}", failures.join("\n"));
}

#[test]
fn serial_backend_matches_cpp_golden_states() {
    run_all(Mode::Plain, "serial", &mut SerialBackend::new());
}

#[test]
fn fused_and_scheduled_program_matches_cpp_golden_states() {
    run_all(
        Mode::Optimized,
        "serial, fusão + agendamento",
        &mut SerialBackend::new(),
    );
}

/// Ajustes de (threads, bits coalescidos, bits da região), do tipo que o
/// `test_cpu_parallel` do C++ exercita: região menor que o circuito, região igual
/// ao circuito, coalescida 0, coalescida colada na região, 1 e vários threads.
const TUNINGS: [(usize, u32, u32); 7] = [
    (1, 0, 3),
    (2, 2, 4),
    (4, 3, 5),
    (8, 4, 6),
    (3, 0, 13),
    (2, 5, 5),
    (4, 11, 14),
];

#[test]
fn parallel_backend_matches_cpp_golden_states() {
    for (threads, coalesced_bits, region_bits) in TUNINGS {
        let tuning = Tuning {
            threads,
            coalesced_bits,
            region_bits,
        };
        let mut backend = ParallelBackend::new(tuning).unwrap();
        run_all(
            Mode::Plain,
            &format!("paralelo t={threads} c={coalesced_bits} r={region_bits}"),
            &mut backend,
        );
    }
}

#[test]
fn parallel_backend_with_fusion_and_scheduling_matches_cpp_golden_states() {
    for (threads, coalesced_bits, region_bits) in TUNINGS {
        let tuning = Tuning {
            threads,
            coalesced_bits,
            region_bits,
        };
        let mut backend = ParallelBackend::new(tuning).unwrap();
        run_all(
            Mode::Optimized,
            &format!("paralelo+otimizado t={threads} c={coalesced_bits} r={region_bits}"),
            &mut backend,
        );
    }
}
