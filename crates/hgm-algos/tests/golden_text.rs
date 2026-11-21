//! Paridade dos geradores de circuito com o C++ (`tests/golden`, casos `kind text`).
//!
//! Cada caso guarda a chamada (`call ...`), o grau da QFT, as linhas que o C++
//! gerou e as portas que a chamada criou. O Rust tem que gerar exatamente as
//! mesmas linhas e criar exatamente o mesmo conjunto de portas (com matrizes
//! iguais a menos de arredondamento).

use std::collections::{BTreeSet, HashMap};

use hgm_algos::grover::{controlled_z, oracle1};
use hgm_algos::shor_circuits::{
    add_f, add_f_ops, c2_add_f, c2_add_mod, c2_sub_f, c2_sub_mod, c_add_f, c_mult_mod, c_sub_f,
    c_swap_r, cr_mult_mod, gen_rot, qft, rqft, sub_f, swap_over, Layout,
};
use hgm_algos::steps::{
    cnot, concatena, controlled1, controlled2, controlled_n, hadamard, pauli_x, pauli_z, toffoli,
};
use hgm_core::GateCatalog;
use hgm_golden::{cases_of_kind, Case};

/// As rotações do C++ saem de produtos de `float complex` (`cpowf`); as do Rust, de
/// uma soma de ângulos em `f64` arredondada uma vez. Diferem de alguns 1e-7.
const MATRIX_TOLERANCE: f32 = 1e-6;

struct Call {
    function: String,
    positional: Vec<String>,
    named: HashMap<String, String>,
}

impl Call {
    fn parse(text: &str) -> Call {
        let mut words = text.split_whitespace();
        let function = words.next().expect("call vazia").to_owned();
        let mut positional = Vec::new();
        let mut named = HashMap::new();
        for word in words {
            match word.split_once('=') {
                Some((key, value)) => {
                    named.insert(key.to_owned(), value.to_owned());
                }
                None => positional.push(word.to_owned()),
            }
        }
        Call {
            function,
            positional,
            named,
        }
    }

    fn int(&self, key: &str) -> i64 {
        self.named
            .get(key)
            .unwrap_or_else(|| panic!("falta `{key}` em `{}`", self.function))
            .parse()
            .unwrap_or_else(|e| panic!("`{key}` inválido em `{}`: {e}", self.function))
    }

    fn size(&self, key: &str) -> usize {
        usize::try_from(self.int(key)).expect("valor negativo")
    }

    fn flag(&self, key: &str) -> bool {
        self.int(key) != 0
    }

    /// `N15` -> layout do Shor para o módulo 15.
    fn layout(&self) -> (Layout, i64) {
        let tag = &self.positional[0];
        let modulus: u64 = tag
            .strip_prefix('N')
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("módulo inválido `{tag}`"));
        (Layout::for_modulus(modulus), modulus as i64)
    }
}

