//! Planejamento das passadas da GPU, sem tocar na GPU (por isso é testável em
//! qualquer máquina).
//!
//! Um circuito vira uma sequência de *passadas*. Uma passada **de região** carrega
//! uma região de `2^k` amplitudes na memória de workgroup, aplica várias portas
//! nela e escreve de volta: uma leitura e uma escrita da memória global para muitas
//! portas, que é o que faz o simulador (limitado por banda) andar. É o mesmo desenho
//! do `ApplyValuesC01` do CUDA: a região é formada pelos `c` bits baixos (para
//! leitura coalescida) mais uma **janela** contígua de `w = k - c` bits que começa em
//! `window_start`. Só o **alvo** de uma porta precisa caber na região (como no
//! `GpuExecution01` do CUDA); os bits de controle dentro da região viram máscara local
//! e os de fora viram uma condição sobre o endereço-base da região, avaliada uma vez
//! por workgroup (`outer_mask`/`outer_value`). Quando o alvo não cabe na passada
//! aberta, ela é fechada e começa outra. Sem janela (`w = 0`), uma porta com alvo acima
//! dos bits coalescidos vira uma passada **global** (uma porta por dispatch).
//!
//! Índices são `u32` (WGSL não tem inteiro de 64 bits portátil), então a GPU fica
//! limitada a 32 qubits, como previsto em `docs/09` §4.1.
//!
//! **Estados em blocos.** Se o estado não cabe num binding só (27 qubits na A4500), ele é
//! dividido em `2^(n - L)` blocos de `2^L` amplitudes, cada um num buffer; o bloco é dado
//! pelos bits do índice de `L` para cima. Uma passada de região nunca cruza blocos (a
//! janela fica nos `L` bits baixos e roda uma vez por bloco, com os bits de bloco entrando
//! no endereço-base), e uma porta com alvo nos bits de bloco vira um passo [`Step::Cross`]:
//! um par de blocos por dispatch, um par de amplitudes por thread.

use hgm_core::fused::FusedPhase;
use hgm_core::pt::{Op, Pt};
use hgm_core::Program;

use crate::error::GpuError;

/// Uma porta de 1 qubit com controles, no formato que a GPU consome: o par
/// `(índice com o alvo em 0, índice com o alvo em 1)` só é tocado se
/// `índice & control_mask == control_value`. A matriz é `[m00, m01, m10, m11]` com
/// parte real e imaginária de cada uma (`[re, im, re, im, ...]`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuGate {
    pub target: u32,
    pub control_mask: u32,
    pub control_value: u32,
    pub matrix: [f32; 8],
}

impl GpuGate {
    /// Converte uma porta compilada pelo núcleo. Recusa o que a GPU (índices de 32
    /// bits) não representa.
    pub fn from_pt(pt: &Pt) -> Result<Self, GpuError> {
        if pt.target_bit >= 32 {
            return Err(GpuError::Unsupported(format!(
                "alvo no bit {} (a GPU indexa com 32 bits)",
                pt.target_bit
            )));
        }
        if pt.control_mask >> 32 != 0 || pt.control_value >> 32 != 0 {
            return Err(GpuError::Unsupported(
                "controle acima do bit 31 (a GPU indexa com 32 bits)".to_owned(),
            ));
        }
        let mut matrix = [0.0f32; 8];
        for (i, value) in pt.matrix.iter().enumerate() {
            matrix[2 * i] = value.re;
            matrix[2 * i + 1] = value.im;
        }
        Ok(Self {
            target: pt.target_bit,
            control_mask: pt.control_mask as u32,
            control_value: pt.control_value as u32,
            matrix,
        })
    }
}

/// Um termo de uma porta de fase fundida: onde `(índice & mask) == value`, soma `steps`
/// (em unidades de `2π / 2^16`) ao ângulo da amplitude (`hgm_core::fused`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GpuTerm {
    pub mask: u32,
    pub value: u32,
    pub steps: u32,
}

