//! Algoritmo de Shor com estimação de fase semiclássica (`src/algorithms/lib_shor.cpp`;
//! ver `docs/06-algoritmo-shor.md`): um único qubit de fase, reaproveitado a cada
//! rodada.
//!
//! Layout dos `2*width + 3` qubits (`width` = bits do número): o qubit de fase, o
//! registrador `reg1` (o expoente da multiplicação, em `|1>`), o qubit de estouro
//! `over`, o registrador `reg2` (o acumulador) e o booleano de estouro.

use hgm_core::error::{HgmError, Result};
use hgm_core::MAX_QUBITS;
use hgm_engine::{SimConfig, Simulator};

use crate::number_theory::{
    modular_pow, mul_inv, qft_degree_for_width, quantum_frac_approx, quantum_gcd, revert_bits,
};
use crate::shor_circuits::{c_mult_mod, c_swap_r, cr_mult_mod, gen_rot, Layout};
use crate::steps::hadamard;

/// Opções do Shor que, no C++, vinham de variáveis de ambiente.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShorOptions {
    /// `SHOR_EMULATE=1`: a multiplicação modular controlada de cada rodada
    /// (`CMultMod` + `CSwapR` + `CRMultMod`) vira uma permutação dos índices do
    /// estado (`apply_mod_mult`), em vez das centenas de portas do circuito. Deixa
    /// de ser um circuito; o circuito continua sendo o padrão e o oráculo dos testes.
    pub emulate_mult: bool,
    /// `SHOR_AQFT=N`: grau da QFT aproximada; `0` é a QFT exata. Sem valor, vale o
    /// padrão por largura (`qft_degree_for_width`), como no C++.
    pub qft_degree: Option<u32>,
}

/// O que uma execução do Shor produziu. `shor()` devolve só `factors`, como o C++;
/// o resto existe para diagnóstico e para os testes conferirem a estimação de fase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShorOutcome {
    /// Base `a` usada (coprima com o número, `2 <= a < N`).
    pub base: u64,
    /// Os `2*width` bits medidos, o primeiro medido no bit mais significativo.
    /// A fase estimada é `revert_bits(measured_phase_bits, 2*width) / 2^(2*width)`.
    pub measured_phase_bits: u64,
    /// Os dois fatores, ou vazio se esta tentativa falhou.
    pub factors: Vec<u64>,
}

fn math_error(what: &str) -> HgmError {
    HgmError::InvalidArgument(format!("aritmética modular inválida: {what}"))
}

/// Tenta extrair um fator de `number` a partir de `period` como o C++:
/// `gcd(N, a^(period/2) ± 1)`, o maior dos dois, aceito se estiver em `(1, N)`.
fn factor_from_period(base: i64, number: i64, period: i32) -> Result<Option<[u64; 2]>> {
    let half =
        modular_pow(base, i64::from(period / 2), number).ok_or_else(|| math_error("a^(p/2)"))?;
    let candidate1 = quantum_gcd(number, half + 1);
    let candidate2 = quantum_gcd(number, half - 1);
    let found = candidate1.max(candidate2);
    if found < number && found > 1 {
        Ok(Some([found as u64, (number / found) as u64]))
    } else {
        Ok(None)
    }
}

/// Valida `number`, monta o layout e cria o simulador com `|reg1> = |1>`.
fn prepare(config: &SimConfig, number: u64) -> Result<(Layout, Simulator)> {
    if number < 3 {
        return Err(HgmError::InvalidArgument(format!(
            "o número a fatorar precisa ser pelo menos 3, recebi {number}"
        )));
    }
    let layout = Layout::for_modulus(number);
    if layout.qubits > MAX_QUBITS {
        return Err(HgmError::TooManyQubits {
            qubits: layout.qubits,
            max: MAX_QUBITS,
        });
    }
    let mut simulator = Simulator::with_config(config, layout.qubits)?;
    simulator.set_basis(1usize << (layout.width + 2))?;
    Ok((layout, simulator))
}

