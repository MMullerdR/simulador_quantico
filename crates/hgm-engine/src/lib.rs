//! O motor: junta o parser, o estado e um backend (`DGM` do C++, sem o que já é
//! de outros crates). Medição, colapso, otimizador e modo híbrido entram nas
//! próximas fases de `docs/09`.

use hgm_core::circuit::compile;
use hgm_core::error::Result;
use hgm_core::{Backend, GateCatalog, StateVector};
use hgm_cpu::SerialBackend;

/// Backends disponíveis. `CpuParallel`, `Gpu` e `Hybrid` entram com as fases 3-5.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    /// `t_CPU`: serial, o oráculo dos demais.
    CpuSerial,
}

pub struct Simulator {
    state: StateVector,
    backend: Box<dyn Backend>,
    gates: GateCatalog,
}

impl Simulator {
    /// Simulador em `|0...0>` com as portas base.
    pub fn new(kind: BackendKind, qubits: usize) -> Result<Self> {
        let backend: Box<dyn Backend> = match kind {
            BackendKind::CpuSerial => Box::new(SerialBackend::new()),
        };
        Ok(Self {
            state: StateVector::basis(qubits, 0)?,
            backend,
            gates: GateCatalog::with_base_gates(),
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

    /// Compila `function` (passos separados por `;`, repetidos `iterations` vezes)
    /// e a executa `repetitions` vezes sobre o estado atual.
    pub fn run(&mut self, function: &str, iterations: usize, repetitions: u32) -> Result<()> {
        let program = compile(function, iterations, &self.gates)?;
        self.backend.execute(&mut self.state, &program, repetitions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grover_like_round_keeps_the_state_normalised() {
        let mut simulator = Simulator::new(BackendKind::CpuSerial, 3).unwrap();
        simulator.run("H,H,H", 1, 1).unwrap();
        simulator
            .run("Control1(1),Control1(1),Target1(Z)", 1, 1)
            .unwrap();
        simulator.run("H,H,H;X,X,X", 1, 1).unwrap();
        assert!((simulator.state().norm_sqr() - 1.0).abs() < 1e-5);
        assert_eq!(simulator.backend_name(), "t_CPU (Rust, serial)");
    }

    #[test]
    fn unknown_gate_surfaces_as_an_error() {
        let mut simulator = Simulator::new(BackendKind::CpuSerial, 2).unwrap();
        assert!(simulator.run("Q,ID", 1, 1).is_err());
    }
}
