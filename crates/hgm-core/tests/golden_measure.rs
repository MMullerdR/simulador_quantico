//! Paridade de medição, colapso e multiplicação modular com o C++
//! (`tests/golden`, casos `kind measure`, `colapse` e `modmult`).
//!
//! - `measure`: com `Mt19937` semeado com `seed`, os bits têm que ser idênticos,
//!   as probabilidades iguais a ~1e-6 e o estado final a 1e-5;
//! - `colapse`: estado final a 1e-5;
//! - `modmult`: é uma permutação, então o estado tem que ser **idêntico**.

use hgm_core::{Mt19937, StateVector};
use hgm_golden::{cases_of_kind, parse_floats, read_bytes, Case};

const STATE_TOLERANCE: f32 = 1e-5;
const PROBABILITY_TOLERANCE: f32 = 1e-6;

fn load_state(case: &Case, key: &str) -> Result<StateVector, String> {
    let qubits = case.get_usize("qubits")?;
    let bytes = read_bytes(&case.file_field(key)?)?;
    StateVector::from_f32_le_bytes(qubits, &bytes).map_err(|e| e.to_string())
}

fn ints(text: &str) -> Result<Vec<i64>, String> {
    text.split_whitespace()
        .map(|t| t.parse().map_err(|e| format!("inteiro inválido `{t}`: {e}")))
        .collect()
}

fn check_measure(case: &Case) -> Result<(), String> {
    let mut state = load_state(case, "init")?;
    let seed = case.get_usize("seed")? as u32;
    let list = ints(case.get("measure").ok_or("falta `measure`")?)?;
    let (count, qubits) = (list[0] as usize, &list[1..]);
    if qubits.len() != count {
        return Err(format!("measure: {count} != {} qubits", qubits.len()));
    }
    let expected_bits = ints(case.get("bits").ok_or("falta `bits`")?)?;
    let expected_values = parse_floats(case.get("values").ok_or("falta `values`")?)?;

    let mut rng = Mt19937::new(seed);
    for (i, &qubit) in qubits.iter().enumerate() {
        let result = state
            .measure_qubit(qubit as usize, &mut rng)
            .map_err(|e| e.to_string())?;
        if i64::from(result.bit) != expected_bits[i] {
            return Err(format!(
                "medida {i} (qubit {qubit}): bit {} != {}",
                result.bit, expected_bits[i]
            ));
        }
        if (result.probability - expected_values[i]).abs() > PROBABILITY_TOLERANCE {
            return Err(format!(
                "medida {i}: probabilidade {} != {}",
                result.probability, expected_values[i]
            ));
        }
    }

    let expected = load_state(case, "final")?;
    let diff = state.max_abs_diff(&expected);
    if diff >= STATE_TOLERANCE {
        return Err(format!("estado final: máx. |diferença| {diff:e}"));
    }
    Ok(())
}

fn check_colapse(case: &Case) -> Result<(), String> {
    let mut state = load_state(case, "init")?;
    let args = ints(case.get("colapse").ok_or("falta `colapse`")?)?;
    state
        .collapse(args[0] as usize, args[1] as u8)
        .map_err(|e| e.to_string())?;

    let expected = load_state(case, "final")?;
    let diff = state.max_abs_diff(&expected);
    if diff >= STATE_TOLERANCE {
        return Err(format!("estado final: máx. |diferença| {diff:e}"));
    }
    Ok(())
}

fn check_modmult(case: &Case) -> Result<(), String> {
    let mut state = load_state(case, "init")?;
    let args = ints(case.get("modmult").ok_or("falta `modmult`")?)?;
    // modmult CTRL REG0 WIDTH MULT MOD
    state
        .apply_mod_mult(
            args[0] as usize,
            args[1] as usize,
            args[2] as usize,
            args[3],
            args[4],
        )
        .map_err(|e| e.to_string())?;

    let expected = load_state(case, "final")?;
    let diff = state.max_abs_diff(&expected);
    if diff != 0.0 {
        return Err(format!("permutação difere: máx. |diferença| {diff:e}"));
    }
    Ok(())
}

fn run(kind: &str, expected_cases: usize, check: fn(&Case) -> Result<(), String>) {
    let cases = cases_of_kind(kind).expect("ler os casos");
    assert_eq!(cases.len(), expected_cases, "casos `{kind}` no INDEX");
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|case| check(case).err().map(|m| format!("{}: {m}", case.name)))
        .collect();
    assert!(failures.is_empty(), "falhas:\n{}", failures.join("\n"));
}

#[test]
fn measurement_matches_cpp_golden() {
    run("measure", 6, check_measure);
}

#[test]
fn collapse_matches_cpp_golden() {
    run("colapse", 3, check_colapse);
}

#[test]
fn modular_multiplication_matches_cpp_golden() {
    run("modmult", 4, check_modmult);
}
