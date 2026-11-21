//! GPU x CPU: circuitos aleatórios e a soma da medição. Precisa de uma GPU (ou de um
//! rasterizador por software com suporte a compute); sem adaptador o teste avisa e
//! passa, para o CI sem GPU não reprovar. O que vale como verificação é o que o log
//! diz ter rodado.
//!
//! Tolerância: `1e-5` por componente, a mesma de `docs/09` (GPU x CPU); a GPU pode
//! contrair multiplicação e soma em FMA e a CPU do Rust não.

use std::sync::OnceLock;

use hgm_core::circuit::compile_steps;
use hgm_core::{Backend, GateCatalog, StateVector, C32};
use hgm_cpu::SerialBackend;
use hgm_gpu::plan::gates_of;
use hgm_gpu::{GpuContext, GpuOptions, RunTuning};

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

fn random_amplitudes(rng: &mut Lcg, qubits: usize) -> Vec<C32> {
    let mut values: Vec<C32> = (0..1usize << qubits)
        .map(|_| C32::new(rng.unit(), rng.unit()))
        .collect();
    let norm: f64 = values.iter().map(|a| f64::from(a.norm_sqr())).sum();
    let scale = (1.0 / norm.sqrt()) as f32;
    for a in &mut values {
        *a *= scale;
    }
    values
}

/// Um contexto para o processo todo, compartilhado pelos testes (que rodam em threads):
/// vários dispositivos destruídos ao mesmo tempo travam o processo (ver
/// `examples/stress.rs`). Estáticos não são destruídos, então não há `drop`.
fn gpu() -> Option<&'static GpuContext> {
    static CONTEXT: OnceLock<Option<GpuContext>> = OnceLock::new();
    CONTEXT
        .get_or_init(|| match GpuContext::new(GpuOptions::default()) {
            Ok(context) => {
                println!("GPU: {}", context.describe());
                Some(context)
            }
            Err(error) => {
                // `HGM_REQUIRE_GPU=1` transforma a ausência de GPU em falha (máquinas
                // que têm GPU e onde um teste que não rodou seria um falso verde).
                assert!(
                    std::env::var("HGM_REQUIRE_GPU").is_err(),
                    "HGM_REQUIRE_GPU definida, mas não há GPU utilizável: {error}"
                );
                println!("SEM GPU, teste não executado: {error}");
                None
            }
        })
        .as_ref()
}

/// Adaptador que roda em CPU (rasterizador por software, como o WARP do Windows ou o
/// lavapipe): serve para corretude, mas é ordens de grandeza mais lento, então os testes
/// pesados encolhem (no CI do Windows a versão completa levou 15 minutos).
fn is_software(context: &GpuContext) -> bool {
    context.info().device_type == wgpu::DeviceType::Cpu
}

fn max_diff(a: &[C32], b: &[C32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).norm())
        .fold(0.0, f32::max)
}

