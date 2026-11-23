//! `GpuBackend`: a GPU como [`Backend`] do motor (`t_GPU` do C++).
//!
//! Dois modos:
//!
//! - **ida e volta** (padrão): cada `execute` sobe o estado do host, roda as portas na
//!   GPU e traz o estado de volta, como o `t_GPU` do `v1.1.0`. Seguro para quem chama
//!   `Backend::execute` direto com estados quaisquer.
//! - **residente** ([`GpuBackend::resident`], o que o `Simulator` usa): o estado sobe uma
//!   vez e fica na GPU; execução, medição, colapso e multiplicação modular rodam lá, e o
//!   vetor só volta ao host quando alguém o pede inteiro (`sync_to_host`). O contrato é
//!   que o `StateVector` do host seja sempre o mesmo, trocado só pelos avisos
//!   `host_state_replaced` e `set_basis`.
//!
//! O dispositivo é um só por processo ([`shared_context`]): vários `GpuContext`
//! destruídos ao mesmo tempo travaram ou derrubaram o processo na A4500 (`docs/09` §4.2),
//! então o contexto mora num estático que nunca é destruído.

use std::sync::OnceLock;

use hgm_core::circuit::Program;
use hgm_core::error::{HgmError, Result as CoreResult};
use hgm_core::measure::{pick_branch, qubit_shift};
use hgm_core::modmult::plan_mod_mult;
use hgm_core::optimize::schedule_for_batching;
use hgm_core::{Backend, Measurement, Mt19937, StateVector, C32};

use crate::context::{CollapseMode, GpuContext, GpuOptions, GpuState, RunTuning};
use crate::error::{GpuError, Result};
use crate::plan::{ops_of, region_shape};

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
    match error {
        // O modo híbrido recua para a CPU neste caso (ver `hgm_engine::hybrid`).
        GpuError::OutOfMemory(message) => HgmError::DeviceOutOfMemory(message),
        other => HgmError::Backend(other.to_string()),
    }
}

fn out_of_state(index: usize, qubits: usize) -> HgmError {
    HgmError::InvalidArgument(format!(
        "índice {index} fora de um estado de {qubits} qubits"
    ))
}

/// Backend de GPU.
pub struct GpuBackend {
    context: &'static GpuContext,
    tuning: RunTuning,
    /// Portas por lote do agendador por comutação (0 = não reordena).
    batch: usize,
    /// Modo residente (ver o cabeçalho do módulo).
    resident: bool,
    /// Maior bloco do estado (log2 de amplitudes) que o pedido aceita; o dispositivo pode
    /// limitar mais. Só serve para testar a divisão em blocos com estados pequenos.
    max_chunk_bits: u32,
    /// O estado na GPU, no modo residente.
    device: Option<GpuState>,
    /// A GPU tem um estado mais novo que o do host (a cópia do host está velha).
    device_ahead: bool,
}

impl GpuBackend {
    /// Abre (ou reaproveita) o dispositivo do processo.
    pub fn new(tuning: RunTuning) -> Result<Self> {
        Ok(Self {
            context: shared_context()?,
            tuning,
            batch: DEFAULT_BATCH,
            resident: false,
            max_chunk_bits: u32::MAX,
            device: None,
            device_ahead: false,
        })
    }

    /// Tamanho do lote do agendador (`BLOCK_SIZE` do C++); 0 desliga a reordenação.
    pub fn with_batch(mut self, batch: usize) -> Self {
        self.batch = batch;
        self
    }

    /// Liga o modo residente (ver o cabeçalho do módulo).
    pub fn resident(mut self, resident: bool) -> Self {
        self.resident = resident;
        self
    }

    /// Limita o bloco do estado a `2^bits` amplitudes (o dispositivo já impõe o seu limite;
    /// isto força a divisão em blocos em estados que caberiam num só).
    pub fn with_max_chunk_bits(mut self, bits: u32) -> Self {
        self.max_chunk_bits = bits;
        self
    }

