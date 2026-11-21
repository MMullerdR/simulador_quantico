//! Paridade do parser e do otimizador com o C++ (`tests/golden`, casos `kind pts`).
//!
//! Compara, operação a operação e na ordem, o que o `hgm-core` produz com o que o
//! tag v1.1.0 produziu, nos quatro modos: `parse` (só o parser), `fuse`
//! (+ `FusePhaseRuns`), `schedule` (+ `ScheduleForBatching`) e `fuse_schedule`.

use hgm_core::circuit::compile_steps;
use hgm_core::optimize::{fuse_phase_runs, schedule_for_batching};
use hgm_core::{GateCatalog, MatrixKind, Op, Pt};
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
            .ok_or_else(|| format!("op {index}: esperava `pt`, veio: {line}"))?,
    )?;
    if numbers.len() < 13 {
        return Err(format!("op {index}: linha curta: {line}"));
    }

    let target = numbers[0] as u32;
    let mask = numbers[1] as u64;
    let value = numbers[2] as u64;
    let count = numbers[3] as usize;
    let kind = numbers[4] as u8;

    if pt.target_bit != target {
        return Err(format!("op {index}: alvo {} != {target}", pt.target_bit));
    }
    if pt.control_mask != mask || pt.control_value != value || pt.control_count() != count {
        return Err(format!(
            "op {index}: controles (mask {}, value {}, n {}) != ({mask}, {value}, {count})",
            pt.control_mask,
            pt.control_value,
            pt.control_count()
        ));
    }
    if pt.kind() as u8 != kind {
        return Err(format!(
            "op {index}: tipo {:?} != {kind} (0 denso, 1 diag. principal, 2 anti-diag.)",
            pt.kind()
        ));
    }
    for (k, expected) in numbers[5..13].iter().enumerate() {
        let element = pt.matrix[k / 2];
        let got = if k % 2 == 0 { element.re } else { element.im };
        if (got - expected).abs() > MATRIX_TOLERANCE {
            return Err(format!("op {index}: matriz[{k}] {got} != {expected}"));
        }
    }
    let positions: Vec<u32> = numbers[13..].iter().map(|v| *v as u32).collect();
    if positions != pt.control_bits {
        return Err(format!(
            "op {index}: posições dos controles {:?} != {positions:?}",
            pt.control_bits
        ));
    }
    Ok(())
}

/// Compara uma fase fundida com a linha `fused N MASK VALUE STEPS ...` (exata).
fn compare_fused(index: usize, op: &Op, line: &str) -> Result<(), String> {
    let Op::FusedPhase(fused) = op else {
        return Err(format!("op {index}: esperava `pt`, mas o Rust tem fase fundida"));
    };
    let numbers = parse_floats(
        line.strip_prefix("fused ")
            .ok_or_else(|| format!("op {index}: esperava `fused`, veio: {line}"))?,
    )?;
    let count = numbers[0] as usize;
    if numbers.len() != 1 + 3 * count || fused.terms.len() != count {
        return Err(format!(
            "op {index}: {} termos != {count} esperados",
            fused.terms.len()
        ));
    }
    for (t, term) in fused.terms.iter().enumerate() {
        let expected = &numbers[1 + 3 * t..4 + 3 * t];
        if (term.mask, term.value, term.steps)
            != (expected[0] as u64, expected[1] as u64, expected[2] as u32)
        {
            return Err(format!(
                "op {index}, termo {t}: ({}, {}, {}) != {expected:?}",
                term.mask, term.value, term.steps
            ));
        }
    }
    Ok(())
}

fn check_case(case: &Case) -> Result<(), String> {
    let catalog = catalog_of(case);
    let mode = case.get("mode").ok_or("falta `mode`")?;
    let program = compile_steps(case.block("steps"), 1, &catalog).map_err(|e| e.to_string())?;
    if program.qubits != case.get_usize("qubits")? {
        return Err(format!("qubits {} != esperado", program.qubits));
    }

    let mut ops = program.ops;
    if mode == "fuse" || mode == "fuse_schedule" {
        let (fused, absorbed) = fuse_phase_runs(ops);
        ops = fused;
        let expected = case.get_usize("absorbed")?;
        if absorbed != expected {
            return Err(format!("absorbed {absorbed} != {expected}"));
        }
    }
    if mode == "schedule" || mode == "fuse_schedule" {
        let params = parse_floats(case.get("schedule").ok_or("falta `schedule`")?)?;
        ops = schedule_for_batching(ops, params[0] as u32, params[1] as u32, params[2] as usize);
    }

    let expected = case.block("pts");
    if ops.len() != expected.len() {
        return Err(format!("{} ops != {} esperadas", ops.len(), expected.len()));
    }
    for (index, (op, line)) in ops.iter().zip(expected).enumerate() {
        match op {
            Op::Gate(pt) => compare_pt(index, pt, line)?,
            Op::FusedPhase(_) => compare_fused(index, op, line)?,
        }
    }
    Ok(())
}

#[test]
fn parser_and_optimizer_match_cpp_golden_pts() {
    let cases = cases_of_kind("pts").expect("ler os casos pts");
    assert_eq!(cases.len(), 64, "o INDEX deveria ter 64 casos pts");

    let mut by_mode = std::collections::BTreeMap::<String, usize>::new();
    let mut failures = Vec::new();

    for case in &cases {
        let mode = case.get("mode").unwrap_or("?").to_owned();
        *by_mode.entry(mode).or_default() += 1;
        if let Err(message) = check_case(case) {
            failures.push(format!("{}: {message}", case.name));
        }
    }

    println!("pts por modo: {by_mode:?}");
    assert_eq!(by_mode.get("parse"), Some(&8));
    assert_eq!(by_mode.get("fuse"), Some(&8));
    assert_eq!(by_mode.get("schedule"), Some(&24));
    assert_eq!(by_mode.get("fuse_schedule"), Some(&24));
    assert!(failures.is_empty(), "falhas:\n{}", failures.join("\n"));
}

#[test]
fn matrix_kind_numbers_match_the_cpp_enum() {
    assert_eq!(MatrixKind::Dense as u8, 0);
    assert_eq!(MatrixKind::DiagPri as u8, 1);
    assert_eq!(MatrixKind::DiagSec as u8, 2);
}
