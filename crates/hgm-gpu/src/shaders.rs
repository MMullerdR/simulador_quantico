//! Kernels WGSL. O texto é gerado com os parâmetros que são constantes de compilação
//! (tamanho do workgroup e da região), para o compilador do driver poder desenrolar
//! os laços (WGSL não tem `#pragma unroll`).
//!
//! Convenção do estado: `array<vec2<f32>>`, parte real em `.x` e imaginária em `.y`,
//! mesmo layout de `Complex<f32>`; o índice é o do `hgm-core` (o qubit `q` do circuito
//! é o bit `n-1-q`, mas os kernels só conhecem *bits*).
//!
//! O estado pode estar dividido em blocos (um buffer por bloco de `2^L` amplitudes, ver
//! `plan.rs`): cada kernel roda sobre **um** bloco (`params.n` = `L`, os índices são locais
//! ao bloco) e `params.chunk` diz qual bloco é, para as contas que dependem do índice
//! global (controles e termos de fase nos bits de bloco, `modmult`). Só os kernels de par
//! (`cross_kernel`, `collapse_pair_kernel`) enxergam dois blocos.

/// Cabeçalho comum: parâmetros da passada e produto complexo.
fn prelude() -> &'static str {
    r#"
struct Params {
    op_start: u32,
    op_count: u32,
    c: u32,
    w: u32,
    s: u32,
    n: u32,
    fused: u32,   // 1 se a passada tem porta de fase fundida
    chunk: u32,   // número do bloco (medição: posição do bloco nos parciais)
}

struct Op {
    head: vec4<u32>,   // x = alvo, y = máscara de controle, z = valor de controle
    outer: vec4<u32>,  // x = máscara, y = valor dos controles fora da região (endereço-base)
    m0: vec4<f32>,     // m00.re, m00.im, m01.re, m01.im
    m1: vec4<f32>,     // m10.re, m10.im, m11.re, m11.im
}

fn cmul(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x);
}
"#
}