/// As `2*width` rodadas de estimação de fase com a base `base`, e a leitura do
/// resultado.
/// Grau da QFT: o pedido (`SHOR_AQFT`; 0 = exata) ou o padrão da largura.
fn effective_qft_degree(options: &ShorOptions, width: usize) -> u32 {
    options
        .qft_degree
        .unwrap_or_else(|| qft_degree_for_width(width as u32))
}

fn run_rounds(
    mut simulator: Simulator,
    layout: Layout,
    number: i64,
    base: i64,
    options: &ShorOptions,
) -> Result<ShorOutcome> {
    let Layout {
        width,
        qubits,
        ctrl: qft_qubit,
        reg1,
        reg2,
        ..
    } = layout;
    let qft_degree = effective_qft_degree(options, width);
    let base_inverse = mul_inv(base, number).ok_or_else(|| math_error("inverso da base"))?;

    let hadamard_phase = hadamard(qubits, qft_qubit, 1);
    let mut measured_phase_bits = 0u64;

    for round in (0..2 * width).rev() {
        let base_pow_mod =
            modular_pow(base, 1i64 << round, number).ok_or_else(|| math_error("a^(2^k)"))?;
        let base_inv_pow_mod = modular_pow(base_inverse, 1i64 << round, number)
            .ok_or_else(|| math_error("a^-(2^k)"))?;

        if options.emulate_mult {
            simulator.run_steps(&[hadamard_phase.as_str()], 1, 1)?;
            simulator.apply_mod_mult(qft_qubit, reg1, width, base_pow_mod, number)?;

            let mut tail = vec![hadamard_phase.clone()];
            if measured_phase_bits != 0 {
                tail.push(gen_rot(
                    qubits,
                    qft_qubit,
                    measured_phase_bits,
                    simulator.gates_mut(),
                ));
            }
            simulator.run_steps(&tail, 1, 1)?;
        } else {
            let gates = simulator.gates_mut();
            let mut steps = vec![hadamard_phase.clone()];
            steps.extend(c_mult_mod(&layout, base_pow_mod, number, qft_degree, gates));
            steps.extend(c_swap_r(qubits, qft_qubit, reg1, reg2, width));
            // Desfazer a soma de `c_mult_mod` exige o inverso: depois da troca
            // reg1 = a*x e reg2 = x, subtrair a^-1 * (a*x) devolve reg2 a 0. Com `a`
            // em vez de a^-1, reg2 saía com x*(1 - a^2) (item 30 de `docs/07`).
            steps.extend(cr_mult_mod(
                &layout,
                base_inv_pow_mod,
                number,
                qft_degree,
                gates,
            ));
            steps.push(hadamard_phase.clone());
            if measured_phase_bits != 0 {
                steps.push(gen_rot(qubits, qft_qubit, measured_phase_bits, gates));
            }
            simulator.run_steps(&steps, 1, 1)?;
        }

        let measured_bit = simulator.measure(qft_qubit)?.bit;
        measured_phase_bits = (measured_phase_bits << 1) | u64::from(measured_bit);
    }

    let factors = factors_from_phase(measured_phase_bits, width, base, number)?;
    Ok(ShorOutcome {
        base: base as u64,
        measured_phase_bits,
        factors,
    })
}

