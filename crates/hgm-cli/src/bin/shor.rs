//! Fatoração de Shor: `shor QUBITS [EXEC_TYPE] [THREADS] [BLOCK_SIZE] [REPEAT_COUNT]
//! [GPU_REGION_BITS]` (equivalente a `outputs/shor.out` do C++).
//!
//! `QUBITS` escolhe o número a fatorar (tabela abaixo). Variáveis de ambiente,
//! como no C++: `SHOR_EMULATE=1` troca a multiplicação modular por uma permutação do
//! estado; `SHOR_AQFT=N` fixa o grau da QFT aproximada (`0` = exata).

use std::process::ExitCode;
use std::time::Instant;

use hgm_algos::shor::{shor, ShorOptions};
use hgm_cli::{finish, parse_backend, parse_qubits, seed_from_environment, CliError};
use hgm_engine::SimConfig;

/// Quantidade de qubits -> número a fatorar (a mesma tabela do `shor.cpp`).
const TARGETS: [(usize, u64); 7] = [
    (15, 57),
    (17, 119),
    (19, 253),
    (21, 485),
    (23, 1017),
    (25, 2045),
    (27, 2863),
];

fn options_from_environment() -> Result<ShorOptions, CliError> {
    let emulate_mult = std::env::var("SHOR_EMULATE").is_ok_and(|v| v.starts_with('1'));
    let qft_degree = match std::env::var("SHOR_AQFT") {
        Ok(text) => {
            let degree: i64 = text.trim().parse().map_err(|_| {
                CliError::Usage(format!(
                    "SHOR_AQFT inválida: `{text}` (esperado um inteiro)"
                ))
            })?;
            Some(degree.max(0) as u32)
        }
        Err(_) => None,
    };
    Ok(ShorOptions {
        emulate_mult,
        qft_degree,
    })
}

fn run(args: &[String]) -> Result<(), CliError> {
    let qubits = parse_qubits(args)?;
    let Some(&(_, number)) = TARGETS.iter().find(|(q, _)| *q == qubits) else {
        return Err(CliError::Usage(format!(
            "The amount of qubits does not map to a valid number to be factored: {qubits}"
        )));
    };
    let backend = parse_backend(args, 0)?;
    let options = options_from_environment()?;
    let config = SimConfig::new(backend, seed_from_environment()?);

    println!("Executing Shor: {qubits} qubits");
    let start = Instant::now();
    let factors = shor(&config, number, &options)?;
    println!("Time: {:.6}", start.elapsed().as_secs_f64());

    if let [first, second] = factors.as_slice() {
        println!("Found factors: {first} -- {second}");
    } else {
        println!("Failed to find factors");
    }
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    finish(run(&args))
}
