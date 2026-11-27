//! O motor: junta o parser, o estado, o otimizador e um backend (o `DGM` do C++,
//! sem o que já mora em outros crates).

pub mod hybrid;

pub use hgm_gpu::MAX_GPU_QUBITS;

use std::collections::BTreeMap;

use hgm_core::circuit::{compile_steps, tokenize};
use hgm_core::error::Result;
use hgm_core::optimize::fuse_phase_runs;
use hgm_core::{Backend, GateCatalog, Measurement, Mt19937, Program, StateVector, C32};
use hgm_cpu::{ParallelBackend, SerialBackend, Tuning};
use hgm_gpu::{GpuBackend, RunTuning};

/// Ajuste do backend de GPU (`gpu_region_bits`, `gpu_coalesced_bits` e o lote do
/// agendador do C++).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuTuning {
    /// Região em memória de workgroup (log2 de amplitudes); o contexto a reduz se o
    /// adaptador oferece menos.
    pub region_bits: u32,
    /// Bits baixos contíguos da região (leitura coalescida).
    pub coalesced_bits: u32,
    /// Portas por lote do agendador por comutação (0 desliga a reordenação).
    pub batch: usize,
    /// Maior bloco do estado na GPU (log2 de amplitudes). O dispositivo impõe o seu limite
    /// (27 qubits na A4500); estados maiores são divididos em blocos. Valores menores só
    /// servem para testar a divisão com estados pequenos.
    pub max_chunk_bits: u32,
}

impl Default for GpuTuning {
    fn default() -> Self {
        Self {
            region_bits: 12,
            coalesced_bits: 4,
            batch: hgm_gpu::backend::DEFAULT_BATCH,
            max_chunk_bits: u32::MAX,
        }
    }
}

/// Ajuste do `t_HYBRID`: a CPU paralela para o que não cabe na GPU, e o da própria GPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HybridTuning {
    pub cpu: Tuning,
    pub gpu: GpuTuning,
    /// Estados com mais qubits que isto nem tentam a GPU (vão direto para a CPU). O padrão
    /// é o limite da própria GPU ([`hgm_gpu::MAX_GPU_QUBITS`], 31): dentro dele a GPU é
    /// tentada e, se faltar memória, o estado volta para o host.
    pub gpu_max_qubits: u32,
}

impl HybridTuning {
    pub fn new(cpu: Tuning, gpu: GpuTuning) -> Self {
        Self {
            cpu,
            gpu,
            gpu_max_qubits: MAX_GPU_QUBITS,
        }
    }
}

/// Backends disponíveis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    /// `t_CPU`: serial, o oráculo dos demais.
    CpuSerial,
    /// `t_PAR_CPU`: regiões em paralelo (rayon).
    CpuParallel(Tuning),
    /// `t_GPU`: GPU portátil (wgpu), com o estado no host entre execuções.
    Gpu(GpuTuning),
    /// `t_HYBRID`: a GPU enquanto o estado cabe, a CPU paralela quando não (ver [`hybrid`]).
    Hybrid(HybridTuning),
}

/// Semente padrão do `std::mt19937` do C++ (`g_rng` sem `seed_rng`).
pub const DEFAULT_SEED: u32 = 5489;

/// O que os algoritmos (`general`, `grover`, `shor`) precisam saber para montar
/// um [`Simulator`]: o backend e a semente do gerador de medições/sorteios. É o
/// `TuningDefaults` + `seed_rng` do C++ com o ajuste de GPU dentro de [`BackendKind::Gpu`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimConfig {
    pub backend: BackendKind,
    pub seed: u32,
    /// Funde corridas de portas de fase antes de executar. `None` usa o padrão do
    /// backend (ver [`SimConfig::fuse_phase_enabled`]).
    pub fuse_phase: Option<bool>,
}

impl SimConfig {
    pub fn new(backend: BackendKind, seed: u32) -> Self {
        Self {
            backend,
            seed,
            fuse_phase: None,
        }
    }

