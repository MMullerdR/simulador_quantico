//! O motor: junta o parser, o estado, o otimizador e um backend (o `DGM` do C++,
//! sem o que já mora em outros crates). Modo híbrido e GPU entram nas próximas
//! fases de `docs/09`.

use std::collections::BTreeMap;

use hgm_core::circuit::{compile_steps, tokenize};
use hgm_core::error::Result;
use hgm_core::optimize::fuse_phase_runs;
use hgm_core::{Backend, GateCatalog, Measurement, Mt19937, Program, StateVector};
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
}

impl Default for GpuTuning {
    fn default() -> Self {
        Self {
            region_bits: 12,
            coalesced_bits: 4,
            batch: hgm_gpu::backend::DEFAULT_BATCH,
        }
    }
}

/// Backends disponíveis. O híbrido entra com a fase 6.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    /// `t_CPU`: serial, o oráculo dos demais.
    CpuSerial,
    /// `t_PAR_CPU`: regiões em paralelo (rayon).
    CpuParallel(Tuning),
    /// `t_GPU`: GPU portátil (wgpu), com o estado no host entre execuções.
    Gpu(GpuTuning),
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
}

impl SimConfig {
    pub fn new(backend: BackendKind, seed: u32) -> Self {
        Self { backend, seed }
    }
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
                GpuBackend::new(RunTuning {
                    region_bits: tuning.region_bits,
                    coalesced_bits: tuning.coalesced_bits,
                })
                .map_err(|e| hgm_core::error::HgmError::Backend(e.to_string()))?
                .with_batch(tuning.batch),
            ),
        };
        Ok(Self {
            state: StateVector::basis(qubits, 0)?,
            backend,
            gates: GateCatalog::with_base_gates(),
            rng: Mt19937::new(config.seed),
            fuse_phase: false,
        })
    }

    pub fn qubits(&self) -> usize {
        self.state.qubits()
    }

    /// Põe o estado na base `|index>` (`DGM::setMemoryValue` depois de zerar).
    pub fn set_basis(&mut self, index: usize) -> Result<()> {
        self.state = StateVector::basis(self.state.qubits(), index)?;
        Ok(())
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

    pub fn state(&self) -> &StateVector {
        &self.state
    }

    pub fn set_state(&mut self, state: StateVector) {
        self.state = state;
    }

    pub fn gates_mut(&mut self) -> &mut GateCatalog {
        &mut self.gates
    }

    /// Reinicia o gerador de medições (`seed_rng` do C++).
    pub fn seed(&mut self, seed: u32) {
        self.rng = Mt19937::new(seed);
    }

    /// Liga a fusão de corridas de portas de fase antes de executar. Desligada por
    /// padrão, como no C++, onde só o caminho de GPU funde: na CPU a porta fundida
    /// percorre os termos por amplitude e, no benchmark de `docs/09`, foi mais lenta
    /// que as portas separadas.
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
        self.state.measure_qubit(qubit, &mut self.rng)
    }

    /// Força um qubit a 0 ou 1 e renormaliza (`DGM::colapse`).
    pub fn collapse(&mut self, qubit: usize, value: u8) -> Result<f64> {
        self.state.collapse(qubit, value)
    }

    /// Distribuição de probabilidade sobre os qubits dados, sem colapsar.
    pub fn probabilities(&self, qubits: &[usize]) -> Result<BTreeMap<u64, f64>> {
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
        self.state
            .apply_mod_mult(ctrl_qubit, reg_first_qubit, width, multiplier, modulus)
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
            assert!((simulator.state().norm_sqr() - 1.0).abs() < 1e-5);
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
            simulator.state().clone()
        };
        assert!(run(false).max_abs_diff(&run(true)) < 1e-5);
    }

    /// `Some(simulador)` se há GPU; senão `None` (o teste passa avisando), a menos que
    /// `HGM_REQUIRE_GPU` esteja definida.
    fn gpu_simulator(qubits: usize) -> Option<Simulator> {
        match Simulator::new(BackendKind::Gpu(GpuTuning::default()), qubits) {
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

    #[test]
    fn gpu_backend_matches_the_serial_oracle_through_the_engine() {
        let Some(mut gpu) = gpu_simulator(8) else {
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
        ];
        for simulator in [&mut cpu, &mut gpu] {
            simulator.run_steps(&steps, 1, 3).unwrap();
        }
        assert!(cpu.state().max_abs_diff(gpu.state()) < 1e-5);
        assert!((gpu.state().norm_sqr() - 1.0).abs() < 1e-5);
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
        assert!((gpu.state().as_slice()[expected].norm() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn mod_mult_goes_through_the_engine() {
        let mut simulator = Simulator::new(BackendKind::CpuSerial, 5).unwrap();
        simulator.run("ID,ID,ID,ID,X", 1, 1).unwrap(); // x = 1
        simulator.run("X,ID,ID,ID,ID", 1, 1).unwrap(); // controle = 1
        simulator.apply_mod_mult(0, 1, 4, 7, 15).unwrap();
        assert_eq!(simulator.state().as_slice()[(1 << 4) | 7].re, 1.0);
    }
}