/// O que um passo do programa faz com o estado: uma porta de 1 qubit ou uma porta de fase
/// fundida (só multiplica cada amplitude por um fator de fase; não mistura pares).
#[derive(Clone, Debug, PartialEq)]
pub enum GpuOp {
    Gate(GpuGate),
    Fused(Vec<GpuTerm>),
}

impl From<GpuGate> for GpuOp {
    fn from(gate: GpuGate) -> Self {
        GpuOp::Gate(gate)
    }
}

impl GpuOp {
    fn from_fused(fused: &FusedPhase) -> Result<Self, GpuError> {
        let mut terms = Vec::with_capacity(fused.terms.len());
        for term in &fused.terms {
            if term.mask >> 32 != 0 || term.value >> 32 != 0 {
                return Err(GpuError::Unsupported(
                    "termo de fase acima do bit 31 (a GPU indexa com 32 bits)".to_owned(),
                ));
            }
            terms.push(GpuTerm {
                mask: term.mask as u32,
                value: term.value as u32,
                steps: term.steps,
            });
        }
        Ok(GpuOp::Fused(terms))
    }
}

/// Converte um programa do núcleo em operações da GPU, com portas de fase fundida. Uma
/// fundida sem termos (todos se cancelaram) é a identidade e some.
pub fn ops_of(program: &Program, repetitions: usize) -> Result<Vec<GpuOp>, GpuError> {
    let mut one = Vec::with_capacity(program.ops.len());
    for op in &program.ops {
        match op {
            Op::Gate(pt) => one.push(GpuOp::Gate(GpuGate::from_pt(pt)?)),
            Op::FusedPhase(fused) if fused.terms.is_empty() => {}
            Op::FusedPhase(fused) => one.push(GpuOp::from_fused(fused)?),
        }
    }
    let mut all = Vec::with_capacity(one.len() * repetitions);
    for _ in 0..repetitions {
        all.extend(one.iter().cloned());
    }
    Ok(all)
}

/// Converte um programa do núcleo em portas da GPU. Programas com portas de fase fundida
/// não cabem aqui (use [`ops_of`]): viram erro.
pub fn gates_of(program: &Program, repetitions: usize) -> Result<Vec<GpuGate>, GpuError> {
    let mut one = Vec::with_capacity(program.ops.len());
    for op in &program.ops {
        match op {
            Op::Gate(pt) => one.push(GpuGate::from_pt(pt)?),
            Op::FusedPhase(_) => {
                return Err(GpuError::Unsupported(
                    "porta de fase fundida (use ops_of)".to_owned(),
                ))
            }
        }
    }
    let mut all = Vec::with_capacity(one.len() * repetitions);
    for _ in 0..repetitions {
        all.extend_from_slice(&one);
    }
    Ok(all)
}

/// Uma passada de região: `2^(n-k)` workgroups, cada um com uma região de `2^k`
/// amplitudes.
#[derive(Clone, Debug, PartialEq)]
pub struct RegionPass {
    /// `c`: bits baixos contíguos da região (leitura coalescida).
    pub coalesced_bits: u32,
    /// `w`: largura da janela de bits mais altos.
    pub window_bits: u32,
    /// `s`: primeiro bit da janela (`s >= c`).
    pub window_start: u32,
    /// Operações já reindexadas para a região (bit `b` global vira `b` se `b < c`, senão
    /// `b - s + c`).
    pub ops: Vec<RegionOp>,
}

/// Operação dentro de uma passada de região.
#[derive(Clone, Debug, PartialEq)]
pub enum RegionOp {
    Gate(RegionGate),
    Fused(Vec<RegionTerm>),
}

/// Termo de fase dentro de uma região: a parte local da máscara (índices locais, por
/// amplitude) e a de fora (índices globais, contra o endereço-base da região).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionTerm {
    pub local_mask: u32,
    pub local_value: u32,
    pub outer_mask: u32,
    pub outer_value: u32,
    pub steps: u32,
}

