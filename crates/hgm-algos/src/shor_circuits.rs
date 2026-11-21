//! Circuitos do Shor: QFT, somador de Draper e multiplicação modular controlada
//! (`src/algorithms/lib_shor_circuits.cpp`; ver `docs/06-algoritmo-shor.md`).
//!
//! Cada função devolve passos de texto da linguagem de circuitos e registra no
//! catálogo `gates` as portas de rotação que esses passos citam (`R<n>`, `R'<n>`,
//! `ADD_<v>_<d>`, `SUB_<v>_<d>`, `Rot_<bits>`). Como no C++, o catálogo **não
//! sobrescreve** um nome que já existe (ver `docs/07` item 1): o nome determina a
//! matriz, então reaproveitar é correto.
//!
//! Diferença em relação ao C++: o grau da QFT aproximada (`set_qft_degree`, uma
//! variável global) é o argumento `qft_degree` das funções que dependem dele;
//! `0` é a QFT exata.
//!
//! As rotações são calculadas em `f64` e arredondadas uma vez para `float`. O
//! C++ multiplica `float complex` fator a fator (`cpowf`), então as matrizes
//! diferem por alguns `1e-7` (ver `tests/golden/README.md`); a estrutura dos
//! passos, que é o que decide o circuito, é igual.

// As assinaturas espelham as do C++ (qubits, ctrl1, ctrl2, reg, over, valor, largura,
// catálogo...), para o porte poder ser lido lado a lado com `lib_shor_circuits.cpp`.
#![allow(clippy::too_many_arguments)]

use std::f64::consts::PI;

use hgm_core::gates::{phase, phase_matrix, GateCatalog};
use hgm_core::C32;

use crate::steps::{cnot, concatena, hadamard, pauli_x};

const ONE: C32 = C32::new(1.0, 0.0);

fn identity_ops(qubits: usize) -> Vec<String> {
    vec!["ID".to_owned(); qubits]
}

/// Porta de correção de fase da estimação de fase semiclássica, a partir dos
/// bits já medidos (`genRot`). Se a rotação der exatamente 1, devolve um passo só
/// de `ID` (nunca uma string vazia, que o parser contaria como 0 qubits).
pub fn gen_rot(qubits: usize, reg: usize, phase_bits: u64, gates: &mut GateCatalog) -> String {
    let mut step_ops = identity_ops(qubits);

    // O bit menos significativo vale e^{-2*pi*i/4}, o seguinte e^{-2*pi*i/8}...
    let mut theta = 0.0f64;
    let mut remaining = phase_bits;
    let mut k = 2;
    while remaining != 0 {
        if remaining & 1 != 0 {
            theta -= 2.0 * PI / 2f64.powi(k);
        }
        remaining >>= 1;
        k += 1;
    }

    let rotation = phase(theta);
    if rotation != ONE {
        // O nome leva os bits originais: com um nome só, toda correção reaproveitaria
        // a matriz da primeira (item 1 de `docs/07`).
        let name = format!("Rot_{phase_bits}");
        gates.add_gate(&name, phase_matrix(rotation));
        step_ops[reg] = name;
    }
    concatena(&step_ops, false)
}

/// Soma (`subtract = false`) ou subtração de `value` no domínio de Fourier
/// (somador de Draper). Devolve os tokens do passo (um por qubit), **sem** juntar.
fn add_sub_f_ops(
    qubits: usize,
    reg: usize,
    over: usize,
    value: i64,
    width: usize,
    controlled: bool,
    subtract: bool,
    gates: &mut GateCatalog,
) -> Vec<String> {
    assert!(
        reg >= 1,
        "o registrador precisa ter um qubit antes (reg >= 1)"
    );
    let digit_count = width + 1;
    let sign = if subtract { -1.0 } else { 1.0 };

    // Cada dígito de `value` que vale 1 gira o dígito `d` e todos os mais altos.
    let mut theta = vec![0.0f64; digit_count];
    for digit in 0..digit_count {
        if (value >> digit) & 1 != 0 {
            for (higher, angle) in theta.iter_mut().enumerate().skip(digit) {
                *angle += sign * 2.0 * PI / 2f64.powi((higher - digit + 1) as i32);
            }
        }
    }

    let prefix = if subtract { "SUB_" } else { "ADD_" };
    let msb_pos = reg + width - 1;
    let mut step_ops = identity_ops(qubits);
    for (digit, &angle) in theta.iter().enumerate() {
        let rotation = phase(angle);
        if rotation != ONE {
            let name = format!("{prefix}{value}_{digit}");
            gates.add_gate(&name, phase_matrix(rotation));
            step_ops[msb_pos - digit] = if controlled {
                format!("Target1({name})")
            } else {
                name
            };
        }
    }

    // O dígito mais alto (que caiu em `reg - 1`) vive, de fato, no qubit `over`.
    let top = std::mem::replace(&mut step_ops[reg - 1], "ID".to_owned());
    step_ops[over] = top;
    step_ops
}

