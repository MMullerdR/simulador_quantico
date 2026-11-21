//! Porta de fase fundida (item 27 de `docs/07`; `PhaseTerm` em `include/core/common.h`).
//!
//! Várias portas `diag(1, e^{iθ})` (Z, R1–R3, rotações da QFT e dos somadores do
//! Shor, com ou sem controles de valor 0/1) comutam entre si e viram UMA operação
//! sobre o vetor de estado:
//!
//! ```text
//! amplitude[i] *= e^{ i·2π·soma(steps dos termos com (i & mask) == value) / 2^16 }
//! ```
//!
//! `steps` é inteiro de propósito: a soma é exata (módulo `2^16`) e a exponencial
//! sai de uma tabela calculada uma vez em `f64` e arredondada para `f32`.

use std::sync::OnceLock;

use crate::C32;

/// Bits de resolução do ângulo: uma volta = `2^16` passos.
pub const PHASE_BITS: u32 = 16;
pub const PHASE_STEPS: u64 = 1 << PHASE_BITS;
/// Máximo de termos distintos por porta fundida.
pub const MAX_TERMS: usize = 64;

/// Um termo: onde `(índice & mask) == value`, soma `steps` ao ângulo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhaseTerm {
    /// Bits que precisam valer o que `value` diz (controles e alvo).
    pub mask: u64,
    pub value: u64,
    /// Ângulo em unidades de `2π / 2^16`, já reduzido a `[0, 2^16)`.
    pub steps: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FusedPhase {
    pub terms: Vec<PhaseTerm>,
}

impl FusedPhase {
    /// Passos de fase acumulados na amplitude `index` (módulo `2^16`).
    pub fn steps_at(&self, index: u64) -> usize {
        let mut steps = 0u64;
        for term in &self.terms {
            if index & term.mask == term.value {
                steps += u64::from(term.steps);
            }
        }
        (steps & (PHASE_STEPS - 1)) as usize
    }
}

/// Tabela `e^{i·2π·k/2^16}`, `k = 0..2^16`, calculada uma vez (thread-safe).
pub fn phase_table() -> &'static [C32] {
    static TABLE: OnceLock<Vec<C32>> = OnceLock::new();
    TABLE.get_or_init(|| {
        (0..PHASE_STEPS)
            .map(|step| {
                let angle = 2.0 * std::f64::consts::PI * step as f64 / PHASE_STEPS as f64;
                C32::new(angle.cos() as f32, angle.sin() as f32)
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_the_expected_landmarks() {
        let table = phase_table();
        assert_eq!(table.len(), 1 << 16);
        assert_eq!(table[0], C32::new(1.0, 0.0));
        assert!((table[1 << 14].im - 1.0).abs() < 1e-6); // um quarto de volta
        assert!((table[1 << 15].re + 1.0).abs() < 1e-6); // meia volta
    }

    #[test]
    fn steps_add_where_the_mask_matches() {
        let fused = FusedPhase {
            terms: vec![
                PhaseTerm {
                    mask: 0b11,
                    value: 0b11,
                    steps: 1 << 15,
                },
                PhaseTerm {
                    mask: 0b10,
                    value: 0b10,
                    steps: 1 << 15,
                },
            ],
        };
        assert_eq!(fused.steps_at(0b00), 0);
        assert_eq!(fused.steps_at(0b10), 1 << 15);
        assert_eq!(fused.steps_at(0b11), 0); // duas meias voltas = volta inteira
    }
}