/// Porta dentro de uma passada de região. Os controles que caem dentro da região estão
/// em `control_mask`/`control_value` (índices locais, comparados por par de amplitudes);
/// os de fora estão em `outer_mask`/`outer_value` (índices **globais**, comparados com o
/// endereço-base da região: a porta inteira é pulada nas regiões que não casam).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RegionGate {
    pub target: u32,
    pub control_mask: u32,
    pub control_value: u32,
    pub outer_mask: u32,
    pub outer_value: u32,
    pub matrix: [f32; 8],
}

impl RegionPass {
    /// `k = c + w`: log2 do tamanho da região.
    pub fn region_bits(&self) -> u32 {
        self.coalesced_bits + self.window_bits
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Region(RegionPass),
    Global(GpuGate),
    /// Porta com o alvo nos bits que escolhem o bloco: os dois ramos do alvo estão em
    /// blocos diferentes.
    Cross(GpuGate),
}

/// Forma da região depois de ajustar aos limites: nunca maior que o estado.
pub fn region_shape(qubits: u32, coalesced_bits: u32, region_bits: u32) -> (u32, u32) {
    let k = region_bits.min(qubits);
    let c = coalesced_bits.min(k);
    (c, k - c)
}

fn to_local(bit: u32, c: u32, s: u32) -> u32 {
    if bit < c {
        bit
    } else {
        bit - s + c
    }
}

fn remap_mask(mask: u32, c: u32, s: u32) -> u32 {
    let mut out = 0u32;
    for bit in 0..32 {
        if mask >> bit & 1 != 0 {
            out |= 1u32 << to_local(bit, c, s);
        }
    }
    out
}

/// Posição do alvo se ele está acima dos `c` bits coalescidos (só esses precisam caber
/// na janela); `None` se está entre os bits baixos, que entram em qualquer passada.
fn upper_span(op: &GpuOp, c: u32) -> Option<(u32, u32)> {
    match op {
        GpuOp::Gate(gate) => (gate.target >= c).then_some((gate.target, gate.target)),
        // Uma porta de fase não tem alvo: cabe em qualquer passada (como no C++, onde a
        // fundida vale `target_bit = 0`).
        GpuOp::Fused(_) => None,
    }
}

struct Open {
    ops: Vec<GpuOp>,
    span: Option<(u32, u32)>,
}

fn close(open: Open, local_bits: u32, c: u32, w: u32, steps: &mut Vec<Step>) {
    if open.ops.is_empty() {
        return;
    }
    // Janela: começa no menor bit usado, mas recua se passaria do fim do bloco.
    // `local_bits >= c + w` garante `local_bits - w >= c`.
    let start = match open.span {
        None => c,
        Some((low, _)) => low.min(local_bits - w),
    };
    // Bits dentro da região: os `c` baixos e a janela `[start, start + w)`.
    let mut inside = if c >= 32 { u32::MAX } else { (1u32 << c) - 1 };
    for bit in start..start + w {
        inside |= 1u32 << bit;
    }
    let ops = open
        .ops
        .iter()
        .map(|op| match op {
            GpuOp::Gate(g) => {
                let local_mask = g.control_mask & inside;
                let outer_mask = g.control_mask & !inside;
                RegionOp::Gate(RegionGate {
                    target: to_local(g.target, c, start),
                    control_mask: remap_mask(local_mask, c, start),
                    control_value: remap_mask(g.control_value & local_mask, c, start),
                    outer_mask,
                    outer_value: g.control_value & outer_mask,
                    matrix: g.matrix,
                })
            }
            GpuOp::Fused(terms) => RegionOp::Fused(
                terms
                    .iter()
                    .map(|t| {
                        let local_mask = t.mask & inside;
                        let outer_mask = t.mask & !inside;
                        RegionTerm {
                            local_mask: remap_mask(local_mask, c, start),
                            local_value: remap_mask(t.value & local_mask, c, start),
                            outer_mask,
                            outer_value: t.value & outer_mask,
                            steps: t.steps,
                        }
                    })
                    .collect(),
            ),
        })
        .collect();
    steps.push(Step::Region(RegionPass {
        coalesced_bits: c,
        window_bits: w,
        window_start: start,
        ops,
    }));
}

/// Agrupa `gates`, na ordem dada, em passadas (ver [`plan_ops`]).
pub fn plan(gates: &[GpuGate], qubits: u32, coalesced_bits: u32, region_bits: u32) -> Vec<Step> {
    let ops: Vec<GpuOp> = gates.iter().copied().map(GpuOp::Gate).collect();
    plan_ops(&ops, qubits, coalesced_bits, region_bits)
}

/// Agrupa `ops`, na ordem dada, em passadas, com o estado num bloco só. A ordem das
/// operações é preservada (não há reordenação por comutação aqui; ela é feita antes, por
/// `schedule_for_batching`).
pub fn plan_ops(ops: &[GpuOp], qubits: u32, coalesced_bits: u32, region_bits: u32) -> Vec<Step> {
    plan_ops_chunked(ops, qubits, qubits, coalesced_bits, region_bits)
}

/// Como [`plan_ops`], com o estado dividido em blocos de `2^local_bits` amplitudes (ver o
/// cabeçalho do módulo). `local_bits == qubits` é o estado num bloco só.
pub fn plan_ops_chunked(
    ops: &[GpuOp],
    qubits: u32,
    local_bits: u32,
    coalesced_bits: u32,
    region_bits: u32,
) -> Vec<Step> {
    let local_bits = local_bits.min(qubits);
    let (c, w) = region_shape(local_bits, coalesced_bits, region_bits);
    let mut steps = Vec::new();
    let mut open = Open {
        ops: Vec::new(),
        span: None,
    };

    for op in ops {
        if let GpuOp::Gate(gate) = op {
            if gate.target >= local_bits {
                let finished = std::mem::replace(
                    &mut open,
                    Open {
                        ops: Vec::new(),
                        span: None,
                    },
                );
                close(finished, local_bits, c, w, &mut steps);
                steps.push(Step::Cross(*gate));
                continue;
            }
        }
        let span = upper_span(op, c);
        if let Some((low, high)) = span {
            if high - low + 1 > w {
                // Sem janela (`w = 0`) o alvo não cabe nem sozinho. Só porta com alvo cai
                // aqui (a de fase não tem `span`).
                let GpuOp::Gate(gate) = op else {
                    continue;
                };
                let finished = std::mem::replace(
                    &mut open,
                    Open {
                        ops: Vec::new(),
                        span: None,
                    },
                );
                close(finished, local_bits, c, w, &mut steps);
                steps.push(Step::Global(*gate));
                continue;
            }
        }

        let merged = match (open.span, span) {
            (Some((a, b)), Some((low, high))) => Some((a.min(low), b.max(high))),
            (Some(current), None) => Some(current),
            (None, other) => other,
        };
        let fits = merged.is_none_or(|(low, high)| high - low < w);
        if !fits {
            let finished = std::mem::replace(
                &mut open,
                Open {
                    ops: Vec::new(),
                    span: None,
                },
            );
            close(finished, local_bits, c, w, &mut steps);
            open.span = span;
        } else {
            open.span = merged;
        }
        open.ops.push(op.clone());
    }
    close(open, local_bits, c, w, &mut steps);
    steps
}

/// Modelo em CPU do que os kernels fazem, com a mesma aritmética de índices do WGSL.
/// Serve para testar o planejador sem GPU e como segunda opinião do que a GPU deve
/// dar (a menos de FMA).
pub mod model {
    use super::{GpuGate, GpuOp, GpuTerm, RegionOp, RegionPass, Step};
    use hgm_core::fused::phase_table;
    use hgm_core::C32;

