//! Kernels WGSL. O texto é gerado com os parâmetros que são constantes de compilação
//! (tamanho do workgroup e da região), para o compilador do driver poder desenrolar
//! os laços (WGSL não tem `#pragma unroll`).
//!
//! Convenção do estado: `array<vec2<f32>>`, parte real em `.x` e imaginária em `.y`,
//! mesmo layout de `Complex<f32>`; o índice é o do `hgm-core` (o qubit `q` do circuito
//! é o bit `n-1-q`, mas os kernels só conhecem *bits*).

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
    pad0: u32,
    pad1: u32,
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

var<workgroup> tile: array<vec2<f32>, {tile}>;

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

    let size = 1u << k;
    for (var j = lid; j < size; j += {wg}u) {{
        let g = origin | (j & low_mask) | ((j >> c) << s);
        tile[j] = state[g];
    }}
    workgroupBarrier();

    let pairs = size >> 1u;
    for (var o = 0u; o < params.op_count; o += 1u) {{
        let op = ops[params.op_start + o];
        // Controles fora da região: a condição é a mesma para o workgroup inteiro (só
        // depende do endereço-base), então pular a porta aqui é uniforme.
        if ((origin & op.outer.x) == op.outer.y) {{
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
/// em `partial[2*wid]` e `partial[2*wid+1]`.
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
        partial[2u * wid.x] = zero_sums[0];
        partial[2u * wid.x + 1u] = one_sums[0];
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
