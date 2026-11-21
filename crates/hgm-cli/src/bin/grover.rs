//! Busca de Grover: `grover QUBITS [EXEC_TYPE] [THREADS] [BLOCK_SIZE] [REPEAT_COUNT]
//! [GPU_REGION_BITS] [SEARCH_VALUE]` (equivalente a `outputs/grover.out` do C++).
//!
//! `SEARCH_VALUE` (padrão 10) precisa caber no registrador de busca (`QUBITS - 1`
//! bits). Imprime o tempo em segundos e `Found value: N` ou `Failed to find value`.

use std::process::ExitCode;
use std::time::Instant;

use hgm_algos::grover::grover;
use hgm_cli::{finish, parse_arg, parse_backend, parse_qubits, seed_from_environment, CliError};
use hgm_core::MAX_QUBITS;
use hgm_engine::SimConfig;

const DEFAULT_SEARCH_VALUE: u64 = 10;

fn run(args: &[String]) -> Result<(), CliError> {
    let qubits = parse_qubits(args)?;
    if !(1..=MAX_QUBITS).contains(&qubits) {
        return Err(CliError::Usage(format!(
            "Invalid qubit count: {qubits} (precisa estar entre 1 e {MAX_QUBITS})"
        )));
    }
    let backend = parse_backend(args, 0)?;
    let search_value = parse_arg::<u64>(args, 6, "SEARCH_VALUE")?.unwrap_or(DEFAULT_SEARCH_VALUE);

    // O oráculo só olha `qubits - 1` bits: um valor fora disso seria truncado em
    // silêncio e a busca seria por outro valor.
    let search_space = 1u64 << (qubits - 1);
    if search_value >= search_space {
        return Err(CliError::Usage(format!(
            "Invalid search_value: {search_value} (precisa estar entre 0 e {} pra {qubits} qubits)",
            search_space - 1
        )));
    }

    let config = SimConfig::new(backend, seed_from_environment()?);
    let start = Instant::now();
    let result = grover(&config, qubits, search_value)?;
    println!("{:.6}", start.elapsed().as_secs_f64());

    if result == search_value {
        println!("Found value: {result}");
    } else {
        println!("Failed to find value (measured {result}, expected {search_value})");
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    finish(run(&args))
}
