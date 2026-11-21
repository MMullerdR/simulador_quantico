//! Busca de Grover (`src/algorithms/lib_grover.cpp`; ver `docs/05-algoritmo-grover.md`).
//!
//! O qubit 0 é um ancilla (começa em `|1>`, vira `|->` depois do `H`); os
//! `qubits - 1` restantes são o registrador de busca.

use std::f64::consts::PI;

use hgm_core::error::{HgmError, Result};
use hgm_core::MAX_QUBITS;
use hgm_engine::{SimConfig, Simulator};

use crate::steps::{concatena, hadamard, pauli_x};

/// Limite de portas por bloco compilado. Compilar as `~sqrt(2^(n-1))` iterações de
/// uma vez gastava centenas de MB a ~31 qubits; repetir um bloco com `execute`
/// custa pouco (ver o comentário em `Grover()` no C++, item 22 de `docs/07`).
const PT_BLOCK_LIMIT: usize = 20_000;

/// Oráculo: inverte a fase do estado em que o registrador de busca vale
/// `search_value` (o `X` no ancilla, controlado pelos bits de `search_value`).
pub fn oracle1(qubits: usize, search_value: u64) -> String {
    let mut step_ops = vec![String::new(); qubits];
    let mut remaining = search_value;
    for slot in step_ops.iter_mut().skip(1).rev() {
        *slot = if remaining & 1 != 0 {
            "Control1(1)"
        } else {
            "Control1(0)"
        }
        .to_owned();
        remaining >>= 1;
    }
    step_ops[0] = "Target1(X)".to_owned();
    concatena(&step_ops, false)
}

/// `Z` multi-controlado (controles com valor 0) sobre os qubits de busca. Com um
/// `X` antes e depois no qubit alvo, inverte a fase de `|00...0>`: o miolo do
/// difusor.
pub fn controlled_z(qubits: usize) -> String {
    let mut step_ops = vec!["ID".to_owned()];
    step_ops.extend((0..qubits.saturating_sub(2)).map(|_| "Control1(0)".to_owned()));
    step_ops.push("Target1(Z)".to_owned());
    concatena(&step_ops, false)
}

/// Número de iterações: `floor(pi/4 * sqrt(2^(qubits-1)))`.
pub fn iteration_count(qubits: usize) -> usize {
    (PI / 4.0 * ((1u64 << (qubits - 1)) as f64).sqrt()) as usize
}

/// Os passos de uma iteração (oráculo + difusor).
///
/// O difusor é `H` em todos, "inverte a fase de `|00...0>`", `H` em todos. A
/// inversão usa controles de valor 0 (`controlled_z`) em vez de um `X` em cada
/// qubit antes e depois: o mesmo operador com 2 passadas de `X` no alvo em vez de
/// `2*(qubits-1)` (item 22 de `docs/07`).
pub fn grover_step(qubits: usize, search_value: u64) -> Vec<String> {
    let flip_target = pauli_x(qubits, qubits - 1, 1);
    let mut steps = vec![oracle1(qubits, search_value)];
    for qubit in 1..qubits {
        steps.push(hadamard(qubits, qubit, 1));
    }
    steps.push(flip_target.clone());
    steps.push(controlled_z(qubits));
    steps.push(flip_target);
    for qubit in (1..qubits).rev() {
        steps.push(hadamard(qubits, qubit, 1));
    }
    steps
}

/// Procura `search_value` (`0..2^(qubits-1)`) e devolve o valor **medido** no
/// registrador de busca. Grover é probabilístico: com alta probabilidade o valor
/// medido é `search_value`, sem garantia.
pub fn grover(config: &SimConfig, qubits: usize, search_value: u64) -> Result<u64> {
    let mut simulator = amplify(config, qubits, search_value, PT_BLOCK_LIMIT)?;
    let mut result = 0u64;
    for qubit in 1..qubits {
        result = (result << 1) | u64::from(simulator.measure(qubit)?.bit);
    }
    Ok(result)
}

