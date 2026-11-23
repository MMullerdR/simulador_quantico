//! Banda host <-> GPU pelo wgpu (sem memória fixada): base para decidir se um modo híbrido
//! que leva blocos do estado para a GPU e de volta compensa (fase 6 de `docs/09`).
//!
//! ```text
//! cargo run --release -p hgm-gpu --example transfer_bench
//! ```
//!
//! Mede, para estados de 24 a 27 qubits (128 MiB a 1 GiB): subida (`state_from_in_chunks`
//! mais uma leitura de uma amplitude, que força a espera), descida (`read_into`), e uma
//! passada de região na GPU sobre o mesmo estado, para comparar o custo do transporte com o
//! do cálculo. Mediana de 5 repetições.

use std::time::Instant;

use hgm_core::C32;
use hgm_gpu::plan::{GpuGate, GpuOp};
use hgm_gpu::{GpuContext, GpuOptions, RunTuning};

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn hadamard(target: u32) -> GpuGate {
    let h = std::f32::consts::FRAC_1_SQRT_2;
    GpuGate {
        target,
        control_mask: 0,
        control_value: 0,
        matrix: [h, 0.0, h, 0.0, h, 0.0, -h, 0.0],
    }
}

fn main() {
    let context = GpuContext::new(GpuOptions::default()).expect("sem GPU");
    println!("{}", context.describe());
    let tuning = RunTuning::default();

    for qubits in [24u32, 25, 26, 27] {
        let len = 1usize << qubits;
        let bytes = (len * 8) as f64;
        let mut host = vec![C32::new(0.5, 0.25); len];

        let (mut up, mut down, mut pass) = (Vec::new(), Vec::new(), Vec::new());
        for _ in 0..5 {
            let started = Instant::now();
            let state = context.state_from(qubits, &host).expect("subida");
            context.read_range(&state, 0, 1).expect("espera");
            up.push(started.elapsed().as_secs_f64());

            let started = Instant::now();
            context.read_into(&state, &mut host).expect("descida");
            down.push(started.elapsed().as_secs_f64());

            // 300 Hadamards nos bits baixos e altos: algumas passadas de região.
            let gates: Vec<GpuOp> = (0..300)
                .map(|i| GpuOp::Gate(hadamard(i % qubits)))
                .collect();
            let prepared = context.prepare_ops(&state, &gates, tuning).expect("plano");
            let started = Instant::now();
            context.run(&prepared, 1).expect("execução");
            context.read_range(&state, 0, 1).expect("espera");
            pass.push(started.elapsed().as_secs_f64());
        }
        let (up, down, pass) = (median(up), median(down), median(pass));
        println!(
            "{qubits} qubits ({:>5.0} MiB): subida {:.3} s ({:.2} GB/s), descida {:.3} s ({:.2} GB/s), \
             300 portas na GPU {:.3} s",
            bytes / 1048576.0,
            up,
            bytes / up / 1e9,
            down,
            bytes / down / 1e9,
            pass
        );
    }
}
