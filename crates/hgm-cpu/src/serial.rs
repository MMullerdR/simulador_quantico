//! Backend serial: aplica cada porta em sequência sobre o vetor de estado inteiro.
//!
//! Porta fiel de `CpuExecution1` / `CpuExecution1_1/2/3` (`docs/03`), mantendo os
//! mesmos laços (inclusive o salto por máscara nas portas controladas) para que o
//! resultado coincida bit a bit com o C++ sem FMA. Índices seguros (sem `unsafe`).

use hgm_core::backend::Backend;
use hgm_core::circuit::Program;
use hgm_core::error::{HgmError, Result};
use hgm_core::pt::{MatrixKind, Pt};
use hgm_core::state::StateVector;
use hgm_core::C32;

#[derive(Debug, Default)]
pub struct SerialBackend;

impl SerialBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Backend for SerialBackend {
    fn name(&self) -> &str {
        "t_CPU (Rust, serial)"
    }

    fn execute(
        &mut self,
        state: &mut StateVector,
        program: &Program,
        repetitions: u32,
    ) -> Result<()> {
        if program.qubits != state.qubits() {
            return Err(HgmError::QubitMismatch {
                program: program.qubits,
                state: state.qubits(),
            });
        }
        validate(program)?;

        let amplitudes = state.as_mut_slice();
        for _ in 0..repetitions {
            for pt in &program.pts {
                apply_pt(amplitudes, pt);
            }
        }
        Ok(())
    }
}

/// Garante que toda porta cabe no estado, para os laços abaixo nunca indexarem
/// fora (no C++ isso era confiança no parser).
fn validate(program: &Program) -> Result<()> {
    let qubits = program.qubits as u32;
    let limit = 1u64 << qubits;
    for pt in &program.pts {
        if pt.target_bit >= qubits {
            return Err(HgmError::PtOutOfRange("bit do alvo"));
        }
        if pt.control_mask >= limit || pt.control_value & !pt.control_mask != 0 {
            return Err(HgmError::PtOutOfRange("máscara ou valor de controle"));
        }
        if pt.control_mask & (1u64 << pt.target_bit) != 0 {
            return Err(HgmError::PtOutOfRange("alvo também é controle"));
        }
    }
    Ok(())
}

/// Aplica uma porta ao vetor de estado inteiro.
pub fn apply_pt(state: &mut [C32], pt: &Pt) {
    match pt.kind() {
        MatrixKind::Dense => dense(state, pt),
        MatrixKind::DiagPri => diagonal_main(state, pt),
        MatrixKind::DiagSec => diagonal_anti(state, pt),
    }
}

/// Matriz densa (ex.: `H`): 4 multiplicações e 2 somas por par `(pos0, pos1)`.
fn dense(state: &mut [C32], pt: &Pt) {
    let m = &pt.matrix;
    let target_mask = 1u64 << pt.target_bit;
    let mem_size = state.len() as u64;

    let mut mix = |pos0: u64, pos1: u64| {
        let (a, b) = (state[pos0 as usize], state[pos1 as usize]);
        state[pos1 as usize] = m[2] * a + m[3] * b;
        state[pos0 as usize] = m[0] * a + m[1] * b;
    };

    if pt.control_count() == 0 {
        for pos in 0..mem_size / 2 {
            let pos0 = (pos * 2) - (pos & (target_mask - 1));
            mix(pos0, pos0 | target_mask);
        }
    } else {
        let mask = !(pt.control_mask | target_mask);
        let inc = (!mask).wrapping_add(1);
        let mut pos = 0u64;
        while pos < mem_size {
            let pos0 = pos | pt.control_value;
            mix(pos0, pos0 | target_mask);
            pos = pos.wrapping_add(inc) & mask;
        }
    }
}

/// Diagonal principal (ex.: `Z`, `R1..R3`): uma multiplicação por amplitude.
fn diagonal_main(state: &mut [C32], pt: &Pt) {
    let m = &pt.matrix;
    let target = pt.target_bit;
    let mem_size = state.len() as u64;

    if pt.control_count() == 0 {
        for (pos, amplitude) in state.iter_mut().enumerate() {
            *amplitude = m[((pos >> target) & 1) * 3] * *amplitude;
        }
    } else {
        // Como no C++, o laço salta só os bits de controle (o alvo varia livre).
        let mask = !pt.control_mask;
        let inc = (!mask).wrapping_add(1);
        let mut pos = 0u64;
        while pos < mem_size {
            let pos0 = pos | pt.control_value;
            let slot = &mut state[pos0 as usize];
            *slot = m[((pos0 >> target) & 1) as usize * 3] * *slot;
            pos = pos.wrapping_add(inc) & mask;
        }
    }
}

