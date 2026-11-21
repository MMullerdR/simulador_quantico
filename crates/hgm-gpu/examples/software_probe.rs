//! Sonda de custo de partida em adaptadores de software (WARP do Windows, lavapipe).
//!
//! No CI do Windows o teste de paridade levou ~890 s: ~135 s para criar o contexto e
//! ~751 s na primeira execução do kernel de região (3 qubits, uma passada); as seguintes
//! levam milissegundos (`docs/09` §4.2). Este exemplo mede onde o custo está e como ele
//! muda com o tamanho do tile e do workgroup, e se é do kernel de região ou da primeira
//! execução de qualquer kernel.
//!
//! uso: `cargo run --release -p hgm-gpu --example software_probe -- [REGIAO] [WORKGROUP] [PRIMEIRO]`
//! (padrão 12 256 region). `PRIMEIRO` é `region` ou `global`: qual kernel roda antes; o
//! outro vem depois e o primeiro repete no fim.

use std::time::Instant;

use hgm_core::circuit::compile_steps;
use hgm_core::GateCatalog;
use hgm_gpu::plan::gates_of;
use hgm_gpu::{GpuContext, GpuOptions, RunTuning};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let number = |i: usize, default: u32| -> u32 {
        args.get(i)
            .map(|s| s.parse().expect("argumento numérico"))
            .unwrap_or(default)
    };
    let region_bits = number(0, 12);
    let workgroup_size = number(1, 256);
    let first_is_region = args.get(2).map(String::as_str).unwrap_or("region") == "region";

    let started = Instant::now();
    let context = match GpuContext::new(GpuOptions {
        max_region_bits: region_bits,
        workgroup_size,
    }) {
        Ok(context) => context,
        Err(error) => {
            eprintln!("sem GPU: {error}");
            std::process::exit(1);
        }
    };
    println!("adaptador: {}", context.describe());
    println!(
        "região={region_bits} workgroup={workgroup_size} primeiro={}",
        if first_is_region { "region" } else { "global" }
    );
    println!("contexto criado em {:.2?}", started.elapsed());

    let qubits = 3u32;
    let program = compile_steps(&["H,H,H".to_owned()], 1, &GateCatalog::with_base_gates()).unwrap();
    let gates = gates_of(&program, 1).unwrap();
    let region = RunTuning {
        region_bits,
        coalesced_bits: 2,
    };
    // Região 0 força só passadas globais (um dispatch por porta).
    let global = RunTuning {
        region_bits: 0,
        coalesced_bits: 0,
    };

    let order = if first_is_region {
        [("region", region), ("global", global), ("region", region)]
    } else {
        [("global", global), ("region", region), ("global", global)]
    };
    for (round, (name, tuning)) in order.into_iter().enumerate() {
        let state = context.basis_state(qubits, 0).unwrap();
        let begin = Instant::now();
        let prepared = context.prepare(&state, &gates, tuning).unwrap();
        let prepare_time = begin.elapsed();
        let begin = Instant::now();
        let stats = context.run(&prepared, 1).unwrap();
        let run_time = begin.elapsed();
        println!(
            "execução {} ({name}): preparo {prepare_time:.2?}, run {run_time:.2?} \
             ({}+{} passadas, GPU {:?})",
            round + 1,
            stats.region_passes,
            stats.global_passes,
            stats.gpu_seconds
        );
    }
    println!("total {:.2?}", started.elapsed());
}
