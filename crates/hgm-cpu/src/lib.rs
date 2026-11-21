//! Backends de CPU do HybriD-GM.
//!
//! - [`serial`]: porta do `t_CPU` (`src/core/dgm_cpu_exec.cpp`), o oráculo dos demais;
//! - [`parallel`]: porta do `t_PAR_CPU` (regiões + `rayon`), em [`region`] e
//!   [`kernels`].
//!
//! `unsafe` é proibido (`deny`) neste crate, com uma única exceção: o módulo
//! `shared`, que concentra o acesso concorrente a regiões disjuntas do vetor de
//! estado atrás de uma API segura e checada.

pub mod kernels;
pub mod parallel;
pub mod region;
pub mod serial;

#[allow(unsafe_code)]
mod shared;

pub use parallel::ParallelBackend;
pub use region::Tuning;
pub use serial::SerialBackend;
