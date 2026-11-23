//! Backend de GPU portátil (wgpu + WGSL): **spike da fase 2** de
//! `docs/09-plano-migracao-rust.md` (go/no-go).
//!
//! O que já existe, para medir o que o portão pede (tempo na RTX A4500 contra o CUDA do
//! `v1.1.0`, ver `docs/11`):
//!
//! - `plan`: agrupa as portas em passadas de região (memória de workgroup) como o
//!   `ApplyValuesC01` do CUDA, mais um modelo em CPU dos kernels para testar sem GPU;
//! - `shaders`: os kernels em WGSL (região, global, medição, cópia de banda);
//! - `context`: dispositivo, pipelines, execução, leitura e medição (soma em `f32` por
//!   workgroup, fechada em `f64` no host).
//!
//! - `backend`: [`GpuBackend`], a trait `Backend` do núcleo, com o estado no host (uma ida e
//!   volta por execução) ou **residente** na GPU (execução, medição, colapso e `modmult`
//!   lá; fase 4), e o contexto único do processo.
//!
//! Portas de fase fundida rodam no kernel de região (`GpuOp::Fused`). O `Simulator` as usa
//! por padrão na GPU (`SimConfig::with_fuse_phase` ou `HGM_FUSE_PHASE` mudam isso): o kernel
//! lê um termo para quatro amplitudes e, no Shor, ganha de 5% a 25% (fase 7 de `docs/09`).
//!
//! **Estados acima do limite de binding** (27 qubits na A4500) são divididos em blocos, um
//! buffer por bloco (`GpuState::chunk_bits`; ver `plan.rs`): até 31 qubits, se a memória da
//! GPU aguenta. Limites do WGSL
//! (`docs/09` §4.1): sem `f64`, índice `u32` (no máximo 31 qubits aqui) e memória de
//! workgroup pelo limite do adaptador.
//!
//! **Um `GpuContext` por processo.** Vários dispositivos destruídos ao mesmo tempo
//! travaram ou derrubaram o processo na A4500 (`examples/stress.rs`).

pub mod backend;
pub mod context;
pub mod error;
pub mod plan;
pub mod shaders;

pub use backend::{shared_context, GpuBackend};
pub use context::{
    CollapseMode, GpuContext, GpuOptions, GpuState, PreparedRun, RunStats, RunTuning,
    MAX_GPU_QUBITS,
};
pub use error::{GpuError, Result};
pub use plan::{GpuGate, GpuOp, GpuTerm};
