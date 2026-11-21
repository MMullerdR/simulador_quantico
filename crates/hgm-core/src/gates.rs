//! Catálogo de portas de 1 qubit (equivalente ao `Gates` de `src/core/gates.cpp`).
//!
//! Matriz 2x2 em ordem de linhas: `[m00, m01, m10, m11]`. Cada simulação tem o
//! seu catálogo (no C++ também: `DGM::gates`), então portas geradas dinamicamente
//! (rotações da QFT do Shor) não vazam entre execuções.

use std::collections::HashMap;
use std::f64::consts::{FRAC_1_SQRT_2, PI};

use crate::C32;

/// Matriz 2x2 de uma porta, em ordem de linhas.
pub type Matrix2 = [C32; 4];

#[derive(Clone, Debug)]
pub struct GateCatalog {
    gates: HashMap<String, Matrix2>,
}

/// `e^{i*theta}` calculado em `f64` e arredondado uma vez para `f32`.
///
/// O C++ usa `cpowf(M_E, theta*I)` em `float`, cujos últimos bits dependem da
/// libm (ex.: `R1` sai com parte imaginária ~8e-9 em vez de 0). A diferença para
/// este valor é < 1e-7; por isso os testes de paridade comparam matrizes com
/// tolerância (ver `tests/golden/README.md`).
pub fn phase(theta: f64) -> C32 {
    C32::new(theta.cos() as f32, theta.sin() as f32)
}

/// Porta de fase `diag(1, value)`, a forma de todas as rotações geradas pelo Shor
/// (`addGate(name, 1.0, 0.0, 0.0, rot)` no C++).
pub fn phase_matrix(value: C32) -> Matrix2 {
    [
        C32::new(1.0, 0.0),
        C32::new(0.0, 0.0),
        C32::new(0.0, 0.0),
        value,
    ]
}

impl GateCatalog {
    /// Catálogo vazio.
    pub fn empty() -> Self {
        Self {
            gates: HashMap::new(),
        }
    }

    /// Catálogo com as portas base `H X Y Z R1 R2 R3` (como `Gates::init`).
    pub fn with_base_gates() -> Self {
        let mut catalog = Self::empty();
        let one = C32::new(1.0, 0.0);
        let zero = C32::new(0.0, 0.0);
        let h = C32::new(FRAC_1_SQRT_2 as f32, 0.0);

        catalog.add_gate("H", [h, h, h, -h]);
        catalog.add_gate("X", [zero, one, one, zero]);
        catalog.add_gate("Z", [one, zero, zero, -one]);
        catalog.add_gate("Y", [zero, C32::new(0.0, 1.0), C32::new(0.0, -1.0), zero]);
        catalog.add_gate("R1", [one, zero, zero, phase(PI)]);
        catalog.add_gate("R2", [one, zero, zero, phase(PI / 2.0)]);
        catalog.add_gate("R3", [one, zero, zero, phase(PI / 4.0)]);
        catalog
    }

    /// Adiciona uma porta. Devolve `false` (sem sobrescrever) se o nome já
    /// existe, como `Gates::addGate` do C++ (ver `docs/07` item 1).
    pub fn add_gate(&mut self, name: &str, matrix: Matrix2) -> bool {
        if self.gates.contains_key(name) {
            return false;
        }
        self.gates.insert(name.to_owned(), matrix);
        true
    }

    pub fn get(&self, name: &str) -> Option<Matrix2> {
        self.gates.get(name).copied()
    }

    /// Nomes das portas do catálogo, em ordem qualquer.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.gates.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.gates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.gates.is_empty()
    }
}

impl Default for GateCatalog {
    fn default() -> Self {
        Self::with_base_gates()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_gates_exist() {
        let catalog = GateCatalog::with_base_gates();
        for name in ["H", "X", "Y", "Z", "R1", "R2", "R3"] {
            assert!(catalog.get(name).is_some(), "falta {name}");
        }
        assert_eq!(catalog.len(), 7);
        assert!(catalog.get("nao-existe").is_none());
    }

    #[test]
    fn add_gate_does_not_overwrite() {
        let mut catalog = GateCatalog::with_base_gates();
        let before = catalog.get("H").unwrap();
        assert!(!catalog.add_gate("H", [C32::new(0.0, 0.0); 4]));
        assert_eq!(catalog.get("H").unwrap(), before);
    }

    #[test]
    fn r_gates_are_phases() {
        let catalog = GateCatalog::with_base_gates();
        let r3 = catalog.get("R3").unwrap();
        assert!((r3[3].re - 0.707_106_77).abs() < 1e-6);
        assert!((r3[3].im - 0.707_106_77).abs() < 1e-6);
    }
}
