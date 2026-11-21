//! Diagnóstico: vários dispositivos na mesma máquina, em threads de um processo. Cada
//! thread abre o seu `GpuContext` e repete execução + leitura, com marcas em cada fase.
//! uso: `stress [THREADS] [RODADAS] [COMPARTILHA]`; COMPARTILHA=1 usa um contexto só.

use std::sync::Arc;

use hgm_core::circuit::compile_steps;
use hgm_core::GateCatalog;
use hgm_gpu::plan::gates_of;
use hgm_gpu::{GpuContext, GpuOptions, RunTuning};

fn work(id: usize, context: &GpuContext, rounds: usize) {
    let program = compile_steps(
        &["H,H,H,H,H,H,H,H,H,H,H,H".to_owned()],
        3,
        &GateCatalog::with_base_gates(),
    )
    .unwrap();
    let gates = gates_of(&program, 1).unwrap();
    for round in 0..rounds {
        eprintln!("t{id} r{round} alloc");
        let state = context.basis_state(12, 0).unwrap();
        eprintln!("t{id} r{round} execute");
        context
            .execute(&state, &gates, RunTuning::default())
            .unwrap();
        eprintln!("t{id} r{round} read");
        let _ = context.read_state(&state).unwrap();
        eprintln!("t{id} r{round} done");
    }
}

fn main() {
    let args: Vec<usize> = std::env::args()
        .skip(1)
        .map(|a| a.parse().unwrap())
        .collect();
    let threads = args.first().copied().unwrap_or(4);
    let rounds = args.get(1).copied().unwrap_or(20);
    let shared = args.get(2).copied().unwrap_or(0) == 1;
    let forget = args.get(3).copied().unwrap_or(0) == 1;

    let one = shared.then(|| Arc::new(GpuContext::new(GpuOptions::default()).unwrap()));
    let handles: Vec<_> = (0..threads)
        .map(|id| {
            let one = one.clone();
            std::thread::spawn(move || {
                eprintln!("t{id} open");
                let own = if one.is_none() {
                    Some(GpuContext::new(GpuOptions::default()).unwrap())
                } else {
                    None
                };
                eprintln!("t{id} opened");
                let context: &GpuContext = match (&one, &own) {
                    (Some(c), _) => c,
                    (None, Some(c)) => c,
                    _ => unreachable!(),
                };
                work(id, context, rounds);
                if forget {
                    // Não destrói o dispositivo: testa se o travamento está no `drop`.
                    std::mem::forget(own);
                }
                eprintln!("t{id} leaving");
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    eprintln!("ok");
}