/// Resultado: a fase medida, com os bits na ordem certa, é `k/2^(2*width)` para um
/// `k` que, em fração contínua, dá o período `r` como denominador.
fn factors_from_phase(
    measured_phase_bits: u64,
    width: usize,
    base: i64,
    number: i64,
) -> Result<Vec<u64>> {
    let numerator = revert_bits(measured_phase_bits as i32, 2 * width as u32);
    if numerator == 0 {
        return Ok(Vec::new());
    }
    let denominator = 1i32 << (2 * width);
    let (_, denominator) = quantum_frac_approx(numerator, denominator, width as u32);

    // O denominador da fração pode ser só um divisor do período: procura o menor
    // múltiplo dele com a^m = 1 (mod N), sem passar de 2^width.
    let cf_denominator = denominator;
    let mut period = denominator;
    let mut multiple = 1i32;
    while cf_denominator > 0 && i64::from(cf_denominator) * i64::from(multiple) < (1i64 << width) {
        let candidate = cf_denominator * multiple;
        if modular_pow(base, i64::from(candidate), number) == Some(1) {
            period = candidate;
            break;
        }
        multiple += 1;
    }
    if i64::from(period) >= number {
        period = cf_denominator;
    }

    if let Some(factors) = factor_from_period(base, number, period)? {
        return Ok(factors.to_vec());
    }
    if cf_denominator != period {
        if let Some(factors) = factor_from_period(base, number, cf_denominator)? {
            return Ok(factors.to_vec());
        }
    }
    Ok(Vec::new())
}

/// Fatora `number` e devolve, além dos fatores, a base sorteada e os bits medidos.
/// `number` precisa ser pelo menos 3 e caber em `(MAX_QUBITS - 3) / 2` bits.
pub fn shor_detailed(
    config: &SimConfig,
    number: u64,
    options: &ShorOptions,
) -> Result<ShorOutcome> {
    let (layout, mut simulator) = prepare(config, number)?;
    let number = number as i64;

    // Base aleatória coprima com `number`, do mesmo gerador das medições (item 18 de
    // `docs/07`: nada de `rand()`).
    let mut base = 0i64;
    while quantum_gcd(number, base) > 1 || base < 2 {
        base = simulator.rng_mut().uniform_int(0, number - 1)?;
    }
    run_rounds(simulator, layout, number, base, options)
}

/// Como [`shor_detailed`], mas com a base escolhida por quem chama (que precisa ser
/// coprima com `number` e estar em `2..number`). Existe para testar cada base
/// sem depender de sementes.
pub fn shor_with_base(
    config: &SimConfig,
    number: u64,
    base: u64,
    options: &ShorOptions,
) -> Result<ShorOutcome> {
    let (layout, simulator) = prepare(config, number)?;
    if base < 2 || base >= number || quantum_gcd(number as i64, base as i64) > 1 {
        return Err(HgmError::InvalidArgument(format!(
            "a base {base} precisa estar em 2..{number} e ser coprima com {number}"
        )));
    }
    run_rounds(simulator, layout, number as i64, base as i64, options)
}

