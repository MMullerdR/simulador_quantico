//! Paridade da teoria dos números com o C++ (`tests/golden/data/cases/0310-number-theory.case`).
//! Todas as linhas são comparadas com igualdade exata (são inteiros).

use hgm_algos::number_theory::{
    modular_pow, mul_inv, qft_degree_for_width, quantum_frac_approx, quantum_gcd, revert_bits,
};
use hgm_golden::cases_of_kind;

fn ints(text: &str) -> Vec<i64> {
    text.split_whitespace()
        .map(|t| {
            t.parse()
                .unwrap_or_else(|e| panic!("inteiro inválido `{t}`: {e}"))
        })
        .collect()
}

fn check_row(row: &str) -> Result<(), String> {
    let (left, right) = row
        .split_once('=')
        .ok_or_else(|| format!("linha sem `=`: {row}"))?;
    let (function, args) = left.trim().split_once(' ').unwrap_or((left.trim(), ""));
    let args = ints(args);
    let expected = ints(right);

    let got: Vec<i64> = match (function, args.as_slice()) {
        ("modular_pow", [b, e, m]) => vec![modular_pow(*b, *e, *m).ok_or("sem resultado")?],
        ("mul_inv", [v, m]) => vec![mul_inv(*v, *m).ok_or("sem inverso")?],
        ("quantum_gcd", [a, b]) => vec![quantum_gcd(*a, *b)],
        ("revert_bits", [v, n]) => vec![i64::from(revert_bits(*v as i32, *n as u32))],
        ("qft_degree_for_width", [w]) => vec![i64::from(qft_degree_for_width(*w as u32))],
        ("frac_approx", [n, d, w]) => {
            let (num, den) = quantum_frac_approx(*n as i32, *d as i32, *w as u32);
            vec![i64::from(num), i64::from(den)]
        }
        _ => return Err(format!("função ou aridade desconhecida: {row}")),
    };

    if got == expected {
        Ok(())
    } else {
        Err(format!("{row}: Rust deu {got:?}"))
    }
}

#[test]
fn number_theory_matches_cpp_golden_table() {
    let cases = cases_of_kind("table").expect("ler o caso table");
    assert_eq!(cases.len(), 1);
    let rows = cases[0].block("table");
    assert_eq!(rows.len(), 324 + 152 + 121 + 117 + 26 + 16);

    let failures: Vec<String> = rows.iter().filter_map(|row| check_row(row).err()).collect();
    assert!(failures.is_empty(), "falhas:\n{}", failures.join("\n"));
}