    /// Força a fusão de fase ligada ou desligada (`None` volta ao padrão do backend).
    pub fn with_fuse_phase(mut self, fuse_phase: Option<bool>) -> Self {
        self.fuse_phase = fuse_phase;
        self
    }

    /// Se a fusão fica ligada: o pedido explícito, ou o padrão do backend.
    pub fn fuse_phase_enabled(&self) -> bool {
        self.fuse_phase
            .unwrap_or_else(|| default_fuse_phase(&self.backend))
    }
}

/// Padrão da fusão de fase por backend (ver `docs/09`, fase 7). Na GPU ela liga: com o
/// kernel que lê um termo para quatro amplitudes ela nunca foi pior e no Shor ganha de 5%
/// a 25% (Shor 27 qubits: 51,7 s contra 68,6 s), como a fusão do C++. Na CPU desliga: ajuda
/// a carga mista e atrapalha a de fases puras.
fn default_fuse_phase(backend: &BackendKind) -> bool {
    matches!(backend, BackendKind::Gpu(_) | BackendKind::Hybrid(_))
}

impl Default for SimConfig {
    fn default() -> Self {
        Self::new(BackendKind::CpuSerial, DEFAULT_SEED)
    }
}

pub struct Simulator {
    state: StateVector,
    backend: Box<dyn Backend>,
    gates: GateCatalog,
    rng: Mt19937,
    fuse_phase: bool,
}

/// O backend de GPU residente: o estado fica nela entre execuções, medições e colapsos.
fn gpu_backend(tuning: GpuTuning) -> std::result::Result<GpuBackend, hgm_gpu::GpuError> {
    Ok(GpuBackend::new(RunTuning {
        region_bits: tuning.region_bits,
        coalesced_bits: tuning.coalesced_bits,
    })?
    .with_batch(tuning.batch)
    .with_max_chunk_bits(tuning.max_chunk_bits)
    .resident(true))
}

impl Simulator {
    /// Simulador em `|0...0>` com as portas base.
    pub fn new(kind: BackendKind, qubits: usize) -> Result<Self> {
        Self::with_config(&SimConfig::new(kind, DEFAULT_SEED), qubits)
    }

    /// Simulador em `|0...0>` com o backend e a semente de `config`.
    pub fn with_config(config: &SimConfig, qubits: usize) -> Result<Self> {
        let backend: Box<dyn Backend> = match config.backend {
            BackendKind::CpuSerial => Box::new(SerialBackend::new()),
            BackendKind::CpuParallel(tuning) => Box::new(ParallelBackend::new(tuning)?),
            BackendKind::Gpu(tuning) => Box::new(
                gpu_backend(tuning)
                    .map_err(|e| hgm_core::error::HgmError::Backend(e.to_string()))?,
            ),
            BackendKind::Hybrid(tuning) => {
                let cpu = Box::new(ParallelBackend::new(tuning.cpu)?);
                let (gpu, why_not): (Option<Box<dyn Backend>>, Option<String>) =
                    if qubits as u64 > u64::from(tuning.gpu_max_qubits) {
                        (
                            None,
                            Some(format!("{qubits} qubits passam do limite da GPU")),
                        )
                    } else {
                        match gpu_backend(tuning.gpu) {
                            Ok(gpu) => (Some(Box::new(gpu)), None),
                            Err(error) => (None, Some(error.to_string())),
                        }
                    };
                Box::new(hybrid::HybridBackend::new(gpu, cpu, why_not))
            }
        };
        let mut backend = backend;
        let mut state = StateVector::basis(qubits, 0)?;
        backend.initial_state_ready(&mut state)?;
        Ok(Self {
            state,
            backend,
            gates: GateCatalog::with_base_gates(),
            rng: Mt19937::new(config.seed),
            fuse_phase: config.fuse_phase_enabled(),
        })
    }