/// `AddF` com a opção de porta controlada (alvos como `Target1(...)`): os tokens
/// do passo, sem juntar.
pub fn add_f_ops(
    qubits: usize,
    reg: usize,
    over: usize,
    value: i64,
    width: usize,
    controlled: bool,
    gates: &mut GateCatalog,
) -> Vec<String> {
    add_sub_f_ops(qubits, reg, over, value, width, controlled, false, gates)
}

/// `SubF` com a opção de porta controlada: os tokens do passo, sem juntar.
pub fn sub_f_ops(
    qubits: usize,
    reg: usize,
    over: usize,
    value: i64,
    width: usize,
    controlled: bool,
    gates: &mut GateCatalog,
) -> Vec<String> {
    add_sub_f_ops(qubits, reg, over, value, width, controlled, true, gates)
}

/// `reg += value` no domínio de Fourier.
pub fn add_f(
    qubits: usize,
    reg: usize,
    over: usize,
    value: i64,
    width: usize,
    gates: &mut GateCatalog,
) -> String {
    concatena(
        &add_f_ops(qubits, reg, over, value, width, false, gates),
        false,
    )
}

/// `reg -= value` no domínio de Fourier.
pub fn sub_f(
    qubits: usize,
    reg: usize,
    over: usize,
    value: i64,
    width: usize,
    gates: &mut GateCatalog,
) -> String {
    concatena(
        &sub_f_ops(qubits, reg, over, value, width, false, gates),
        false,
    )
}

fn with_controls(mut step_ops: Vec<String>, controls: &[usize]) -> String {
    for &control in controls {
        step_ops[control] = "Control1(1)".to_owned();
    }
    concatena(&step_ops, false)
}

/// `AddF` com um controle em `ctrl1`.
pub fn c_add_f(
    qubits: usize,
    ctrl1: usize,
    reg: usize,
    over: usize,
    value: i64,
    width: usize,
    gates: &mut GateCatalog,
) -> String {
    with_controls(
        add_f_ops(qubits, reg, over, value, width, true, gates),
        &[ctrl1],
    )
}

/// `AddF` com dois controles.
pub fn c2_add_f(
    qubits: usize,
    ctrl1: usize,
    ctrl2: usize,
    reg: usize,
    over: usize,
    value: i64,
    width: usize,
    gates: &mut GateCatalog,
) -> String {
    with_controls(
        add_f_ops(qubits, reg, over, value, width, true, gates),
        &[ctrl1, ctrl2],
    )
}

/// `SubF` com um controle.
pub fn c_sub_f(
    qubits: usize,
    ctrl1: usize,
    reg: usize,
    over: usize,
    value: i64,
    width: usize,
    gates: &mut GateCatalog,
) -> String {
    with_controls(
        sub_f_ops(qubits, reg, over, value, width, true, gates),
        &[ctrl1],
    )
}

/// `SubF` com dois controles.
pub fn c2_sub_f(
    qubits: usize,
    ctrl1: usize,
    ctrl2: usize,
    reg: usize,
    over: usize,
    value: i64,
    width: usize,
    gates: &mut GateCatalog,
) -> String {
    with_controls(
        sub_f_ops(qubits, reg, over, value, width, true, gates),
        &[ctrl1, ctrl2],
    )
}