    fn apply_pair(a: &mut C32, b: &mut C32, m: &[f32; 8]) {
        let m00 = C32::new(m[0], m[1]);
        let m01 = C32::new(m[2], m[3]);
        let m10 = C32::new(m[4], m[5]);
        let m11 = C32::new(m[6], m[7]);
        let (x, y) = (*a, *b);
        *a = m00 * x + m01 * y;
        *b = m10 * x + m11 * y;
    }

    /// `index_with_zero_at(p, t)`: insere um bit 0 na posição `t` de `p`.
    fn insert_zero(p: usize, t: u32) -> usize {
        ((p >> t) << (t + 1)) | (p & ((1usize << t) - 1))
    }

    /// Kernel global: uma porta direto no estado.
    pub fn apply_global(state: &mut [C32], gate: &GpuGate) {
        for p in 0..state.len() / 2 {
            let lo = insert_zero(p, gate.target);
            if lo as u32 & gate.control_mask != gate.control_value {
                continue;
            }
            let hi = lo | (1usize << gate.target);
            let (head, tail) = state.split_at_mut(hi);
            apply_pair(&mut head[lo], &mut tail[0], &gate.matrix);
        }
    }

    /// Porta de fase fundida direto no estado: `a[i] *= e^{i·2π·soma/2^16}` com a soma dos
    /// passos dos termos que casam (módulo `2^16`).
    pub fn apply_fused(state: &mut [C32], terms: &[GpuTerm]) {
        let table = phase_table();
        for (index, amplitude) in state.iter_mut().enumerate() {
            let mut steps = 0u32;
            for term in terms {
                if index as u32 & term.mask == term.value {
                    steps = steps.wrapping_add(term.steps);
                }
            }
            *amplitude *= table[(steps & 0xFFFF) as usize];
        }
    }

