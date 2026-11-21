//! Otimizador de programas: fusão de corridas de portas de fase e agendamento por
//! comutação em lotes. Portas de `src/core/gate_fuse.cpp` e
//! `src/core/gate_schedule.cpp`.
//!
//! As duas passadas só reordenam/fundem operações **equivalentes**: o estado final
//! é o mesmo a menos de arredondamento. A ordem que sai é a do C++ (faz parte do
//! que `tests/golden` compara).

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound::{Excluded, Unbounded};

use crate::fused::{FusedPhase, PhaseTerm, MAX_TERMS, PHASE_STEPS};
use crate::pt::{MatrixKind, Op, Pt};

/// Se `pt` é `diag(1, e^{iθ})` com `θ` múltiplo de `2π/2^16` (com folga para o
/// arredondamento de `float`), devolve o termo de fase equivalente.
fn phase_term_of(pt: &Pt) -> Option<PhaseTerm> {
    if pt.kind() != MatrixKind::DiagPri {
        return None;
    }
    let (m00, m11) = (pt.matrix[0], pt.matrix[3]);
    if m00.re != 1.0 || m00.im != 0.0 {
        return None;
    }

    let (re, im) = (f64::from(m11.re), f64::from(m11.im));
    if ((re * re + im * im).sqrt() - 1.0).abs() > 1e-5 {
        return None;
    }

    let exact = im.atan2(re) / (2.0 * std::f64::consts::PI) * PHASE_STEPS as f64;
    let steps = exact.round() as i64;
    // Ângulo que não é múltiplo de 2π/2^16: fica como porta normal.
    if (exact - steps as f64).abs() > 0.02 {
        return None;
    }

    let target = 1u64 << pt.target_bit;
    Some(PhaseTerm {
        mask: pt.control_mask | target,
        value: pt.control_value | target,
        steps: (steps & (PHASE_STEPS as i64 - 1)) as u32,
    })
}

fn term_of_op(op: &Op) -> Option<PhaseTerm> {
    match op {
        Op::Gate(pt) => phase_term_of(pt),
        Op::FusedPhase(_) => None,
    }
}

/// Funde corridas de portas de fase consecutivas numa só (`FusePhaseRuns`).
/// Devolve as novas operações e quantas portas foram absorvidas (soma dos
/// tamanhos das corridas fundidas). Uma porta sozinha não ganha nada e fica como
/// está; no máximo [`MAX_TERMS`] termos distintos por porta fundida.
///
/// Termos que se cancelam (soma de passos ≡ 0) somem; se todos se cancelam a
/// operação fundida fica sem termos (continua existindo, como no C++).
pub fn fuse_phase_runs(ops: Vec<Op>) -> (Vec<Op>, usize) {
    let mut out = Vec::with_capacity(ops.len());
    let mut absorbed = 0usize;
    let mut iter = ops.into_iter().peekable();

    while let Some(first) = iter.next() {
        let Some(first_term) = term_of_op(&first) else {
            out.push(first);
            continue;
        };

        let mut terms: BTreeMap<(u64, u64), u64> = BTreeMap::new();
        terms.insert(
            (first_term.mask, first_term.value),
            u64::from(first_term.steps),
        );
        let mut run_len = 1usize;

        while let Some(term) = iter.peek().and_then(term_of_op) {
            let key = (term.mask, term.value);
            if !terms.contains_key(&key) && terms.len() >= MAX_TERMS {
                break;
            }
            *terms.entry(key).or_insert(0) += u64::from(term.steps);
            run_len += 1;
            iter.next();
        }

        if run_len < 2 {
            out.push(first);
            continue;
        }

        let fused = terms
            .into_iter()
            .filter_map(|((mask, value), steps)| {
                let steps = (steps & (PHASE_STEPS - 1)) as u32;
                (steps != 0).then_some(PhaseTerm { mask, value, steps })
            })
            .collect();
        absorbed += run_len;
        out.push(Op::FusedPhase(FusedPhase { terms: fused }));
    }

    (out, absorbed)
}

/// Quais qubits (bits) uma operação toca como "Z" (só multiplica fase: controles e
/// alvo diagonal) e como "N" (alvo de matriz não diagonal).
fn z_and_n_bits(op: &Op) -> (u64, u64) {
    match op {
        Op::FusedPhase(fused) => (fused.terms.iter().fold(0, |acc, t| acc | t.mask), 0),
        Op::Gate(pt) => {
            let target = 1u64 << pt.target_bit;
            if pt.kind() == MatrixKind::DiagPri {
                (pt.control_mask | target, 0)
            } else {
                (pt.control_mask, target)
            }
        }
    }
}