/// Fatora `number`. Devolve os dois fatores, ou um vetor vazio se esta tentativa
/// falhar (é esperado falhar uma fração das vezes; quem chama decide se tenta de
/// novo).
pub fn shor(config: &SimConfig, number: u64, options: &ShorOptions) -> Result<Vec<u64>> {
    Ok(shor_detailed(config, number, options)?.factors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(seed: u32) -> SimConfig {
        SimConfig {
            seed,
            ..SimConfig::default()
        }
    }

    const EMULATED: ShorOptions = ShorOptions {
        emulate_mult: true,
        qft_degree: None,
    };
    /// Circuito com QFT exata: o oráculo da emulação.
    const EXACT_CIRCUIT: ShorOptions = ShorOptions {
        emulate_mult: false,
        qft_degree: Some(0),
    };

    /// Bases coprimas com 15 e a ordem de cada uma módulo 15.
    const BASES_OF_15: [(u64, u32); 7] =
        [(2, 4), (4, 2), (7, 4), (8, 4), (11, 2), (13, 4), (14, 2)];

    #[test]
    fn qft_degree_defaults_to_the_width_rule_unless_requested() {
        // Para 15 (4 bits) o padrão é 4: a rotação de nível 5 é descartada. Uma
        // diferença de 1 no grau muda a QFT, mas só por ~1e-2 nas probabilidades,
        // pequeno demais para uma amostragem de poucas sementes notar.
        let default = ShorOptions::default();
        assert_eq!(effective_qft_degree(&default, 4), 4);
        assert_eq!(effective_qft_degree(&default, 8), 5);
        let exact = ShorOptions {
            qft_degree: Some(0),
            ..default
        };
        assert_eq!(effective_qft_degree(&exact, 4), 0);
    }

    #[test]
    fn rejects_numbers_that_cannot_be_factored() {
        assert!(shor(&config(1), 2, &ShorOptions::default()).is_err());
        // 14 bits cabem (31 qubits); 15 bits dariam 33. Recusa antes de alocar.
        assert!(matches!(
            shor(&config(1), 1 << 14, &ShorOptions::default()),
            Err(HgmError::TooManyQubits { .. })
        ));
    }

    #[test]
    fn rejects_bad_bases() {
        let options = ShorOptions::default();
        for base in [0u64, 1, 3, 5, 6, 9, 10, 12, 15, 16] {
            assert!(
                shor_with_base(&config(1), 15, base, &options).is_err(),
                "base {base}"
            );
        }
    }

    #[test]
    fn emulated_shor_factors_15_for_many_seeds() {
        let mut successes = 0;
        for seed in 1..=40u32 {
            let factors = shor(&config(seed), 15, &EMULATED).unwrap();
            if !factors.is_empty() {
                assert_eq!(factors.len(), 2);
                assert_eq!(factors[0] * factors[1], 15, "semente {seed}: {factors:?}");
                successes += 1;
            }
        }
        // Algoritmo probabilístico: a taxa de sucesso do Shor para 15 é alta
        // (o período é 2 ou 4), bem acima de 1 em 4.
        assert!(successes >= 10, "só {successes}/40 sucessos");
    }

    #[test]
    fn measured_phase_sits_on_an_exact_peak_of_the_period() {
        // Quando o período `r` divide 2^(2*width), a estimação de fase é exata: o
        // numerador medido (bits revertidos) é um múltiplo de 2^(2*width)/r. Cada bit
        // invertido, bit fora de ordem ou rodada com multiplicador errado desloca o
        // pico, então este é o teste que pega erro na orquestração das rodadas.
        for (base, order) in BASES_OF_15 {
            for seed in 1..=6u32 {
                let outcome = shor_with_base(&config(seed), 15, base, &EMULATED).unwrap();
                let numerator = revert_bits(outcome.measured_phase_bits as i32, 8);
                assert_eq!(
                    numerator % (256 / order as i32),
                    0,
                    "base {base} (ordem {order}), semente {seed}: numerador {numerator}"
                );
            }
        }
    }

    #[test]
    fn circuit_and_emulation_measure_the_same_bits_for_every_base() {
        // O circuito (QFT exata, com o inverso modular do item 30) é o oráculo da
        // emulação: mesma base e mesma semente têm que dar os mesmos bits. As bases
        // 2, 7, 8 e 13 não são o próprio inverso módulo 15, então um circuito que
        // desfaça a soma com `a` em vez de `a^-1` falha aqui.
        for (base, _) in BASES_OF_15 {
            let circuit = shor_with_base(&config(3), 15, base, &EXACT_CIRCUIT).unwrap();
            let emulated = shor_with_base(&config(3), 15, base, &EMULATED).unwrap();
            assert_eq!(circuit, emulated, "base {base}");
        }
    }

    #[test]
    fn factors_are_correct_for_an_order_four_base() {
        // 7 tem ordem 4 módulo 15: 7^2 = 4, gcd(15, 4 +- 1) = 5 ou 3.
        let mut found = 0;
        for seed in 1..=10u32 {
            let outcome = shor_with_base(&config(seed), 15, 7, &EMULATED).unwrap();
            if !outcome.factors.is_empty() {
                let mut factors = outcome.factors.clone();
                factors.sort_unstable();
                assert_eq!(factors, [3, 5]);
                found += 1;
            }
        }
        assert!(found >= 4, "só {found}/10 sucessos");
    }
}