/// Passada de região: carrega `2^k` amplitudes (os `c` bits baixos + uma janela de
/// `w` bits a partir de `s`) na memória de workgroup, aplica as portas e escreve de
/// volta. `tile_bits` é o tamanho máximo da região (`2^tile_bits` amplitudes);
/// `workgroup_size` threads cuidam de uma região.
pub fn region_kernel(tile_bits: u32, workgroup_size: u32) -> String {
    format!(
        r#"{prelude}
@group(0) @binding(0) var<storage, read_write> state: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read> ops: array<Op>;
@group(0) @binding(2) var<uniform> params: Params;
@group(0) @binding(3) var<storage, read> terms: array<vec4<u32>>;
@group(0) @binding(4) var<storage, read> phase: array<vec2<f32>>;

var<workgroup> tile: array<vec2<f32>, {tile}>;
// Termos de fase que valem nesta região (os de fora já casaram com o endereço-base), com
// a máscara e o valor locais empacotados em 16 bits cada: (mask | value << 16, passos).
var<workgroup> active_terms: array<vec2<u32>, 64>;
var<workgroup> active_count: u32;

@compute @workgroup_size({wg})
fn main(
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(num_workgroups) nwg: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
) {{
    let region = wid.x + wid.y * nwg.x;
    let c = params.c;
    let w = params.w;
    let s = params.s;
    let k = c + w;
    let low_mask = (1u << c) - 1u;
    let gap = s - c;
    let rl = region & ((1u << gap) - 1u);
    let rh = region >> gap;
    var origin = rl << c;
    if (s + w < 32u) {{
        origin = origin | (rh << (s + w));
    }}
    // Endereço-base global: os bits do bloco entram acima dos `n` bits locais.
    let gorigin = origin | (params.chunk << params.n);

    let size = 1u << k;
    for (var j = lid; j < size; j += {wg}u) {{
        let g = origin | (j & low_mask) | ((j >> c) << s);
        tile[j] = state[g];
    }}
    workgroupBarrier();

    let pairs = size >> 1u;
    for (var o = 0u; o < params.op_count; o += 1u) {{
        let op = ops[params.op_start + o];
        // `params.fused == 1` marca passada com portas de fase: a condição é uniforme, então
        // a barreira dentro dela é permitida.
        if (params.fused == 1u) {{
            if (lid == 0u && op.head.w == 1u) {{
                var n = 0u;
                for (var t = 0u; t < op.head.y; t += 1u) {{
                    let a = terms[2u * (op.head.x + t)];
                    if ((gorigin & a.z) == a.w) {{
                        active_terms[n] = vec2<u32>(a.x | (a.y << 16u), terms[2u * (op.head.x + t) + 1u].x);
                        n += 1u;
                    }}
                }}
                active_count = n;
            }}
            workgroupBarrier();
        }}
        if (op.head.w == 1u) {{
            // Porta de fase fundida: cada amplitude ganha o fator de fase da soma dos
            // termos que casam. Dois vec4 por termo: [máscara local, valor local, máscara
            // de fora, valor de fora] e [passos, 0, 0, 0]. A parte de fora compara com o
            // endereço-base da região, como nos controles das portas.
            // Quatro amplitudes por leitura de termo (como o `ApplyValuesC01` do CUDA): a leitura
            // do termo na memória de workgroup é o que mais pesa, e assim ela é dividida por 4.
            // A soma de inteiros não depende da ordem dos termos.
            let count = active_count;
            for (var j0 = lid; j0 < size; j0 += 4u * {wg}u) {{
                let j1 = j0 + {wg}u;
                let j2 = j1 + {wg}u;
                let j3 = j2 + {wg}u;
                var s0 = 0u;
                var s1 = 0u;
                var s2 = 0u;
                var s3 = 0u;
                for (var t = 0u; t < count; t += 1u) {{
                    let a = active_terms[t];
                    let m = a.x & 65535u;
                    let v = a.x >> 16u;
                    s0 += select(0u, a.y, (j0 & m) == v);
                    s1 += select(0u, a.y, (j1 & m) == v);
                    s2 += select(0u, a.y, (j2 & m) == v);
                    s3 += select(0u, a.y, (j3 & m) == v);
                }}
                tile[j0] = cmul(tile[j0], phase[s0 & 65535u]);
                if (j1 < size) {{ tile[j1] = cmul(tile[j1], phase[s1 & 65535u]); }}
                if (j2 < size) {{ tile[j2] = cmul(tile[j2], phase[s2 & 65535u]); }}
                if (j3 < size) {{ tile[j3] = cmul(tile[j3], phase[s3 & 65535u]); }}
            }}
        }} else if ((gorigin & op.outer.x) == op.outer.y) {{
            // Controles fora da região: a condição é a mesma para o workgroup inteiro (só
            // depende do endereço-base), então pular a porta aqui é uniforme.
            let t = op.head.x;
            let cm = op.head.y;
            let cv = op.head.z;
            let m00 = op.m0.xy;
            let m01 = op.m0.zw;
            let m10 = op.m1.xy;
            let m11 = op.m1.zw;
            let tmask = (1u << t) - 1u;
            for (var p = lid; p < pairs; p += {wg}u) {{
                let lo = ((p >> t) << (t + 1u)) | (p & tmask);
                if ((lo & cm) == cv) {{
                    let hi = lo | (1u << t);
                    let a = tile[lo];
                    let b = tile[hi];
                    tile[lo] = cmul(m00, a) + cmul(m01, b);
                    tile[hi] = cmul(m10, a) + cmul(m11, b);
                }}
            }}
        }}
        workgroupBarrier();
    }}

    for (var j = lid; j < size; j += {wg}u) {{
        let g = origin | (j & low_mask) | ((j >> c) << s);
        state[g] = tile[j];
    }}
}}
"#,
        prelude = prelude(),
        tile = 1u32 << tile_bits,
        wg = workgroup_size,
    )
}

/// Passada global: uma porta, um par de amplitudes por thread, direto na memória
/// global. É o que o kernel de região evita; serve de comparação e de plano B para
/// portas que não cabem numa região.
pub fn global_kernel(workgroup_size: u32) -> String {
    format!(
        r#"{prelude}
@group(0) @binding(0) var<storage, read_write> state: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read> ops: array<Op>;
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size({wg})
fn main(
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(num_workgroups) nwg: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
) {{
    let p = (wid.x + wid.y * nwg.x) * {wg}u + lid;
    if (p >= (1u << (params.n - 1u))) {{
        return;
    }}
    let op = ops[params.op_start];
    let t = op.head.x;
    let lo = ((p >> t) << (t + 1u)) | (p & ((1u << t) - 1u));
    if ((lo & op.head.y) != op.head.z) {{
        return;
    }}
    let hi = lo | (1u << t);
    let a = state[lo];
    let b = state[hi];
    state[lo] = cmul(op.m0.xy, a) + cmul(op.m0.zw, b);
    state[hi] = cmul(op.m1.xy, a) + cmul(op.m1.zw, b);
}}
"#,
        prelude = prelude(),
        wg = workgroup_size,
    )
}

