//! Multiplicação modular controlada emulada como permutação dos índices do estado
//! (`DGM::applyModMult`, item 29 de `docs/07`).
//!
//! Escrita como "gather": a amplitude nova na posição `d` vem da posição `src(d)` da
//! antiga, então cada destino é escrito uma vez e a troca de buffers fecha.

use std::mem;

use crate::error::{HgmError, Result};
use crate::state::StateVector;
use crate::C32;

fn invalid(message: String) -> HgmError {
    HgmError::InvalidArgument(message)
}

fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

impl StateVector {
    /// `|ctrl> |x>  ->  |ctrl> |a·x mod N>` quando o controle vale 1 e `x < N`; o resto
    /// do espaço não muda. O registrador tem `width` qubits a partir de
    /// `reg_first_qubit`, e o qubit de controle precisa estar acima dele no índice
    /// (`ctrl_qubit < reg_first_qubit`). `multiplier` precisa ser coprimo com `modulus`.
    ///
    /// Onde o C++ fazia `exit(1)` por parâmetro inválido, devolve erro.
    pub fn apply_mod_mult(
        &mut self,
        ctrl_qubit: usize,
        reg_first_qubit: usize,
        width: usize,
        multiplier: i64,
        modulus: i64,
    ) -> Result<()> {
        let qubits = self.qubits;
        if !(1..=16).contains(&width)
            || modulus < 2
            || modulus > (1i64 << width)
            || multiplier < 1
            || ctrl_qubit >= qubits
            || reg_first_qubit + width > qubits
            || (reg_first_qubit..reg_first_qubit + width).contains(&ctrl_qubit)
        {
            return Err(invalid(format!(
                "applyModMult: ctrl={ctrl_qubit}, registrador={reg_first_qubit}+{width}, modulus={modulus}, qubits={qubits}"
            )));
        }

        let a = multiplier % modulus;
        if gcd(a, modulus) != 1 {
            return Err(invalid(format!(
                "applyModMult exige multiplicador coprimo com o módulo ({multiplier} e {modulus})"
            )));
        }

        // Posições de bit no índice (o qubit q é o bit qubits-1-q).
        let ctrl_shift = qubits - 1 - ctrl_qubit;
        let reg_shift = qubits - reg_first_qubit - width;
        if ctrl_shift < reg_shift + width {
            return Err(invalid(
                "applyModMult exige o qubit de controle acima do registrador no índice".to_owned(),
            ));
        }

        // gather[y] = x tal que (a*x) mod modulus == y
        let mut gather = vec![0usize; modulus as usize];
        for x in 0..modulus {
            gather[((a * x) % modulus) as usize] = x as usize;
        }

        let len = self.amplitudes.len();
        let mut target = match self.scratch.take() {
            Some(buffer) => buffer,
            None => {
                let mut buffer = Vec::new();
                buffer
                    .try_reserve_exact(len)
                    .map_err(|_| HgmError::OutOfMemory { qubits })?;
                buffer.resize(len, C32::new(0.0, 0.0));
                buffer
            }
        };

        let run = 1usize << reg_shift; // amplitudes contíguas que andam juntas
        let reg_values = 1usize << width;
        let jobs = (len >> (reg_shift + width)) * reg_values;
        for job in 0..jobs {
            let upper = job >> width;
            let dest_x = job & (reg_values - 1);
            let dest_base = (upper << (reg_shift + width)) | (dest_x << reg_shift);
            let mut src_x = dest_x;
            if (dest_base >> ctrl_shift) & 1 == 1 && (dest_x as i64) < modulus {
                src_x = gather[dest_x];
            }
            let src_base = dest_base ^ ((dest_x ^ src_x) << reg_shift);
            target[dest_base..dest_base + run]
                .copy_from_slice(&self.amplitudes[src_base..src_base + run]);
        }

        self.scratch = Some(mem::replace(&mut self.amplitudes, target));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1 qubit de controle (qubit 0) + registrador de 4 qubits (qubits 1..5) = 5 qubits.
    fn with_value(ctrl: u8, x: usize) -> StateVector {
        let index = (usize::from(ctrl) << 4) | x;
        StateVector::basis(5, index).unwrap()
    }

    fn only_index(state: &StateVector) -> usize {
        let hits: Vec<usize> = state
            .amplitudes
            .iter()
            .enumerate()
            .filter(|(_, a)| a.re != 0.0 || a.im != 0.0)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(hits.len(), 1);
        hits[0]
    }

    #[test]
    fn multiplies_only_when_the_control_is_one() {
        let mut on = with_value(1, 4);
        on.apply_mod_mult(0, 1, 4, 7, 15).unwrap();
        assert_eq!(only_index(&on), (1 << 4) | ((7 * 4) % 15));

        let mut off = with_value(0, 4);
        off.apply_mod_mult(0, 1, 4, 7, 15).unwrap();
        assert_eq!(only_index(&off), 4);
    }

    #[test]
    fn values_at_or_above_the_modulus_are_untouched() {
        let mut state = with_value(1, 15);
        state.apply_mod_mult(0, 1, 4, 7, 15).unwrap();
        assert_eq!(only_index(&state), (1 << 4) | 15);
    }

    #[test]
    fn works_twice_reusing_the_scratch_buffer() {
        let mut state = with_value(1, 2);
        state.apply_mod_mult(0, 1, 4, 7, 15).unwrap();
        state.apply_mod_mult(0, 1, 4, 13, 15).unwrap(); // 13 = 7^-1 mod 15
        assert_eq!(only_index(&state), (1 << 4) | 2);
    }

    #[test]
    fn invalid_arguments_are_errors() {
        let mut state = with_value(1, 2);
        assert!(state.apply_mod_mult(0, 1, 4, 5, 15).is_err()); // gcd(5, 15) != 1
        assert!(state.apply_mod_mult(1, 1, 4, 7, 15).is_err()); // controle dentro do registrador
        assert!(state.apply_mod_mult(0, 1, 4, 7, 17).is_err()); // modulus > 2^width
        assert!(state.apply_mod_mult(0, 2, 4, 7, 15).is_err()); // registrador sai do estado
        assert!(state.apply_mod_mult(4, 0, 4, 7, 15).is_err()); // controle abaixo do registrador
    }
}
