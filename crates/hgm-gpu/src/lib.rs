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
//! O que **não** existe ainda (fase 4): portas de fase fundida, `modmult` emulado,
//! sessão residente entre algoritmos, colapso na GPU, estados acima de 27 qubits (limite
//! de binding da A4500; ver `docs/09` §4.2) e a implementação da trait `Backend`. Limites
//! do WGSL (`docs/09` §4.1): sem `f64`, índice `u32` (no máximo 31 qubits aqui) e memória
//! de workgroup pelo limite do adaptador.
//!
//! **Um `GpuContext` por processo.** Vários dispositivos destruídos ao mesmo tempo
//! travaram ou derrubaram o processo na A4500 (`examples/stress.rs`).

pub mod context;
pub mod error;
pub mod plan;
pub mod shaders;

pub use context::{GpuContext, GpuOptions, GpuState, PreparedRun, RunStats, RunTuning};
pub use error::{GpuError, Result};
pub use plan::GpuGate;