/// QFT (`inverse = false`) ou sua inversa. `reg`/`width` são o registrador,
/// `over` o qubit extra de estouro. `qft_degree > 0` descarta as rotações
/// controladas de nível maior que ele (QFT aproximada); `0` é a exata.
fn qft_impl(
    qubits: usize,
    reg: usize,
    over: usize,
    width: usize,
    inverse: bool,
    qft_degree: u32,
    gates: &mut GateCatalog,
) -> Vec<String> {
    // Nível de uma rotação controlada = distância entre controle e alvo + 1
    // (ângulo 2*pi/2^nível); acima de `max_level` ela é descartada.
    let max_level = if qft_degree > 0 {
        qft_degree as usize
    } else {
        width + 2
    };
    let prefix = if inverse { "R'" } else { "R" };
    let sign = if inverse { -1.0 } else { 1.0 };

    // Todas as rotações de 1 a width+1 entram no catálogo, usadas ou não (o C++
    // faz o mesmo, e os vetores de referência listam todas).
    for level in 1..=width + 1 {
        let angle = sign * 2.0 * PI / 2f64.powi(level as i32);
        gates.add_gate(&format!("{prefix}{level}"), phase_matrix(phase(angle)));
    }

    let mut step_ops = identity_ops(qubits);
    let mut steps = vec![hadamard(qubits, over, 1)];

    for control in 0..width {
        if control + 2 > max_level {
            continue;
        }
        step_ops[control + reg] = "Control1(1)".to_owned();
        step_ops[over] = format!("Target1({prefix}{})", control + 2);
        steps.push(concatena(&step_ops, false));
        step_ops[control + reg] = "ID".to_owned();
    }
    step_ops[over] = "ID".to_owned();

    for target in 0..width {
        steps.push(hadamard(qubits, target + reg, 1));
        for control in target + 1..width {
            if control - target + 1 > max_level {
                continue;
            }
            step_ops[control + reg] = "Control1(1)".to_owned();
            step_ops[target + reg] = format!("Target1({prefix}{})", control - target + 1);
            steps.push(concatena(&step_ops, false));
            step_ops[control + reg] = "ID".to_owned();
        }
        step_ops[target + reg] = "ID".to_owned();
    }

    if inverse {
        steps.reverse();
    }
    steps
}

/// Transformada de Fourier quântica do registrador `reg..reg+width`.
pub fn qft(
    qubits: usize,
    reg: usize,
    over: usize,
    width: usize,
    qft_degree: u32,
    gates: &mut GateCatalog,
) -> Vec<String> {
    qft_impl(qubits, reg, over, width, false, qft_degree, gates)
}

/// Inversa da QFT.
pub fn rqft(
    qubits: usize,
    reg: usize,
    over: usize,
    width: usize,
    qft_degree: u32,
    gates: &mut GateCatalog,
) -> Vec<String> {
    qft_impl(qubits, reg, over, width, true, qft_degree, gates)
}

/// Troca `reg1` e `reg2` (`width` qubits), controlada por `ctrl`, com 3 CNOTs por
/// par de qubits.
pub fn c_swap_r(qubits: usize, ctrl: usize, reg1: usize, reg2: usize, width: usize) -> Vec<String> {
    let mut swap_steps = Vec::with_capacity(3 * width);
    let mut step_ops = identity_ops(qubits);

    for qubit in 0..width {
        step_ops[ctrl] = "Control1(1)".to_owned();
        step_ops[qubit + reg1] = "Target1(X)".to_owned();
        step_ops[qubit + reg2] = "Control1(1)".to_owned();
        let joined1 = concatena(&step_ops, false);

        step_ops[ctrl] = "ID".to_owned();
        step_ops[qubit + reg1] = "Control1(1)".to_owned();
        step_ops[qubit + reg2] = "Target1(X)".to_owned();
        let joined2 = concatena(&step_ops, false);

        step_ops[qubit + reg1] = "ID".to_owned();
        step_ops[qubit + reg2] = "ID".to_owned();

        swap_steps.push(joined2.clone());
        swap_steps.push(joined1);
        swap_steps.push(joined2);
    }
    swap_steps
}

/// Inverte a ordem dos qubits de um registrador (troca dos extremos para o centro).
pub fn swap_over(qubits: usize, reg: usize, width: usize) -> Vec<String> {
    let mut swap_steps = Vec::with_capacity(3 * (width / 2));
    for qubit in 0..width / 2 {
        let far = reg + width - qubit - 1;
        let near = reg + qubit;
        swap_steps.push(cnot(qubits, far, near, true));
        swap_steps.push(cnot(qubits, near, far, true));
        swap_steps.push(cnot(qubits, far, near, true));
    }
    swap_steps
}

