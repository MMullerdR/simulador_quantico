//! Backends de CPU do HybriD-GM.
//!
//! Hoje só o serial ([`serial`]), a porta do `t_CPU` (`src/core/dgm_cpu_exec.cpp`),
//! que é o oráculo dos demais. O paralelo (regiões + rayon, equivalente ao
//! `t_PAR_CPU`) é a fase 3 de `docs/09-plano-migracao-rust.md`.

pub mod serial;

pub use serial::SerialBackend;
