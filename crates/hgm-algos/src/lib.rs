//! Algoritmos do HybriD-GM em Rust (fase 5 de `docs/09`).
//!
//! - [`steps`]: geradores de passos da linguagem de circuitos (`Hadamard`, `CNot`...);
//! - [`number_theory`]: aritmética modular e frações contínuas do Shor;
//! - [`shor_circuits`]: QFT, somador de Draper e multiplicação modular controlada;
//! - [`general`]: benchmark de Hadamard;
//! - [`grover`]: busca de Grover;
//! - [`shor`]: fatoração de Shor.
//!
//! Cada algoritmo recebe um [`hgm_engine::SimConfig`] (backend e semente) e monta o
//! seu próprio [`hgm_engine::Simulator`], como as funções `Grover()`/`Shor()` do C++
//! montavam o seu `DGM`.

pub mod general;
pub mod grover;
pub mod number_theory;
pub mod shor;
pub mod shor_circuits;
pub mod steps;
