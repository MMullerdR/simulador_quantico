//! Vetor de estado: `2^n` amplitudes `Complex32`.

use crate::error::{HgmError, Result};
use crate::{C32, MAX_QUBITS};

#[derive(Clone, Debug)]
pub struct StateVector {
    qubits: usize,
    amplitudes: Vec<C32>,
}

impl StateVector {
    /// Estado de base `|index>` (amplitude 1 em `index`, 0 no resto). A alocação é
    /// falível: falta de memória vira erro, não aborto do processo (o C++ dava
    /// `exit` em `new` falho, `docs/07`).
    pub fn basis(qubits: usize, index: usize) -> Result<Self> {
        if qubits > MAX_QUBITS {
            return Err(HgmError::TooManyQubits {
                qubits,
                max: MAX_QUBITS,
            });
        }
        let len = 1usize << qubits;
        assert!(index < len, "índice de base fora do estado");

        let mut amplitudes = Vec::new();
        amplitudes
            .try_reserve_exact(len)
            .map_err(|_| HgmError::OutOfMemory { qubits })?;
        amplitudes.resize(len, C32::new(0.0, 0.0));
        amplitudes[index] = C32::new(1.0, 0.0);
        Ok(Self { qubits, amplitudes })
    }

    /// Lê o formato de `tests/golden`: `float32` little-endian, pares `(re, im)`,
    /// sem cabeçalho.
    pub fn from_f32_le_bytes(qubits: usize, bytes: &[u8]) -> Result<Self> {
        let mut state = Self::basis(qubits, 0)?;
        let expected = state.amplitudes.len() * 8;
        if bytes.len() != expected {
            return Err(HgmError::BadStateSize {
                qubits,
                expected,
                found: bytes.len(),
            });
        }
        // Tamanho já conferido acima: não sobra resto.
        let (pairs, _) = bytes.as_chunks::<8>();
        for (amplitude, pair) in state.amplitudes.iter_mut().zip(pairs) {
            let re = f32::from_le_bytes([pair[0], pair[1], pair[2], pair[3]]);
            let im = f32::from_le_bytes([pair[4], pair[5], pair[6], pair[7]]);
            *amplitude = C32::new(re, im);
        }
        Ok(state)
    }

    pub fn qubits(&self) -> usize {
        self.qubits
    }

    pub fn as_slice(&self) -> &[C32] {
        &self.amplitudes
    }

    pub fn as_mut_slice(&mut self) -> &mut [C32] {
        &mut self.amplitudes
    }

    /// Soma de `|a|^2`, acumulada em `f64` (como as somas de medição do C++).
    pub fn norm_sqr(&self) -> f64 {
        self.amplitudes
            .iter()
            .map(|a| f64::from(a.re) * f64::from(a.re) + f64::from(a.im) * f64::from(a.im))
            .sum()
    }

    /// Maior `|diferença|` (módulo complexo) entre duas amplitudes correspondentes.
    pub fn max_abs_diff(&self, other: &StateVector) -> f32 {
        assert_eq!(self.qubits, other.qubits);
        self.amplitudes
            .iter()
            .zip(&other.amplitudes)
            .map(|(a, b)| (*a - *b).norm())
            .fold(0.0, f32::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basis_state_is_normalised() {
        let state = StateVector::basis(3, 5).unwrap();
        assert_eq!(state.as_slice().len(), 8);
        assert_eq!(state.as_slice()[5], C32::new(1.0, 0.0));
        assert!((state.norm_sqr() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn too_many_qubits_is_an_error() {
        assert!(matches!(
            StateVector::basis(MAX_QUBITS + 1, 0),
            Err(HgmError::TooManyQubits { .. })
        ));
    }

    #[test]
    fn reads_little_endian_pairs() {
        let mut bytes = Vec::new();
        for value in [0.5f32, -0.5, 0.0, 1.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let state = StateVector::from_f32_le_bytes(1, &bytes).unwrap();
        assert_eq!(state.as_slice()[0], C32::new(0.5, -0.5));
        assert_eq!(state.as_slice()[1], C32::new(0.0, 1.0));
        assert!(StateVector::from_f32_le_bytes(1, &bytes[..15]).is_err());
    }
}
