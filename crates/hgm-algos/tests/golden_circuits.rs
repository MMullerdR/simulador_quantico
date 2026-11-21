//! Os circuitos que o Rust gera (Grover, QFT, somador de Draper, rodada do Shor)
//! contra os casos `kind state` do C++ que os usam.
//!
//! Dois níveis de prova por caso:
//!
//! 1. os **passos** gerados são idênticos, linha a linha, aos que o C++ usou;
//! 2. o circuito gerado **e** o catálogo de portas gerado pelo Rust, executados no
//!    backend serial a partir do estado inicial do caso, dão o estado final do C++.
//!    Aqui as matrizes vêm do Rust (não das linhas `gate` do caso), então o teste
//!    pega erro de ângulo ou de nome de porta que a igualdade de texto não pegaria.

use hgm_algos::grover::{grover_step, iteration_count};
use hgm_algos::number_theory::mul_inv;
use hgm_algos::shor_circuits::{add_f, qft, rqft, shor_round_steps, Layout};
use hgm_algos::steps::hadamard;
use hgm_core::circuit::compile_steps;
use hgm_core::{Backend, GateCatalog, StateVector};
use hgm_cpu::SerialBackend;
use hgm_golden::{cases_of_kind, read_bytes, Case};

/// Rodadas do Shor aplicam milhares de portas com matrizes do Rust (`f64` arredondado)
/// em vez das do C++ (`cpowf` em `float`): o erro de cada porta (~1e-7) soma-se. A
/// pior diferença medida (qflex, 49 casos) é 5,5e-6, dentro da tolerância de
/// `tests/golden/README.md` para a CPU serial.
const STATE_TOLERANCE: f32 = 1e-5;

fn initial_state(case: &Case, qubits: usize) -> Result<StateVector, String> {
    let init = case.get("init").ok_or("falta `init`")?;
    if let Some(index) = init.strip_prefix("basis ") {
        let index: usize = index.trim().parse().map_err(|e| format!("init: {e}"))?;
        StateVector::basis(qubits, index).map_err(|e| e.to_string())
    } else {
        let bytes = read_bytes(&case.file_field("init")?)?;
        StateVector::from_f32_le_bytes(qubits, &bytes).map_err(|e| e.to_string())
    }
}

fn number(token: &str, prefix: &str) -> Result<i64, String> {
    token
        .strip_prefix(prefix)
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| format!("`{token}` não começa com `{prefix}` seguido de número"))
}

/// Passos que o Rust gera para o caso, ou `None` se o nome não é de um circuito
/// que o `hgm-algos` gera. O catálogo `gates` recebe as portas geradas.
fn generate(case: &Case, gates: &mut GateCatalog) -> Result<Option<Vec<String>>, String> {
    // O nome do caso começa com o índice (`0096-grover-q4-v5`).
    let parts: Vec<&str> = case.name.split('-').skip(1).collect();
    let degree = case.get("qft_degree").map_or(Ok(0), |d| {
        d.trim()
            .parse::<u32>()
            .map_err(|e| format!("qft_degree: {e}"))
    })?;

    let steps = match parts.as_slice() {
        ["grover", q, v] => {
            let qubits = number(q, "q")? as usize;
            let value = number(v, "v")? as u64;
            let mut steps = vec![hadamard(qubits, 0, qubits)];
            let iteration = grover_step(qubits, value);
            for _ in 0..iteration_count(qubits) {
                steps.extend(iteration.iter().cloned());
            }
            steps
        }
        ["shor", "qft", modulus, _, _] => {
            let l = Layout::for_modulus(number(modulus, "N")? as u64);
            qft(l.qubits, l.reg2, l.over, l.width, degree, gates)
        }
        ["shor", "qft", "rqft", modulus, _, _] => {
            let l = Layout::for_modulus(number(modulus, "N")? as u64);
            let mut steps = qft(l.qubits, l.reg2, l.over, l.width, degree, gates);
            steps.extend(rqft(l.qubits, l.reg2, l.over, l.width, degree, gates));
            steps
        }
        ["shor", "draper", "add", modulus, _, _] => {
            let l = Layout::for_modulus(number(modulus, "N")? as u64);
            let mut steps = qft(l.qubits, l.reg2, l.over, l.width, degree, gates);
            steps.push(add_f(l.qubits, l.reg2, l.over, 7, l.width, gates));
            steps.extend(rqft(l.qubits, l.reg2, l.over, l.width, degree, gates));
            steps
        }
        ["shor", "round", modulus, multiplier, _, _, _] => {
            let n = number(modulus, "N")?;
            let a = number(multiplier, "a")?;
            let l = Layout::for_modulus(n as u64);
            let inverse = mul_inv(a, n).ok_or("sem inverso modular")?;
            shor_round_steps(&l, a, inverse, n, degree, gates)
        }
        _ => return Ok(None),
    };
    Ok(Some(steps))
}

/// `Ok(Some(diferença de estado))` se o caso é de um circuito gerado e confere;
/// `Ok(None)` se não é.
fn check_case(case: &Case) -> Result<Option<f32>, String> {
    let mut gates = GateCatalog::with_base_gates();
    let Some(steps) = generate(case, &mut gates)? else {
        return Ok(None);
    };

    if steps != case.block("steps") {
        let first = steps
            .iter()
            .zip(case.block("steps"))
            .position(|(got, want)| got != want);
        return Err(format!(
            "passos diferentes (Rust {}, C++ {}; primeira diferença: {first:?})",
            steps.len(),
            case.block("steps").len()
        ));
    }

    let qubits = case.get_usize("qubits")?;
    let iterations = case.get_usize("iterations")?;
    let program = compile_steps(&steps, 1, &gates).map_err(|e| e.to_string())?;
    let mut state = initial_state(case, qubits)?;
    SerialBackend::new()
        .execute(&mut state, &program, iterations as u32)
        .map_err(|e| e.to_string())?;

    let expected = StateVector::from_f32_le_bytes(qubits, &read_bytes(&case.file_field("final")?)?)
        .map_err(|e| e.to_string())?;
    let diff = state.max_abs_diff(&expected);
    if diff < STATE_TOLERANCE {
        Ok(Some(diff))
    } else {
        Err(format!(
            "estado: máx. |diferença| {diff:e} >= {STATE_TOLERANCE:e}"
        ))
    }
}

#[test]
fn generated_circuits_match_cpp_golden_states() {
    let cases = cases_of_kind("state").expect("ler os casos state");

    let mut checked = 0;
    let mut worst = 0.0f32;
    let mut failures = Vec::new();
    for case in &cases {
        match check_case(case) {
            Ok(Some(diff)) => {
                checked += 1;
                worst = worst.max(diff);
            }
            Ok(None) => {}
            Err(message) => failures.push(format!("{}: {message}", case.name)),
        }
    }
    println!("circuitos gerados: {checked} casos, pior diferença de estado {worst:e}");
    assert!(failures.is_empty(), "falhas:\n{}", failures.join("\n"));
    // 3 Grover + 6 QFT + 6 QFT/RQFT + 6 Draper + 28 rodadas do Shor (12 + 12 + 4):
    // se alguém renomear casos e nada mais casar, o teste não pode passar em silêncio.
    assert_eq!(checked, 49, "casos de circuito conferidos");
}
