//! Parser da linguagem de circuitos (`docs/02-linguagem-de-circuitos.md`).
//!
//! Porta `DGM::setFunction` / `genGroups` / `genPTs` (`src/core/dgm_parser.cpp`):
//! cada passo é uma linha com um token por qubit (`ID`, `H`, `Control1(1)`,
//! `Target1(X)`, ...), separados por `,`; passos são separados por `;`.
//!
//! Diferenças deliberadas em relação ao C++ (todas viram erro em vez de
//! comportamento indefinido):
//!
//! - porta desconhecida → [`HgmError::UnknownGate`] (o C++ devolvia ponteiro nulo
//!   e quebrava na execução);
//! - passos com números de qubits diferentes → [`HgmError::InconsistentQubits`]
//!   (o C++ ficava com o do último passo);
//! - `Control`/`Target` só são reconhecidos como prefixo do token (o C++ usava
//!   `find`, então um nome de porta contendo "Control" seria mal interpretado).

use std::collections::BTreeMap;

use crate::error::{HgmError, Result};
use crate::gates::GateCatalog;
use crate::pt::{Op, Pt};
use crate::MAX_QUBITS;

/// Circuito compilado: lista plana de portas na ordem de execução.
#[derive(Clone, Debug)]
pub struct Program {
    pub qubits: usize,
    pub ops: Vec<Op>,
}

/// Divide `text` em tokens por qualquer caractere de `delimiters`, ignorando
/// delimitadores repetidos e as pontas (como `Tokenize` do C++).
pub fn tokenize<'a>(text: &'a str, delimiters: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    text.split(move |c: char| delimiters.contains(c))
        .filter(|token| !token.is_empty())
}

/// Equivalente a `strtol(s, ..., 10)`: espaços, sinal, dígitos; sem dígitos → 0.
fn parse_leading_int(s: &str) -> i64 {
    let s = s.trim_start();
    let (sign, rest) = match s.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, s.strip_prefix('+').unwrap_or(s)),
    };
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    sign * rest[..end].parse::<i64>().unwrap_or(0)
}

/// Controles e alvos de um mesmo grupo (mesmo número depois de `Control`/`Target`).
#[derive(Default)]
struct Group {
    control_values: Vec<bool>,
    control_positions: Vec<usize>,
    ops: Vec<String>,
    op_positions: Vec<usize>,
}

fn bad_token(step: usize, token: &str, reason: &'static str) -> HgmError {
    HgmError::BadToken {
        step,
        token: token.to_owned(),
        reason,
    }
}

/// Agrupa controles e alvos de um passo por número de grupo (`genGroups`).
/// Devolve o número de qubits do passo (um token por qubit) e os grupos, em
/// ordem crescente do número do grupo (como o `std::map` do C++).
fn gen_groups(step: &str, step_index: usize) -> Result<(usize, BTreeMap<i64, Group>)> {
    let tokens: Vec<&str> = tokenize(step, ",").collect();
    let mut groups: BTreeMap<i64, Group> = BTreeMap::new();

    for (position, token) in tokens.iter().enumerate() {
        if let Some(rest) = token.strip_prefix("Control") {
            let open = rest
                .find('(')
                .ok_or_else(|| bad_token(step_index, token, "falta '('"))?;
            let group = groups.entry(parse_leading_int(&rest[..open])).or_default();
            group
                .control_values
                .push(parse_leading_int(&rest[open + 1..]) != 0);
            group.control_positions.push(position);
        } else if let Some(rest) = token.strip_prefix("Target") {
            let open = rest
                .find('(')
                .ok_or_else(|| bad_token(step_index, token, "falta '('"))?;
            let name = rest[open + 1..]
                .strip_suffix(')')
                .ok_or_else(|| bad_token(step_index, token, "falta ')'"))?;
            let group = groups.entry(parse_leading_int(&rest[..open])).or_default();
            group.ops.push(name.to_owned());
            group.op_positions.push(position);
        } else if *token != "ID" {
            let group = groups.entry(0).or_default();
            group.ops.push((*token).to_owned());
            group.op_positions.push(position);
        }
    }

    Ok((tokens.len(), groups))
}

/// Transforma os grupos em portas compiladas (`genPTs`).
fn gen_pts(qubits: usize, groups: &BTreeMap<i64, Group>, catalog: &GateCatalog) -> Result<Vec<Pt>> {
    let mut pts = Vec::new();

    for group in groups.values() {
        let mut control_mask = 0u64;
        let mut control_value = 0u64;
        let mut control_bits = Vec::with_capacity(group.control_positions.len());

        for (index, &position) in group.control_positions.iter().enumerate() {
            let bit = (qubits - position - 1) as u32;
            control_mask |= 1u64 << bit;
            if group.control_values[index] {
                control_value |= 1u64 << bit;
            }
            control_bits.push(bit);
        }

        for (name, &position) in group.ops.iter().zip(&group.op_positions) {
            let matrix = catalog
                .get(name)
                .ok_or_else(|| HgmError::UnknownGate(name.clone()))?;
            pts.push(Pt {
                target_bit: (qubits - position - 1) as u32,
                matrix,
                control_mask,
                control_value,
                control_bits: control_bits.clone(),
            });
        }
    }

    Ok(pts)
}

