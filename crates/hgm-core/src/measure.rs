//! Medição, colapso e distribuição de probabilidade sobre o vetor de estado
//! (`DGM::measure` / `DGM::colapse`, `src/core/dgm_core.cpp`). Caminho de CPU; o
//! de GPU (`GpuMeasureSums` / `GpuCollapse`) vem com o backend de GPU.

use std::collections::BTreeMap;

use crate::error::{HgmError, Result};
use crate::rng::Mt19937;
use crate::state::StateVector;
use crate::C32;

/// Resultado de medir um qubit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measurement {
    /// 0 ou 1.
    pub bit: u8,
    /// Probabilidade do ramo medido, como `float` (o `measure_value` do C++).
    pub probability: f32,
}

fn probability(amplitude: C32) -> f64 {
    let (re, im) = (f64::from(amplitude.re), f64::from(amplitude.im));
    re * re + im * im
}

/// `amplitude / divisor` com o divisor em `double`, como `float complex / double`
/// em C: componentes divididos em `double` e arredondados uma vez.
fn divide(amplitude: C32, divisor: f64) -> C32 {
    C32::new(
        (f64::from(amplitude.re) / divisor) as f32,
        (f64::from(amplitude.im) / divisor) as f32,
    )
}

impl StateVector {
    fn bit_shift(&self, qubit: usize) -> Result<u32> {
        if qubit >= self.qubits {
            return Err(HgmError::InvalidArgument(format!(
                "qubit {qubit} fora de um estado de {} qubits",
                self.qubits
            )));
        }
        Ok((self.qubits - 1 - qubit) as u32)
    }

    /// Mede o qubit `qubit` (posição no circuito): soma as probabilidades dos dois
    /// lados em `f64`, sorteia com `rng` (uma amostra convertida para `f32`, como no
    /// C++: sai 0 se `P(0) > amostra`, senão 1), colapsa e renormaliza.
    ///
    /// Desvio do C++: se o ramo sorteado tem probabilidade 0 (só acontece por erro de
    /// arredondamento em estados quase determinísticos; o C++ produzia NaN), mede o
    /// outro ramo.
    pub fn measure_qubit(&mut self, qubit: usize, rng: &mut Mt19937) -> Result<Measurement> {
        let shift = self.bit_shift(qubit)?;

        let (mut zero, mut one) = (0.0f64, 0.0f64);
        for (index, &amplitude) in self.amplitudes.iter().enumerate() {
            if (index >> shift) & 1 == 1 {
                one += probability(amplitude);
            } else {
                zero += probability(amplitude);
            }
        }

        let sample = rng.uniform_f64() as f32;
        let mut bit = u8::from(zero <= f64::from(sample));
        if (bit == 0 && zero <= 0.0) || (bit == 1 && one <= 0.0) {
            bit ^= 1;
        }
        let branch = if bit == 1 { one } else { zero };
        if branch <= 0.0 {
            return Err(HgmError::ZeroProbability);
        }
        let norm = branch.sqrt();

        let low_bits_mask = (1usize << shift) - 1;
        let measured_mask = usize::from(bit) << shift;
        for half in 0..self.amplitudes.len() / 2 {
            let pos = (half << 1) - (half & low_bits_mask);
            self.amplitudes[pos] = divide(self.amplitudes[pos | measured_mask], norm);
            self.amplitudes[pos | (1usize << shift)] = C32::new(0.0, 0.0);
        }

        Ok(Measurement {
            bit,
            probability: branch as f32,
        })
    }