/// Reordena `ops` para a GPU gastar menos passadas sobre o vetor de estado
/// (`ScheduleForBatching`; item 24 de `docs/07`).
///
/// Duas operações comutam se nenhum qubit compartilhado é alvo não diagonal de uma
/// delas. Monta-se o grafo de dependências (por qubit: a última porta "N" e as "Z"
/// desde então) e esvazia-se uma lista de prontas em ordem original, enchendo um
/// lote por vez com o critério de janela de `GpuExecution01`: alvo abaixo de
/// `coalesced_bits` entra sempre; os demais precisam caber numa janela de
/// `extra_bits` bits consecutivos; no máximo `max_batch` por lote.
///
/// Programas com menos de 3 operações, ou `max_batch < 1`, ficam como estão.
pub fn schedule_for_batching(
    ops: Vec<Op>,
    coalesced_bits: u32,
    extra_bits: u32,
    max_batch: usize,
) -> Vec<Op> {
    let n = ops.len();
    if n < 3 || max_batch < 1 {
        return ops;
    }

    const MAX_BITS: usize = 64;
    let mut successors: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut pending = vec![0usize; n];
    let mut stamp: Vec<Option<usize>> = vec![None; n];
    let mut last_n: [Option<usize>; MAX_BITS] = [None; MAX_BITS];
    let mut z_since: Vec<Vec<usize>> = vec![Vec::new(); MAX_BITS];

    for (i, op) in ops.iter().enumerate() {
        let (z_bits, n_bits) = z_and_n_bits(op);

        for bit in 0..MAX_BITS {
            let mask = 1u64 << bit;
            if (z_bits | n_bits) & mask == 0 {
                continue;
            }

            // Quem esta operação precisa esperar neste qubit.
            if let Some(last) = last_n[bit] {
                if stamp[last] != Some(i) {
                    stamp[last] = Some(i);
                    successors[last].push(i);
                    pending[i] += 1;
                }
            }
            if n_bits & mask != 0 {
                for &earlier in &z_since[bit] {
                    if stamp[earlier] == Some(i) {
                        continue;
                    }
                    stamp[earlier] = Some(i);
                    successors[earlier].push(i);
                    pending[i] += 1;
                }
            }
        }

        for bit in 0..MAX_BITS {
            let mask = 1u64 << bit;
            if z_bits & mask != 0 {
                z_since[bit].push(i);
            }
            if n_bits & mask != 0 {
                last_n[bit] = Some(i);
                z_since[bit].clear();
            }
        }
    }

    let mut ready: BTreeSet<usize> = (0..n).filter(|&i| pending[i] == 0).collect();
    let mut order: Vec<usize> = Vec::with_capacity(n);

    while order.len() < n {
        let mut batch_size = 0usize;
        // Janela de bits do lote; None até o primeiro alvo fora da região coalescida.
        let mut window: Option<(u32, u32)> = None;
        let mut progress = true;

        while progress && batch_size < max_batch {
            progress = false;
            // Cursor sobre `ready` em ordem crescente, que, como o iterador do
            // `std::set` do C++, continua valendo depois de apagar o elemento atual
            // (e ignora, nesta passada, o que for inserido antes dele).
            let mut cursor = ready.iter().next().copied();

            while let Some(i) = cursor {
                if batch_size >= max_batch {
                    break;
                }
                let bit = ops[i].target_bit();
                let new_window = if bit < coalesced_bits {
                    Some(window)
                } else {
                    match window {
                        None => Some(Some((bit, bit))),
                        Some((min, max)) => {
                            let low = bit.min(min);
                            let high = bit.max(max);
                            (high - low < extra_bits).then_some(Some((low, high)))
                        }
                    }
                };

                let next = ready.range((Excluded(i), Unbounded)).next().copied();
                let Some(new_window) = new_window else {
                    cursor = next;
                    continue;
                };

                window = new_window;
                order.push(i);
                batch_size += 1;
                progress = true;

                ready.remove(&i);
                for &successor in &successors[i] {
                    pending[successor] -= 1;
                    if pending[successor] == 0 {
                        ready.insert(successor);
                    }
                }
                cursor = next;
            }
        }
    }

    // `order` é uma permutação de 0..n.
    let mut slots: Vec<Option<Op>> = ops.into_iter().map(Some).collect();
    order
        .into_iter()
        .map(|i| slots[i].take().expect("índice repetido no agendamento"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::circuit::compile;
    use crate::GateCatalog;

    fn gates(function: &str) -> Vec<Op> {
        compile(function, 1, &GateCatalog::with_base_gates())
            .unwrap()
            .ops
    }

    #[test]
    fn two_phase_gates_fuse_into_one() {
        // Z e R1 em qubits diferentes, ambos fase pura.
        let (ops, absorbed) = fuse_phase_runs(gates("Z,R1"));
        assert_eq!(absorbed, 2);
        assert_eq!(ops.len(), 1);
        assert!(matches!(&ops[0], Op::FusedPhase(f) if f.terms.len() == 2));
    }

    #[test]
    fn a_lone_phase_gate_is_left_alone() {
        let (ops, absorbed) = fuse_phase_runs(gates("Z,ID"));
        assert_eq!(absorbed, 0);
        assert!(matches!(ops[0], Op::Gate(_)));
    }

    #[test]
    fn dense_gates_break_runs() {
        let (ops, absorbed) = fuse_phase_runs(gates("Z,Z;H,ID;ID,Z;ID,Z"));
        // Corrida 1: dois Z (qubits 0 e 1) fundem; H separa; corrida 2: dois Z no
        // mesmo qubit se cancelam (steps 2^15 + 2^15 = 2^16 ≡ 0).
        assert_eq!(absorbed, 4);
        assert_eq!(ops.len(), 3);
        assert!(matches!(&ops[2], Op::FusedPhase(f) if f.terms.is_empty()));
    }

    #[test]
    fn scheduling_keeps_a_permutation_and_small_programs() {
        let ops = gates("H,ID");
        assert_eq!(schedule_for_batching(ops.clone(), 0, 6, 300).len(), 1);

        let ops = gates("H,H,H;X,X,X;Z,Z,Z");
        let n = ops.len();
        let scheduled = schedule_for_batching(ops, 0, 6, 300);
        assert_eq!(scheduled.len(), n);
    }
}
