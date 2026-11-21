//! Núcleos de CPU sobre pares de blocos contíguos de amplitudes
//! (`dense_run`, `diag_run`, `sec_run` de `src/core/dgm_par_exec.cpp`).
//!
//! `a[j]` e `b[j]` são os dois lados `(pos0, pos1)` de cada par. Escritos como laços
//! simples sobre fatias, sem `unsafe`, para o compilador vetorizar. Nenhum usa
//! multiplicação-soma fundida: a ordem das contas é a do C++ (`m0r*ar - m0i*ai +
//! m1r*br - m1i*bi`, da esquerda para a direita).

use hgm_core::C32;

/// Matriz densa: `a' = m0*a + m1*b`, `b' = m2*a + m3*b`.
pub fn dense(a: &mut [C32], b: &mut [C32], m: &[C32; 4]) {
    let [m0, m1, m2, m3] = *m;
    for (x, y) in a.iter_mut().zip(b.iter_mut()) {
        let (ar, ai, br, bi) = (x.re, x.im, y.re, y.im);
        x.re = m0.re * ar - m0.im * ai + m1.re * br - m1.im * bi;
        x.im = m0.re * ai + m0.im * ar + m1.re * bi + m1.im * br;
        y.re = m2.re * ar - m2.im * ai + m3.re * br - m3.im * bi;
        y.im = m2.re * ai + m2.im * ar + m3.re * bi + m3.im * br;
    }
}

/// Diagonal principal: `a *= m0`, `b *= m3`. Com `m0 == 1` (Z, R1..R3) só toca em `b`.
pub fn diagonal(a: &mut [C32], b: &mut [C32], m: &[C32; 4]) {
    let (p0, p3) = (m[0], m[3]);
    if p0.re == 1.0 && p0.im == 0.0 {
        for y in b.iter_mut() {
            let (br, bi) = (y.re, y.im);
            y.re = p3.re * br - p3.im * bi;
            y.im = p3.re * bi + p3.im * br;
        }
    } else {
        for (x, y) in a.iter_mut().zip(b.iter_mut()) {
            let (ar, ai, br, bi) = (x.re, x.im, y.re, y.im);
            x.re = p0.re * ar - p0.im * ai;
            x.im = p0.re * ai + p0.im * ar;
            y.re = p3.re * br - p3.im * bi;
            y.im = p3.re * bi + p3.im * br;
        }
    }
}

/// Anti-diagonal: `a' = m1*b`, `b' = m2*a`. Com `m1 == m2 == 1` (X) é só uma troca.
pub fn anti_diagonal(a: &mut [C32], b: &mut [C32], m: &[C32; 4]) {
    let (q1, q2) = (m[1], m[2]);
    if q1.re == 1.0 && q1.im == 0.0 && q2.re == 1.0 && q2.im == 0.0 {
        a.swap_with_slice(b);
    } else {
        for (x, y) in a.iter_mut().zip(b.iter_mut()) {
            let (ar, ai, br, bi) = (x.re, x.im, y.re, y.im);
            x.re = q1.re * br - q1.im * bi;
            x.im = q1.re * bi + q1.im * br;
            y.re = q2.re * ar - q2.im * ai;
            y.im = q2.re * ai + q2.im * ar;
        }
    }
}

/// Assinatura comum dos núcleos.
pub type PairKernel = fn(&mut [C32], &mut [C32], &[C32; 4]);

/// Aplica `kernel` a todos os pares de um bloco em que o alvo está nos bits baixos:
/// o bloco alterna trechos de `tmask` amplitudes do lado 0 e do lado 1.
pub fn pairs_in_block(block: &mut [C32], tmask: usize, m: &[C32; 4], kernel: PairKernel) {
    for chunk in block.chunks_exact_mut(2 * tmask) {
        let (a, b) = chunk.split_at_mut(tmask);
        kernel(a, b, m);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(re: f32, im: f32) -> C32 {
        C32::new(re, im)
    }

    #[test]
    fn dense_matches_the_complex_formula() {
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let m = [c(h, 0.0), c(h, 0.0), c(h, 0.0), c(-h, 0.0)];
        let (mut a, mut b) = (
            vec![c(1.0, 0.0), c(0.0, 1.0)],
            vec![c(0.0, 0.0), c(1.0, 0.0)],
        );
        dense(&mut a, &mut b, &m);
        assert!((a[0].re - h).abs() < 1e-7 && (b[0].re - h).abs() < 1e-7);
        assert!((a[1].re - h).abs() < 1e-7 && (a[1].im - h).abs() < 1e-7);
        assert!((b[1].re + h).abs() < 1e-7 && (b[1].im - h).abs() < 1e-7);
    }

    #[test]
    fn x_is_a_swap_and_phase_only_touches_b() {
        let x = [c(0.0, 0.0), c(1.0, 0.0), c(1.0, 0.0), c(0.0, 0.0)];
        let (mut a, mut b) = (vec![c(1.0, 0.0)], vec![c(2.0, 0.0)]);
        anti_diagonal(&mut a, &mut b, &x);
        assert_eq!((a[0].re, b[0].re), (2.0, 1.0));

        let z = [c(1.0, 0.0), c(0.0, 0.0), c(0.0, 0.0), c(-1.0, 0.0)];
        let (mut a, mut b) = (vec![c(3.0, 0.0)], vec![c(5.0, 0.0)]);
        diagonal(&mut a, &mut b, &z);
        assert_eq!((a[0].re, b[0].re), (3.0, -5.0));
    }

    #[test]
    fn pairs_in_block_walks_alternating_runs() {
        let x = [c(0.0, 0.0), c(1.0, 0.0), c(1.0, 0.0), c(0.0, 0.0)];
        let mut block: Vec<C32> = (0..8).map(|i| c(i as f32, 0.0)).collect();
        pairs_in_block(&mut block, 2, &x, anti_diagonal);
        let got: Vec<f32> = block.iter().map(|a| a.re).collect();
        assert_eq!(got, [2.0, 3.0, 0.0, 1.0, 6.0, 7.0, 4.0, 5.0]);
    }
}
