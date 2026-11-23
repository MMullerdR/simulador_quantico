//! Despacho de AVX2 em tempo de execução para o backend paralelo.
//!
//! O C++ marca os laços de CPU com `target_clones("avx2", "default")` e o ligador escolhe a
//! versão pela CPU. O `rustc` compila para o x86-64 básico (SSE2), a menos que se peça
//! `target-cpu=native`, o que amarraria o binário a uma máquina. Aqui o mesmo código
//! (`parallel::apply_batch`, com os núcleos expandidos no lugar por `#[inline(always)]`) é
//! compilado duas vezes, uma delas com `avx2` habilitado, e a versão certa é escolhida uma
//! vez por execução.
//!
//! O resultado **não muda**: só se habilita `avx2` (sem `fma`), e o Rust nunca funde
//! multiplicação e soma por conta própria, então cada conta é a mesma, na mesma ordem,
//! com os mesmos arredondamentos que na versão básica (a igualdade exata entre o serial e
//! o paralelo continua valendo; há teste para isso).

use hgm_core::pt::Op;

use crate::shared::RegionView;

/// Aplica um lote de portas a uma região.
pub type BatchApplier = fn(&mut RegionView<'_>, &[Op]);

/// A versão AVX2 se a CPU a tem, senão a básica.
pub fn batch_applier() -> BatchApplier {
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("avx2") {
            return apply_batch_avx2;
        }
    }
    crate::parallel::apply_batch
}

#[cfg(target_arch = "x86_64")]
fn apply_batch_avx2(view: &mut RegionView<'_>, batch: &[Op]) {
    // SAFETY: `batch_applier` só devolve esta função depois de `is_x86_feature_detected!
    // ("avx2")` dar verdadeiro, então a CPU executa as instruções AVX2.
    unsafe { apply_batch_avx2_inner(view, batch) }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn apply_batch_avx2_inner(view: &mut RegionView<'_>, batch: &[Op]) {
    crate::parallel::apply_batch(view, batch);
}
