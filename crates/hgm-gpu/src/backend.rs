//! `GpuBackend`: a GPU como [`Backend`] do motor (`t_GPU` do C++).
//!
//! Esta é a forma "ida e volta": cada `execute` sobe o estado do host, roda as portas na
//! GPU e traz o estado de volta, como o `t_GPU` do `v1.1.0`. A sessão residente (estado
//! que fica na GPU entre execuções, medição e colapso lá) é o passo seguinte da fase 4.
//!
//! O dispositivo é um só por processo ([`shared_context`]): vários `GpuContext`
//! destruídos ao mesmo tempo travaram ou derrubaram o processo na A4500 (`docs/09` §4.2),
//! então o contexto mora num estático que nunca é destruído.

use std::sync::OnceLock;

use hgm_core::circuit::Program;
use hgm_core::error::{HgmError, Result as CoreResult};
use hgm_core::optimize::schedule_for_batching;
use hgm_core::{Backend, StateVector};

use crate::context::{GpuContext, GpuOptions, RunTuning};
use crate::error::{GpuError, Result};
use crate::plan::{gates_of, region_shape};

/// Portas por lote do agendador: o `OPS_BLOCK` do C++.
pub const DEFAULT_BATCH: usize = 300;

/// O contexto de GPU do processo, criado na primeira chamada. Uma falha (sem adaptador)
/// também fica guardada: as chamadas seguintes recebem o mesmo erro sem tentar de novo.
pub fn shared_context() -> Result<&'static GpuContext> {
    static CONTEXT: OnceLock<std::result::Result<GpuContext, String>> = OnceLock::new();
    CONTEXT
        .get_or_init(|| GpuContext::new(GpuOptions::default()).map_err(|e| e.to_string()))
        .as_ref()
        .map_err(|message| GpuError::NoAdapter(message.clone()))
}

fn backend_error(error: GpuError) -> HgmError {
    HgmError::Backend(error.to_string())
}

/// Backend de GPU com o estado no host (ida e volta a cada execução).
pub struct GpuBackend {
    context: &'static GpuContext,
    tuning: RunTuning,
    /// Portas por lote do agendador por comutação (0 = não reordena).
    batch: usize,
}

impl GpuBackend {
    /// Abre (ou reaproveita) o dispositivo do processo.
    pub fn new(tuning: RunTuning) -> Result<Self> {
        Ok(Self {
            context: shared_context()?,
            tuning,
            batch: DEFAULT_BATCH,
        })
    }

    /// Tamanho do lote do agendador (`BLOCK_SIZE` do C++); 0 desliga a reordenação.
    pub fn with_batch(mut self, batch: usize) -> Self {
        self.batch = batch;
        self
    }

    pub fn context(&self) -> &'static GpuContext {
        self.context
    }
}

impl Backend for GpuBackend {
    fn name(&self) -> &str {
        "t_GPU (Rust, wgpu)"
    }

    fn execute(
        &mut self,
        state: &mut StateVector,
        program: &Program,
        repetitions: u32,
    ) -> CoreResult<()> {
        if program.qubits != state.qubits() {
            return Err(HgmError::QubitMismatch {
                program: program.qubits,
                state: state.qubits(),
            });
        }
        // Reordena por comutação para gastar menos passadas, como o `ScheduleForBatching`
        // do C++ (ligado por padrão no caminho de GPU). O resultado não muda.
        let ops = if self.batch > 0 {
            let region_bits = self.tuning.region_bits.min(self.context.max_region_bits());
            let (coalesced, window) = region_shape(
                state.qubits() as u32,
                self.tuning.coalesced_bits,
                region_bits,
            );
            schedule_for_batching(program.ops.clone(), coalesced, window, self.batch)
        } else {
            program.ops.clone()
        };
        let ordered = Program {
            qubits: program.qubits,
            ops,
        };
        let gates = gates_of(&ordered, 1).map_err(backend_error)?;
        if gates.is_empty() || repetitions == 0 {
            return Ok(());
        }
        let qubits = u32::try_from(state.qubits())
            .map_err(|_| HgmError::InvalidArgument("número de qubits".to_owned()))?;
        let gpu_state = self
            .context
            .state_from(qubits, state.as_slice())
            .map_err(backend_error)?;
        let prepared = self
            .context
            .prepare(&gpu_state, &gates, self.tuning)
            .map_err(backend_error)?;
        self.context
            .run(&prepared, repetitions)
            .map_err(backend_error)?;
        let result = self.context.read_state(&gpu_state).map_err(backend_error)?;
        state.as_mut_slice().copy_from_slice(&result);
        Ok(())
    }
}
