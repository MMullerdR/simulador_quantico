//! Medição do spike da fase 2 (`docs/09`): o mesmo "general" do C++ (H em todos os
//! qubits, 3 iterações), a banda de memória (P8 de `docs/11`) e a precisão da medição.
//!
//! uso: `cargo run --release -p hgm-gpu --example spike -- [QUBITS] [REGIAO] [COALESCIDA] [RODADAS] [ITERACOES] [LOTE]`
//! (padrão 26 12 4 5 3 300). O C++ do `v1.1.0` na RTX A4500 deu 0,61 s para `general.out 26 2 1`
//! (`tests/gpu-battery/resultados/baseline-cpp-v1.1.0-qflex/tempos.csv`), contado do
//! começo ao fim do processo: contexto, alocação, execução e liberação.

use std::time::Instant;

use hgm_core::circuit::compile_steps;
use hgm_core::optimize::schedule_for_batching;
use hgm_core::{GateCatalog, StateVector};
use hgm_gpu::plan::gates_of;
use hgm_gpu::{GpuContext, GpuOptions, RunTuning};

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize, default: u32| -> u32 {
        args.get(i)
            .map(|s| s.parse().expect("argumento numérico"))
            .unwrap_or(default)
    };
    let qubits = arg(0, 26);
    let region_bits = arg(1, 12);
    let coalesced_bits = arg(2, 4);
    let rounds = arg(3, 5) as usize;
    let iterations = arg(4, 3) as usize;
    // Portas por lote do agendador (0 = sem reordenar). O C++ usa 300 (`OPS_BLOCK`).
    let max_batch = arg(5, 300) as usize;

    // Como o `general.out` do C++: tudo conta, do contexto à leitura da amplitude 0.
    let process = Instant::now();
    let context = match GpuContext::new(GpuOptions::default()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("sem GPU: {e}");
            std::process::exit(1);
        }
    };
    let opened = process.elapsed().as_secs_f64();
    println!("adaptador: {}", context.describe());

    let steps = vec![vec!["H"; qubits as usize].join(",")];
    let mut program = compile_steps(&steps, iterations, &GateCatalog::with_base_gates()).unwrap();
    if max_batch > 0 {
        // Reordena por comutação para gastar menos passadas, como o `ScheduleForBatching` do C++
        // (ligado por padrão no caminho de GPU).
        let (c, w) = hgm_gpu::plan::region_shape(qubits, coalesced_bits, region_bits);
        program.ops = schedule_for_batching(program.ops, c, w, max_batch);
    }
    let gates = gates_of(&program, 1).unwrap();
    let tuning = RunTuning {
        region_bits,
        coalesced_bits,
    };

    let state = context.basis_state(qubits, 0).unwrap();
    let first = context.execute(&state, &gates, tuning).unwrap();
    let a0 = context.read_range(&state, 0, 1).unwrap()[0];
    let end_to_end = process.elapsed().as_secs_f64();
    let expected = 2f32.powf(-(qubits as f32) / 2.0);
    println!(
        "\ngeneral {qubits} (H x{iterations}), região={region_bits} coalescida={coalesced_bits}\n  \
         ponta a ponta (contexto {opened:.3} s + execução + leitura): {end_to_end:.3} s   \
         [C++ CUDA: 0,61 s]\n  amplitude 0 = {:.6} (esperado {expected:.6})",
        a0.re
    );

    // Precisão da medição: logo depois da primeira execução o estado é uniforme (3
    // camadas de H sobre |0...0>), então cada bit tem p0 = p1 = 0,5.
    let (p0, p1) = context.measure_sums(&state, qubits / 2).unwrap();
    println!(
        "\nmedição no estado uniforme (bit {}): p0 = {p0:.9}, p1 = {p1:.9} (erro {:.2e})",
        qubits / 2,
        (p0 - 0.5).abs().max((p1 - 0.5).abs())
    );

    // Equivalente ao `general.out` do C++, que aloca o estado no host, copia tudo para a
    // GPU, executa, copia tudo de volta e só então lê a amplitude 0. O número acima
    // (estado residente na GPU, lê 1 amplitude) é o que o simulador em Rust vai fazer
    // de fato, mas NÃO é comparável com os 0,61 s do C++; este é.
    {
        let t = Instant::now();
        let mut host = StateVector::basis(qubits as usize, 0).unwrap();
        let t_host = t.elapsed().as_secs_f64();
        let on_gpu = context.state_from(qubits, host.as_slice()).unwrap();
        let t_up = t.elapsed().as_secs_f64();
        context.execute(&on_gpu, &gates, tuning).unwrap();
        let t_run = t.elapsed().as_secs_f64();
        context.read_into(&on_gpu, host.as_mut_slice()).unwrap();
        let back = host.as_slice();
        let roundtrip = t.elapsed().as_secs_f64();
        println!(
            "    fases: estado no host {t_host:.3} s, subida {:.3} s, execução {:.3} s, descida {:.3} s",
            t_up - t_host,
            t_run - t_up,
            roundtrip - t_run
        );
        println!(
            "  com cópia do estado inteiro para a GPU e de volta (como o C++): {:.3} s              depois do contexto => ponta a ponta {:.3} s [C++ CUDA: 0,61 s]; amplitude 0 = {:.6}",
            roundtrip,
            opened + roundtrip,
            back[0].re
        );
    }

    // Tempos só de kernel e de parede, mediana de várias rodadas.
    let mut gpu_times = Vec::new();
    let mut wall_times = Vec::new();
    let mut stats = first;
    for _ in 0..rounds {
        stats = context.execute(&state, &gates, tuning).unwrap();
        if let Some(g) = stats.gpu_seconds {
            gpu_times.push(g);
        }
        wall_times.push(stats.wall_seconds);
    }
    let gpu_text = if gpu_times.is_empty() {
        "sem timestamps".to_owned()
    } else {
        format!("{:.1} ms", median(gpu_times) * 1e3)
    };
    println!(
        "  passadas: {} de região + {} globais; kernels na GPU: {gpu_text}; \
         parede de execute(): {:.1} ms",
        stats.region_passes,
        stats.global_passes,
        median(wall_times) * 1e3
    );

    // Mesmo circuito sem região (uma porta por dispatch): quanto a região ganha.
    let global_tuning = RunTuning {
        region_bits: 0,
        coalesced_bits: 0,
    };
    let global_state = context.basis_state(qubits, 0).unwrap();
    let mut global_gpu = Vec::new();
    let mut global_wall = Vec::new();
    let mut global_stats = context
        .execute(&global_state, &gates, global_tuning)
        .unwrap();
    for _ in 0..rounds {
        global_stats = context
            .execute(&global_state, &gates, global_tuning)
            .unwrap();
        if let Some(g) = global_stats.gpu_seconds {
            global_gpu.push(g);
        }
        global_wall.push(global_stats.wall_seconds);
    }
    let global_text = if global_gpu.is_empty() {
        "sem timestamps".to_owned()
    } else {
        format!("{:.1} ms", median(global_gpu) * 1e3)
    };
    println!(
        "  sem região ({} dispatches de uma porta): kernels {global_text}; parede {:.1} ms",
        global_stats.global_passes,
        median(global_wall) * 1e3
    );

    // Banda de memória: o teto para um simulador limitado por banda.
    let bytes = 8u64 << qubits.min(26);
    match context.copy_bandwidth(bytes, 10) {
        Ok(gbs) => {
            println!(
                "\nbanda de cópia (P8), buffer de {} MiB: {gbs:.0} GB/s",
                bytes >> 20
            );
            let state_bytes = 8u64 << qubits;
            // Cada passada de região lê e escreve o estado uma vez.
            let floor_ms =
                2.0 * state_bytes as f64 / (gbs * 1e9) * 1e3 * stats.region_passes as f64;
            println!(
                "  piso por banda para {} passadas de região em {qubits} qubits: {floor_ms:.1} ms",
                stats.region_passes
            );
        }
        Err(e) => println!("banda: erro {e}"),
    }
}
