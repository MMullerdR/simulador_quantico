//! Circuitos aleatórios: o backend paralelo tem que dar o mesmo estado que o serial
//! (a menos de arredondamento) para qualquer combinação de qubits, região,
//! coalescida e threads, com e sem fusão/agendamento. Equivalente em espírito ao
//! `test_cpu_parallel` do C++, com controles dentro e fora da região.

use hgm_core::circuit::compile_steps;
use hgm_core::optimize::{fuse_phase_runs, schedule_for_batching};
use hgm_core::{Backend, GateCatalog, StateVector, C32};
use hgm_cpu::{ParallelBackend, SerialBackend, Tuning};

/// Gerador congruencial simples (determinístico, sem dependência).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn unit(&mut self) -> f32 {
        (self.next() % 2_000_001) as f32 / 1_000_000.0 - 1.0
    }
}

const GATES: [&str; 7] = ["H", "X", "Y", "Z", "R1", "R2", "R3"];

/// Um passo: uma porta (às vezes mais de uma, em qubits distintos) com 0 a 2
/// controles de valor aleatório.
fn random_step(rng: &mut Lcg, qubits: usize) -> String {
    let mut tokens = vec!["ID".to_owned(); qubits];
    let mut free: Vec<usize> = (0..qubits).collect();
    let mut take = |rng: &mut Lcg| free.swap_remove(rng.below(free.len() as u64) as usize);

    let controls = if qubits >= 3 { rng.below(3) } else { 0 } as usize;
    let control_qubits: Vec<usize> = (0..controls).map(|_| take(rng)).collect();
    let target = take(rng);
    let gate = GATES[rng.below(GATES.len() as u64) as usize];

    if control_qubits.is_empty() {
        tokens[target] = gate.to_owned();
    } else {
        tokens[target] = format!("Target1({gate})");
        for qubit in control_qubits {
            tokens[qubit] = format!("Control1({})", rng.below(2));
        }
    }
    tokens.join(",")
}

fn random_state(rng: &mut Lcg, qubits: usize) -> StateVector {
    let len = 1usize << qubits;
    let mut bytes = Vec::with_capacity(len * 8);
    let mut norm = 0.0f64;
    let values: Vec<C32> = (0..len)
        .map(|_| {
            let a = C32::new(rng.unit(), rng.unit());
            norm += f64::from(a.norm_sqr());
            a
        })
        .collect();
    let scale = (1.0 / norm.sqrt()) as f32;
    for a in values {
        bytes.extend_from_slice(&(a.re * scale).to_le_bytes());
        bytes.extend_from_slice(&(a.im * scale).to_le_bytes());
    }
    StateVector::from_f32_le_bytes(qubits, &bytes).unwrap()
}

fn run_case(qubits: usize, tuning: Tuning, optimized: bool, seed: u64) -> f32 {
    let mut rng = Lcg(seed);
    let steps: Vec<String> = (0..40).map(|_| random_step(&mut rng, qubits)).collect();
    let mut program = compile_steps(&steps, 1, &GateCatalog::with_base_gates()).unwrap();
    if optimized {
        let (fused, _) = fuse_phase_runs(program.ops);
        program.ops = schedule_for_batching(fused, tuning.coalesced_bits, 5, 50);
    }

    let initial = random_state(&mut rng, qubits);
    let mut serial_state = initial.clone();
    let mut parallel_state = initial;

    SerialBackend::new()
        .execute(&mut serial_state, &program, 2)
        .unwrap();
    ParallelBackend::new(tuning)
        .unwrap()
        .execute(&mut parallel_state, &program, 2)
        .unwrap();

    serial_state.max_abs_diff(&parallel_state)
}

#[test]
fn parallel_matches_serial_on_random_circuits() {
    // (qubits, região, coalescida, threads)
    let combos: [(usize, u32, u32, usize); 14] = [
        (3, 3, 0, 1),
        (4, 2, 0, 2),
        (6, 3, 0, 2),
        (6, 4, 2, 3),
        (8, 5, 5, 4),
        (8, 8, 8, 2),
        (9, 5, 5, 4),
        (10, 4, 1, 4),
        (10, 6, 3, 8),
        (10, 10, 4, 3),
        (12, 7, 4, 4),
        (12, 12, 12, 2),
        (14, 14, 11, 4),
        (14, 6, 0, 8),
    ];

    let mut worst = 0.0f32;
    for (index, &(qubits, region_bits, coalesced_bits, threads)) in combos.iter().enumerate() {
        let tuning = Tuning {
            threads,
            coalesced_bits,
            region_bits,
        };
        for optimized in [false, true] {
            for seed in 0..3u64 {
                let diff = run_case(qubits, tuning, optimized, seed * 7919 + index as u64);
                worst = worst.max(diff);
                assert!(
                    diff < 1e-4,
                    "q={qubits} r={region_bits} c={coalesced_bits} t={threads} otimizado={optimized} seed={seed}: diferença {diff:e}"
                );
            }
        }
    }
    println!("paralelo x serial: pior diferença {worst:e}");
}
