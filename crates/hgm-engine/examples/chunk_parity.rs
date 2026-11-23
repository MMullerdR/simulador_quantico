//! Paridade GPU x CPU paralela em estados grandes, acima do que cabe num binding da GPU
//! (27 qubits na RTX A4500): o estado vai para a GPU em vários blocos.
//!
//! ```text
//! cargo run --release -p hgm-engine --example chunk_parity -- QUBITS [THREADS] [MODMULT] [hybrid]
//! ```
//!
//! Roda o mesmo circuito (Hadamards, fases, portas controladas com controle e alvo nos
//! bits altos e baixos, medições e, com `MODMULT = 1`, multiplicações modulares com o
//! registrador dentro e atravessando o limite do bloco) na CPU paralela e na GPU, com a
//! mesma semente, e compara os bits medidos e o estado final. Com `hybrid` no lugar da GPU
//! entra o `t_HYBRID`: em 31 qubits o `modmult` precisa de mais memória do que a placa de
//! 20 GB tem (o estado de 16 GiB mais outro tanto de rascunho) e o estado volta para a CPU.

use std::time::Instant;

use hgm_cpu::Tuning;
use hgm_engine::{BackendKind, GpuTuning, HybridTuning, SimConfig, Simulator};

/// Um passo de `qubits` qubits com `tokens` nas posições dadas e `ID` no resto.
fn step(qubits: usize, tokens: &[(usize, &str)]) -> String {
    let mut all = vec!["ID".to_owned(); qubits];
    for &(qubit, token) in tokens {
        all[qubit] = token.to_owned();
    }
    all.join(",")
}

fn circuit(qubits: usize) -> Vec<String> {
    let last = qubits - 1;
    let mut steps = vec![vec!["H"; qubits].join(",")];
    let phases = ["R1", "R2", "R3", "Z"];
    let phase_step: Vec<&str> = (0..qubits).map(|q| phases[q % 4]).collect();
    steps.push(phase_step.join(","));
    for (control, target, gate) in [
        (0, last, "R2"),
        (last, 0, "R3"),
        (1, 2, "H"),
        (3, qubits - 2, "R1"),
        (0, 1, "X"),
        (2, 0, "H"),
    ] {
        steps.push(step(
            qubits,
            &[
                (control, "Control1(1)"),
                (target, &format!("Target1({gate})")),
            ],
        ));
    }
    for target in [0, 1, 2, 3, 4, last] {
        steps.push(step(qubits, &[(target, "H")]));
    }
    steps.push(step(
        qubits,
        &[(0, "Control0(1)"), (qubits / 2, "Target1(Y)")],
    ));
    steps
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let qubits: usize = args
        .first()
        .and_then(|a| a.parse().ok())
        .expect("uso: chunk_parity QUBITS [THREADS] [MODMULT]");
    let threads: usize = args.get(1).and_then(|a| a.parse().ok()).unwrap_or(16);
    let modmult = args.get(2).is_some_and(|a| a == "1");
    let hybrid = args.get(3).is_some_and(|a| a == "hybrid");

    let build = |kind| {
        let config = SimConfig::new(kind, 7);
        Simulator::with_config(&config, qubits).expect("simulador")
    };
    let cpu_tuning = Tuning {
        threads,
        coalesced_bits: 11,
        region_bits: 14,
    };
    let mut cpu = build(BackendKind::CpuParallel(cpu_tuning));
    let mut gpu = build(if hybrid {
        BackendKind::Hybrid(HybridTuning::new(cpu_tuning, GpuTuning::default()))
    } else {
        BackendKind::Gpu(GpuTuning::default())
    });
    println!("{}", gpu.backend_name());

    let steps = circuit(qubits);
    for (name, simulator) in [("CPU", &mut cpu), ("GPU", &mut gpu)] {
        let started = Instant::now();
        simulator.run_steps(&steps, 2, 1).expect("circuito");
        println!("{name}: circuito em {:.2?}", started.elapsed());
    }

    let mut bits = Vec::new();
    for qubit in [0usize, 2, qubits - 1, 1] {
        let want = cpu.measure(qubit).expect("medição na CPU");
        let got = gpu.measure(qubit).expect("medição na GPU");
        println!(
            "medir {qubit}: CPU {} (p={:.6}), GPU {} (p={:.6})",
            want.bit, want.probability, got.bit, got.probability
        );
        assert_eq!(want.bit, got.bit, "bit medido do qubit {qubit}");
        assert!((want.probability - got.probability).abs() < 1e-4);
        bits.push(got.bit);
        for simulator in [&mut cpu, &mut gpu] {
            simulator
                .run_steps(&[step(qubits, &[(qubit, "H")])], 1, 1)
                .expect("H depois da medição");
        }
    }

    if modmult {
        // Controle no qubit 0 (o bit mais alto) e registrador logo abaixo; com 29 qubits
        // e blocos de 27, o registrador de 13 qubits cruza o limite do bloco.
        let width = 13.min(qubits - 2);
        let modulus = (1i64 << width) - 1;
        for (name, simulator) in [("CPU", &mut cpu), ("GPU", &mut gpu)] {
            let started = Instant::now();
            simulator
                .apply_mod_mult(0, 1, width, 3, modulus)
                .expect("modmult");
            simulator
                .run_steps(&[step(qubits, &[(0, "H")])], 1, 1)
                .expect("H");
            simulator
                .apply_mod_mult(0, 1, width, 7, modulus)
                .expect("modmult");
            println!("{name}: modmult em {:.2?}", started.elapsed());
        }
    }

    println!("{}", gpu.backend_name());
    let started = Instant::now();
    let diff = {
        let cpu_state = cpu.state().expect("estado da CPU").clone();
        let gpu_state = gpu.state().expect("estado da GPU");
        cpu_state.max_abs_diff(gpu_state)
    };
    println!(
        "{qubits} qubits: diferença máxima entre os estados {diff:e} (comparação em {:.2?})",
        started.elapsed()
    );
    assert!(diff < 1e-5, "estados diferentes: {diff}");
    println!("OK");
}
