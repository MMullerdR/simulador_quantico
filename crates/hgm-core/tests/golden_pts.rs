//! Paridade do parser com o C++ (`tests/golden`, casos `kind pts`, `mode parse`).
//!
//! Compara, porta a porta e na ordem, o que `compile_steps` produz com o que o
//! `DGM::setFunction` do tag v1.1.0 produziu. Os modos `fuse`, `schedule` e
//! `fuse_schedule` exigem o otimizador (ainda não portado) e ficam de fora; o
//! teste diz quantos pulou.

use hgm_core::circuit::compile_steps;
use hgm_core::{GateCatalog, MatrixKind, Pt};
use hgm_golden::{cases_of_kind, parse_floats, Case};

/// Tolerância das matrizes: as portas `R1..R3` do C++ usam `cpowf` em `float`
/// (últimos bits dependem da libm); o Rust calcula em `f64` e arredonda. A
/// diferença medida é < 1e-7.
const MATRIX_TOLERANCE: f32 = 2e-7;

fn catalog_of(case: &Case) -> GateCatalog {
    let mut catalog = GateCatalog::with_base_gates();
    for (name, matrix) in &case.gates {
        catalog.add_gate(name, *matrix);
    }
    catalog
}

/// Compara uma porta com a linha `pt TARGET CMASK CVALUE CCOUNT TYPE m0r..m3i POS...`.
fn compare_pt(index: usize, pt: &Pt, line: &str) -> Result<(), String> {
    let numbers = parse_floats(
        line.strip_prefix("pt ")
            .ok_or_else(|| format!("pt {index}: linha não começa com `pt `: {line}"))?,
    )?;
    if numbers.len() < 13 {
        return Err(format!("pt {index}: linha curta: {line}"));
    }

    let target = numbers[0] as u32;
    let mask = numbers[1] as u64;
    let value = numbers[2] as u64;
    let count = numbers[3] as usize;
    let kind = numbers[4] as u8;

    if pt.target_bit != target {
        return Err(format!("pt {index}: alvo {} != {target}", pt.target_bit));
    }
    if pt.control_mask != mask || pt.control_value != value || pt.control_count() != count {
        return Err(format!(
            "pt {index}: controles (mask {}, value {}, n {}) != ({mask}, {value}, {count})",
            pt.control_mask,
            pt.control_value,
            pt.control_count()
        ));
    }
    if pt.kind() as u8 != kind {
        return Err(format!(
            "pt {index}: tipo {:?} != {kind} (0 denso, 1 diag. principal, 2 anti-diag.)",
            pt.kind()
        ));
    }
    for (k, expected) in numbers[5..13].iter().enumerate() {
        let element = pt.matrix[k / 2];
        let got = if k % 2 == 0 { element.re } else { element.im };
        if (got - expected).abs() > MATRIX_TOLERANCE {
            return Err(format!("pt {index}: matriz[{k}] {got} != {expected}"));
        }
    }
    let positions: Vec<u32> = numbers[13..].iter().map(|v| *v as u32).collect();
    if positions != pt.control_bits {
        return Err(format!(
            "pt {index}: posições dos controles {:?} != {positions:?}",
            pt.control_bits
        ));
    }
    Ok(())
}

fn check_case(case: &Case) -> Result<(), String> {
    let catalog = catalog_of(case);
    let program = compile_steps(case.block("steps"), 1, &catalog).map_err(|e| e.to_string())?;

    if program.qubits != case.get_usize("qubits")? {
        return Err(format!("qubits {} != esperado", program.qubits));
    }
    let expected = case.block("pts");
    if program.pts.len() != expected.len() {
        return Err(format!(
            "{} portas != {} esperadas",
            program.pts.len(),
            expected.len()
        ));
    }
    for (index, (pt, line)) in program.pts.iter().zip(expected).enumerate() {
        compare_pt(index, pt, line)?;
    }
    Ok(())
}

#[test]
fn parser_matches_cpp_golden_pts() {
    let cases = cases_of_kind("pts").expect("ler os casos pts");
    assert_eq!(cases.len(), 64, "o INDEX deveria ter 64 casos pts");

    let mut checked = 0;
    let mut skipped = 0;
    let mut failures = Vec::new();

    for case in &cases {
        if case.get("mode") != Some("parse") {
            skipped += 1;
            continue;
        }
        checked += 1;
        if let Err(message) = check_case(case) {
            failures.push(format!("{}: {message}", case.name));
        }
    }

    println!("pts: {checked} conferidos, {skipped} pulados (fuse/schedule ainda não portados)");
    assert_eq!(checked, 8, "esperava 8 casos mode parse");
    assert!(failures.is_empty(), "falhas:\n{}", failures.join("\n"));
}

#[test]
fn matrix_kind_numbers_match_the_cpp_enum() {
    assert_eq!(MatrixKind::Dense as u8, 0);
    assert_eq!(MatrixKind::DiagPri as u8, 1);
    assert_eq!(MatrixKind::DiagSec as u8, 2);
}
