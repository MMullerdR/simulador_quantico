//! Planejamento de regiões do backend paralelo (`compute_region` e o clamp de
//! `DGM::validateTuning`, `src/core/dgm_par_exec.cpp` e `dgm_core.cpp`).
//!
//! Uma *região* é um conjunto de portas consecutivas cujos alvos cabem numa máscara
//! de até `region_bits` bits (os `coalesced_bits` mais baixos mais os bits dos alvos
//! das portas seguintes). Cada região da máscara é processada por uma thread, com
//! todas as portas do lote aplicadas de uma vez, em vez de uma passada por porta.

use hgm_core::error::{HgmError, Result};
use hgm_core::pt::Op;

/// Parâmetros de ajuste da CPU paralela (`cpu_region_bits`, `cpu_coalesced_bits`,
/// `thread_count` do C++; os CLIs usam região 14 e coalescida 11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tuning {
    pub threads: usize,
    /// Bits baixos sempre incluídos na região (contíguos na memória).
    pub coalesced_bits: u32,
    /// Bits da região (`2^region_bits` amplitudes por região).
    pub region_bits: u32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
            coalesced_bits: 11,
            region_bits: 14,
        }
    }
}

impl Tuning {
    /// Ajusta para um circuito de `qubits` qubits (`DGM::validateTuning`):
    /// a região e a faixa coalescida não passam dos qubits, e uma região menor que o
    /// circuito precisa ser maior que a faixa coalescida (senão não absorve nenhuma
    /// porta com alvo fora dela e o laço não avançaria).
    pub fn validated(mut self, qubits: u32) -> Self {
        self.threads = self.threads.max(1);
        self.region_bits = self.region_bits.min(qubits);
        self.coalesced_bits = self.coalesced_bits.min(qubits);
        if self.region_bits < qubits && self.coalesced_bits >= self.region_bits {
            self.coalesced_bits = self.region_bits.saturating_sub(1);
        }
        self
    }
}

/// Uma região planejada a partir de `op_start`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionPlan {
    /// Bits que a região enumera.
    pub mask: u64,
    /// Bits efetivos (pode ser menor que o pedido se o circuito não tem tantos qubits).
    pub bits: u32,
    /// Primeiro índice de `ops` fora desta região.
    pub op_end: usize,
}

/// Agrupa as operações consecutivas a partir de `ops[op_start]` numa região de até
/// `region_bits` bits (`compute_region`). `qubits` limita os bits que podem entrar
/// no preenchimento final. Devolve erro se nenhuma operação coubesse (o C++ ficava
/// num laço infinito).
pub fn compute_region(
    ops: &[Op],
    op_start: usize,
    coalesced_bits: u32,
    region_bits: u32,
    qubits: u32,
) -> Result<RegionPlan> {
    let mut count = coalesced_bits;
    let mut mask: u64 = if coalesced_bits > 0 {
        (1u64 << coalesced_bits) - 1
    } else {
        0
    };

    let mut op_index = op_start;
    while count < region_bits && op_index < ops.len() {
        let target = ops[op_index].target_bit();
        if (mask >> target) & 1 == 0 {
            count += 1;
        }
        if count <= region_bits {
            mask |= 1u64 << target;
        }
        op_index += 1;
    }
    while op_index < ops.len() && (mask >> ops[op_index].target_bit()) & 1 == 1 {
        op_index += 1;
    }

    // Preenche com os bits livres mais baixos até chegar em `region_bits`.
    let mut bit = 0;
    while count < region_bits && bit < qubits {
        if (mask >> bit) & 1 == 0 {
            mask |= 1u64 << bit;
            count += 1;
        }
        bit += 1;
    }

    if op_index == op_start {
        return Err(HgmError::Backend(
            "a região planejada não absorve nenhuma porta (ajuste de região/coalescida inválido)"
                .to_owned(),
        ));
    }

    Ok(RegionPlan {
        mask,
        bits: region_bits.min(count),
        op_end: op_index,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hgm_core::circuit::compile;
    use hgm_core::GateCatalog;

    fn ops(function: &str) -> Vec<Op> {
        compile(function, 1, &GateCatalog::with_base_gates())
            .unwrap()
            .ops
    }

    #[test]
    fn tuning_is_clamped_like_validate_tuning() {
        let tuning = Tuning {
            threads: 0,
            coalesced_bits: 11,
            region_bits: 14,
        };
        let small = tuning.validated(10);
        assert_eq!((small.region_bits, small.coalesced_bits), (10, 10));
        assert_eq!(small.threads, 1);

        let big = tuning.validated(20);
        assert_eq!((big.region_bits, big.coalesced_bits), (14, 11));

        let equal = Tuning {
            threads: 2,
            coalesced_bits: 5,
            region_bits: 5,
        }
        .validated(9);
        assert_eq!((equal.region_bits, equal.coalesced_bits), (5, 4));
    }

    #[test]
    fn region_grows_with_the_targets_and_stops_when_full() {
        // 6 qubits, alvos (bit) 0..=5 em passo par: ordem crescente de bit.
        let list = ops("H,H,H,H,H,H");
        // coalescida 2, região 4: bits {0,1} + os dois primeiros alvos novos (2 e 3).
        let plan = compute_region(&list, 0, 2, 4, 6).unwrap();
        assert_eq!(plan.mask, 0b001111);
        assert_eq!(plan.bits, 4);
        // As quatro primeiras portas (bits 0..3) cabem; as de bit 4 e 5 não.
        assert_eq!(plan.op_end, 4);

        let rest = compute_region(&list, plan.op_end, 2, 4, 6).unwrap();
        assert_eq!(rest.op_end, 6);
        assert_eq!(rest.mask & 0b110000, 0b110000);
    }

    #[test]
    fn whole_state_region_takes_everything() {
        let list = ops("H,H,H;X,X,X");
        let plan = compute_region(&list, 0, 3, 3, 3).unwrap();
        assert_eq!(plan.mask, 0b111);
        assert_eq!(plan.op_end, list.len());
    }

    #[test]
    fn a_region_that_absorbs_nothing_is_an_error() {
        let list = ops("ID,ID,H");
        // coalescida == região < qubits sem validar: não absorve a porta de bit 0? bit 0 está
        // dentro; usa um alvo de bit alto com a região cheia de bits baixos.
        let high = ops("H,ID,ID");
        assert!(compute_region(&high, 0, 2, 2, 3).is_err());
        assert!(compute_region(&list, 0, 2, 2, 3).is_ok());
    }
}