/// Soma de `|a|^2` dos índices com o bit `params.s` em 0 e em 1, em `f32` por
/// workgroup (soma em árvore). O fechamento (soma dos parciais) é em `f64` no host:
/// WGSL não tem `f64` portátil (`docs/09` §4.1). Cada workgroup escreve 2 parciais
/// em `partial[2*slot]` e `partial[2*slot+1]`, com `slot = params.chunk * nwg.x + wid`
/// (cada bloco do estado tem o seu trecho).
pub fn measure_kernel(workgroup_size: u32) -> String {
    format!(
        r#"{prelude}
@group(0) @binding(0) var<storage, read> state: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read_write> partial: array<f32>;
@group(0) @binding(2) var<uniform> params: Params;

var<workgroup> zero_sums: array<f32, {wg}>;
var<workgroup> one_sums: array<f32, {wg}>;

@compute @workgroup_size({wg})
fn main(
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(num_workgroups) nwg: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
) {{
    let total = 1u << params.n;
    let stride = nwg.x * {wg}u;
    let bit = params.s;
    var zero = 0.0;
    var one = 0.0;
    for (var i = wid.x * {wg}u + lid; i < total; i += stride) {{
        let a = state[i];
        let e = a.x * a.x + a.y * a.y;
        if (((i >> bit) & 1u) == 1u) {{
            one += e;
        }} else {{
            zero += e;
        }}
    }}
    zero_sums[lid] = zero;
    one_sums[lid] = one;
    workgroupBarrier();

    for (var step = {wg}u / 2u; step > 0u; step = step / 2u) {{
        if (lid < step) {{
            zero_sums[lid] += zero_sums[lid + step];
            one_sums[lid] += one_sums[lid + step];
        }}
        workgroupBarrier();
    }}
    if (lid == 0u) {{
        let slot = params.chunk * nwg.x + wid.x;
        partial[2u * slot] = zero_sums[0];
        partial[2u * slot + 1u] = one_sums[0];
    }}
}}
"#,
        prelude = prelude(),
        wg = workgroup_size,
    )
}

/// Cópia `src -> dst` de `vec4<f32>`: mede a banda de memória (P8 de `docs/11`), que
/// é o teto de um simulador limitado por banda.
pub fn copy_kernel(workgroup_size: u32) -> String {
    format!(
        r#"
@group(0) @binding(0) var<storage, read> src: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> dst: array<vec4<f32>>;

@compute @workgroup_size({wg})
fn main(
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(num_workgroups) nwg: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
) {{
    let i = (wid.x + wid.y * nwg.x) * {wg}u + lid;
    if (i < arrayLength(&src)) {{
        dst[i] = src[i];
    }}
}}
"#,
        wg = workgroup_size,
    )
}

/// Colapso de um qubit, um thread por par `(pos, pos | 1<<shift)` (`pos` tem o bit em 0).
///
/// `mode == 0` é a medição do C++ (`DGM::measure`): o ramo sorteado, escalado por
/// `scale = 1/norma`, vai para a posição `pos` (a do bit em 0) e a posição com o bit em 1
/// é zerada; ou seja, depois de medir 1 o qubit passa a valer 0, igual ao host
/// (`StateVector::measure_qubit`). `mode == 1` é `colapse`: o ramo `bit` fica no lugar,
/// escalado, e o outro é zerado.
pub fn collapse_kernel(workgroup_size: u32) -> String {
    format!(
        r#"
struct Edit {{
    shift: u32,
    bit: u32,
    mode: u32,
    n: u32,
    scale: f32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}}

@group(0) @binding(0) var<storage, read_write> state: array<vec2<f32>>;
@group(0) @binding(1) var<uniform> edit: Edit;

@compute @workgroup_size({wg})
fn main(
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(num_workgroups) nwg: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
) {{
    let pair = (wid.x + wid.y * nwg.x) * {wg}u + lid;
    if (pair >= (1u << (edit.n - 1u))) {{
        return;
    }}
    let low = (1u << edit.shift) - 1u;
    let pos = (pair << 1u) - (pair & low);
    let other = pos | (1u << edit.shift);
    let a = state[pos];
    let b = state[other];
    let zero = vec2<f32>(0.0, 0.0);
    if (edit.mode == 0u) {{
        var kept = a;
        if (edit.bit == 1u) {{
            kept = b;
        }}
        state[pos] = kept * edit.scale;
        state[other] = zero;
    }} else if (edit.bit == 1u) {{
        state[pos] = zero;
        state[other] = b * edit.scale;
    }} else {{
        state[pos] = a * edit.scale;
        state[other] = zero;
    }}
}}
"#,
        wg = workgroup_size,
    )
}

