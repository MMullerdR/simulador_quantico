//! Geradores de passos da linguagem de circuitos (`docs/02`): as funções
//! `Hadamard`, `CNot`, `Toffoli`... de `src/core/gates.cpp`.
//!
//! Cada função monta um passo, uma string com um token por qubit
//! (`ID`/`Control1(v)`/`Target1(porta)`/nome de porta) separados por `,`.
//!
//! Os índices (`reg`, `ctrl`, `target`) são posições de qubit do circuito e
//! precisam estar dentro de `0..qubits`; um índice fora dele é erro de quem
//! chama (como o acesso fora do vetor no C++) e dá pânico com a mensagem do
//! índice, nunca escrita fora do passo.

/// Junta os tokens com `,`; com `reverse`, do último para o primeiro
/// (`concatena` do C++).
pub fn concatena<S: AsRef<str>>(step_ops: &[S], reverse: bool) -> String {
    let mut tokens: Vec<&str> = step_ops.iter().map(AsRef::as_ref).collect();
    if reverse {
        tokens.reverse();
    }
    tokens.join(",")
}

fn identity_ops(qubits: usize) -> Vec<String> {
    vec!["ID".to_owned(); qubits]
}

fn control_token(value: bool) -> &'static str {
    if value {
        "Control1(1)"
    } else {
        "Control1(0)"
    }
}

/// `H` nos `width` qubits a partir de `reg`.
pub fn hadamard(qubits: usize, reg: usize, width: usize) -> String {
    gate_on_register(qubits, reg, width, "H")
}

/// `X` nos `width` qubits a partir de `reg`.
pub fn pauli_x(qubits: usize, reg: usize, width: usize) -> String {
    gate_on_register(qubits, reg, width, "X")
}

/// `Z` nos `width` qubits a partir de `reg`.
pub fn pauli_z(qubits: usize, reg: usize, width: usize) -> String {
    gate_on_register(qubits, reg, width, "Z")
}

fn gate_on_register(qubits: usize, reg: usize, width: usize, gate: &str) -> String {
    let mut step_ops = identity_ops(qubits);
    for slot in &mut step_ops[reg..reg + width] {
        *slot = gate.to_owned();
    }
    concatena(&step_ops, false)
}

/// `X` em `target` controlado por `ctrl` (valor `control_value`: `true` = 1).
pub fn cnot(qubits: usize, ctrl: usize, target: usize, control_value: bool) -> String {
    controlled1(qubits, ctrl, target, "X", control_value)
}

/// `X` em `target` controlado por `c1` e `c2`. Os bits de `control_value`
/// (`0..=3`) dão o valor de cada controle: bit 1 para `c1`, bit 0 para `c2`.
pub fn toffoli(qubits: usize, c1: usize, c2: usize, target: usize, control_value: u32) -> String {
    controlled2(qubits, c1, c2, target, "X", control_value)
}

/// `op` em `target` com um controle em `ctrl`.
pub fn controlled1(
    qubits: usize,
    ctrl: usize,
    target: usize,
    op: &str,
    control_value: bool,
) -> String {
    let mut step_ops = identity_ops(qubits);
    step_ops[ctrl] = control_token(control_value).to_owned();
    step_ops[target] = format!("Target1({op})");
    concatena(&step_ops, false)
}

/// `op` em `target` com dois controles: bit 1 de `control_value` para `c1`, bit 0
/// para `c2` (o C++ testa `control_value & 2` e `& 1`).
pub fn controlled2(
    qubits: usize,
    c1: usize,
    c2: usize,
    target: usize,
    op: &str,
    control_value: u32,
) -> String {
    let mut step_ops = identity_ops(qubits);
    step_ops[c1] = control_token(control_value & 2 != 0).to_owned();
    step_ops[c2] = control_token(control_value & 1 != 0).to_owned();
    step_ops[target] = format!("Target1({op})");
    concatena(&step_ops, false)
}

/// `op` em `target` com um controle em cada qubit de `ctrls`. `control_value`
/// dá o valor de cada controle, com o último de `ctrls` no bit menos
/// significativo; `-1` quer dizer "todos em 1".
pub fn controlled_n(
    qubits: usize,
    ctrls: &[usize],
    target: usize,
    op: &str,
    control_value: i64,
) -> String {
    let mut value = if control_value == -1 {
        (1i64 << ctrls.len()) - 1
    } else {
        control_value
    };
    let mut step_ops = identity_ops(qubits);
    for &ctrl in ctrls.iter().rev() {
        step_ops[ctrl] = control_token(value & 1 != 0).to_owned();
        value >>= 1;
    }
    step_ops[target] = format!("Target1({op})");
    concatena(&step_ops, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concatena_joins_and_reverses() {
        assert_eq!(concatena(&["a", "b", "c"], false), "a,b,c");
        assert_eq!(concatena(&["a", "b", "c"], true), "c,b,a");
        assert_eq!(concatena::<&str>(&[], false), "");
    }

    #[test]
    fn register_gates_cover_exactly_the_register() {
        assert_eq!(hadamard(5, 1, 3), "ID,H,H,H,ID");
        assert_eq!(pauli_x(4, 0, 4), "X,X,X,X");
        assert_eq!(pauli_z(3, 2, 1), "ID,ID,Z");
        assert_eq!(hadamard(3, 3, 0), "ID,ID,ID");
    }

    #[test]
    fn controlled_gates_set_control_values() {
        assert_eq!(cnot(3, 0, 2, true), "Control1(1),ID,Target1(X)");
        assert_eq!(cnot(3, 2, 0, false), "Target1(X),ID,Control1(0)");
        assert_eq!(
            toffoli(4, 0, 1, 3, 0b10),
            "Control1(1),Control1(0),ID,Target1(X)"
        );
        assert_eq!(
            controlled2(4, 3, 0, 1, "R2", 0b01),
            "Control1(1),Target1(R2),ID,Control1(0)"
        );
        assert_eq!(controlled1(3, 1, 0, "H", true), "Target1(H),Control1(1),ID");
    }

    #[test]
    fn controlled_n_reads_bits_from_the_last_control() {
        // Valor 5 = 0b101 nos controles [5, 3, 1]: o 1 (último) fica com o bit 0.
        assert_eq!(
            controlled_n(6, &[5, 3, 1], 0, "Z", 5),
            "Target1(Z),Control1(1),ID,Control1(0),ID,Control1(1)"
        );
        assert_eq!(
            controlled_n(3, &[0, 1], 2, "X", -1),
            "Control1(1),Control1(1),Target1(X)"
        );
    }
}
