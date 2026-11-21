//! Núcleo do HybriD-GM: tudo o que independe de CPU paralela, GPU ou I/O.
//!
//! Espelha, em Rust, os blocos "parser" e "estado" de
//! `docs/10-simulador-em-blocos.md`:
//!
//! - [`gates`]: catálogo de portas de 1 qubit (matrizes 2x2);
//! - [`circuit`]: parser da linguagem de circuitos (`docs/02`) para uma lista de
//!   [`Pt`] ("Pauli Term"), com a mesma ordenação por passo do C++;
//! - [`pt`]: a porta compilada, a classificação da matriz e [`Op`];
//! - [`fused`]: porta de fase fundida e a tabela de fases;
//! - [`optimize`]: fusão de fase e agendamento por comutação;
//! - [`state`]: o vetor de estado (`2^n` amplitudes `Complex32`);
//! - [`measure`]: medição, colapso e distribuição de probabilidade;
//! - [`modmult`]: multiplicação modular emulada como permutação (item 29);
//! - [`rng`]: o `std::mt19937` do C++, para sorteios reproduzíveis;
//! - [`backend`]: a trait que CPU, GPU (e CUDA, se houver) implementam.
//!
//! Convenção de índice (igual ao C++, `docs/01`): o qubit `q` do circuito é o bit
//! `qubits - 1 - q` do índice do estado.

pub mod backend;
pub mod circuit;
pub mod error;
pub mod fused;
pub mod gates;
pub mod measure;
pub mod modmult;
pub mod optimize;
pub mod pt;
pub mod rng;
pub mod state;

pub use backend::Backend;
pub use circuit::Program;
pub use error::{HgmError, Result};
pub use gates::GateCatalog;
pub use num_complex::Complex32;
pub use fused::{FusedPhase, PhaseTerm};
pub use measure::Measurement;
pub use pt::{MatrixKind, Op, Pt};
pub use rng::Mt19937;
pub use state::StateVector;

/// Amplitude de estado: `float` complexo, como o `float complex` do C++.
pub type C32 = Complex32;

/// Máximo de qubits. 32 porque o índice do estado precisa caber em `u32` na GPU
/// (WGSL não tem inteiro de 64 bits portátil, ver `docs/09` §5).
pub const MAX_QUBITS: usize = 32;