    pub fn qubits(&self) -> usize {
        self.state.qubits()
    }

    /// Põe o estado na base `|index>` (`DGM::setMemoryValue` depois de zerar).
    pub fn set_basis(&mut self, index: usize) -> Result<()> {
        self.backend.set_basis(&mut self.state, index)
    }

    /// O gerador compartilhado por medições e sorteios do algoritmo (o `g_rng`
    /// do C++: o Shor sorteia a base e mede com o mesmo gerador).
    pub fn rng_mut(&mut self) -> &mut Mt19937 {
        &mut self.rng
    }

    pub fn gates(&self) -> &GateCatalog {
        &self.gates
    }

    pub fn backend_name(&self) -> &str {
        self.backend.name()
    }

    /// O vetor de estado inteiro. Com a GPU como backend o estado mora lá; este método
    /// o traz para o host se estiver desatualizado, o que custa uma cópia do vetor todo
    /// (use [`Simulator::amplitude`] para ler poucas amplitudes).
    pub fn state(&mut self) -> Result<&StateVector> {
        self.backend.sync_to_host(&mut self.state)?;
        Ok(&self.state)
    }

    /// Uma amplitude do estado, sem trazer o vetor inteiro para o host.
    pub fn amplitude(&mut self, index: usize) -> Result<C32> {
        self.backend.amplitude(&mut self.state, index)
    }

    pub fn set_state(&mut self, state: StateVector) {
        self.state = state;
        self.backend.host_state_replaced();
    }

    pub fn gates_mut(&mut self) -> &mut GateCatalog {
        &mut self.gates
    }

    /// Reinicia o gerador de medições (`seed_rng` do C++).
    pub fn seed(&mut self, seed: u32) {
        self.rng = Mt19937::new(seed);
    }

    /// Liga ou desliga a fusão de corridas de portas de fase antes de executar. O
    /// padrão depende do backend (ver [`SimConfig::fuse_phase_enabled`]): ligada na GPU
    /// e no híbrido, desligada na CPU, onde a porta fundida percorre os termos por
    /// amplitude e, no benchmark de `docs/09`, foi mais lenta que as portas separadas.
    pub fn set_fuse_phase(&mut self, enabled: bool) {
        self.fuse_phase = enabled;
    }

    /// Compila `function` (passos separados por `;`, repetidos `iterations` vezes)
    /// e a executa `repetitions` vezes sobre o estado atual.
    pub fn run(&mut self, function: &str, iterations: usize, repetitions: u32) -> Result<()> {
        let steps: Vec<&str> = tokenize(function, ";").collect();
        self.run_steps(&steps, iterations, repetitions)
    }

    /// Compila `steps` (um passo por item) repetidos `iterations` vezes, com o
    /// catálogo de portas atual, sem executar (`setFunction`). A fusão de fase,
    /// se ligada, é aplicada aqui.
    pub fn compile_steps<S: AsRef<str>>(&self, steps: &[S], iterations: usize) -> Result<Program> {
        let mut program = compile_steps(steps, iterations, &self.gates)?;
        if self.fuse_phase {
            program.ops = fuse_phase_runs(program.ops).0;
        }
        Ok(program)
    }

    /// Executa um programa já compilado `repetitions` vezes (`DGM::execute`).
    pub fn run_program(&mut self, program: &Program, repetitions: u32) -> Result<()> {
        self.backend.execute(&mut self.state, program, repetitions)
    }

    /// Compila e executa passos já separados (`DGM::executeFunction(vector)`).
    pub fn run_steps<S: AsRef<str>>(
        &mut self,
        steps: &[S],
        iterations: usize,
        repetitions: u32,
    ) -> Result<()> {
        let program = self.compile_steps(steps, iterations)?;
        self.run_program(&program, repetitions)
    }

