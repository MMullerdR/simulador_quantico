//! Execuções completas contra o C++ (`tests/golden/data/runs.txt`, gerado por
//! `make golden-runs`): `Grover()` e `Shor()` do tag `v1.1.0` no `t_CPU`, com o
//! gerador semeado (`seed_rng(seed)`).
//!
//! O Rust usa o mesmo MT19937, a mesma `uniform_int_distribution` do libstdc++ (para
//! a base do Shor) e a mesma regra de medição, então com a mesma semente tem que
//! escolher a mesma base, medir os mesmos bits e devolver exatamente o mesmo valor
//! (Grover) ou os mesmos fatores, na mesma ordem (Shor). É a prova de ponta a ponta
//! de que os executáveis calculam o mesmo que o C++, não só que cada peça confere.
//!
//! Uma diferença aqui só pode vir de uma medição cuja amostra cai a ~1e-6 do limiar
//! (as probabilidades do Rust e do C++ diferem desse tanto, por arredondamento); se
//! acontecer, o teste diz qual linha foi.

use std::fs;

use hgm_algos::grover::grover;
use hgm_algos::shor::{shor, ShorOptions};
use hgm_cpu::Tuning;
use hgm_engine::{BackendKind, SimConfig};
use hgm_golden::data_dir;

enum Run {
    Grover {
        qubits: usize,
        value: u64,
        seed: u32,
        result: u64,
    },
    Shor {
        number: u64,
        seed: u32,
        options: ShorOptions,
        factors: Vec<u64>,
    },
}

fn field<'a>(fields: &'a [(&'a str, &'a str)], key: &str, line: &str) -> &'a str {
    fields
        .iter()
        .find(|(k, _)| *k == key)
        .unwrap_or_else(|| panic!("falta `{key}` em `{line}`"))
        .1
}

fn parse_runs() -> Vec<(String, Run)> {
    let path = data_dir().join("runs.txt");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut runs = Vec::new();

    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') || line.starts_with("format ") {
            continue;
        }
        let (kind, rest) = line.split_once(' ').expect("linha sem campos");
        let fields: Vec<(&str, &str)> = rest
            .split_whitespace()
            .map(|pair| {
                pair.split_once('=')
                    .unwrap_or_else(|| panic!("`{pair}` sem `=`"))
            })
            .collect();
        let get = |key| field(&fields, key, line);
        let number = |key| -> u64 { get(key).parse().unwrap_or_else(|e| panic!("`{key}`: {e}")) };

        let run = match kind {
            "grover" => Run::Grover {
                qubits: number("qubits") as usize,
                value: number("value"),
                seed: number("seed") as u32,
                result: number("result"),
            },
            "shor" => Run::Shor {
                number: number("n"),
                seed: number("seed") as u32,
                options: ShorOptions {
                    emulate_mult: get("emulate") == "1",
                    qft_degree: match get("aqft") {
                        "-" => None,
                        degree => Some(degree.parse().expect("aqft")),
                    },
                },
                factors: match get("factors") {
                    "none" => Vec::new(),
                    pair => pair.split(',').map(|f| f.parse().expect("fator")).collect(),
                },
            },
            other => panic!("tipo de execução desconhecido: {other}"),
        };
        runs.push((line.to_owned(), run));
    }
    runs
}

fn check_all(backend: BackendKind, label: &str) {
    let runs = parse_runs();
    assert_eq!(
        runs.len(),
        108,
        "o runs.txt deveria ter 40 Grover + 68 Shor"
    );

    let mut failures = Vec::new();
    for (line, run) in &runs {
        match run {
            Run::Grover {
                qubits,
                value,
                seed,
                result,
            } => {
                let config = SimConfig::new(backend, *seed);
                let got = grover(&config, *qubits, *value).expect("grover");
                if got != *result {
                    failures.push(format!("{line}: Rust mediu {got}"));
                }
            }
            Run::Shor {
                number,
                seed,
                options,
                factors,
            } => {
                let config = SimConfig::new(backend, *seed);
                let got = shor(&config, *number, options).expect("shor");
                if got != *factors {
                    failures.push(format!("{line}: Rust deu {got:?}"));
                }
            }
        }
    }
    println!("runs ({label}): {} execuções conferidas", runs.len());
    assert!(failures.is_empty(), "falhas:\n{}", failures.join("\n"));
}

#[test]
fn serial_backend_reproduces_cpp_runs() {
    check_all(BackendKind::CpuSerial, "serial");
}

#[test]
fn parallel_backend_reproduces_cpp_runs() {
    check_all(
        BackendKind::CpuParallel(Tuning {
            threads: 4,
            coalesced_bits: 3,
            region_bits: 8,
        }),
        "paralelo",
    );
}