/// Gera as linhas que o C++ geraria para `call`, registrando portas em `gates`.
fn generate(call: &Call, degree: u32, gates: &mut GateCatalog) -> Vec<String> {
    match call.function.as_str() {
        "Hadamard" => vec![hadamard(
            call.size("qubits"),
            call.size("reg"),
            call.size("width"),
        )],
        "Pauli_X" => vec![pauli_x(
            call.size("qubits"),
            call.size("reg"),
            call.size("width"),
        )],
        "Pauli_Z" => vec![pauli_z(
            call.size("qubits"),
            call.size("reg"),
            call.size("width"),
        )],
        "CNot" => vec![cnot(
            call.size("qubits"),
            call.size("ctrl"),
            call.size("target"),
            call.flag("cv"),
        )],
        "Toffoli" => vec![toffoli(
            call.size("qubits"),
            call.size("c1"),
            call.size("c2"),
            call.size("target"),
            call.size("cv") as u32,
        )],
        "Controlled1" => vec![controlled1(
            call.size("qubits"),
            call.size("ctrl"),
            call.size("target"),
            &call.named["op"],
            call.flag("cv"),
        )],
        "Controlled2" => vec![controlled2(
            call.size("qubits"),
            call.size("c1"),
            call.size("c2"),
            call.size("target"),
            &call.named["op"],
            call.size("cv") as u32,
        )],
        "ControlledN" => {
            let ctrls: Vec<usize> = call.named["ctrls"]
                .split(',')
                .map(|c| c.parse().expect("ctrl"))
                .collect();
            vec![controlled_n(
                call.size("qubits"),
                &ctrls,
                call.size("target"),
                &call.named["op"],
                call.int("cv"),
            )]
        }
        "concatena" => {
            let tokens: Vec<&str> = call.positional[0].split(',').collect();
            vec![concatena(&tokens, call.named["reverse"] == "true")]
        }
        "Oracle1" => vec![oracle1(call.size("qubits"), call.int("value") as u64)],
        "ControledZ" => vec![controlled_z(call.size("qubits"))],
        "genRot" => vec![gen_rot(
            call.size("qubits"),
            call.size("reg"),
            call.int("phase_bits") as u64,
            gates,
        )],
        // ---- circuitos do Shor: `N15`/`N21` dão o layout; os registradores vêm dele
        "QFT" => {
            let (l, _) = call.layout();
            qft(l.qubits, l.reg2, l.over, l.width, degree, gates)
        }
        "RQFT" => {
            let (l, _) = call.layout();
            rqft(l.qubits, l.reg2, l.over, l.width, degree, gates)
        }
        "AddF" => {
            let (l, _) = call.layout();
            vec![add_f(
                l.qubits,
                l.reg2,
                l.over,
                call.int("value"),
                l.width,
                gates,
            )]
        }
        "AddF(controlled)" => {
            let (l, _) = call.layout();
            add_f_ops(
                l.qubits,
                l.reg2,
                l.over,
                call.int("value"),
                l.width,
                true,
                gates,
            )
        }
        "SubF" => {
            let (l, _) = call.layout();
            vec![sub_f(
                l.qubits,
                l.reg2,
                l.over,
                call.int("value"),
                l.width,
                gates,
            )]
        }
        "CAddF" => {
            let (l, _) = call.layout();
            vec![c_add_f(
                l.qubits,
                call.size("ctrl"),
                l.reg2,
                l.over,
                call.int("value"),
                l.width,
                gates,
            )]
        }
        "CSubF" => {
            let (l, _) = call.layout();
            vec![c_sub_f(
                l.qubits,
                call.size("ctrl"),
                l.reg2,
                l.over,
                call.int("value"),
                l.width,
                gates,
            )]
        }
        "C2AddF" => {
            let (l, _) = call.layout();
            vec![c2_add_f(
                l.qubits,
                call.size("ctrl1"),
                call.size("ctrl2"),
                l.reg2,
                l.over,
                call.int("value"),
                l.width,
                gates,
            )]
        }
        "C2SubF" => {
            let (l, _) = call.layout();
            vec![c2_sub_f(
                l.qubits,
                call.size("ctrl1"),
                call.size("ctrl2"),
                l.reg2,
                l.over,
                call.int("value"),
                l.width,
                gates,
            )]
        }
        "CSwapR" => {
            let (l, _) = call.layout();
            c_swap_r(l.qubits, l.ctrl, l.reg1, l.reg2, l.width)
        }
        "SwapOver" => {
            let (l, _) = call.layout();
            swap_over(l.qubits, l.reg1, l.width)
        }
        // O exportador fixa os controles da soma modular em 0 e 4.
        "C2AddMod" => {
            let (l, n) = call.layout();
            c2_add_mod(&l, 0, 4, call.int("a"), n, degree, gates)
        }
        "C2SubMod" => {
            let (l, n) = call.layout();
            c2_sub_mod(&l, 0, 4, call.int("a"), n, degree, gates)
        }
        "CMultMod" => {
            let (l, n) = call.layout();
            c_mult_mod(&l, call.int("a"), n, degree, gates)
        }
        "CRMultMod" => {
            let (l, n) = call.layout();
            cr_mult_mod(&l, call.int("a"), n, degree, gates)
        }
        other => panic!("gerador desconhecido nos vetores de referência: {other}"),
    }
}

/// `Ok(pior diferença de matriz)` ou a descrição da falha.
fn check_case(case: &Case) -> Result<f32, String> {
    let call_text = case.get("call").ok_or("falta `call`")?;
    let degree = case.get_usize("qft_degree")? as u32;
    let call = Call::parse(call_text);

    let mut catalog = GateCatalog::with_base_gates();
    let base: BTreeSet<String> = catalog.names().map(str::to_owned).collect();
    let lines = generate(&call, degree, &mut catalog);

    if lines != case.block("lines") {
        let first = lines
            .iter()
            .zip(case.block("lines"))
            .position(|(got, want)| got != want);
        return Err(format!(
            "linhas diferentes (Rust {} linhas, C++ {}; primeira diferença: {first:?})",
            lines.len(),
            case.block("lines").len()
        ));
    }

    let created: BTreeSet<String> = catalog
        .names()
        .filter(|name| !base.contains(*name))
        .map(str::to_owned)
        .collect();
    let expected: BTreeSet<String> = case.gates.iter().map(|(name, _)| name.clone()).collect();
    if created != expected {
        let only_rust: Vec<_> = created.difference(&expected).collect();
        let only_cpp: Vec<_> = expected.difference(&created).collect();
        return Err(format!(
            "portas criadas diferentes: só no Rust {only_rust:?}, só no C++ {only_cpp:?}"
        ));
    }

    let mut worst = 0.0f32;
    for (name, want) in &case.gates {
        let got = catalog.get(name).expect("porta criada");
        for (g, w) in got.iter().zip(want) {
            worst = worst.max((g.re - w.re).abs()).max((g.im - w.im).abs());
        }
        if worst >= MATRIX_TOLERANCE {
            return Err(format!("matriz de {name}: diferença {worst:e}"));
        }
    }
    Ok(worst)
}

#[test]
fn generators_match_cpp_golden_text() {
    let cases = cases_of_kind("text").expect("ler os casos text");
    assert_eq!(cases.len(), 88, "o INDEX deveria ter 88 casos text");

    let mut worst = 0.0f32;
    let mut failures = Vec::new();
    for case in &cases {
        match check_case(case) {
            Ok(diff) => worst = worst.max(diff),
            Err(message) => failures.push(format!("{}: {message}", case.name)),
        }
    }
    println!(
        "text: {} casos, pior diferença de matriz {worst:e}",
        cases.len()
    );
    assert!(failures.is_empty(), "falhas:\n{}", failures.join("\n"));
}
