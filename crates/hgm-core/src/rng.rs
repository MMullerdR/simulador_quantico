//! `std::mt19937` do C++ (Mersenne Twister de 32 bits), com a mesma semeadura e a
//! mesma regra de `std::uniform_real_distribution<double>` do libstdc++.
//!
//! Existe para que uma medição com a mesma semente dê o mesmo resultado que o
//! C++ (`g_rng` em `src/core/common.cpp`) e para os sorteios serem iguais em
//! qualquer sistema, sem depender de `rand()` (ver `docs/07` item 18).

use crate::error::{HgmError, Result};

const N: usize = 624;
const M: usize = 397;

#[derive(Clone, Debug)]
pub struct Mt19937 {
    state: [u32; N],
    index: usize,
}

impl Mt19937 {
    /// `std::mt19937::seed(value)` (a semente entra módulo `2^32`).
    pub fn new(seed: u32) -> Self {
        let mut state = [0u32; N];
        state[0] = seed;
        for i in 1..N {
            let previous = state[i - 1];
            state[i] = 1_812_433_253u32
                .wrapping_mul(previous ^ (previous >> 30))
                .wrapping_add(i as u32);
        }
        Self { state, index: N }
    }

    fn twist(&mut self) {
        for i in 0..N {
            let y = (self.state[i] & 0x8000_0000) | (self.state[(i + 1) % N] & 0x7fff_ffff);
            let mut next = self.state[(i + M) % N] ^ (y >> 1);
            if y & 1 != 0 {
                next ^= 0x9908_b0df;
            }
            self.state[i] = next;
        }
        self.index = 0;
    }

