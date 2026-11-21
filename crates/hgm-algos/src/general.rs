//! Benchmark de Hadamard (`src/algorithms/lib_general.cpp`): `H` em todos os qubits,
//! `iterations` vezes seguidas, a partir de `|0...0>`.

use hgm_core::error::{HgmError, Result};
use hgm_core::{C32, MAX_QUBITS};
use hgm_engine::{SimConfig, Simulator};

use crate::steps::hadamard;

/// Aplica `H` a todos os qubits `iterations` vezes e devolve a amplitude de
/// `|0...0>`. Com `iterations` ímpar o estado é a superposição uniforme, e toda
/// amplitude vale `1/sqrt(2^qubits)`; com par, volta a `|0...0>`. Devolver só uma
/// amplitude custa O(1), seguro em qualquer número de qubits (imprimir o vetor
/// inteiro seria inviável a partir de ~20).
pub fn hadamard_n_qubits(config: &SimConfig, qubits: usize, iterations: usize) -> Result<C32> {
    if !(1..=MAX_QUBITS).contains(&qubits) {
        return Err(HgmError::InvalidArgument(format!(
            "a quantidade de qubits precisa estar entre 1 e {MAX_QUBITS}, recebi {qubits}"
        )));
    }
    let mut simulator = Simulator::with_config(config, qubits)?;
    let step = hadamard(qubits, 0, qubits);
    simulator.run_steps(&[step], iterations, 1)?;
    Ok(simulator.state().as_slice()[0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use hgm_cpu::Tuning;
    use hgm_engine::BackendKind;

    #[test]
    fn odd_iterations_give_the_uniform_superposition() {
        // Os mesmos valores esperados do `tests/smoke_test.sh` do C++.
        for (qubits, expected) in [(10usize, 0.03125f32), (14, 0.0078125)] {
            let amplitude = hadamard_n_qubits(&SimConfig::default(), qubits, 3).unwrap();
            assert!((amplitude.re - expected).abs() < 2e-6, "{qubits} qubits");
            assert!(amplitude.im.abs() < 1e-9);
        }
    }

    #[test]
    fn even_iterations_return_to_the_ground_state() {
        let amplitude = hadamard_n_qubits(&SimConfig::default(), 6, 2).unwrap();
        assert!((amplitude.re - 1.0).abs() < 1e-5);
    }

    #[test]
    fn parallel_backend_gives_the_same_amplitude() {
        let config = SimConfig::new(
            BackendKind::CpuParallel(Tuning {
                threads: 2,
                coalesced_bits: 3,
                region_bits: 6,
            }),
            1,
        );
        let amplitude = hadamard_n_qubits(&config, 10, 3).unwrap();
        assert!((amplitude.re - 0.03125).abs() < 2e-6);
    }

    #[test]
    fn rejects_bad_qubit_counts() {
        assert!(hadamard_n_qubits(&SimConfig::default(), 0, 1).is_err());
        assert!(hadamard_n_qubits(&SimConfig::default(), 40, 1).is_err());
    }
}
