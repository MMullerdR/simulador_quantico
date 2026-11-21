//! O ÚNICO módulo de `hgm-cpu` com `unsafe`.
//!
//! O backend paralelo divide o vetor de estado em *regiões*: dado um conjunto de
//! bits `mask`, duas regiões diferem no valor dos bits **fora** de `mask` (o
//! `id`), e uma operação aplicada dentro de uma região só lê e escreve índices
//! cujos bits fora de `mask` valem `id`. Regiões com `id` diferentes são, portanto,
//! conjuntos de índices disjuntos e podem ser processadas por threads diferentes
//! ao mesmo tempo, sem sincronização.
//!
//! O compilador não consegue ver essa disjunção (os índices de uma região são
//! espalhados pelo vetor, não um pedaço contíguo), então este módulo a garante por
//! construção:
//!
//! - [`par_for_each_region`] recebe o vetor por `&mut` (exclusividade total durante
//!   a chamada) e entrega a cada tarefa um [`RegionView`] de uma região **distinta**
//!   (o `id` vem de espalhar os bits de um contador injetivo pelas posições fora da
//!   máscara);
//! - toda leitura/escrita por um [`RegionView`] é **checada em tempo de execução**
//!   contra a sua região (`índice & !mask == id`) e contra o tamanho do vetor, então
//!   mesmo um erro de índice em quem chama vira pânico, não comportamento indefinido;
//! - os métodos de bloco exigem `&mut self`, então dois blocos de um mesmo
//!   `RegionView` não podem coexistir (e os de `block_pair_mut` são checados como
//!   não sobrepostos).

use std::marker::PhantomData;

use hgm_core::C32;
use rayon::prelude::*;

/// Visão exclusiva de uma região do vetor de estado.
pub struct RegionView<'a> {
    ptr: *mut C32,
    len: usize,
    mask: u64,
    id: u64,
    _borrow: PhantomData<&'a mut [C32]>,
}

// SAFETY: um `RegionView` só acessa índices da sua região; regiões entregues
// simultaneamente por `par_for_each_region` são disjuntas (ver o módulo). Mover a
// visão para outra thread não cria acesso concorrente ao mesmo índice.
unsafe impl Send for RegionView<'_> {}

impl RegionView<'_> {
    /// Bits que a região enumera.
    pub fn mask(&self) -> u64 {
        self.mask
    }

    /// Valor dos bits fora da máscara (identifica a região).
    pub fn id(&self) -> u64 {
        self.id
    }

    #[inline]
    fn check_index(&self, index: usize) {
        assert!(
            index < self.len && (index as u64 & !self.mask) == self.id,
            "índice {index} fora da região (mask {:#x}, id {:#x}) ou do vetor",
            self.mask,
            self.id
        );
    }

    #[inline]
    pub fn get(&self, index: usize) -> C32 {
        self.check_index(index);
        // SAFETY: `index < len` e pertence à região desta visão (checado acima);
        // nenhuma outra visão ativa toca índices desta região.
        unsafe { *self.ptr.add(index) }
    }

    #[inline]
    pub fn set(&mut self, index: usize, value: C32) {
        self.check_index(index);
        // SAFETY: idem `get`; `&mut self` impede outra referência a esta visão.
        unsafe { *self.ptr.add(index) = value }
    }

    /// Valida `[base, base + len)` como um bloco contíguo inteiro da região: tamanho
    /// potência de 2, alinhado, com todos os bits baixos do bloco dentro da máscara
    /// (então todo índice do bloco tem o mesmo `id`) e dentro do vetor.
    #[inline]
    fn check_block(&self, base: usize, len: usize) {
        assert!(
            len.is_power_of_two()
                && base.is_multiple_of(len)
                && base + len <= self.len
                && ((len - 1) as u64 & !self.mask) == 0
                && (base as u64 & !self.mask) == self.id,
            "bloco [{base}, {base}+{len}) não pertence à região (mask {:#x}, id {:#x})",
            self.mask,
            self.id
        );
    }

    /// Um bloco contíguo da região (as `len` amplitudes que só variam nos bits
    /// baixos, todos dentro da máscara).
    #[inline]
    pub fn block_mut(&mut self, base: usize, len: usize) -> &mut [C32] {
        self.check_block(base, len);
        // SAFETY: o bloco está dentro da região (checado) e do vetor; `&mut self`
        // garante que não há outro bloco desta visão vivo.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.add(base), len) }
    }

    /// Dois blocos contíguos disjuntos da região, ao mesmo tempo.
    #[inline]
    pub fn block_pair_mut(
        &mut self,
        a_base: usize,
        b_base: usize,
        len: usize,
    ) -> (&mut [C32], &mut [C32]) {
        self.check_block(a_base, len);
        self.check_block(b_base, len);
        assert!(
            a_base + len <= b_base || b_base + len <= a_base,
            "blocos sobrepostos"
        );
        // SAFETY: ambos dentro da região e do vetor, não sobrepostos (checado);
        // `&mut self` impede outros blocos desta visão.
        unsafe {
            (
                std::slice::from_raw_parts_mut(self.ptr.add(a_base), len),
                std::slice::from_raw_parts_mut(self.ptr.add(b_base), len),
            )
        }
    }
}