    /// Uma operação direto no estado (referência para os testes).
    pub fn apply_op(state: &mut [C32], op: &GpuOp) {
        match op {
            GpuOp::Gate(gate) => apply_global(state, gate),
            GpuOp::Fused(terms) => apply_fused(state, terms),
        }
    }

    /// Kernel de região: a conta de índices de `region_kernel` em `shaders.rs`.
    pub fn apply_region(state: &mut [C32], qubits: u32, pass: &RegionPass) {
        let (c, w, s) = (pass.coalesced_bits, pass.window_bits, pass.window_start);
        let k = c + w;
        let regions = 1usize << (qubits - k);
        let gap = s - c;
        let low_mask = (1usize << c) - 1;
        let mut tile = vec![C32::new(0.0, 0.0); 1 << k];

        for region in 0..regions {
            let rl = region & ((1usize << gap) - 1);
            let rh = region >> gap;
            let base = (rl << c) | (rh << (s + w));
            let global = |j: usize| base | (j & low_mask) | ((j >> c) << s);

            for (j, slot) in tile.iter_mut().enumerate() {
                *slot = state[global(j)];
            }
            for op in &pass.ops {
                match op {
                    RegionOp::Gate(gate) => {
                        if base as u32 & gate.outer_mask != gate.outer_value {
                            continue;
                        }
                        apply_global(
                            &mut tile,
                            &GpuGate {
                                target: gate.target,
                                control_mask: gate.control_mask,
                                control_value: gate.control_value,
                                matrix: gate.matrix,
                            },
                        );
                    }
                    RegionOp::Fused(terms) => {
                        let table = phase_table();
                        for (j, slot) in tile.iter_mut().enumerate() {
                            let mut steps = 0u32;
                            for term in terms {
                                if base as u32 & term.outer_mask == term.outer_value
                                    && j as u32 & term.local_mask == term.local_value
                                {
                                    steps = steps.wrapping_add(term.steps);
                                }
                            }
                            *slot *= table[(steps & 0xFFFF) as usize];
                        }
                    }
                }
            }
            for (j, value) in tile.iter().enumerate() {
                state[global(j)] = *value;
            }
        }
    }

