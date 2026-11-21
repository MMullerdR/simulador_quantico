//! Algoritmos do HybriD-GM em Rust.
//!
//! Por enquanto só a teoria dos números do Shor ([`number_theory`]), que não
//! depende do simulador e já tem vetores de referência (`kind table`). Os
//! geradores de circuito (`general`, `grover`, `shor`) vêm depois do otimizador e
//! do backend paralelo (`docs/09` §8).

pub mod number_theory;