/// Posições dos registradores do circuito do Shor para um módulo de `width` bits
/// (`lib_shor.cpp`): qubit de fase, `reg1`, qubit de estouro `over`, `reg2` e o
/// booleano de estouro `over_bool` (o último qubit).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub width: usize,
    pub qubits: usize,
    pub ctrl: usize,
    pub reg1: usize,
    pub over: usize,
    pub reg2: usize,
    pub over_bool: usize,
}

impl Layout {
    /// Layout para fatorar `modulus`: `width` é o número de bits dele.
    pub fn for_modulus(modulus: u64) -> Self {
        let width = (u64::BITS - modulus.leading_zeros()) as usize;
        let qubits = 2 * width + 3;
        Self {
            width,
            qubits,
            ctrl: 0,
            reg1: 1,
            over: width + 1,
            reg2: width + 2,
            over_bool: qubits - 1,
        }
    }
}

/// Soma modular controlada por dois qubits (`reg += value mod N`).
pub fn c2_add_mod(
    layout: &Layout,
    ctrl1: usize,
    ctrl2: usize,
    value: i64,
    modulus: i64,
    qft_degree: u32,
    gates: &mut GateCatalog,
) -> Vec<String> {
    let Layout {
        qubits,
        reg2: reg,
        over,
        over_bool,
        width,
        ..
    } = *layout;
    // `qft` não entra nos passos abaixo, mas a chamada registra as portas `R<n>`,
    // como no C++ (o catálogo final é parte do que os vetores de referência conferem).
    let qft = qft(qubits, reg, over, width, qft_degree, gates);
    let rqft = rqft(qubits, reg, over, width, qft_degree, gates);

    let c2_add_a = c2_add_f(qubits, ctrl1, ctrl2, reg, over, value, width, gates);
    let c2_sub_a = c2_sub_f(qubits, ctrl1, ctrl2, reg, over, value, width, gates);
    let sub_n = sub_f(qubits, reg, over, modulus, width, gates);
    let c_add_n = c_add_f(qubits, over_bool, reg, over, modulus, width, gates);
    let n_over = pauli_x(qubits, over, 1);
    let c_over = cnot(qubits, over, over_bool, true);

    let mut steps = vec![c2_add_a.clone(), sub_n];
    steps.extend(rqft.iter().cloned());
    steps.push(c_over.clone());
    steps.extend(qft.iter().cloned());
    steps.push(c_add_n);
    steps.push(c2_sub_a);
    steps.extend(rqft);
    steps.push(n_over.clone());
    steps.push(c_over);
    steps.push(n_over);
    steps.extend(qft);
    steps.push(c2_add_a);
    steps
}

/// Subtração modular controlada por dois qubits. Não é o espelho de
/// [`c2_add_mod`]: a sequência de passos do cálculo e do desfazer é diferente.
pub fn c2_sub_mod(
    layout: &Layout,
    ctrl1: usize,
    ctrl2: usize,
    value: i64,
    modulus: i64,
    qft_degree: u32,
    gates: &mut GateCatalog,
) -> Vec<String> {
    let Layout {
        qubits,
        reg2: reg,
        over,
        over_bool,
        width,
        ..
    } = *layout;
    let qft = qft(qubits, reg, over, width, qft_degree, gates);
    let rqft = rqft(qubits, reg, over, width, qft_degree, gates);

    let c2_add_a = c2_add_f(qubits, ctrl1, ctrl2, reg, over, value, width, gates);
    let c2_sub_a = c2_sub_f(qubits, ctrl1, ctrl2, reg, over, value, width, gates);
    let add_n = add_f(qubits, reg, over, modulus, width, gates);
    // O C++ também monta `CAddF(...)` aqui, sem usar o passo; só registraria
    // `ADD_N_<d>`, que `add_n` acima já registrou (mesmo nome, mesma matriz).
    let c_sub_n = c_sub_f(qubits, over_bool, reg, over, modulus, width, gates);
    let n_over = pauli_x(qubits, over, 1);
    let c_over = cnot(qubits, over, over_bool, true);

    let mut steps = vec![c2_sub_a.clone()];
    steps.extend(rqft.iter().cloned());
    steps.push(n_over.clone());
    steps.push(c_over.clone());
    steps.push(n_over);
    steps.extend(qft.iter().cloned());
    steps.push(c2_add_a);
    steps.push(c_sub_n);
    steps.extend(rqft);
    steps.push(c_over);
    steps.extend(qft);
    steps.push(add_n);
    steps.push(c2_sub_a);
    steps
}

