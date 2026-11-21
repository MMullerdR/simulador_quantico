//! O motor: junta o parser, o estado, o otimizador e um backend (o `DGM` do C++,
//! sem o que já mora em outros crates). Modo híbrido e GPU entram nas próximas
//! fases de `docs/09`.

use std::collections::BTreeMap;

use hgm_core::circuit::compile;
use hgm_core::error::Result;
use hgm_core::optimize::fuse_phase_runs;
use hgm_core::{Backend, GateCatalog, Measurement, Mt19937, StateVector};
use hgm_cpu::{ParallelBackend, SerialBackend, Tuning};

/// Backends disponíveis. `Gpu` e `Hybrid` entram com as fases 4-5.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    /// `t_CPU`: serial, o oráculo dos demais.
    CpuSerial,
    /// `t_PAR_CPU`: regiões em paralelo (rayon).
    CpuParallel(Tuning),
}

/// Semente padrão do `std::mt19937` do C++ (`g_rng` sem `seed_rng`).
const DEFAULT_SEED: u32 = 5489;

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
        let backend: Box<dyn Backend> = match kind {
            BackendKind::CpuSerial => Box::new(SerialBackend::new()),
            BackendKind::CpuParallel(tuning) => Box::new(ParallelBackend::new(tuning)?),
        };
        Ok(Self {
            state: StateVector::basis(qubits, 0)?,
            backend,
            gates: GateCatalog::with_base_gates(),
            rng: Mt19937::new(DEFAULT_SEED),
            fuse_phase: false,
        })
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
        let mut program = compile(function, iterations, &self.gates)?;
        if self.fuse_phase {
            program.ops = fuse_phase_runs(program.ops).0;
        }
        self.backend
            .execute(&mut self.state, &program, repetitions)
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
            simulator
                .run("H,H,H;Z,R1,R2;R3,Z,R1;H,H,H", 1, 1)
                .unwrap();
            simulator.state().clone()
        };
        assert!(run(false).max_abs_diff(&run(true)) < 1e-5);
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
