//! Backend de CPU paralelo (`t_PAR_CPU`: `PCpuExecution1` / `PCpuExecution1_0` /
//! `apply_gate` de `src/core/dgm_par_exec.cpp`), com `rayon` no lugar do OpenMP.
//!
//! O programa é cortado em lotes de portas cujos alvos cabem numa região de até
//! `region_bits` bits ([`compute_region`]); cada região do vetor de estado é
//! entregue a uma thread, que aplica o lote inteiro sobre as suas amplitudes numa só
//! passada. Todo este arquivo é código seguro: o acesso concorrente fica em
//! [`crate::shared`].

use hgm_core::backend::Backend;
use hgm_core::circuit::Program;
use hgm_core::error::{HgmError, Result};
use hgm_core::fused::{phase_table, FusedPhase};
use hgm_core::pt::{MatrixKind, Op, Pt};
use hgm_core::state::StateVector;

use crate::kernels::{self, PairKernel};
use crate::region::{compute_region, Tuning};
use crate::serial::validate;
use crate::shared::{par_for_each_region, RegionView};

pub struct ParallelBackend {
    tuning: Tuning,
    pool: rayon::ThreadPool,
}

impl ParallelBackend {
    pub fn new(tuning: Tuning) -> Result<Self> {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(tuning.threads.max(1))
            .build()
            .map_err(|e| HgmError::Backend(format!("não consegui criar o pool de threads: {e}")))?;
        Ok(Self { tuning, pool })
    }

    pub fn tuning(&self) -> Tuning {
        self.tuning
    }
}

impl Backend for ParallelBackend {
    fn name(&self) -> &str {
        "t_PAR_CPU (Rust, rayon)"
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

        let qubits = program.qubits as u32;
        let tuning = self.tuning.validated(qubits);
        let ops = &program.ops;
        let amplitudes = state.as_mut_slice();
        let mut region_bits = tuning.region_bits;

        for _ in 0..repetitions {
            let mut op_index = 0;
            while op_index < ops.len() {
                let plan =
                    compute_region(ops, op_index, tuning.coalesced_bits, region_bits, qubits)?;
                // As passadas seguintes partem do valor já reduzido (como no C++).
                region_bits = plan.bits;
                let batch = &ops[op_index..plan.op_end];

                self.pool.install(|| {
                    par_for_each_region(amplitudes, qubits, plan.mask, |view| {
                        apply_batch(view, batch);
                    });
                });
                op_index = plan.op_end;
            }
        }
        Ok(())
    }
}

/// Aplica as portas de um lote a uma região (`PCpuExecution1_0`).
fn apply_batch(view: &mut RegionView<'_>, batch: &[Op]) {
    let (mask, id) = (view.mask(), view.id());
    for op in batch {
        match op {
            Op::FusedPhase(fused) => apply_fused(view, fused),
            Op::Gate(pt) => {
                if pt.control_count() == 0 {
                    apply_gate(view, pt, mask, id);
                } else if (pt.control_mask & id & !mask) == (pt.control_value & !mask) {
                    // Os controles que caem dentro da região saem da máscara (não se
                    // itera sobre eles) e o valor deles entra no id.
                    apply_gate(view, pt, mask & !pt.control_mask, id | pt.control_value);
                }
                // Senão: a parte global do controle não casa com esta região e a porta
                // não faz nada nela.
            }
        }
    }
}

/// `ApplyFusedPhase` restrito à região: `state[id | sub] *= tabela[passos]` para todo
/// subconjunto `sub` da máscara.
fn apply_fused(view: &mut RegionView<'_>, fused: &FusedPhase) {
    let table = phase_table();
    let (mask, id) = (view.mask(), view.id());
    let mut sub = 0u64;
    loop {
        let index = id | sub;
        let amplitude = view.get(index as usize);
        view.set(index as usize, amplitude * table[fused.steps_at(index)]);
        sub = sub.wrapping_sub(mask) & mask;
        if sub == 0 {
            break;
        }
    }
}

/// Aplica uma porta sobre todos os pares da região descrita por `(mask, id)`
/// (`apply_gate`): `mask` são os bits que se enumeram (inclui o alvo; já sem os
/// controles que caem dentro da região) e `id` o valor dos demais (inclui o dos
/// controles). Em blocos contíguos quando os bits baixos da máscara são contíguos;
/// senão, um par por vez.
fn apply_gate(view: &mut RegionView<'_>, pt: &Pt, mask: u64, id: u64) {
    let kind = pt.kind();
    let target_mask = 1u64 << pt.target_bit;
    let region_size = mask.count_ones();
    let low_bits = (!mask).trailing_zeros(); // bits baixos contíguos dentro da região

    if low_bits >= 2 {
        let kernel: PairKernel = match kind {
            MatrixKind::Dense => kernels::dense,
            MatrixKind::DiagPri => kernels::diagonal,
            MatrixKind::DiagSec => kernels::anti_diagonal,
        };
        let run_len = 1usize << low_bits;
        let low_mask = (run_len as u64) - 1;
        let mut pos = 0u64;

        if pt.target_bit >= low_bits {
            // Alvo fora da faixa baixa: `a` e `b` são dois blocos de `run_len`
            // amplitudes, a `target_mask` de distância.
            let outer_mask = mask & !target_mask & !low_mask;
            let outer_count = 1u64 << (region_size - 1 - low_bits);
            let inc = (!outer_mask).wrapping_add(1);
            for _ in 0..outer_count {
                let base = (pos | id) as usize;
                let (a, b) = view.block_pair_mut(base, base + target_mask as usize, run_len);
                kernel(a, b, &pt.matrix);
                pos = pos.wrapping_add(inc) & outer_mask;
            }
        } else {
            // Alvo dentro da faixa baixa: o bloco tem os dois lados de cada par.
            let outer_mask = mask & !low_mask;
            let outer_count = 1u64 << (region_size - low_bits);
            let inc = (!outer_mask).wrapping_add(1);
            for _ in 0..outer_count {
                let block = view.block_mut((pos | id) as usize, run_len);
                kernels::pairs_in_block(block, target_mask as usize, &pt.matrix, kernel);
                pos = pos.wrapping_add(inc) & outer_mask;
            }
        }
        return;
    }

    // Caminho genérico: um par por iteração.
    let m = &pt.matrix;
    let pos_mask = mask & !target_mask;
    let inc = (!pos_mask).wrapping_add(1);
    let pair_count = 1u64 << (region_size - 1);
    let mut pos = 0u64;
    for _ in 0..pair_count {
        let pos0 = (pos | id) as usize;
        let pos1 = pos0 | target_mask as usize;
        pos = pos.wrapping_add(inc) & pos_mask;

        let (s0, s1) = (view.get(pos0), view.get(pos1));
        let (new0, new1) = match kind {
            MatrixKind::Dense => (m[0] * s0 + m[1] * s1, m[2] * s0 + m[3] * s1),
            MatrixKind::DiagPri => (s0 * m[0], m[3] * s1),
            MatrixKind::DiagSec => (m[1] * s1, m[2] * s0),
        };
        view.set(pos0, new0);
        view.set(pos1, new1);
    }
}
