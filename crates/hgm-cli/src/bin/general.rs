//! Benchmark de Hadamard: `general QUBITS EXEC_TYPE [THREADS] [BLOCK_SIZE] [REPEAT_COUNT]
//! [GPU_REGION_BITS]` (equivalente a `outputs/general.out` do C++).
//!
//! Imprime a amplitude de `|0...0>` (`0:\t%.6f %.6f`, a primeira linha, que o
//! `tests/smoke_test.sh` confere) e depois o tempo em segundos. O C++ exige
//! `EXEC_TYPE` e rejeita o serial; aqui o serial também vale.

use std::process::ExitCode;
use std::time::Instant;

use hgm_algos::general::hadamard_n_qubits;
use hgm_cli::{finish, parse_backend, parse_qubits, seed_from_environment, CliError};
use hgm_core::MAX_QUBITS;
use hgm_engine::SimConfig;

/// Repetições de `H` em todos os qubits (fixas no C++).
const ITERATIONS: usize = 3;

fn run(args: &[String]) -> Result<(), CliError> {
    if args.len() < 2 {
        return Err(CliError::Usage(
            "You need to define the execution parameters".to_owned(),
        ));
    }
    let qubits = parse_qubits(args)?;
    if !(1..=MAX_QUBITS).contains(&qubits) {
        return Err(CliError::Usage(format!(
            "Invalid qubit count: {qubits} (precisa estar entre 1 e {MAX_QUBITS})"
        )));
    }
    let backend = parse_backend(args, 0)?;
    let config = SimConfig::new(backend, seed_from_environment()?);

    let start = Instant::now();
    let amplitude = hadamard_n_qubits(&config, qubits, ITERATIONS)?;
    println!("0:\t{:.6} {:.6}", amplitude.re, amplitude.im);
    println!("{:.6}", start.elapsed().as_secs_f64());
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    finish(run(&args))
}
