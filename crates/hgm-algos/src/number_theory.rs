//! Teoria dos números do Shor: porta de `src/algorithms/lib_shor_number_theory.cpp`,
//! `modular_pow` (`src/core/common.cpp`) e `qft_degree_for_width`
//! (`src/algorithms/lib_shor_circuits.cpp`).
//!
//! Os inteiros são `i64` (o `long` do C++ no Linux). Onde o C++ tinha overflow ou
//! divisão por zero (comportamento indefinido), aqui o produto usa `i128` ou a
//! função devolve `Option`.

/// `base^exponent mod modulus` por quadrados sucessivos (como `modular_pow`).
/// `exponent` negativo ou `modulus` zero não têm sentido aqui e devolvem `None`.
pub fn modular_pow(base: i64, exponent: i64, modulus: i64) -> Option<i64> {
    if modulus == 0 || exponent < 0 {
        return None;
    }
    let modulus = i128::from(modulus);
    let mut result: i128 = 1;
    let mut base = i128::from(base) % modulus;
    let mut exponent = exponent;
    while exponent != 0 {
        if exponent & 1 == 1 {
            result = (result * base) % modulus;
        }
        exponent >>= 1;
        base = (base * base) % modulus;
    }
    i64::try_from(result).ok()
}

/// Máximo divisor comum por Euclides, com a mesma sequência de restos do C++.
pub fn quantum_gcd(mut value1: i64, mut value2: i64) -> i64 {
    while value2 != 0 {
        let remainder = value2;
        value2 = value1 % value2;
        value1 = remainder;
    }
    value1
}

/// Inverso multiplicativo de `value` módulo `modulus` (Euclides estendido). `None`
/// quando não existe (o C++ dividia por zero ou devolvia lixo nesse caso).
pub fn mul_inv(mut value: i64, mut modulus: i64) -> Option<i64> {
    let modulus_orig = modulus;
    if modulus == 1 {
        return Some(1);
    }
    let (mut coeff_prev, mut coeff_curr) = (0i64, 1i64);
    while value > 1 {
        if modulus == 0 {
            return None;
        }
        let quotient = value / modulus;
        let remainder = value % modulus;
        value = modulus;
        modulus = remainder;
        let next = coeff_curr - quotient * coeff_prev;
        coeff_curr = coeff_prev;
        coeff_prev = next;
    }
    // O laço só termina com `value == 1` quando há inverso; `value <= 0` é gcd != 1.
    if value != 1 {
        return None;
    }
    if coeff_curr < 0 {
        coeff_curr += modulus_orig;
    }
    Some(coeff_curr)
}

/// Inverte a ordem dos `bit_count` bits de `value` (o bit 0 vira o mais
/// significativo); desfaz a ordem em que a QFT semiclássica mede a fase.
pub fn revert_bits(mut value: i32, bit_count: u32) -> i32 {
    let mut reversed = 0i32;
    for _ in 0..bit_count {
        reversed = (reversed << 1) | (value & 1);
        value >>= 1;
    }
    reversed
}

/// Grau da QFT aproximada usado pelo Shor: `ceil(log2(width)) + 2`.
pub fn qft_degree_for_width(width: u32) -> u32 {
    let mut log2_ceil = 0;
    while (1u64 << log2_ceil) < u64::from(width) {
        log2_ceil += 1;
    }
    log2_ceil + 2
}

/// Aproxima `numerator/denominator` pela fração contínua mais simples com
/// denominador < `2^width` (extrai o período candidato da fase medida).
///
/// Reproduz a aritmética mista do C++: `target_ratio` e `residual` são `float`,
/// mas as contas com literais `double` (`+ 0.000005`, `1.0 / residual`) são
/// feitas em `double` e só depois guardadas em `float`; os inteiros dão a volta
/// em vez de estourar. Devolve `(numerador, denominador)`.
// `!(error > tolerance)` (e não `error <= tolerance`) de propósito: com NaN o laço
// do C++ termina, e a negação preserva isso.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn quantum_frac_approx(numerator: i32, denominator: i32, width: u32) -> (i32, i32) {
    let target_ratio: f32 = numerator as f32 / denominator as f32;
    let mut residual: f32 = target_ratio;
    let limit = 1i32 << width;
    let tolerance = 1.0 / f64::from(2 * limit);

    let (mut num_prev2, mut den_prev2) = (0i32, 1i32);
    let (mut num_prev1, mut den_prev1) = (1i32, 0i32);
    // Como no C++, ficam em 0 se a primeira volta já estourar o limite.
    let (mut num_curr, mut den_curr) = (0i32, 0i32);

    loop {
        let term = (f64::from(residual) + 0.000005) as i32;

        residual = (f64::from(residual) - (f64::from(term) - 0.000005)) as f32;
        residual = (1.0 / f64::from(residual)) as f32;

        if term.wrapping_mul(den_prev1).wrapping_add(den_prev2) > limit {
            break;
        }

        num_curr = term.wrapping_mul(num_prev1).wrapping_add(num_prev2);
        den_curr = term.wrapping_mul(den_prev1).wrapping_add(den_prev2);

        num_prev2 = num_prev1;
        den_prev2 = den_prev1;
        num_prev1 = num_curr;
        den_prev1 = den_curr;

        let error = (f64::from(num_curr) / f64::from(den_curr) - f64::from(target_ratio)).abs();
        if !(error > tolerance) {
            break;
        }
    }

    (num_curr, den_curr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modular_pow_small_cases() {
        assert_eq!(modular_pow(2, 10, 15), Some(4));
        assert_eq!(modular_pow(7, 0, 15), Some(1));
        assert_eq!(modular_pow(2, 5, 0), None);
    }

    #[test]
    fn modular_pow_does_not_overflow() {
        // (5e9-1)^2 ~ 2,5e19 estoura i64 no C++; aqui o produto é i128.
        let modulus: i64 = 5_000_000_000;
        let base = modulus - 1;
        let expected = (i128::from(base) * i128::from(base) % i128::from(modulus)) as i64;
        assert_eq!(modular_pow(base, 2, modulus), Some(expected));
    }

    #[test]
    fn inverse_exists_only_when_coprime() {
        assert_eq!(mul_inv(2, 15), Some(8));
        assert_eq!(mul_inv(3, 15), None);
        assert_eq!(mul_inv(5, 1), Some(1));
    }

    #[test]
    fn gcd_and_bit_reversal() {
        assert_eq!(quantum_gcd(12, 18), 6);
        assert_eq!(quantum_gcd(7, 0), 7);
        assert_eq!(revert_bits(1, 3), 4);
        assert_eq!(revert_bits(5, 3), 5);
    }

    #[test]
    fn qft_degree() {
        assert_eq!(qft_degree_for_width(1), 2);
        assert_eq!(qft_degree_for_width(5), 5);
        assert_eq!(qft_degree_for_width(16), 6);
    }
}