/// Multiplicação modular controlada como permutação, em forma de "gather": cada thread
/// escreve uma posição de destino `d` e lê a origem `s`, como o laço do host
/// (`hgm_core::modmult`). Precisa de um buffer de destino separado, porque a origem de um
/// thread é o destino de outro.
pub fn mod_mult_kernel(workgroup_size: u32) -> String {
    format!(
        r#"
struct ModMult {{
    n: u32,
    ctrl_shift: u32,
    reg_shift: u32,
    width: u32,
    modulus: u32,
    dst_chunk: u32,
    src_chunk: u32,
    pad0: u32,
}}

@group(0) @binding(0) var<storage, read> src: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read_write> dst: array<vec2<f32>>;
@group(0) @binding(2) var<storage, read> gather: array<u32>;
@group(0) @binding(3) var<uniform> mm: ModMult;

@compute @workgroup_size({wg})
fn main(
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(num_workgroups) nwg: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
) {{
    let d = (wid.x + wid.y * nwg.x) * {wg}u + lid;
    if (d >= (1u << mm.n)) {{
        return;
    }}
    // Índices globais: o bloco de destino entra acima dos `n` bits locais. A origem pode
    // estar noutro bloco (registrador que cruza o limite do bloco); cada dispatch trata
    // as posições cuja origem está no bloco `src_chunk`.
    let dg = d | (mm.dst_chunk << mm.n);
    let x = (dg >> mm.reg_shift) & ((1u << mm.width) - 1u);
    var sg = dg;
    if (((dg >> mm.ctrl_shift) & 1u) == 1u && x < mm.modulus) {{
        sg = dg ^ ((x ^ gather[x]) << mm.reg_shift);
    }}
    if ((sg >> mm.n) == mm.src_chunk) {{
        dst[d] = src[sg & ((1u << mm.n) - 1u)];
    }}
}}
"#,
        wg = workgroup_size,
    )
}

/// Porta com o alvo nos bits de bloco: `lo_state` é o bloco com o alvo em 0 e `hi_state`
/// o com o alvo em 1 (mesma posição local `p` nos dois). Um thread por `p`; os controles
/// locais (`op.head.y`/`.z`) são conferidos aqui, os dos bits de bloco, no host (que
/// pula os pares de blocos que não casam).
pub fn cross_kernel(workgroup_size: u32) -> String {
    format!(
        r#"{prelude}
@group(0) @binding(0) var<storage, read_write> lo_state: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read> ops: array<Op>;
@group(0) @binding(2) var<uniform> params: Params;
@group(0) @binding(3) var<storage, read_write> hi_state: array<vec2<f32>>;

@compute @workgroup_size({wg})
fn main(
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(num_workgroups) nwg: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
) {{
    let p = (wid.x + wid.y * nwg.x) * {wg}u + lid;
    if (p >= (1u << params.n)) {{
        return;
    }}
    let op = ops[params.op_start];
    if ((p & op.head.y) != op.head.z) {{
        return;
    }}
    let a = lo_state[p];
    let b = hi_state[p];
    lo_state[p] = cmul(op.m0.xy, a) + cmul(op.m0.zw, b);
    hi_state[p] = cmul(op.m1.xy, a) + cmul(op.m1.zw, b);
}}
"#,
        prelude = prelude(),
        wg = workgroup_size,
    )
}

/// Colapso de um qubit que está nos bits de bloco: `state_a` é o bloco com o bit em 0 e
/// `state_b` o com o bit em 1 (mesma posição local nos dois). Mesmas regras do
/// [`collapse_kernel`] (`mode` 0 = medição do C++, 1 = colapso no lugar).
pub fn collapse_pair_kernel(workgroup_size: u32) -> String {
    format!(
        r#"
struct Edit {{
    shift: u32,
    bit: u32,
    mode: u32,
    n: u32,
    scale: f32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}}

@group(0) @binding(0) var<storage, read_write> state_a: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read_write> state_b: array<vec2<f32>>;
@group(0) @binding(2) var<uniform> edit: Edit;

@compute @workgroup_size({wg})
fn main(
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(num_workgroups) nwg: vec3<u32>,
    @builtin(local_invocation_index) lid: u32,
) {{
    let p = (wid.x + wid.y * nwg.x) * {wg}u + lid;
    if (p >= (1u << edit.n)) {{
        return;
    }}
    let a = state_a[p];
    let b = state_b[p];
    let zero = vec2<f32>(0.0, 0.0);
    if (edit.mode == 0u) {{
        var kept = a;
        if (edit.bit == 1u) {{
            kept = b;
        }}
        state_a[p] = kept * edit.scale;
        state_b[p] = zero;
    }} else if (edit.bit == 1u) {{
        state_a[p] = zero;
        state_b[p] = b * edit.scale;
    }} else {{
        state_a[p] = a * edit.scale;
        state_b[p] = zero;
    }}
}}
"#,
        wg = workgroup_size,
    )
}