/// Multiplicação modular controlada (`reg2 += value * reg1 mod N`) por somas
/// controladas no domínio de Fourier.
pub fn c_mult_mod(
    layout: &Layout,
    value: i64,
    modulus: i64,
    qft_degree: u32,
    gates: &mut GateCatalog,
) -> Vec<String> {
    let Layout {
        qubits,
        ctrl,
        reg1,
        reg2,
        over,
        width,
        ..
    } = *layout;
    // Registra as portas `R<n>` como o C++ (o `qft` não entra nos passos).
    let _qft = qft(qubits, reg2, over, width, qft_degree, gates);
    let rqft = rqft(qubits, reg2, over, width, qft_degree, gates);

    let mut steps = vec![hadamard(qubits, over, 1), hadamard(qubits, reg2, width)];

    let ctrl2 = reg1 + width - 1;
    let mut term = value;
    for bit in 0..width {
        steps.extend(c2_add_mod(
            layout,
            ctrl,
            ctrl2 - bit,
            term,
            modulus,
            qft_degree,
            gates,
        ));
        term = (term * 2) % modulus;
    }

    steps.extend(rqft);
    steps
}

/// Desfaz [`c_mult_mod`] (`reg2 -= value * reg1 mod N`). `value` precisa ser o
/// **inverso modular** do multiplicador usado em `c_mult_mod`, depois da troca
/// `reg1 <-> reg2` (item 30 de `docs/07`).
pub fn cr_mult_mod(
    layout: &Layout,
    value: i64,
    modulus: i64,
    qft_degree: u32,
    gates: &mut GateCatalog,
) -> Vec<String> {
    let Layout {
        qubits,
        ctrl,
        reg1,
        reg2,
        over,
        width,
        ..
    } = *layout;
    let qft = qft(qubits, reg2, over, width, qft_degree, gates);
    let rqft = rqft(qubits, reg2, over, width, qft_degree, gates);

    // Cada bloco novo entra **no começo** (o C++ insere em `begin()`), então os
    // blocos saem em ordem inversa à de criação.
    let ctrl2 = reg1 + width - 1;
    let mut blocks = Vec::with_capacity(width);
    let mut term = value;
    for bit in 0..width {
        blocks.push(c2_sub_mod(
            layout,
            ctrl,
            ctrl2 - bit,
            term,
            modulus,
            qft_degree,
            gates,
        ));
        term = (term * 2) % modulus;
    }

    let mut steps = qft;
    for block in blocks.into_iter().rev() {
        steps.extend(block);
    }
    steps.extend(rqft);
    steps
}