    /// Força o qubit a `value` (0 ou 1), zerando as amplitudes incompatíveis e
    /// renormalizando o resto (`DGM::colapse`). Devolve a soma de probabilidades do
    /// ramo antes de normalizar (o C++ imprimia esse número).
    pub fn collapse(&mut self, qubit: usize, value: u8) -> Result<f64> {
        let shift = self.bit_shift(qubit)?;
        if value > 1 {
            return Err(HgmError::InvalidArgument(format!(
                "valor {value} não é 0 nem 1"
            )));
        }

        let matches = |index: usize| ((index >> shift) & 1) == usize::from(value);
        let sum: f64 = self
            .amplitudes
            .iter()
            .enumerate()
            .filter(|&(index, _)| matches(index))
            .map(|(_, &amplitude)| probability(amplitude))
            .sum();
        if sum <= 0.0 {
            return Err(HgmError::ZeroProbability);
        }

        let norm = sum.sqrt();
        for (index, amplitude) in self.amplitudes.iter_mut().enumerate() {
            *amplitude = if matches(index) {
                divide(*amplitude, norm)
            } else {
                C32::new(0.0, 0.0)
            };
        }
        Ok(sum)
    }

    /// Distribuição de probabilidade sobre as combinações dos `qubits` dados, sem
    /// colapsar (`DGM::measure(vector<int>)`). A chave tem os bits desses qubits na
    /// posição que ocupam no índice do estado.
    ///
    /// Desvio do C++: ele acumulava num `float` (arredondando a cada soma); aqui a
    /// soma é em `f64` e o valor devolvido é `f64`.
    pub fn probabilities(&self, qubits: &[usize]) -> Result<BTreeMap<u64, f64>> {
        let mut mask = 0u64;
        for &qubit in qubits {
            mask |= 1u64 << self.bit_shift(qubit)?;
        }
        let mut distribution = BTreeMap::new();
        for (index, &amplitude) in self.amplitudes.iter().enumerate() {
            *distribution.entry(index as u64 & mask).or_insert(0.0) += probability(amplitude);
        }
        Ok(distribution)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bell() -> StateVector {
        let mut state = StateVector::basis(2, 0).unwrap();
        let h = std::f32::consts::FRAC_1_SQRT_2;
        state.amplitudes[0b00] = C32::new(h, 0.0);
        state.amplitudes[0b11] = C32::new(h, 0.0);
        state
    }

    #[test]
    fn measuring_a_bell_pair_gives_correlated_bits() {
        for seed in 0..20 {
            let mut state = bell();
            let mut rng = Mt19937::new(seed);
            let first = state.measure_qubit(0, &mut rng).unwrap();
            let second = state.measure_qubit(1, &mut rng).unwrap();
            assert_eq!(first.bit, second.bit);
            assert!((state.norm_sqr() - 1.0).abs() < 1e-6);
            assert!((second.probability - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn certain_outcomes_never_flip() {
        let mut rng = Mt19937::new(7);
        for _ in 0..50 {
            let mut state = StateVector::basis(3, 0b100).unwrap();
            assert_eq!(state.measure_qubit(0, &mut rng).unwrap().bit, 1);
            assert_eq!(state.measure_qubit(1, &mut rng).unwrap().bit, 0);
        }
    }

    #[test]
    fn collapse_renormalises_and_zeroes_the_rest() {
        let mut state = bell();
        let sum = state.collapse(0, 1).unwrap();
        assert!((sum - 0.5).abs() < 1e-6);
        assert!((state.amplitudes[3].re - 1.0).abs() < 1e-6);
        assert_eq!(state.amplitudes[0], C32::new(0.0, 0.0));
        assert!(state.collapse(0, 0).is_err()); // ramo 0 agora tem probabilidade 0
    }

    #[test]
    fn probabilities_sum_to_one() {
        let distribution = bell().probabilities(&[0]).unwrap();
        assert_eq!(distribution.len(), 2);
        assert!((distribution.values().sum::<f64>() - 1.0).abs() < 1e-6);
        assert!(bell().probabilities(&[2]).is_err());
    }

    #[test]
    fn out_of_range_qubit_is_an_error() {
        let mut rng = Mt19937::new(1);
        assert!(bell().measure_qubit(2, &mut rng).is_err());
        assert!(bell().collapse(0, 2).is_err());
    }
}