#[test]
fn gpu_matches_serial_cpu_on_random_circuits() {
    let Some(context) = gpu() else { return };

    // (qubits, região, coalescida): região 0 força só passadas globais; as pequenas
    // forçam várias passadas e janelas deslocadas.
    let combos: [(usize, u32, u32); 13] = [
        (3, 12, 4),
        (5, 3, 1),
        (6, 4, 2),
        (8, 12, 4),
        (8, 5, 0),
        (9, 0, 0),
        (10, 6, 3),
        (11, 12, 4),
        (12, 7, 2),
        (14, 12, 4),
        (15, 9, 5),
        (17, 12, 4),
        (22, 12, 4),
    ];

    let mut worst = 0.0f32;
    let max_qubits = if is_software(context) { 14 } else { 22 };
    for (index, &(qubits, region_bits, coalesced_bits)) in combos.iter().enumerate() {
        if qubits > max_qubits {
            continue;
        }
        let mut rng = Lcg(1000 + index as u64);
        let steps: Vec<String> = (0..40).map(|_| random_step(&mut rng, qubits)).collect();
        let program = compile_steps(&steps, 2, &GateCatalog::with_base_gates()).unwrap();
        let initial = random_amplitudes(&mut rng, qubits);

        let mut cpu_bytes = Vec::new();
        for a in &initial {
            cpu_bytes.extend_from_slice(&a.re.to_le_bytes());
            cpu_bytes.extend_from_slice(&a.im.to_le_bytes());
        }
        let mut cpu_state = StateVector::from_f32_le_bytes(qubits, &cpu_bytes).unwrap();
        SerialBackend::new()
            .execute(&mut cpu_state, &program, 1)
            .unwrap();

        let gates = gates_of(&program, 1).unwrap();
        let gpu_state = context.state_from(qubits as u32, &initial).unwrap();
        let stats = context
            .execute(
                &gpu_state,
                &gates,
                RunTuning {
                    region_bits,
                    coalesced_bits,
                },
            )
            .unwrap();
        let got = context.read_state(&gpu_state).unwrap();

        let diff = max_diff(cpu_state.as_slice(), &got);
        worst = worst.max(diff);
        assert!(
            diff < 1e-5,
            "{qubits}q região={region_bits} coalescida={coalesced_bits}: diferença {diff} \
             ({} passadas de região, {} globais)",
            stats.region_passes,
            stats.global_passes
        );
    }
    println!("pior diferença GPU x CPU serial: {worst:e}");
}

#[test]
fn measurement_sums_match_f64_on_the_host() {
    let Some(context) = gpu() else { return };

    for (qubits, seed) in [(10usize, 1u64), (16, 2), (20, 3)] {
        let mut rng = Lcg(seed);
        let amplitudes = random_amplitudes(&mut rng, qubits);
        let state = context.state_from(qubits as u32, &amplitudes).unwrap();
        for bit in [0u32, 1, qubits as u32 / 2, qubits as u32 - 1] {
            let (mut zero, mut one) = (0.0f64, 0.0f64);
            for (index, a) in amplitudes.iter().enumerate() {
                let energy = f64::from(a.re).powi(2) + f64::from(a.im).powi(2);
                if index >> bit & 1 == 1 {
                    one += energy;
                } else {
                    zero += energy;
                }
            }
            let (gpu_zero, gpu_one) = context.measure_sums(&state, bit).unwrap();
            let error = ((gpu_zero - zero).abs() / zero).max((gpu_one - one).abs() / one);
            assert!(error < 1e-6, "{qubits}q bit {bit}: erro relativo {error:e}");
        }
    }
}

#[test]
fn oversized_or_empty_states_are_errors_not_panics() {
    let Some(context) = gpu() else { return };
    assert!(context.basis_state(0, 0).is_err());
    assert!(context.basis_state(32, 0).is_err());
}

#[test]
fn repeated_runs_give_identical_results() {
    // R1 de `docs/11`: o mesmo circuito, 50 vezes, tem que dar sempre o mesmo estado
    // (pega corrida de dados e driver instável).
    let Some(context) = gpu() else { return };
    let qubits = 14usize;
    let mut rng = Lcg(77);
    let steps: Vec<String> = (0..30).map(|_| random_step(&mut rng, qubits)).collect();
    let program = compile_steps(&steps, 2, &GateCatalog::with_base_gates()).unwrap();
    let gates = gates_of(&program, 1).unwrap();
    let initial = random_amplitudes(&mut rng, qubits);

    let mut first: Option<Vec<C32>> = None;
    let runs = if is_software(context) { 5 } else { 50 };
    for run in 0..runs {
        let state = context.state_from(qubits as u32, &initial).unwrap();
        context
            .execute(&state, &gates, RunTuning::default())
            .unwrap();
        let got = context.read_state(&state).unwrap();
        match &first {
            None => first = Some(got),
            Some(expected) => assert!(expected == &got, "execução {run} diferente da primeira"),
        }
    }
}
