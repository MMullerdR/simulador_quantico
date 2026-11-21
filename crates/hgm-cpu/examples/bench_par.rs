//! Benchmark de CPU do Rust, com as MESMAS cargas de `benches/cpp/bench_par.cpp`.
//!
//! uso: `cargo run --release -p hgm-cpu --example bench_par -- QUBITS CARGA THREADS
//! REGIAO COALESCIDA REPS [serial] [otimizado]`; `CARGA` é `hadamard`, `phase` ou
//! `mixed`. Imprime `tempo_s=... a0=... a1=... checksum=...` (amplitudes 0 e 12345 e uma soma
//! ponderada do vetor inteiro).

use std::time::Instant;

use hgm_core::circuit::compile_steps;
use hgm_core::optimize::{fuse_phase_runs, schedule_for_batching};
use hgm_core::{Backend, GateCatalog, StateVector};
use hgm_cpu::{ParallelBackend, SerialBackend, Tuning};

fn workload_steps(name: &str, qubits: usize) -> Vec<String> {
    match name {
        "hadamard" => vec![vec!["H"; qubits].join(",")],
        "phase" => {
            let gates = ["R1", "R2", "R3"];
            vec![(0..qubits)
                .map(|i| gates[i % 3])
                .collect::<Vec<_>>()
                .join(",")]
        }
        _ => {
            let mut steps = vec![vec!["H"; qubits].join(",")];
            let gates = ["X", "Z", "Y", "R2"];
            for s in 0..8usize {
                let mut tokens = vec!["ID".to_owned(); qubits];
                let control = (5 * s + 2) % qubits;
                let mut target = (7 * s + 3) % qubits;
                if target == control {
                    target = (target + 1) % qubits;
                }
                tokens[control] = if s % 2 == 1 {
                    "Control1(1)"
                } else {
                    "Control1(0)"
                }
                .to_owned();
                tokens[target] = format!("Target1({})", gates[s % 4]);
                steps.push(tokens.join(","));
            }
            steps
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 6 {
        eprintln!(
            "uso: bench_par QUBITS CARGA THREADS REGIAO COALESCIDA REPS [serial] [otimizado]"
        );
        std::process::exit(2);
    }
    let qubits: usize = args[0].parse().expect("QUBITS");
    let workload = &args[1];
    let threads: usize = args[2].parse().expect("THREADS");
    let region_bits: u32 = args[3].parse().expect("REGIAO");
    let coalesced_bits: u32 = args[4].parse().expect("COALESCIDA");
    let reps: u32 = args[5].parse().expect("REPS");
    let serial = args.iter().any(|a| a == "serial");
    let optimized = args.iter().any(|a| a == "otimizado");

    let steps = workload_steps(workload, qubits);
    let mut program = compile_steps(&steps, 1, &GateCatalog::with_base_gates()).expect("compilar");
    if optimized {
        let (fused, _) = fuse_phase_runs(program.ops);
        program.ops = schedule_for_batching(fused, coalesced_bits, 5, 50);
    }
    let mut state = StateVector::basis(qubits, 0).expect("alocar");

    let mut backend: Box<dyn Backend> = if serial {
        Box::new(SerialBackend::new())
    } else {
        Box::new(
            ParallelBackend::new(Tuning {
                threads,
                coalesced_bits,
                region_bits,
            })
            .expect("pool"),
        )
    };

    let start = Instant::now();
    backend
        .execute(&mut state, &program, reps)
        .expect("executar");
    let seconds = start.elapsed().as_secs_f64();

    let probe = 12345 % (1usize << qubits);
    let (a0, a1) = (state.as_slice()[0], state.as_slice()[probe]);
    // Checksum do vetor inteiro (mesma fórmula do C++): soma ponderada em f64.
    let checksum: f64 = state
        .as_slice()
        .iter()
        .enumerate()
        .map(|(i, a)| f64::from(a.re) * (i % 7 + 1) as f64 + f64::from(a.im) * (i % 5 + 1) as f64)
        .sum();
    println!(
        "tempo_s={seconds:.6} a0={:.7},{:.7} a1={:.7},{:.7} checksum={checksum:.6}",
        a0.re, a0.im, a1.re, a1.im
    );
}
