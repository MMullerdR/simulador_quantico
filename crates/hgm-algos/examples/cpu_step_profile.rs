//! Tempo de cada passo de uma iteração do Grover na CPU paralela, para achar o passo que
//! pesa (fase 7 de `docs/09`: o Grover paralelo estava mais lento que o do C++).
//!
//! ```text
//! cargo run --release -p hgm-algos --example cpu_step_profile -- QUBITS [THREADS] [REPS]
//! ```
//!
//! Cada passo de `grover_step` é compilado e executado sozinho `REPS` vezes (padrão 20);
//! imprime o tempo médio por execução e a soma, que aproxima o custo de uma iteração.

use std::time::Instant;

use hgm_algos::grover::grover_step;
use hgm_cpu::Tuning;
use hgm_engine::{BackendKind, Simulator};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let qubits: usize = args
        .first()
        .and_then(|a| a.parse().ok())
        .expect("uso: cpu_step_profile QUBITS [THREADS] [REPS]");
    let threads: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(16);
    let reps: u32 = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(20);

    let mut simulator = Simulator::new(
        BackendKind::CpuParallel(Tuning {
            threads,
            coalesced_bits: 11,
            region_bits: 14,
        }),
        qubits,
    )
    .expect("simulador");
    simulator.set_basis(1usize << (qubits - 1)).expect("base");

    let steps = grover_step(qubits, 10);
    let mut total = 0.0;
    for (index, step) in steps.iter().enumerate() {
        let program = simulator
            .compile_steps(std::slice::from_ref(step), 1)
            .expect("programa");
        simulator.run_program(&program, 1).expect("aquecimento");
        let started = Instant::now();
        simulator.run_program(&program, reps).expect("execução");
        let each = started.elapsed().as_secs_f64() / f64::from(reps);
        total += each;
        let label: String = step.chars().take(48).collect();
        println!("{index:>3} {:>9.3} ms  {label}", each * 1e3);
    }
    println!("soma de uma iteração: {:.3} ms", total * 1e3);
}