    /// Mede um qubit, colapsando o estado (`DGM::measure(int)`).
    pub fn measure(&mut self, qubit: usize) -> Result<Measurement> {
        self.backend
            .measure_qubit(&mut self.state, qubit, &mut self.rng)
    }

    /// Força um qubit a 0 ou 1 e renormaliza (`DGM::colapse`).
    pub fn collapse(&mut self, qubit: usize, value: u8) -> Result<f64> {
        self.backend.collapse(&mut self.state, qubit, value)
    }

    /// Distribuição de probabilidade sobre os qubits dados, sem colapsar.
    pub fn probabilities(&mut self, qubits: &[usize]) -> Result<BTreeMap<u64, f64>> {
        self.backend.sync_to_host(&mut self.state)?;
        self.state.probabilities(qubits)
    }

    /// Multiplicação modular controlada emulada como permutação (`applyModMult`).
    pub fn apply_mod_mult(
        &mut self,
        ctrl_qubit: usize,
        reg_first_qubit: usize,
        width: usize,
        multiplier: i64,
        modulus: i64,
    ) -> Result<()> {
        self.backend.apply_mod_mult(
            &mut self.state,
            ctrl_qubit,
            reg_first_qubit,
            width,
            multiplier,
            modulus,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parallel() -> BackendKind {
        BackendKind::CpuParallel(Tuning {
            threads: 2,
            coalesced_bits: 1,
            region_bits: 3,
        })
    }

    #[test]
    fn grover_like_round_keeps_the_state_normalised() {
        for kind in [BackendKind::CpuSerial, parallel()] {
            let mut simulator = Simulator::new(kind, 3).unwrap();
            simulator.run("H,H,H", 1, 1).unwrap();
            simulator
                .run("Control1(1),Control1(1),Target1(Z)", 1, 1)
                .unwrap();
            simulator.run("H,H,H;X,X,X", 1, 1).unwrap();
            assert!((simulator.state().unwrap().norm_sqr() - 1.0).abs() < 1e-5);
        }
        let simulator = Simulator::new(BackendKind::CpuSerial, 2).unwrap();
        assert_eq!(simulator.backend_name(), "t_CPU (Rust, serial)");
    }

    #[test]
    fn unknown_gate_surfaces_as_an_error() {
        let mut simulator = Simulator::new(BackendKind::CpuSerial, 2).unwrap();
        assert!(simulator.run("Q,ID", 1, 1).is_err());
    }

    #[test]
    fn bell_measurements_are_correlated_and_reproducible() {
        let outcome = |seed| {
            let mut simulator = Simulator::new(parallel(), 2).unwrap();
            simulator.seed(seed);
            simulator.run("H,ID;Control1(1),Target1(X)", 1, 1).unwrap();
            let first = simulator.measure(0).unwrap().bit;
            let second = simulator.measure(1).unwrap().bit;
            assert_eq!(first, second);
            first
        };
        assert_eq!(outcome(3), outcome(3));
        assert!((0..16).any(|seed| outcome(seed) == 0) && (0..16).any(|seed| outcome(seed) == 1));
    }

    #[test]
    fn fusion_does_not_change_the_result() {
        let run = |fuse| {
            let mut simulator = Simulator::new(parallel(), 3).unwrap();
            simulator.set_fuse_phase(fuse);
            simulator.run("H,H,H;Z,R1,R2;R3,Z,R1;H,H,H", 1, 1).unwrap();
            simulator.state().unwrap().clone()
        };
        assert!(run(false).max_abs_diff(&run(true)) < 1e-5);
    }

    /// `Some(simulador)` se há GPU; senão `None` (o teste passa avisando), a menos que
    /// `HGM_REQUIRE_GPU` esteja definida.
    fn gpu_simulator(qubits: usize) -> Option<Simulator> {
        gpu_simulator_with(GpuTuning::default(), qubits)
    }

    fn gpu_simulator_with(tuning: GpuTuning, qubits: usize) -> Option<Simulator> {
        match Simulator::new(BackendKind::Gpu(tuning), qubits) {
            Ok(simulator) => Some(simulator),
            Err(error) => {
                assert!(
                    std::env::var("HGM_REQUIRE_GPU").is_err(),
                    "HGM_REQUIRE_GPU definida, mas não há GPU utilizável: {error}"
                );
                println!("SEM GPU, teste não executado: {error}");
                None
            }
        }
    }

    /// Tamanhos de bloco para repetir os testes de GPU: `u32::MAX` é o estado num bloco só
    /// (o que o dispositivo permitir); os outros dividem o estado em 2^(n-L) blocos, o que
    /// exercita as portas cruzadas, o colapso e a medição entre blocos e o `modmult` com o
    /// registrador cruzando o limite do bloco.
    const CHUNK_CAPS: [u32; 5] = [u32::MAX, 6, 4, 3, 2];

    fn chunked(cap: u32) -> GpuTuning {
        GpuTuning {
            max_chunk_bits: cap,
            ..GpuTuning::default()
        }
    }

    #[test]
    fn gpu_backend_matches_the_serial_oracle_through_the_engine() {
        for cap in CHUNK_CAPS {
            let Some(mut gpu) = gpu_simulator_with(chunked(cap), 8) else {
                return;
            };
            assert_eq!(gpu.backend_name(), "t_GPU (Rust, wgpu)");
            let mut cpu = Simulator::new(BackendKind::CpuSerial, 8).unwrap();
            // Hadamards, controles dentro e fora de uma região pequena, fases e repetições.
            let steps = [
                "H,H,H,H,H,H,H,H",
                "Control1(1),ID,ID,ID,ID,ID,ID,Target1(X)",
                "ID,R1,Z,ID,H,ID,R3,ID",
                "Control1(1),Control1(1),ID,ID,ID,ID,ID,Target1(H)",
                "Target1(R2),ID,ID,ID,ID,ID,ID,Control0(1)",
                "R1,H,ID,ID,ID,ID,ID,ID",
            ];
            for simulator in [&mut cpu, &mut gpu] {
                simulator.run_steps(&steps, 1, 3).unwrap();
            }
            let diff = cpu.state().unwrap().max_abs_diff(gpu.state().unwrap());
            assert!(diff < 1e-5, "bloco {cap}: diferença {diff}");
            assert!((gpu.state().unwrap().norm_sqr() - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn gpu_backend_keeps_measurements_consistent_with_the_host_state() {
        let Some(mut gpu) = gpu_simulator(2) else {
            return;
        };
        gpu.seed(11);
        gpu.run("H,ID;Control1(1),Target1(X)", 1, 1).unwrap();
        let first = gpu.measure(0).unwrap().bit;
        assert_eq!(first, gpu.measure(1).unwrap().bit);
        // Uma segunda execução parte do estado colapsado, que subiu de novo para a GPU.
        gpu.run("X,X", 1, 1).unwrap();
        let expected = if first == 0 { 3 } else { 0 };
        assert!((gpu.amplitude(expected).unwrap().norm() - 1.0).abs() < 1e-5);
    }

    /// Mesmos passos em CPU serial e em GPU residente, com a mesma semente: estados iguais
    /// (até 1e-5) e os mesmos bits medidos, em ordem, com o estado colapsado igual.
    #[test]
    fn resident_gpu_measurements_follow_the_serial_oracle() {
        let qubits = 6;
        for cap in CHUNK_CAPS {
            let Some(mut gpu) = gpu_simulator_with(chunked(cap), qubits) else {
                return;
            };
            let mut cpu = Simulator::new(BackendKind::CpuSerial, qubits).unwrap();
            for seed in 0..6u32 {
                for simulator in [&mut cpu, &mut gpu] {
                    simulator.seed(seed);
                    simulator.set_basis(5).unwrap();
                    simulator.run("H,H,H,H,H,H", 1, 1).unwrap();
                    simulator
                        .run("Control1(1),ID,ID,ID,ID,Target1(R2)", 1, 1)
                        .unwrap();
                }
                for qubit in [0, 3, 5, 1, 2, 4] {
                    let want = cpu.measure(qubit).unwrap();
                    let got = gpu.measure(qubit).unwrap();
                    assert_eq!(want.bit, got.bit, "bloco {cap}, seed {seed}, qubit {qubit}");
                    assert!((want.probability - got.probability).abs() < 1e-5);
                    // O estado colapsado segue igual, e continua evoluindo igual.
                    for simulator in [&mut cpu, &mut gpu] {
                        simulator.run("H,ID,ID,ID,ID,H", 1, 1).unwrap();
                    }
                }
                let diff = cpu.state().unwrap().max_abs_diff(gpu.state().unwrap());
                assert!(diff < 1e-5, "bloco {cap}, seed {seed}: diferença {diff}");
            }
        }
    }

    #[test]
    fn resident_gpu_collapse_and_amplitudes_follow_the_host() {
        for cap in CHUNK_CAPS {
            let Some(mut gpu) = gpu_simulator_with(chunked(cap), 5) else {
                return;
            };
            let mut cpu = Simulator::new(BackendKind::CpuSerial, 5).unwrap();
            for simulator in [&mut cpu, &mut gpu] {
                simulator.run("H,H,H,H,H", 1, 1).unwrap();
                simulator.run("R1,ID,Z,ID,R3", 1, 1).unwrap();
            }
            for (qubit, value) in [(2, 1), (0, 0), (4, 1)] {
                let want = cpu.collapse(qubit, value).unwrap();
                let got = gpu.collapse(qubit, value).unwrap();
                assert!((want - got).abs() < 1e-5, "bloco {cap}");
            }
            // Lê amplitudes uma a uma (sem sincronizar) e depois o vetor inteiro.
            for index in 0..32 {
                let want = cpu.amplitude(index).unwrap();
                let got = gpu.amplitude(index).unwrap();
                assert!((want - got).norm() < 1e-5, "bloco {cap}, índice {index}");
            }
            assert!(cpu.state().unwrap().max_abs_diff(gpu.state().unwrap()) < 1e-5);
            assert!(gpu.amplitude(32).is_err());
            // Zero de probabilidade no ramo já colapsado.
            assert!(cpu.collapse(2, 0).is_err() && gpu.collapse(2, 0).is_err());
        }
    }

    fn hybrid_kind(cap: u32, gpu_max_qubits: u32) -> BackendKind {
        BackendKind::Hybrid(HybridTuning {
            cpu: Tuning {
                threads: 2,
                coalesced_bits: 1,
                region_bits: 3,
            },
            gpu: chunked(cap),
            gpu_max_qubits,
        })
    }

    /// O híbrido faz o mesmo que a CPU serial em tudo que o motor oferece (execução, medição,
    /// colapso, `modmult`, amplitudes), tanto com a GPU (se há adaptador) quanto sem ela
    /// (`gpu_max_qubits` = 0 manda tudo para a CPU paralela).
    #[test]
    fn hybrid_backend_matches_the_serial_oracle_with_and_without_the_gpu() {
        let qubits = 7;
        for cap in CHUNK_CAPS {
            for gpu_max_qubits in [u32::MAX, 0] {
                let mut hybrid = Simulator::new(hybrid_kind(cap, gpu_max_qubits), qubits).unwrap();
                let name = hybrid.backend_name().to_owned();
                if gpu_max_qubits == 0 {
                    assert!(name.contains("CPU paralela"), "{name}");
                    assert!(name.contains("limite"), "{name}");
                } else {
                    assert!(name.starts_with("t_HYBRID"), "{name}");
                    if std::env::var("HGM_REQUIRE_GPU").is_ok() {
                        assert!(name.contains("estado na GPU"), "{name}");
                    }
                }
                let mut cpu = Simulator::new(BackendKind::CpuSerial, qubits).unwrap();
                for seed in 0..4u32 {
                    for simulator in [&mut cpu, &mut hybrid] {
                        simulator.seed(seed);
                        simulator.set_basis(3).unwrap();
                        simulator.run("H,H,R1,H,Z,H,H", 1, 1).unwrap();
                        simulator.apply_mod_mult(0, 1, 4, 7, 15).unwrap();
                        simulator
                            .run("Control1(1),ID,ID,ID,ID,ID,Target1(R2)", 1, 1)
                            .unwrap();
                    }
                    for qubit in [6, 0, 3] {
                        let want = cpu.measure(qubit).unwrap();
                        let got = hybrid.measure(qubit).unwrap();
                        assert_eq!(want.bit, got.bit, "bloco {cap}, seed {seed}, qubit {qubit}");
                        assert!((want.probability - got.probability).abs() < 1e-5);
                    }
                    let want = cpu.collapse(1, 0);
                    let got = hybrid.collapse(1, 0);
                    assert_eq!(want.is_ok(), got.is_ok(), "bloco {cap}, seed {seed}");
                    if let (Ok(want), Ok(got)) = (want, got) {
                        assert!((want - got).abs() < 1e-5);
                    }
                    for index in [0, 5, 77, 127] {
                        let want = cpu.amplitude(index).unwrap();
                        let got = hybrid.amplitude(index).unwrap();
                        assert!((want - got).norm() < 1e-5, "bloco {cap}, índice {index}");
                    }
                    let diff = cpu.state().unwrap().max_abs_diff(hybrid.state().unwrap());
                    assert!(diff < 1e-5, "bloco {cap}, seed {seed}: diferença {diff}");
                }
            }
        }
    }

    #[test]
    fn resident_gpu_mod_mult_matches_the_host_permutation() {
        // 1 qubit de controle + registrador de 4 qubits + 2 qubits à parte, em
        // superposição, para a permutação ter o que embaralhar. Com blocos pequenos o
        // controle e o registrador ficam (em parte ou inteiros) nos bits de bloco.
        for cap in CHUNK_CAPS {
            let Some(mut gpu) = gpu_simulator_with(chunked(cap), 7) else {
                return;
            };
            let mut cpu = Simulator::new(BackendKind::CpuSerial, 7).unwrap();
            for simulator in [&mut cpu, &mut gpu] {
                simulator.run("H,H,R1,H,Z,H,H", 1, 1).unwrap();
                simulator.apply_mod_mult(0, 1, 4, 7, 15).unwrap();
                simulator.run("H,ID,ID,ID,ID,ID,ID", 1, 1).unwrap();
                simulator.apply_mod_mult(0, 1, 4, 13, 15).unwrap();
                simulator.apply_mod_mult(0, 1, 4, 2, 11).unwrap(); // módulo menor que 2^4
                                                                   // Registrador nos qubits de baixo, controle no meio.
                simulator.apply_mod_mult(2, 3, 4, 7, 15).unwrap();
                simulator.run("H,H,H,H,H,H,H", 1, 1).unwrap();
                simulator.apply_mod_mult(0, 1, 5, 3, 29).unwrap();
            }
            let diff = cpu.state().unwrap().max_abs_diff(gpu.state().unwrap());
            assert!(diff < 1e-5, "bloco {cap}: diferença {diff}");
            for simulator in [&mut cpu, &mut gpu] {
                assert!(simulator.apply_mod_mult(0, 1, 4, 5, 15).is_err()); // gcd != 1
                assert!(simulator.apply_mod_mult(1, 1, 4, 7, 15).is_err()); // controle no registrador
            }
        }
    }

    #[test]
    fn replacing_the_host_state_invalidates_the_gpu_copy() {
        let Some(mut gpu) = gpu_simulator(3) else {
            return;
        };
        gpu.run("H,H,H", 1, 1).unwrap();
        let mut replacement = StateVector::basis(3, 6).unwrap();
        replacement.as_mut_slice()[6] = C32::new(0.0, 1.0);
        gpu.set_state(replacement);
        assert_eq!(gpu.amplitude(6).unwrap(), C32::new(0.0, 1.0));
        gpu.run("X,ID,ID", 1, 1).unwrap();
        assert_eq!(gpu.state().unwrap().as_slice()[2], C32::new(0.0, 1.0));
        // set_basis sem passar pelo host
        gpu.set_basis(1).unwrap();
        assert_eq!(gpu.amplitude(1).unwrap(), C32::new(1.0, 0.0));
        assert_eq!(gpu.state().unwrap().as_slice()[1], C32::new(1.0, 0.0));
        assert!(gpu.set_basis(8).is_err());
    }

    /// Fases fundidas na GPU contra as portas separadas na CPU serial. Os três ajustes
    /// de região fazem os termos caírem dentro e fora da região (a parte "de fora" é
    /// comparada com o endereço-base, igual aos controles das portas).
    #[test]
    fn gpu_fused_phases_match_the_unfused_serial_oracle() {
        let steps = [
            "H,H,H,H,H,H,H,H,H",
            "Z,R1,R2,R3,Z,R1,R2,ID,Z",
            "ID,Control1(1),ID,ID,ID,ID,ID,ID,Target1(R3)",
            "R2,Z,ID,Control0(1),ID,ID,ID,Target1(R1),ID",
            "Control1(1),ID,ID,ID,ID,ID,ID,ID,Target1(Z)",
            "H,ID,H,ID,H,ID,H,ID,H",
            "R1,R3,Z,R2,R1,Z,R3,R2,R1",
            "R1,R3,Z,R2,R1,Z,R3,R2,R1",
        ];
        let shapes = [
            GpuTuning::default(),
            GpuTuning {
                region_bits: 5,
                coalesced_bits: 2,
                batch: 300,
                ..GpuTuning::default()
            },
            GpuTuning {
                region_bits: 4,
                coalesced_bits: 4,
                batch: 0,
                ..GpuTuning::default()
            },
            // Estado em blocos: termos com máscara nos bits de bloco.
            GpuTuning {
                region_bits: 5,
                coalesced_bits: 2,
                batch: 300,
                max_chunk_bits: 6,
            },
            GpuTuning {
                region_bits: 4,
                coalesced_bits: 1,
                batch: 0,
                max_chunk_bits: 4,
            },
        ];
        for tuning in shapes {
            let Some(mut gpu) = gpu_simulator_with(tuning, 9) else {
                return;
            };
            gpu.set_fuse_phase(true);
            let program = gpu.compile_steps(&steps, 1).unwrap();
            assert!(
                program
                    .ops
                    .iter()
                    .any(|op| matches!(op, hgm_core::Op::FusedPhase(_))),
                "o programa de teste deveria ter fases fundidas"
            );
            let mut cpu = Simulator::new(BackendKind::CpuSerial, 9).unwrap();
            cpu.run_steps(&steps, 1, 2).unwrap();
            gpu.run_program(&program, 2).unwrap();
            let diff = cpu.state().unwrap().max_abs_diff(gpu.state().unwrap());
            assert!(diff < 1e-5, "{tuning:?}: diferença {diff}");
        }
    }

    #[test]
    fn mod_mult_goes_through_the_engine() {
        let mut simulator = Simulator::new(BackendKind::CpuSerial, 5).unwrap();
        simulator.run("ID,ID,ID,ID,X", 1, 1).unwrap(); // x = 1
        simulator.run("X,ID,ID,ID,ID", 1, 1).unwrap(); // controle = 1
        simulator.apply_mod_mult(0, 1, 4, 7, 15).unwrap();
        assert_eq!(simulator.amplitude((1 << 4) | 7).unwrap().re, 1.0);
    }
}
