//! Custo de um lote de portas na CPU paralela e na GPU (residente), para decidir se um modo
//! híbrido (parte do estado na CPU, parte levada à GPU e de volta) compensa: fase 6 de
//! `docs/09`.
//!
//! ```text
//! cargo run --release -p hgm-engine --example hybrid_probe -- QUBITS [THREADS] [GATES]
//! ```
//!
//! O lote são `GATES` Hadamards (padrão 300, o lote do agendador) nos qubits em rodízio.

use std::time::Instant;

use hgm_cpu::Tuning;
use hgm_engine::{BackendKind, GpuTuning, SimConfig, Simulator};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let qubits: usize = args
        .first()
        .and_then(|a| a.parse().ok())
        .expect("uso: hybrid_probe QUBITS [THREADS] [GATES]");
    let threads: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(16);
    let gates: usize = args.get(2).and_then(|a| a.parse().ok()).unwrap_or(300);

    let steps: Vec<String> = (0..gates)
        .map(|i| {
            let mut tokens = vec!["ID"; qubits];
            tokens[i % qubits] = "H";
            tokens.join(",")
        })
        .collect();

    for (name, kind) in [
        (
            "CPU paralela",
            BackendKind::CpuParallel(Tuning {
                threads,
                coalesced_bits: 11,
                region_bits: 14,
            }),
        ),
        ("GPU residente", BackendKind::Gpu(GpuTuning::default())),
    ] {
        let mut simulator =
            Simulator::with_config(&SimConfig::new(kind, 1), qubits).expect("simulador");
        let program = simulator.compile_steps(&steps, 1).expect("programa");
        // Aquecimento (pipelines, páginas do estado) e depois a mediana de 5.
        simulator.run_program(&program, 1).expect("aquecimento");
        let mut times = Vec::new();
        for _ in 0..5 {
            let started = Instant::now();
            simulator.run_program(&program, 1).expect("execução");
            // A GPU é assíncrona: ler uma amplitude espera o fim.
            simulator.amplitude(0).expect("amplitude");
            times.push(started.elapsed().as_secs_f64());
        }
        times.sort_by(f64::total_cmp);
        let median = times[2];
        let bytes = (8usize << qubits) as f64;
        println!(
            "{qubits} qubits, {gates} portas, {name}: {median:.3} s ({:.2} GB/s de estado por lote)",
            bytes / median / 1e9
        );
    }
}
