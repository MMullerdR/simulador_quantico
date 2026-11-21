//! Grover na GPU em Rust, para comparar com `grover.out N 2 1` do C++ (8,34 s para 24
//! qubits na A4500, `tests/gpu-battery/resultados/baseline-cpp-v1.1.0-qflex/tempos.csv`).
//!
//! Mesma ordem de trabalho do `Grover()` do C++: ancilla em |1>, H em tudo, depois
//! `floor(pi/4 * sqrt(2^(q-1)))` iterações em blocos. Aqui **não** há medição qubit a
//! qubit no fim (o C++ faz 23): confere-se a probabilidade do valor procurado, lendo as
//! duas amplitudes dele.
//!
//! uso: `grover_bench [QUBITS] [VALOR] [REGIAO] [COALESCIDA] [LOTE]`

use std::time::Instant;

use hgm_algos::grover::{grover_step, iteration_count};
use hgm_algos::steps::hadamard;
use hgm_core::circuit::compile_steps;
use hgm_core::optimize::schedule_for_batching;
use hgm_core::{GateCatalog, Program};
use hgm_gpu::plan::{gates_of, region_shape};
use hgm_gpu::{GpuContext, GpuOptions, GpuState, RunTuning};

/// Como o `PT_BLOCK_LIMIT` do `Grover()`: no máximo tantas portas por bloco compilado.
const PT_BLOCK_LIMIT: usize = 20_000;

struct Totals {
    gpu: f64,
    host: f64,
    region_passes: usize,
    global_passes: usize,
    gates: usize,
}

fn run_program(
    context: &GpuContext,
    state: &GpuState,
    mut program: Program,
    repetitions: u32,
    tuning: RunTuning,
    max_batch: usize,
    totals: &mut Totals,
) {
    let host = Instant::now();
    if max_batch > 0 {
        let (c, w) = region_shape(state.qubits(), tuning.coalesced_bits, tuning.region_bits);
        program.ops = schedule_for_batching(program.ops, c, w, max_batch);
    }
    let gates = gates_of(&program, 1).unwrap();
    let prepared = context.prepare(state, &gates, tuning).unwrap();
    totals.host += host.elapsed().as_secs_f64();

    let stats = context.run(&prepared, repetitions).unwrap();
    totals.gpu += stats.gpu_seconds.unwrap_or(f64::NAN);
    totals.region_passes += stats.region_passes;
    totals.global_passes += stats.global_passes;
    totals.gates += gates.len() * repetitions as usize;
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize, default: u32| -> u32 {
        args.get(i)
            .map(|s| s.parse().expect("argumento numérico"))
            .unwrap_or(default)
    };
    let qubits = arg(0, 24);
    let value = u64::from(arg(1, 10));
    let tuning = RunTuning {
        region_bits: arg(2, 12),
        coalesced_bits: arg(3, 4),
    };
    let max_batch = arg(4, 300) as usize;

    let process = Instant::now();
    let context = GpuContext::new(GpuOptions::default()).expect("sem GPU");
    println!("adaptador: {}", context.describe());
    let catalog = GateCatalog::with_base_gates();

    let state = context.basis_state(qubits, 1 << (qubits - 1)).unwrap();
    let mut totals = Totals {
        gpu: 0.0,
        host: 0.0,
        region_passes: 0,
        global_passes: 0,
        gates: 0,
    };

    let n = qubits as usize;
    let prepare = hadamard(n, 0, n);
    let step = grover_step(n, value);
    let total = iteration_count(n);
    let chunk = (PT_BLOCK_LIMIT / step.len()).max(1);
    let first = total.min(chunk);

    let mut program = compile_steps(&[prepare], 1, &catalog).unwrap();
    program
        .append(compile_steps(&step, first, &catalog).unwrap())
        .unwrap();
    run_program(&context, &state, program, 1, tuning, max_batch, &mut totals);

    let mut left = total - first;
    if left >= chunk {
        let block = compile_steps(&step, chunk, &catalog).unwrap();
        run_program(
            &context,
            &state,
            block,
            (left / chunk) as u32,
            tuning,
            max_batch,
            &mut totals,
        );
        left %= chunk;
    }
    if left > 0 {
        let rest = compile_steps(&step, left, &catalog).unwrap();
        run_program(&context, &state, rest, 1, tuning, max_batch, &mut totals);
    }

    let wanted = value as u32;
    let amplitudes = [
        context.read_range(&state, u64::from(wanted), 1).unwrap()[0],
        context
            .read_range(&state, u64::from(wanted) | (1 << (qubits - 1)), 1)
            .unwrap()[0],
    ];
    let probability: f32 = amplitudes.iter().map(|a| a.norm_sqr()).sum();
    let elapsed = process.elapsed().as_secs_f64();

    println!(
        "grover {qubits} qubits, valor {value}, {total} iterações, região={} coalescida={} lote={max_batch}\n  \
         portas: {}; passadas: {} de região + {} globais\n  \
         kernels na GPU: {:.3} s; planejamento no host: {:.3} s; ponta a ponta (sem medir qubit a qubit): {elapsed:.3} s\n  \
         probabilidade do valor procurado: {probability:.4}   [C++ CUDA, 24 qubits: 8,34 s]",
        tuning.region_bits,
        tuning.coalesced_bits,
        totals.gates,
        totals.region_passes,
        totals.global_passes,
        totals.gpu,
        totals.host,
    );
}