    pub fn context(&self) -> &'static GpuContext {
        self.context
    }

    /// O programa na ordem em que a GPU o roda: reordenado por comutação, se o lote não
    /// é 0 (o resultado não muda).
    fn ordered(&self, program: &Program, qubits: usize) -> Program {
        let ops = if self.batch > 0 {
            let region_bits = self.tuning.region_bits.min(self.context.max_region_bits());
            // A janela fica nos bits locais do bloco (ver `plan.rs`).
            let local = self.context.local_bits(qubits as u32, self.max_chunk_bits);
            let (coalesced, window) = region_shape(local, self.tuning.coalesced_bits, region_bits);
            schedule_for_batching(program.ops.clone(), coalesced, window, self.batch)
        } else {
            program.ops.clone()
        };
        Program {
            qubits: program.qubits,
            ops,
        }
    }

    /// Garante um estado na GPU (sobe o do host se ainda não há um) e o devolve.
    fn device_state(&mut self, host: &StateVector) -> CoreResult<&mut GpuState> {
        let qubits = u32::try_from(host.qubits())
            .map_err(|_| HgmError::InvalidArgument("número de qubits".to_owned()))?;
        let usable = self.device.as_ref().is_some_and(|d| d.qubits() == qubits);
        if !usable {
            self.device = Some(
                self.context
                    .state_from_in_chunks(qubits, host.as_slice(), self.max_chunk_bits)
                    .map_err(backend_error)?,
            );
            self.device_ahead = false;
        }
        self.device
            .as_mut()
            .ok_or_else(|| HgmError::Backend("estado de GPU ausente".to_owned()))
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
        let ops = ops_of(&self.ordered(program, state.qubits()), 1).map_err(backend_error)?;
        if ops.is_empty() || repetitions == 0 {
            return Ok(());
        }
        let context = self.context;
        let tuning = self.tuning;

        if !self.resident {
            let qubits = u32::try_from(state.qubits())
                .map_err(|_| HgmError::InvalidArgument("número de qubits".to_owned()))?;
            let gpu_state = context
                .state_from_in_chunks(qubits, state.as_slice(), self.max_chunk_bits)
                .map_err(backend_error)?;
            let prepared = context
                .prepare_ops(&gpu_state, &ops, tuning)
                .map_err(backend_error)?;
            context.run(&prepared, repetitions).map_err(backend_error)?;
            context
                .read_into(&gpu_state, state.as_mut_slice())
                .map_err(backend_error)?;
            return Ok(());
        }

        let device = self.device_state(state)?;
        let prepared = context
            .prepare_ops(device, &ops, tuning)
            .map_err(backend_error)?;
        context.run(&prepared, repetitions).map_err(backend_error)?;
        self.device_ahead = true;
        Ok(())
    }

    fn initial_state_ready(&mut self, state: &mut StateVector) -> CoreResult<()> {
        if self.resident {
            // |0...0> direto na GPU; o host (também em |0...0>) não precisa subir.
            self.set_basis(state, 0)?;
        }
        Ok(())
    }

    fn host_state_replaced(&mut self) {
        self.device = None;
        self.device_ahead = false;
    }

    fn set_basis(&mut self, state: &mut StateVector, index: usize) -> CoreResult<()> {
        if !self.resident {
            *state = StateVector::basis(state.qubits(), index)?;
            return Ok(());
        }
        // Sem alocar nem subir um vetor de 2^n: a GPU zera o buffer e recebe uma
        // amplitude. A cópia do host fica velha até alguém pedi-la.
        if index >= state.as_slice().len() {
            return Err(out_of_state(index, state.qubits()));
        }
        let qubits = u32::try_from(state.qubits())
            .map_err(|_| HgmError::InvalidArgument("número de qubits".to_owned()))?;
        self.device = Some(
            self.context
                .basis_state_in_chunks(qubits, index as u32, self.max_chunk_bits)
                .map_err(backend_error)?,
        );
        self.device_ahead = true;
        Ok(())
    }

    fn sync_to_host(&mut self, state: &mut StateVector) -> CoreResult<()> {
        if self.device_ahead {
            if let Some(device) = &self.device {
                self.context
                    .read_into(device, state.as_mut_slice())
                    .map_err(backend_error)?;
            }
            self.device_ahead = false;
        }
        Ok(())
    }

    fn measure_qubit(
        &mut self,
        state: &mut StateVector,
        qubit: usize,
        rng: &mut Mt19937,
    ) -> CoreResult<Measurement> {
        if !self.resident {
            return state.measure_qubit(qubit, rng);
        }
        let shift = qubit_shift(state.qubits(), qubit)?;
        let context = self.context;
        let device = self.device_state(state)?;
        let (zero, one) = context.measure_sums(device, shift).map_err(backend_error)?;
        // O sorteio e a escolha do ramo são os do host (`pick_branch`): o mesmo gerador,
        // gasto do mesmo jeito.
        let (bit, branch) = pick_branch(zero, one, rng)?;
        let scale = (1.0 / branch.sqrt()) as f32;
        context
            .collapse_bit(device, shift, bit, scale, CollapseMode::Measure)
            .map_err(backend_error)?;
        self.device_ahead = true;
        Ok(Measurement {
            bit,
            probability: branch as f32,
        })
    }

    fn collapse(&mut self, state: &mut StateVector, qubit: usize, value: u8) -> CoreResult<f64> {
        if !self.resident {
            return state.collapse(qubit, value);
        }
        let shift = qubit_shift(state.qubits(), qubit)?;
        if value > 1 {
            return Err(HgmError::InvalidArgument(format!(
                "valor {value} não é 0 nem 1"
            )));
        }
        let context = self.context;
        let device = self.device_state(state)?;
        let (zero, one) = context.measure_sums(device, shift).map_err(backend_error)?;
        let sum = if value == 1 { one } else { zero };
        if sum <= 0.0 {
            return Err(HgmError::ZeroProbability);
        }
        context
            .collapse_bit(
                device,
                shift,
                value,
                (1.0 / sum.sqrt()) as f32,
                CollapseMode::InPlace,
            )
            .map_err(backend_error)?;
        self.device_ahead = true;
        Ok(sum)
    }

    fn apply_mod_mult(
        &mut self,
        state: &mut StateVector,
        ctrl_qubit: usize,
        reg_first_qubit: usize,
        width: usize,
        multiplier: i64,
        modulus: i64,
    ) -> CoreResult<()> {
        if !self.resident {
            return state.apply_mod_mult(ctrl_qubit, reg_first_qubit, width, multiplier, modulus);
        }
        let plan = plan_mod_mult(
            state.qubits(),
            ctrl_qubit,
            reg_first_qubit,
            width,
            multiplier,
            modulus,
        )?;
        let context = self.context;
        let device = self.device_state(state)?;
        context.mod_mult(device, &plan).map_err(backend_error)?;
        self.device_ahead = true;
        Ok(())
    }

    fn amplitude(&mut self, state: &mut StateVector, index: usize) -> CoreResult<C32> {
        if index >= state.as_slice().len() {
            return Err(out_of_state(index, state.qubits()));
        }
        if !self.resident {
            return Ok(state.as_slice()[index]);
        }
        let context = self.context;
        let device = self.device_state(state)?;
        let read = context
            .read_range(device, index as u64, 1)
            .map_err(backend_error)?;
        read.first()
            .copied()
            .ok_or_else(|| HgmError::Backend("leitura de amplitude vazia".to_owned()))
    }
}