/// Uma rodada do Shor sem a medição: `CMultMod` + `CSwapR` + `CRMultMod` com o
/// inverso modular do multiplicador. `inverse` é `multiplier^-1 mod N`.
pub fn shor_round_steps(
    layout: &Layout,
    multiplier: i64,
    inverse: i64,
    modulus: i64,
    qft_degree: u32,
    gates: &mut GateCatalog,
) -> Vec<String> {
    let mut steps = c_mult_mod(layout, multiplier, modulus, qft_degree, gates);
    steps.extend(c_swap_r(
        layout.qubits,
        layout.ctrl,
        layout.reg1,
        layout.reg2,
        layout.width,
    ));
    steps.extend(cr_mult_mod(layout, inverse, modulus, qft_degree, gates));
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_the_cpp_formulas() {
        let l = Layout::for_modulus(15);
        assert_eq!(
            l,
            Layout {
                width: 4,
                qubits: 11,
                ctrl: 0,
                reg1: 1,
                over: 5,
                reg2: 6,
                over_bool: 10,
            }
        );
        assert_eq!(Layout::for_modulus(21).qubits, 13);
        assert_eq!(Layout::for_modulus(2863).qubits, 27);
    }

    #[test]
    fn gen_rot_zero_is_identity_and_names_use_the_bits() {
        let mut gates = GateCatalog::with_base_gates();
        assert_eq!(gen_rot(3, 1, 0, &mut gates), "ID,ID,ID");
        assert_eq!(gen_rot(3, 1, 5, &mut gates), "ID,Rot_5,ID");
        assert!(gates.get("Rot_5").is_some());
        // O bit menos significativo vale -2*pi/4 = -pi/2.
        let one = gen_rot(2, 0, 1, &mut gates);
        assert_eq!(one, "Rot_1,ID");
        let matrix = gates.get("Rot_1").unwrap();
        assert!(matrix[3].re.abs() < 1e-6 && (matrix[3].im + 1.0).abs() < 1e-6);
    }

    #[test]
    fn qft_registers_every_level_and_inverse_reverses() {
        let layout = Layout::for_modulus(15);
        let mut gates = GateCatalog::empty();
        let forward = qft(
            layout.qubits,
            layout.reg2,
            layout.over,
            layout.width,
            0,
            &mut gates,
        );
        for level in 1..=layout.width + 1 {
            assert!(gates.get(&format!("R{level}")).is_some());
        }
        let mut inverse_gates = GateCatalog::empty();
        let mut backward = rqft(
            layout.qubits,
            layout.reg2,
            layout.over,
            layout.width,
            0,
            &mut inverse_gates,
        );
        assert!(inverse_gates.get("R'1").is_some());
        // Mesmos passos, ordem invertida, com R' no lugar de R.
        backward.reverse();
        let renamed: Vec<String> = backward.iter().map(|s| s.replace("R'", "R")).collect();
        assert_eq!(renamed, forward);
    }

    #[test]
    fn approximate_qft_drops_high_levels() {
        let layout = Layout::for_modulus(57);
        let mut gates = GateCatalog::empty();
        let exact = qft(
            layout.qubits,
            layout.reg2,
            layout.over,
            layout.width,
            0,
            &mut gates,
        );
        let approx = qft(
            layout.qubits,
            layout.reg2,
            layout.over,
            layout.width,
            3,
            &mut gates,
        );
        assert!(approx.len() < exact.len());
        assert!(approx.iter().all(|step| !step.contains("R4")));
    }

    #[test]
    fn swap_over_and_cswap_have_three_steps_per_pair() {
        assert_eq!(swap_over(8, 1, 4).len(), 6);
        assert_eq!(swap_over(8, 1, 5).len(), 6);
        let steps = c_swap_r(11, 0, 1, 6, 4);
        assert_eq!(steps.len(), 12);
        assert_eq!(steps[0], steps[2]);
    }

    #[test]
    fn add_f_puts_the_top_digit_on_the_overflow_qubit() {
        let layout = Layout::for_modulus(15);
        let mut gates = GateCatalog::empty();
        let ops = add_f_ops(
            layout.qubits,
            layout.reg2,
            layout.over,
            7,
            layout.width,
            false,
            &mut gates,
        );
        assert_eq!(ops.len(), layout.qubits);
        // Dígitos 0..width-1 ficam em reg2+width-1 descendo; o dígito `width` (o mais
        // alto) cai em `reg2 - 1`, que no layout do Shor já é `over`.
        assert_eq!(layout.over, layout.reg2 - 1);
        assert_eq!(ops[layout.over], "ADD_7_4");
        assert_eq!(ops[layout.reg2 + layout.width - 1], "ADD_7_0");
        assert_eq!(ops[layout.over_bool], "ID");
    }

    #[test]
    fn controlled_add_marks_controls_and_targets() {
        let layout = Layout::for_modulus(15);
        let mut gates = GateCatalog::empty();
        let step = c2_add_f(
            layout.qubits,
            0,
            1,
            layout.reg2,
            layout.over,
            6,
            layout.width,
            &mut gates,
        );
        let tokens: Vec<&str> = step.split(',').collect();
        assert_eq!(&tokens[..2], ["Control1(1)", "Control1(1)"]);
        assert!(tokens.iter().any(|t| t.starts_with("Target1(ADD_6_")));
    }
}