/// Anti-diagonal (ex.: `X`, `Y`): cada amplitude troca de posição multiplicada
/// por um fator, metade das multiplicações do caso denso.
fn diagonal_anti(state: &mut [C32], pt: &Pt) {
    let m = &pt.matrix;
    let target_mask = 1u64 << pt.target_bit;
    let mem_size = state.len() as u64;

    let mut swap = |pos0: u64, pos1: u64| {
        let (a, b) = (state[pos0 as usize], state[pos1 as usize]);
        state[pos1 as usize] = m[2] * a;
        state[pos0 as usize] = m[1] * b;
    };

    if pt.control_count() == 0 {
        for pos in 0..mem_size / 2 {
            let pos0 = (pos * 2) - (pos & (target_mask - 1));
            swap(pos0, pos0 | target_mask);
        }
    } else {
        let mask = !(pt.control_mask | target_mask);
        let inc = (!mask).wrapping_add(1);
        let mut pos = 0u64;
        while pos < mem_size {
            let pos0 = pos | pt.control_value;
            swap(pos0, pos0 | target_mask);
            pos = pos.wrapping_add(inc) & mask;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hgm_core::circuit::compile;
    use hgm_core::GateCatalog;

    fn run(function: &str, qubits: usize, start: usize) -> StateVector {
        let program = compile(function, 1, &GateCatalog::with_base_gates()).unwrap();
        let mut state = StateVector::basis(qubits, start).unwrap();
        SerialBackend::new()
            .execute(&mut state, &program, 1)
            .unwrap();
        state
    }

    #[test]
    fn x_flips_the_right_bit() {
        // Qubit 0 do circuito é o bit mais significativo do índice.
        let state = run("X,ID,ID", 3, 0);
        assert_eq!(state.as_slice()[0b100], C32::new(1.0, 0.0));
    }

    #[test]
    fn controlled_x_only_fires_when_control_matches() {
        let on = run("Control1(1),Target1(X)", 2, 0b10);
        assert_eq!(on.as_slice()[0b11], C32::new(1.0, 0.0));
        let off = run("Control1(1),Target1(X)", 2, 0b00);
        assert_eq!(off.as_slice()[0b00], C32::new(1.0, 0.0));
        let negated = run("Control1(0),Target1(X)", 2, 0b00);
        assert_eq!(negated.as_slice()[0b01], C32::new(1.0, 0.0));
    }

    #[test]
    fn bell_state() {
        let state = run("H,ID;Control1(1),Target1(X)", 2, 0);
        let h = std::f32::consts::FRAC_1_SQRT_2;
        assert!((state.as_slice()[0b00].re - h).abs() < 1e-6);
        assert!((state.as_slice()[0b11].re - h).abs() < 1e-6);
        assert!(state.as_slice()[0b01].norm() < 1e-6);
        assert!(state.as_slice()[0b10].norm() < 1e-6);
    }

    #[test]
    fn hadamard_twice_is_identity() {
        let state = run("H,ID,ID;H,ID,ID", 3, 0b101);
        assert!((state.as_slice()[0b101].re - 1.0).abs() < 1e-6);
        assert!((state.norm_sqr() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn controlled_phase_hits_only_the_matching_amplitude() {
        // Z controlado por qubit 0 em |11>: só |11> troca de sinal.
        let state = run("Control1(1),Target1(Z)", 2, 0b11);
        assert_eq!(state.as_slice()[0b11], C32::new(-1.0, 0.0));
    }

    #[test]
    fn mismatched_qubits_are_rejected() {
        let program = compile("H,ID", 1, &GateCatalog::with_base_gates()).unwrap();
        let mut state = StateVector::basis(3, 0).unwrap();
        let error = SerialBackend::new()
            .execute(&mut state, &program, 1)
            .unwrap_err();
        assert!(matches!(error, HgmError::QubitMismatch { .. }));
    }
}
