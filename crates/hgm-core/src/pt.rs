//! A porta compilada ("PT" no C++, `include/core/common.h`).
//!
//! Porta de fase fundida (`FusePhaseRuns`) e agendamento por comutação
//! (`ScheduleForBatching`) ainda NÃO foram portados: entram junto com o
//! otimizador (fase 3/4 de `docs/09`). Por isso `Pt` só descreve a porta simples.

use crate::gates::Matrix2;
use crate::C32;

/// Classificação da matriz 2x2, que escolhe o caminho de execução mais barato.
/// Os valores numéricos são os do enum `DENSE/DIAG_PRI/DIAG_SEC` do C++ e os que
/// aparecem nos arquivos de `tests/golden`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum MatrixKind {
    /// Mistura o par de amplitudes (ex.: `H`).
    Dense = 0,
    /// Só a diagonal principal (ex.: `Z`, `R1`..`R3`): uma multiplicação por amplitude.
    DiagPri = 1,
    /// Só a anti-diagonal (ex.: `X`, `Y`): troca o par multiplicando.
    DiagSec = 2,
}

/// Uma porta de 1 qubit, possivelmente controlada, pronta para ser aplicada.
#[derive(Clone, Debug)]
pub struct Pt {
    /// Bit do índice do estado em que o alvo mora (`qubits - 1 - posição`).
    pub target_bit: u32,
    pub matrix: Matrix2,
    /// Bits que são controles.
    pub control_mask: u64,
    /// Valor exigido em `índice & control_mask`.
    pub control_value: u64,
    /// Bit de cada controle, na ordem em que aparecem no passo.
    pub control_bits: Vec<u32>,
}

impl Pt {
    pub fn control_count(&self) -> usize {
        self.control_bits.len()
    }

    /// Mesmo critério de `PT::matrixType()`: comparação exata com zero.
    pub fn kind(&self) -> MatrixKind {
        let zero = C32::new(0.0, 0.0);
        let m = &self.matrix;
        if m[1] == zero && m[2] == zero {
            MatrixKind::DiagPri
        } else if m[0] == zero && m[3] == zero {
            MatrixKind::DiagSec
        } else {
            MatrixKind::Dense
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GateCatalog;

    fn pt_of(name: &str) -> Pt {
        Pt {
            target_bit: 0,
            matrix: GateCatalog::with_base_gates().get(name).unwrap(),
            control_mask: 0,
            control_value: 0,
            control_bits: vec![],
        }
    }

    #[test]
    fn classification_matches_cpp() {
        assert_eq!(pt_of("H").kind(), MatrixKind::Dense);
        assert_eq!(pt_of("X").kind(), MatrixKind::DiagSec);
        assert_eq!(pt_of("Y").kind(), MatrixKind::DiagSec);
        assert_eq!(pt_of("Z").kind(), MatrixKind::DiagPri);
        assert_eq!(pt_of("R1").kind(), MatrixKind::DiagPri);
        assert_eq!(pt_of("R3").kind(), MatrixKind::DiagPri);
    }
}