    pub fn apply_plan(state: &mut [C32], qubits: u32, steps: &[Step]) {
        for step in steps {
            match step {
                Step::Region(pass) => apply_region(state, qubits, pass),
                Step::Global(gate) | Step::Cross(gate) => apply_global(state, gate),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hgm_core::C32;

    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0 >> 33
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }

        fn unit(&mut self) -> f32 {
            (self.next() % 2_000_001) as f32 / 1_000_000.0 - 1.0
        }
    }

    fn rotation(rng: &mut Lcg) -> [f32; 8] {
        // Uma matriz unitária qualquer: rotação de Givens complexa.
        let theta = rng.unit() * 3.0;
        let phi = rng.unit() * 3.0;
        let (s, c) = theta.sin_cos();
        [
            c,
            0.0,
            -s * phi.cos(),
            s * phi.sin(),
            s * phi.cos(),
            s * phi.sin(),
            c,
            0.0,
        ]
    }

    fn random_gates(rng: &mut Lcg, qubits: u32, count: usize, max_controls: u64) -> Vec<GpuGate> {
        (0..count)
            .map(|_| {
                let target = rng.below(u64::from(qubits)) as u32;
                let mut mask = 0u32;
                let mut value = 0u32;
                for _ in 0..rng.below(max_controls + 1) {
                    let bit = rng.below(u64::from(qubits)) as u32;
                    if bit != target {
                        mask |= 1 << bit;
                        if rng.below(2) == 1 {
                            value |= 1 << bit;
                        } else {
                            value &= !(1 << bit);
                        }
                    }
                }
                value &= mask;
                GpuGate {
                    target,
                    control_mask: mask,
                    control_value: value,
                    matrix: rotation(rng),
                }
            })
            .collect()
    }

    fn random_state(rng: &mut Lcg, qubits: u32) -> Vec<C32> {
        (0..1usize << qubits)
            .map(|_| C32::new(rng.unit(), rng.unit()))
            .collect()
    }

    fn max_diff(a: &[C32], b: &[C32]) -> f32 {
        a.iter()
            .zip(b)
            .map(|(x, y)| (x - y).norm())
            .fold(0.0, f32::max)
    }

    #[test]
    fn planned_passes_equal_gate_by_gate_application() {
        // (qubits, coalescida, região): inclui região maior que o estado, janela de 0
        // bits e coalescida 0.
        let shapes = [
            (3, 4, 12),
            (6, 2, 4),
            (8, 0, 3),
            (8, 3, 3),
            (9, 3, 6),
            (10, 4, 7),
            (11, 2, 5),
            (12, 4, 12),
        ];
        for (seed, &(qubits, c, k)) in shapes.iter().enumerate() {
            let mut rng = Lcg(seed as u64 + 1);
            let gates = random_gates(&mut rng, qubits, 60, 3);
            let initial = random_state(&mut rng, qubits);

            let mut expected = initial.clone();
            for gate in &gates {
                model::apply_global(&mut expected, gate);
            }

            let steps = plan(&gates, qubits, c, k);
            let mut got = initial;
            model::apply_plan(&mut got, qubits, &steps);

            let diff = max_diff(&expected, &got);
            assert!(diff < 1e-5, "{qubits}q c={c} k={k}: diferença {diff}");
        }
    }

    #[test]
    fn every_gate_lands_in_exactly_one_pass_in_order() {
        let mut rng = Lcg(99);
        let gates = random_gates(&mut rng, 10, 200, 2);
        let steps = plan(&gates, 10, 3, 7);
        let counted: usize = steps
            .iter()
            .map(|s| match s {
                Step::Region(p) => p.ops.len(),
                Step::Global(_) | Step::Cross(_) => 1,
            })
            .sum();
        assert_eq!(counted, gates.len());
    }

    #[test]
    fn hadamard_layer_takes_few_passes() {
        // H em todos os 26 qubits, região de 12 bits com 4 coalescidos: a janela tem 8
        // bits, então os 22 bits altos cabem em 3 passadas e os 4 baixos vêm de graça
        // na primeira.
        let h = [
            std::f32::consts::FRAC_1_SQRT_2,
            0.0,
            std::f32::consts::FRAC_1_SQRT_2,
            0.0,
            std::f32::consts::FRAC_1_SQRT_2,
            0.0,
            -std::f32::consts::FRAC_1_SQRT_2,
            0.0,
        ];
        let gates: Vec<GpuGate> = (0..26)
            .map(|target| GpuGate {
                target,
                control_mask: 0,
                control_value: 0,
                matrix: h,
            })
            .collect();
        let steps = plan(&gates, 26, 4, 12);
        assert_eq!(steps.len(), 3, "{steps:?}");
        assert!(steps.iter().all(|s| matches!(s, Step::Region(_))));
    }

    #[test]
    fn controls_outside_the_region_become_a_condition_on_the_region_address() {
        let gate = GpuGate {
            target: 5,
            control_mask: 1 << 20 | 1 << 1,
            control_value: 1 << 20,
            matrix: [0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0],
        };
        // Alvo no bit 5, controles nos bits 1 (coalescido, dentro) e 20 (fora da janela).
        let steps = plan(&[gate], 22, 2, 6);
        let [Step::Region(pass)] = steps.as_slice() else {
            panic!("esperava uma passada de região: {steps:?}");
        };
        let RegionOp::Gate(local) = pass.ops[0] else {
            panic!("esperava uma porta: {:?}", pass.ops);
        };
        assert_eq!(local.control_mask, 1 << 1);
        assert_eq!(local.control_value, 0);
        assert_eq!(local.outer_mask, 1 << 20);
        assert_eq!(local.outer_value, 1 << 20);
    }

    #[test]
    fn without_a_window_a_high_target_runs_alone_in_global_memory() {
        let gate = GpuGate {
            target: 5,
            control_mask: 0,
            control_value: 0,
            matrix: [0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0],
        };
        // Região só com bits coalescidos (c = k = 4): o alvo 5 não cabe em lugar nenhum.
        let steps = plan(&[gate], 22, 4, 4);
        assert_eq!(steps, vec![Step::Global(gate)]);
    }

    fn random_terms(rng: &mut Lcg, qubits: u32) -> Vec<GpuTerm> {
        (0..1 + rng.below(6))
            .map(|_| {
                let mut mask = 0u32;
                for _ in 0..rng.below(4) {
                    mask |= 1 << rng.below(u64::from(qubits));
                }
                // Valor: um subconjunto qualquer dos bits da máscara.
                let value = mask & (rng.next() as u32);
                GpuTerm {
                    mask,
                    value,
                    steps: rng.below(1 << 16) as u32,
                }
            })
            .collect()
    }

    #[test]
    fn planned_passes_with_fused_phases_equal_op_by_op_application() {
        let shapes = [
            (3, 4, 12),
            (6, 2, 4),
            (8, 3, 3),
            (9, 3, 6),
            (10, 4, 7),
            (12, 4, 12),
        ];
        for (seed, &(qubits, c, k)) in shapes.iter().enumerate() {
            let mut rng = Lcg(seed as u64 + 500);
            // Mistura portas e fundidas na ordem.
            let gates = random_gates(&mut rng, qubits, 30, 3);
            let mut ops = Vec::new();
            for gate in gates {
                ops.push(GpuOp::Gate(gate));
                if rng.below(2) == 1 {
                    ops.push(GpuOp::Fused(random_terms(&mut rng, qubits)));
                }
            }
            let initial = random_state(&mut rng, qubits);

            let mut expected = initial.clone();
            for op in &ops {
                model::apply_op(&mut expected, op);
            }
            let steps = plan_ops(&ops, qubits, c, k);
            let mut got = initial;
            model::apply_plan(&mut got, qubits, &steps);

            let diff = max_diff(&expected, &got);
            assert!(diff < 1e-5, "{qubits}q c={c} k={k}: diferença {diff}");
        }
    }

    #[test]
    fn a_fused_phase_fits_any_pass_and_splits_its_mask_into_local_and_outer() {
        // Região de 6 bits (c = 2, janela de 4 a partir do bit 2), estado de 22 qubits.
        let fused = GpuOp::Fused(vec![GpuTerm {
            mask: 1 << 1 | 1 << 3 | 1 << 20,
            value: 1 << 3 | 1 << 20,
            steps: 1 << 14,
        }]);
        let steps = plan_ops(&[fused], 22, 2, 6);
        let [Step::Region(pass)] = steps.as_slice() else {
            panic!("esperava uma passada de região: {steps:?}");
        };
        let RegionOp::Fused(terms) = &pass.ops[0] else {
            panic!("esperava uma fundida");
        };
        assert_eq!(pass.window_start, 2); // sem alvo, a janela fica logo acima dos coalescidos
        assert_eq!(terms[0].local_mask, 1 << 1 | 1 << 3); // bit 3: local 3 - 2 + 2
        assert_eq!(terms[0].local_value, 1 << 3);
        assert_eq!(terms[0].outer_mask, 1 << 20);
        assert_eq!(terms[0].outer_value, 1 << 20);
    }

    #[test]
    fn chunked_plans_keep_windows_in_the_local_bits_and_equal_op_by_op_application() {
        // (qubits, bits locais, coalescida, região): blocos de 2 a 16.
        let shapes = [
            (8, 7, 2, 4),
            (10, 8, 3, 6),
            (10, 6, 2, 5),
            (12, 8, 4, 7),
            (12, 9, 0, 4),
            (11, 7, 3, 3),
        ];
        for (seed, &(qubits, local, c, k)) in shapes.iter().enumerate() {
            let mut rng = Lcg(seed as u64 + 900);
            let gates = random_gates(&mut rng, qubits, 60, 3);
            let mut ops = Vec::new();
            for gate in gates {
                ops.push(GpuOp::Gate(gate));
                if rng.below(3) == 0 {
                    ops.push(GpuOp::Fused(random_terms(&mut rng, qubits)));
                }
            }
            let initial = random_state(&mut rng, qubits);

            let mut expected = initial.clone();
            for op in &ops {
                model::apply_op(&mut expected, op);
            }
            let steps = plan_ops_chunked(&ops, qubits, local, c, k);
            let mut crossed = 0;
            for step in &steps {
                match step {
                    Step::Region(pass) => {
                        assert!(
                            pass.window_start + pass.window_bits <= local,
                            "{qubits}q bloco {local}: janela {}+{} passa do bloco",
                            pass.window_start,
                            pass.window_bits
                        );
                        assert!(pass.region_bits() <= local);
                    }
                    Step::Global(gate) => assert!(gate.target < local),
                    Step::Cross(gate) => {
                        assert!(gate.target >= local);
                        crossed += 1;
                    }
                }
            }
            let targets_in_chunk_bits = ops
                .iter()
                .filter(|op| matches!(op, GpuOp::Gate(g) if g.target >= local))
                .count();
            assert_eq!(crossed, targets_in_chunk_bits);

            let mut got = initial;
            model::apply_plan(&mut got, qubits, &steps);
            let diff = max_diff(&expected, &got);
            assert!(
                diff < 1e-5,
                "{qubits}q bloco {local} c={c} k={k}: diferença {diff}"
            );
        }
    }

    #[test]
    fn region_never_exceeds_the_state() {
        assert_eq!(region_shape(3, 4, 12), (3, 0));
        assert_eq!(region_shape(20, 4, 12), (4, 8));
        assert_eq!(region_shape(20, 14, 12), (12, 0));
    }
}