/// Prepara o estado e aplica as iterações de amplificação, sem medir. As iterações
/// são compiladas em blocos de até `block_limit` portas (ver `PT_BLOCK_LIMIT`).
fn amplify(
    config: &SimConfig,
    qubits: usize,
    search_value: u64,
    block_limit: usize,
) -> Result<Simulator> {
    if !(2..=MAX_QUBITS).contains(&qubits) {
        return Err(HgmError::InvalidArgument(format!(
            "Grover precisa de 2 a {MAX_QUBITS} qubits, recebi {qubits}"
        )));
    }
    let search_space = 1u64 << (qubits - 1);
    if search_value >= search_space {
        // O oráculo só olha `qubits - 1` bits: um valor maior seria truncado em
        // silêncio e a busca seria por outro valor.
        return Err(HgmError::InvalidArgument(format!(
            "search_value {search_value} fora de 0..{search_space} para {qubits} qubits"
        )));
    }

    let mut simulator = Simulator::with_config(config, qubits)?;
    simulator.set_basis(1usize << (qubits - 1))?;

    let prepare = hadamard(qubits, 0, qubits);
    let step = grover_step(qubits, search_value);
    let total = iteration_count(qubits);
    let chunk = (block_limit / step.len()).max(1);
    let first = total.min(chunk);

    // Preparação + primeiro bloco num programa só (`setFunction(..., reset=false)`),
    // depois blocos de `chunk` iterações repetidos por `execute`.
    let mut program = simulator.compile_steps(&[prepare], 1)?;
    program.append(simulator.compile_steps(&step, first)?)?;
    simulator.run_program(&program, 1)?;

    let mut left = total - first;
    if left >= chunk {
        let block = simulator.compile_steps(&step, chunk)?;
        simulator.run_program(&block, (left / chunk) as u32)?;
        left %= chunk;
    }
    if left > 0 {
        let rest = simulator.compile_steps(&step, left)?;
        simulator.run_program(&rest, 1)?;
    }

    Ok(simulator)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oracle_puts_search_bits_on_the_controls() {
        // 4 qubits, busca em 3 bits: 5 = 0b101 -> qubits 1, 2, 3 = 1, 0, 1.
        assert_eq!(
            oracle1(4, 5),
            "Target1(X),Control1(1),Control1(0),Control1(1)"
        );
        assert_eq!(oracle1(3, 0), "Target1(X),Control1(0),Control1(0)");
        assert_eq!(oracle1(2, 1), "Target1(X),Control1(1)");
    }

    #[test]
    fn controlled_z_targets_the_last_qubit() {
        assert_eq!(controlled_z(4), "ID,Control1(0),Control1(0),Target1(Z)");
        assert_eq!(controlled_z(2), "ID,Target1(Z)");
    }

    #[test]
    fn iteration_counts_follow_the_formula() {
        // floor(pi/4 * sqrt(2^(n-1)))
        assert_eq!(iteration_count(2), 1);
        assert_eq!(iteration_count(4), 2);
        assert_eq!(iteration_count(8), 8);
        assert_eq!(iteration_count(12), 35); // pi/4 * sqrt(2048) = 35,5
    }

    #[test]
    fn step_has_oracle_and_symmetric_diffuser() {
        let step = grover_step(4, 5);
        // oráculo + 3 H + X + Z + X + 3 H
        assert_eq!(step.len(), 1 + 3 + 3 + 3);
        assert_eq!(step[0], oracle1(4, 5));
        assert_eq!(step[4], step[6]);
    }

    #[test]
    fn block_size_does_not_change_the_state() {
        // Com 8 qubits, uma iteração tem 1 + 7 + 3 + 7 = 18 portas-passo e são 8
        // iterações. Limite 40 -> blocos de 2; 60 -> de 3 (primeiro bloco 3, depois
        // 1 bloco de 3 e 2 sobrando); 1_000_000 -> tudo no primeiro bloco. As três
        // formas têm que dar o mesmo estado: repetir blocos só economiza memória.
        let config = SimConfig::default();
        let mut reference = amplify(&config, 8, 100, 1_000_000).unwrap();
        for limit in [1, 40, 60, 100] {
            let mut chunked = amplify(&config, 8, 100, limit).unwrap();
            let diff = reference
                .state()
                .unwrap()
                .max_abs_diff(chunked.state().unwrap());
            assert!(diff < 1e-6, "limite {limit}: diferença {diff:e}");
        }
        // E o estado é mesmo o amplificado: a amplitude do valor buscado domina.
        let target = 100usize | (1 << 7); // o ancilla (qubit 0) é o bit 7 do índice
        let probability: f64 = [target, target & !(1 << 7)]
            .iter()
            .map(|&i| f64::from(reference.amplitude(i).unwrap().norm_sqr()))
            .sum();
        assert!(probability > 0.99, "probabilidade {probability}");
    }

    #[test]
    fn rejects_bad_arguments() {
        let config = SimConfig::default();
        assert!(grover(&config, 1, 0).is_err());
        assert!(grover(&config, 33, 0).is_err());
        assert!(grover(&config, 4, 8).is_err());
    }

    #[test]
    fn finds_the_value_on_small_searches() {
        // Probabilístico, mas a semente fixa torna o resultado determinístico: nas
        // 12 sementes abaixo o valor sai certo (a probabilidade de acerto passa de
        // 94% nesses tamanhos).
        for (qubits, value) in [(4usize, 5u64), (6, 21), (8, 100)] {
            let hits = (1..=12u32)
                .filter(|&seed| {
                    let config = SimConfig {
                        seed,
                        ..SimConfig::default()
                    };
                    grover(&config, qubits, value).unwrap() == value
                })
                .count();
            assert!(hits >= 10, "{qubits} qubits: {hits}/12 acertos");
        }
    }
}