/// Compila um passo: portas na ordem que o C++ usa para executar.
///
/// Passos de índice par ordenam por bit do alvo **crescente**, ímpares por bit
/// **decrescente** (no C++: `decreasing`/`increasing` sobre `span_start_bit =
/// qubits - posição`, ou seja, ordem inversa do bit). Alterna a direção para
/// melhorar a localidade entre passos vizinhos. Os alvos de um passo são todos
/// distintos (um token por qubit), então a ordem é total e a estabilidade do
/// sort não importa.
pub fn compile_step(
    step: &str,
    step_index: usize,
    catalog: &GateCatalog,
) -> Result<(usize, Vec<Pt>)> {
    let (qubits, groups) = gen_groups(step, step_index)?;
    if qubits > MAX_QUBITS {
        return Err(HgmError::TooManyQubits {
            qubits,
            max: MAX_QUBITS,
        });
    }

    let mut pts = gen_pts(qubits, &groups, catalog)?;
    if step_index.is_multiple_of(2) {
        pts.sort_by_key(|pt| pt.target_bit);
    } else {
        pts.sort_by_key(|pt| std::cmp::Reverse(pt.target_bit));
    }
    Ok((qubits, pts))
}

/// Compila uma lista de passos, repetida `iterations` vezes (`setFunction`).
/// A paridade do índice do passo (que decide a direção da ordenação) reinicia a
/// cada repetição, como no C++.
pub fn compile_steps<S: AsRef<str>>(
    steps: &[S],
    iterations: usize,
    catalog: &GateCatalog,
) -> Result<Program> {
    let mut qubits = None;
    let mut one_iteration: Vec<Pt> = Vec::new();

    for (step_index, step) in steps.iter().enumerate() {
        let (step_qubits, pts) = compile_step(step.as_ref(), step_index, catalog)?;
        match qubits {
            None => qubits = Some(step_qubits),
            Some(expected) if expected != step_qubits => {
                return Err(HgmError::InconsistentQubits {
                    step: step_index,
                    found: step_qubits,
                    expected,
                })
            }
            Some(_) => {}
        }
        one_iteration.extend(pts);
    }

    let mut all = Vec::with_capacity(one_iteration.len().saturating_mul(iterations));
    for _ in 0..iterations {
        all.extend(one_iteration.iter().cloned());
    }

    Ok(Program {
        qubits: qubits.unwrap_or(0),
        ops: all.into_iter().map(Op::Gate).collect(),
    })
}

/// Compila uma função inteira: passos separados por `;`.
pub fn compile(function: &str, iterations: usize, catalog: &GateCatalog) -> Result<Program> {
    let steps: Vec<&str> = tokenize(function, ";").collect();
    compile_steps(&steps, iterations, catalog)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> GateCatalog {
        GateCatalog::with_base_gates()
    }

    #[test]
    fn tokenize_skips_empty_tokens() {
        let tokens: Vec<_> = tokenize(",,a,,b,", ",").collect();
        assert_eq!(tokens, ["a", "b"]);
        assert_eq!(tokenize("", ",").count(), 0);
    }

    #[test]
    fn bit_is_qubits_minus_one_minus_position() {
        let (qubits, pts) = compile_step("H,ID,ID,ID", 0, &base()).unwrap();
        assert_eq!(qubits, 4);
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0].target_bit, 3);
    }

    #[test]
    fn controlled_gate_has_mask_and_value() {
        // Do golden 0246: "Target1(X),ID,ID,ID,Control1(0),Control1(0)" em 6 qubits
        // vira "pt 5 3 0 2 ... 1 0".
        let (_, pts) =
            compile_step("Target1(X),ID,ID,ID,Control1(0),Control1(0)", 0, &base()).unwrap();
        assert_eq!(pts.len(), 1);
        assert_eq!(pts[0].target_bit, 5);
        assert_eq!(pts[0].control_mask, 3);
        assert_eq!(pts[0].control_value, 0);
        assert_eq!(pts[0].control_bits, [1, 0]);
    }

    #[test]
    fn step_order_alternates() {
        // Passo par: bit do alvo crescente; passo ímpar: decrescente.
        let step = "U,ID,ID,R2,ID,X";
        let mut catalog = base();
        catalog.add_gate("U", catalog.get("H").unwrap());
        let (_, even) = compile_step(step, 0, &catalog).unwrap();
        let (_, odd) = compile_step(step, 1, &catalog).unwrap();
        let even_bits: Vec<_> = even.iter().map(|p| p.target_bit).collect();
        let odd_bits: Vec<_> = odd.iter().map(|p| p.target_bit).collect();
        assert_eq!(even_bits, [0, 2, 5]);
        assert_eq!(odd_bits, [5, 2, 0]);
    }

    #[test]
    fn unknown_gate_is_an_error() {
        let error = compile_step("Q,ID", 0, &base()).unwrap_err();
        assert!(matches!(error, HgmError::UnknownGate(name) if name == "Q"));
    }

    #[test]
    fn malformed_tokens_are_errors() {
        assert!(compile_step("Control1,ID", 0, &base()).is_err());
        assert!(compile_step("Target1(X,ID", 0, &base()).is_err());
    }

    #[test]
    fn inconsistent_qubit_counts_are_errors() {
        let error = compile("H,ID;H,ID,ID", 1, &base()).unwrap_err();
        assert!(matches!(
            error,
            HgmError::InconsistentQubits { step: 1, .. }
        ));
    }

    #[test]
    fn iterations_repeat_the_program() {
        let program = compile("H,ID;ID,H", 3, &base()).unwrap();
        assert_eq!(program.qubits, 2);
        assert_eq!(program.ops.len(), 6);
    }

    #[test]
    fn control_without_target_makes_no_gate() {
        let (_, pts) = compile_step("Control1(1),ID", 0, &base()).unwrap();
        assert!(pts.is_empty());
    }
}