    /// Próximo número de 32 bits (`operator()`).
    pub fn next_u32(&mut self) -> u32 {
        if self.index >= N {
            self.twist();
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }

    /// `std::uniform_real_distribution<double>(0, 1)(gen)` do libstdc++:
    /// `generate_canonical<double, 53>` usa dois sorteios de 32 bits,
    /// `(x1 + x2 * 2^32) / 2^64`, limitado ao maior `double` abaixo de 1.
    pub fn uniform_f64(&mut self) -> f64 {
        let mut sum = 0.0f64;
        let mut scale = 1.0f64;
        for _ in 0..2 {
            sum += f64::from(self.next_u32()) * scale;
            scale *= 4_294_967_296.0;
        }
        let value = sum / scale;
        if value >= 1.0 {
            1.0 - f64::EPSILON / 2.0
        } else {
            value
        }
    }

    /// `std::uniform_int_distribution<long>(low, high)(gen)` do libstdc++ 11 ou
    /// mais novo: para faixas menores que `2^32 - 1` usa a redução de Lemire (um
    /// produto de 64 bits por sorteio, `>> 32`, com rejeição só quando o resto
    /// baixo é pequeno); a faixa cheia de 32 bits usa o sorteio direto.
    ///
    /// Faixas maiores que `2^32 - 1` viram erro: o libstdc++ combina vários
    /// sorteios nesse caso e este porte não o reproduz (o Shor só sorteia bases
    /// menores que o número a fatorar, que cabe em 32 qubits).
    pub fn uniform_int(&mut self, low: i64, high: i64) -> Result<i64> {
        if low > high {
            return Err(HgmError::InvalidArgument(format!(
                "faixa vazia: [{low}, {high}]"
            )));
        }
        let range = i128::from(high) - i128::from(low);
        if range > i128::from(u32::MAX) {
            return Err(HgmError::InvalidArgument(format!(
                "faixa [{low}, {high}] maior que 2^32 - 1 não é suportada"
            )));
        }

        let offset = if range == i128::from(u32::MAX) {
            u64::from(self.next_u32())
        } else {
            // `range + 1 <= u32::MAX`, então cabe em `u32`.
            let span = (range + 1) as u32;
            let mut product = u64::from(self.next_u32()) * u64::from(span);
            let mut low_word = product as u32;
            if low_word < span {
                let threshold = span.wrapping_neg() % span;
                while low_word < threshold {
                    product = u64::from(self.next_u32()) * u64::from(span);
                    low_word = product as u32;
                }
            }
            product >> 32
        };
        Ok(low + offset as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_standard_10000th_value() {
        // Valor exigido pelo padrão C++ ([rand.predef]) para mt19937 default
        // (semente 5489): o 10000º número gerado é 4123659995.
        let mut rng = Mt19937::new(5489);
        let mut last = 0;
        for _ in 0..10_000 {
            last = rng.next_u32();
        }
        assert_eq!(last, 4_123_659_995);
    }

    #[test]
    fn first_values_match_the_reference_sequence() {
        // Primeiras saídas conhecidas do mt19937 com semente 5489.
        let mut rng = Mt19937::new(5489);
        let first: Vec<u32> = (0..5).map(|_| rng.next_u32()).collect();
        assert_eq!(
            first,
            [
                3_499_211_612,
                581_869_302,
                3_890_346_734,
                3_586_334_585,
                545_404_204
            ]
        );
    }

    #[test]
    fn checksum_of_ten_thousand_values_matches() {
        // Um valor só (o 10000º) pode coincidir por acaso depois de um erro nas
        // constantes de têmpera; a soma de verificação de todos não.
        // Valor calculado com uma implementação independente em Python, que também
        // reproduz as saídas acima e o 10000º do padrão.
        let mut rng = Mt19937::new(5489);
        let mut checksum = 0u32;
        for _ in 0..10_000 {
            checksum = checksum.wrapping_mul(31).wrapping_add(rng.next_u32());
        }
        assert_eq!(checksum, 3_225_766_033);
    }

    #[test]
    fn uniform_int_matches_libstdcxx() {
        // Saídas de `std::uniform_int_distribution<long>(0, hi)` com `std::mt19937`
        // semeado, geradas no qflex (g++ 13.3.0, libstdc++ de Ubuntu 24.04).
        let cases: [(u32, i64, [i64; 12]); 5] = [
            (1, 14, [6, 14, 10, 13, 0, 1, 4, 14, 2, 3, 1, 5]),
            (42, 56, [21, 45, 54, 10, 41, 44, 34, 34, 8, 25, 8, 5]),
            (
                5489,
                2862,
                [
                    2332, 387, 2593, 2390, 363, 2773, 2614, 632, 1810, 882, 279, 1566,
                ],
            ),
            (
                1,
                1_000_000_000,
                [
                    417_021_998,
                    997_184_809,
                    932_557_362,
                    114_381,
                    128_124_447,
                    999_040_516,
                    146_755_892,
                    236_088_976,
                    92_338_595,
                    396_580_726,
                    186_260_211,
                    387_910_740,
                ],
            ),
            (42, 14, [5, 11, 14, 2, 10, 11, 8, 8, 2, 6, 2, 1]),
        ];
        for (seed, high, expected) in cases {
            let mut rng = Mt19937::new(seed);
            let got: Vec<i64> = (0..12).map(|_| rng.uniform_int(0, high).unwrap()).collect();
            assert_eq!(got, expected, "semente {seed}, faixa [0, {high}]");
        }
    }

    #[test]
    fn uniform_int_handles_offsets_and_bad_ranges() {
        let mut rng = Mt19937::new(1);
        // [10, 24] é [0, 14] deslocado: mesma sequência mais 10.
        assert_eq!(rng.uniform_int(10, 24).unwrap(), 16);
        assert_eq!(rng.uniform_int(-3, -3).unwrap(), -3);
        assert!(rng.uniform_int(5, 4).is_err());
        assert!(rng.uniform_int(0, i64::from(u32::MAX) + 1).is_err());
        // Faixa cheia de 32 bits: devolve o sorteio cru.
        let mut raw = Mt19937::new(7);
        let mut cooked = Mt19937::new(7);
        assert_eq!(
            cooked.uniform_int(0, i64::from(u32::MAX)).unwrap(),
            i64::from(raw.next_u32())
        );
    }

    #[test]
    fn uniform_is_in_the_unit_interval() {
        let mut rng = Mt19937::new(1);
        for _ in 0..1000 {
            let value = rng.uniform_f64();
            assert!((0.0..1.0).contains(&value));
        }
    }
}