/// Aponta os bits de `counter` para as posições de `0..qubits` que NÃO estão em
/// `mask` (em ordem crescente). Injetivo em `counter < 2^(qubits - popcount(mask))`.
pub fn region_id(counter: u64, mask: u64, qubits: u32) -> u64 {
    let mut id = 0u64;
    let mut source_bit = 0;
    for position in 0..qubits {
        if (mask >> position) & 1 == 0 {
            id |= ((counter >> source_bit) & 1) << position;
            source_bit += 1;
        }
    }
    id
}

/// Roda `work` em paralelo (no pool do rayon corrente) uma vez para cada região do
/// vetor `state` definida por `mask`: `2^(qubits - popcount(mask))` regiões, cada
/// uma com `2^popcount(mask)` amplitudes. `state.len()` precisa ser `2^qubits`.
pub fn par_for_each_region<F>(state: &mut [C32], qubits: u32, mask: u64, work: F)
where
    F: Fn(&mut RegionView<'_>) + Sync + Send,
{
    assert_eq!(
        state.len(),
        1usize << qubits,
        "vetor de estado com tamanho errado"
    );
    assert!(
        mask < (1u64 << qubits) || qubits >= 64,
        "máscara fora dos qubits"
    );

    let regions = 1u64 << (qubits - mask.count_ones());
    let ptr = SendPtr(state.as_mut_ptr());
    let len = state.len();

    (0..regions).into_par_iter().for_each(|counter| {
        // `ptr.get()` (e não `ptr.0`): o método captura o `SendPtr` inteiro, que é
        // `Send`/`Sync`; o campo sozinho (`*mut`) não seria.
        let mut view = RegionView {
            ptr: ptr.get(),
            len,
            mask,
            id: region_id(counter, mask, qubits),
            _borrow: PhantomData,
        };
        work(&mut view);
    });
}

#[derive(Clone, Copy)]
struct SendPtr(*mut C32);

impl SendPtr {
    fn get(self) -> *mut C32 {
        self.0
    }
}

// SAFETY: o ponteiro só é usado por `RegionView`s de regiões disjuntas, criados
// dentro de `par_for_each_region`, enquanto o `&mut [C32]` original está emprestado.
unsafe impl Send for SendPtr {}
unsafe impl Sync for SendPtr {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_ids_are_distinct_and_outside_the_mask() {
        let (qubits, mask) = (6u32, 0b010_101u64);
        let regions = 1u64 << (qubits - mask.count_ones());
        let mut seen = std::collections::BTreeSet::new();
        for counter in 0..regions {
            let id = region_id(counter, mask, qubits);
            assert_eq!(id & mask, 0);
            assert!(seen.insert(id), "id repetido");
        }
        assert_eq!(seen.len() as u64, regions);
    }

    #[test]
    fn every_index_is_visited_exactly_once() {
        let qubits = 8u32;
        for mask in [0b0000_0011u64, 0b1000_0001, 0b0101_0101, 0b1111_1111, 0] {
            let mut state = vec![C32::new(0.0, 0.0); 1 << qubits];
            par_for_each_region(&mut state, qubits, mask, |view| {
                // Percorre todos os índices da região: sub enumera os subconjuntos da máscara.
                let mut sub = 0u64;
                loop {
                    let index = (view.id() | sub) as usize;
                    let current = view.get(index);
                    view.set(index, C32::new(current.re + 1.0, 0.0));
                    sub = sub.wrapping_sub(view.mask()) & view.mask();
                    if sub == 0 {
                        break;
                    }
                }
            });
            assert!(state.iter().all(|a| a.re == 1.0), "máscara {mask:#b}");
        }
    }

    #[test]
    #[should_panic(expected = "fora da região")]
    fn access_outside_the_region_panics() {
        let mut state = vec![C32::new(0.0, 0.0); 1 << 4];
        par_for_each_region(&mut state, 4, 0b0011, |view| {
            // Índice 0b1100 tem bits fora da máscara; vale `id` só em algumas regiões.
            let outside = (view.id() ^ 0b0100) as usize;
            let _ = view.get(outside);
        });
    }

    #[test]
    fn blocks_are_validated() {
        let mut state = vec![C32::new(0.0, 0.0); 1 << 4];
        par_for_each_region(&mut state, 4, 0b0111, |view| {
            let base = view.id() as usize;
            let (a, b) = view.block_pair_mut(base, base + 4, 4);
            a[0] = C32::new(1.0, 0.0);
            b[3] = C32::new(2.0, 0.0);
        });
        assert_eq!(state[0].re, 1.0);
        assert_eq!(state[7].re, 2.0);
    }
}
