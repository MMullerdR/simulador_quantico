//! Dispositivo, pipelines e execução (wgpu). Tudo o que fala com a GPU fica aqui;
//! o planejamento das passadas está em `plan.rs` e é testável sem GPU.

use std::sync::mpsc;
use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use hgm_core::modmult::ModMultPlan;
use hgm_core::C32;
use wgpu::util::DeviceExt;

use crate::error::{GpuError, Result};
use crate::plan::{plan, GpuGate, RegionGate, Step};
use crate::shaders;

/// Teto da região em adaptadores de software (`DeviceType::Cpu`, como o WARP do Windows).
/// No WARP o custo de partida cresce depressa com o tile: contexto mais primeira execução
/// levam 1,7 s com 4 bits, 2,9 s com 6 e 6,6 s com 8; com 12 bits passaram de 15 minutos
/// (`docs/09` §4.2, `examples/software_probe.rs`). Em GPU de verdade o teto não se aplica.
const SOFTWARE_MAX_REGION_BITS: u32 = 8;

/// Parâmetros de construção.
#[derive(Clone, Copy, Debug)]
pub struct GpuOptions {
    /// Maior região (log2 de amplitudes) que os kernels aceitam. O padrão, 12, usa
    /// 32 KiB de memória de workgroup, como o CUDA do `v1.1.0`. Se o adaptador
    /// oferece menos, cai sozinho (em adaptador de software, para no máximo 8).
    pub max_region_bits: u32,
    /// Threads por workgroup.
    pub workgroup_size: u32,
}

impl Default for GpuOptions {
    fn default() -> Self {
        Self {
            max_region_bits: 12,
            workgroup_size: 256,
        }
    }
}

/// O que o colapso faz com o ramo que sobra.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollapseMode {
    /// Medição do C++: o ramo sorteado vai para a posição do bit em 0.
    Measure,
    /// `colapse` do C++: o ramo fica onde está.
    InPlace,
}

/// Parâmetros de uma execução (ajuste fino, como `gpu_region_bits` e
/// `gpu_coalesced_bits` do C++).
#[derive(Clone, Copy, Debug)]
pub struct RunTuning {
    pub region_bits: u32,
    pub coalesced_bits: u32,
}

impl Default for RunTuning {
    fn default() -> Self {
        Self {
            region_bits: 12,
            coalesced_bits: 4,
        }
    }
}

/// O que uma execução fez e quanto demorou.
#[derive(Clone, Copy, Debug, Default)]
pub struct RunStats {
    pub region_passes: usize,
    pub global_passes: usize,
    /// Tempo medido pela própria GPU entre o começo e o fim dos kernels
    /// (`TIMESTAMP_QUERY`), se o adaptador oferece.
    pub gpu_seconds: Option<f64>,
    /// Tempo de parede de `execute` (planejamento, envio, execução e espera).
    pub wall_seconds: f64,
}

/// Dispatches por comando enviado à GPU.
const SUBMIT_CHUNK: u64 = 512;

struct Launch {
    region: bool,
    /// Deslocamento (dinâmico) da passada no buffer de parâmetros.
    offset: u32,
    groups: (u32, u32),
}

/// Uma sequência de passadas já planejada e carregada na GPU para um estado.
#[derive(Default)]
pub struct PreparedRun {
    bind_group: Option<wgpu::BindGroup>,
    launches: Vec<Launch>,
    _buffers: Vec<wgpu::Buffer>,
}

impl PreparedRun {
    /// Passadas por repetição: `(de região, globais)`.
    pub fn passes(&self) -> (usize, usize) {
        let region = self.launches.iter().filter(|l| l.region).count();
        (region, self.launches.len() - region)
    }
}

/// Estado na GPU: `2^qubits` amplitudes `vec2<f32>`.
pub struct GpuState {
    buffer: wgpu::Buffer,
    /// Buffer de destino da multiplicação modular (troca de lugar com `buffer`); só
    /// existe depois da primeira.
    scratch: Option<wgpu::Buffer>,
    qubits: u32,
}

impl GpuState {
    pub fn qubits(&self) -> u32 {
        self.qubits
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ParamsRaw {
    op_start: u32,
    op_count: u32,
    c: u32,
    w: u32,
    s: u32,
    n: u32,
    pad: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct OpRaw {
    /// alvo, máscara de controle, valor de controle, (livre)
    head: [u32; 4],
    /// máscara e valor dos controles fora da região (endereço-base), 2 livres
    outer: [u32; 4],
    matrix: [f32; 8],
}

impl OpRaw {
    fn global(gate: &GpuGate) -> Self {
        Self {
            head: [gate.target, gate.control_mask, gate.control_value, 0],
            outer: [0; 4],
            matrix: gate.matrix,
        }
    }

    fn region(gate: &RegionGate) -> Self {
        Self {
            head: [gate.target, gate.control_mask, gate.control_value, 0],
            outer: [gate.outer_mask, gate.outer_value, 0, 0],
            matrix: gate.matrix,
        }
    }
}

pub struct GpuContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    info: wgpu::AdapterInfo,
    limits: wgpu::Limits,
    timestamps: bool,
    options: GpuOptions,
    max_region_bits: u32,
    uniform_slot: u64,
    region_pipeline: wgpu::ComputePipeline,
    global_pipeline: wgpu::ComputePipeline,
    measure_pipeline: wgpu::ComputePipeline,
    copy_pipeline: wgpu::ComputePipeline,
    collapse_pipeline: wgpu::ComputePipeline,
    mod_mult_pipeline: wgpu::ComputePipeline,
    layout_ops: wgpu::BindGroupLayout,
    layout_copy: wgpu::BindGroupLayout,
    layout_collapse: wgpu::BindGroupLayout,
    layout_mod_mult: wgpu::BindGroupLayout,
}

fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// Uniform sem deslocamento dinâmico (parâmetros de uma chamada só).
fn plain_uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: true,
            min_binding_size: None,
        },
        count: None,
    }
}

fn floor_log2(value: u64) -> u32 {
    63 - value.max(1).leading_zeros()
}

impl GpuContext {
    /// Abre a GPU de maior desempenho que houver e compila os kernels.
    ///
    /// **Um contexto por processo.** Com vários contextos vivos ao mesmo tempo no
    /// processo (um por thread), a destruição simultânea travou ou derrubou o processo
    /// na RTX A4500 (Vulkan, driver 580): ver `examples/stress.rs`. Um único contexto
    /// compartilhado entre threads (`GpuContext` é `Sync`) não teve o problema.
    pub fn new(options: GpuOptions) -> Result<Self> {
        pollster::block_on(Self::new_async(options))
    }

    async fn new_async(options: GpuOptions) -> Result<Self> {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
                apply_limit_buckets: false,
            })
            .await
            .map_err(|e| GpuError::NoAdapter(e.to_string()))?;

        let info = adapter.get_info();
        let limits = adapter.limits();
        let timestamps = adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        let mut features = wgpu::Features::empty();
        if timestamps {
            features |= wgpu::Features::TIMESTAMP_QUERY;
        }

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("hgm-gpu"),
                required_features: features,
                // O padrão do wgpu é bem abaixo do que a GPU oferece (16 KiB de memória
                // de workgroup, 128 MiB de buffer): pede os limites do adaptador.
                required_limits: limits.clone(),
                ..Default::default()
            })
            .await
            .map_err(|e| GpuError::Device(e.to_string()))?;

        let workgroup_bytes = u64::from(limits.max_compute_workgroup_storage_size);
        let device_region_bits = floor_log2(workgroup_bytes / 8);
        let mut max_region_bits = options.max_region_bits.min(device_region_bits);
        if info.device_type == wgpu::DeviceType::Cpu {
            max_region_bits = max_region_bits.min(SOFTWARE_MAX_REGION_BITS);
        }
        let wg = options.workgroup_size;

        let compile = |label: &str, source: String| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            })
        };
        let region_module = compile("region", shaders::region_kernel(max_region_bits, wg));
        let global_module = compile("global", shaders::global_kernel(wg));
        let measure_module = compile("measure", shaders::measure_kernel(wg));
        let copy_module = compile("copy", shaders::copy_kernel(wg));
        let collapse_module = compile("collapse", shaders::collapse_kernel(wg));
        let mod_mult_module = compile("mod_mult", shaders::mod_mult_kernel(wg));

        // state (rw), ops (ro), parâmetros (uniform com offset dinâmico).
        let layout_ops = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ops"),
            entries: &[
                storage_entry(0, false),
                storage_entry(1, true),
                uniform_entry(2),
            ],
        });
        // medição: state (ro), parciais (rw), parâmetros.
        let layout_measure = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("measure"),
            entries: &[
                storage_entry(0, true),
                storage_entry(1, false),
                uniform_entry(2),
            ],
        });
        let layout_copy = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("copy"),
            entries: &[storage_entry(0, true), storage_entry(1, false)],
        });

        let layout_collapse = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("collapse"),
            entries: &[storage_entry(0, false), plain_uniform_entry(1)],
        });
        let layout_mod_mult = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mod_mult"),
            entries: &[
                storage_entry(0, true),
                storage_entry(1, false),
                storage_entry(2, true),
                plain_uniform_entry(3),
            ],
        });

        let pipeline = |label: &str,
                        module: &wgpu::ShaderModule,
                        layout: &wgpu::BindGroupLayout|
         -> wgpu::ComputePipeline {
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let region_pipeline = pipeline("region", &region_module, &layout_ops);
        let global_pipeline = pipeline("global", &global_module, &layout_ops);
        let measure_pipeline = pipeline("measure", &measure_module, &layout_measure);
        let copy_pipeline = pipeline("copy", &copy_module, &layout_copy);
        let collapse_pipeline = pipeline("collapse", &collapse_module, &layout_collapse);
        let mod_mult_pipeline = pipeline("mod_mult", &mod_mult_module, &layout_mod_mult);

        let uniform_slot = u64::from(limits.min_uniform_buffer_offset_alignment)
            .max(std::mem::size_of::<ParamsRaw>() as u64);

        let context = Self {
            device,
            queue,
            info,
            limits,
            timestamps,
            options,
            max_region_bits,
            uniform_slot,
            region_pipeline,
            global_pipeline,
            measure_pipeline,
            copy_pipeline,
            collapse_pipeline,
            mod_mult_pipeline,
            layout_ops,
            layout_copy,
            layout_collapse,
            layout_mod_mult,
        };
        // Erros de validação de shader/pipeline aparecem aqui, não na primeira execução.
        context.drain_errors()?;
        Ok(context)
    }

    fn drain_errors(&self) -> Result<()> {
        // Os erros de criação de pipeline chegam pelo handler de erros não capturados;
        // uma ida e volta à GPU garante que já chegaram.
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        Ok(())
    }

    pub fn info(&self) -> &wgpu::AdapterInfo {
        &self.info
    }

    pub fn limits(&self) -> &wgpu::Limits {
        &self.limits
    }

    pub fn has_timestamps(&self) -> bool {
        self.timestamps
    }

    /// Maior região (log2) que este dispositivo e estas opções permitem.
    pub fn max_region_bits(&self) -> u32 {
        self.max_region_bits
    }

    pub fn workgroup_size(&self) -> u32 {
        self.options.workgroup_size
    }

    /// Uma linha para relatórios.
    pub fn describe(&self) -> String {
        format!(
            "{} ({:?}, {:?}); região máx. {} bits; buffer máx. {} MiB; binding máx. {} MiB; \
             timestamps: {}",
            self.info.name,
            self.info.backend,
            self.info.device_type,
            self.max_region_bits,
            self.limits.max_buffer_size >> 20,
            self.limits.max_storage_buffer_binding_size >> 20,
            self.timestamps,
        )
    }

    /// Estado de base `|index>`.
    pub fn basis_state(&self, qubits: u32, index: u32) -> Result<GpuState> {
        let state = self.alloc_state(qubits)?;
        let one = [1.0f32, 0.0];
        self.queue.write_buffer(
            &state.buffer,
            u64::from(index) * 8,
            bytemuck::cast_slice(&one),
        );
        Ok(state)
    }

    /// Estado com as amplitudes dadas (`2^qubits` valores).
    pub fn state_from(&self, qubits: u32, amplitudes: &[C32]) -> Result<GpuState> {
        assert_eq!(amplitudes.len(), 1usize << qubits, "tamanho do estado");
        let state = self.alloc_state(qubits)?;
        self.queue
            .write_buffer(&state.buffer, 0, bytemuck::cast_slice(amplitudes));
        Ok(state)
    }

    fn alloc_state(&self, qubits: u32) -> Result<GpuState> {
        if qubits == 0 || qubits > 31 {
            return Err(GpuError::Unsupported(format!(
                "{qubits} qubits (o spike aceita de 1 a 31)"
            )));
        }
        let bytes = 8u64 << qubits;
        let max_binding = self.limits.max_storage_buffer_binding_size;
        if bytes > self.limits.max_buffer_size || bytes > max_binding {
            return Err(GpuError::StateTooLarge {
                qubits,
                bytes,
                max_buffer: self.limits.max_buffer_size,
                max_binding,
            });
        }
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("state"),
            size: bytes,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(GpuState {
            buffer,
            scratch: None,
            qubits,
        })
    }

    /// Dispatch 2D (x, y) para `groups` workgroups, respeitando o limite por dimensão.
    fn dispatch_size(&self, groups: u64) -> (u32, u32) {
        let max_x = 1u64 << floor_log2(u64::from(self.limits.max_compute_workgroups_per_dimension));
        let x = groups.clamp(1, max_x);
        let y = groups.div_ceil(x);
        (x as u32, y as u32)
    }

    /// Planeja `gates` para `state` e deixa tudo pronto na GPU (portas, parâmetros,
    /// grupos de recursos): `run` repete a sequência sem refazer nada disso, como o
    /// `repeat_count` do C++.
    pub fn prepare(
        &self,
        state: &GpuState,
        gates: &[GpuGate],
        tuning: RunTuning,
    ) -> Result<PreparedRun> {
        let region_bits = tuning.region_bits.min(self.max_region_bits);
        let steps = plan(gates, state.qubits, tuning.coalesced_bits, region_bits);

        // Parâmetros e portas de todas as passadas, em dois buffers só.
        let mut ops: Vec<OpRaw> = Vec::new();
        let mut params: Vec<u8> = Vec::new();
        let slot = self.uniform_slot as usize;
        let mut launches: Vec<Launch> = Vec::new();

        for step in &steps {
            let (raw, region, groups) = match step {
                Step::Region(pass) => {
                    let start = ops.len() as u32;
                    ops.extend(pass.gates.iter().map(OpRaw::region));
                    let k = pass.region_bits();
                    (
                        ParamsRaw {
                            op_start: start,
                            op_count: pass.gates.len() as u32,
                            c: pass.coalesced_bits,
                            w: pass.window_bits,
                            s: pass.window_start,
                            n: state.qubits,
                            pad: [0; 2],
                        },
                        true,
                        1u64 << (state.qubits - k),
                    )
                }
                Step::Global(gate) => {
                    let start = ops.len() as u32;
                    ops.push(OpRaw::global(gate));
                    let pairs = 1u64 << (state.qubits - 1);
                    (
                        ParamsRaw {
                            op_start: start,
                            op_count: 1,
                            c: 0,
                            w: 0,
                            s: 0,
                            n: state.qubits,
                            pad: [0; 2],
                        },
                        false,
                        pairs.div_ceil(u64::from(self.options.workgroup_size)),
                    )
                }
            };
            let offset = params.len();
            params.extend_from_slice(bytemuck::bytes_of(&raw));
            params.resize(offset + slot, 0);
            launches.push(Launch {
                region,
                offset: offset as u32,
                groups: self.dispatch_size(groups),
            });
        }
        if ops.is_empty() {
            return Ok(PreparedRun::default());
        }

        let ops_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("ops"),
                contents: bytemuck::cast_slice(&ops),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let params_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("params"),
                contents: &params,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ops"),
            layout: &self.layout_ops,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: state.buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: ops_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &params_buffer,
                        offset: 0,
                        size: wgpu::BufferSize::new(std::mem::size_of::<ParamsRaw>() as u64),
                    }),
                },
            ],
        });
        Ok(PreparedRun {
            bind_group: Some(bind_group),
            launches,
            _buffers: vec![ops_buffer, params_buffer],
        })
    }

    /// Executa a sequência preparada `repetitions` vezes. Os dispatches vão em vários
    /// comandos (no máximo `SUBMIT_CHUNK` cada) para nenhum deles ficar longo o bastante
    /// para o sistema cancelar (TDR do Windows, ~2 s; R2 de `docs/11`).
    pub fn run(&self, prepared: &PreparedRun, repetitions: u32) -> Result<RunStats> {
        let started = Instant::now();
        let Some(bind_group) = &prepared.bind_group else {
            return Ok(RunStats::default());
        };

        let per_rep = prepared.launches.len() as u64;
        let total = per_rep * u64::from(repetitions);
        let mut sent = 0u64;
        let mut chunks: Vec<TimestampQueries> = Vec::new();
        while sent < total {
            let count = (total - sent).min(SUBMIT_CHUNK);
            let timing = self.timestamps.then(|| TimestampQueries::new(&self.device));
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("run") });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("run"),
                    timestamp_writes: timing.as_ref().map(TimestampQueries::writes),
                });
                for index in sent..sent + count {
                    let launch = &prepared.launches[(index % per_rep) as usize];
                    pass.set_pipeline(if launch.region {
                        &self.region_pipeline
                    } else {
                        &self.global_pipeline
                    });
                    pass.set_bind_group(0, bind_group, &[launch.offset]);
                    pass.dispatch_workgroups(launch.groups.0, launch.groups.1, 1);
                }
            }
            if let Some(timing) = timing {
                timing.resolve(&mut encoder);
                chunks.push(timing);
            }
            // Entrega os comandos aos poucos: a GPU começa a trabalhar enquanto o resto
            // ainda está sendo codificado.
            self.queue.submit(Some(encoder.finish()));
            sent += count;
        }
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());

        let gpu_seconds = self.timestamps.then(|| {
            chunks
                .iter()
                .map(|t| t.read(&self.device, &self.queue))
                .sum()
        });
        let (region_per_rep, global_per_rep) = prepared.passes();
        Ok(RunStats {
            region_passes: region_per_rep * repetitions as usize,
            global_passes: global_per_rep * repetitions as usize,
            gpu_seconds,
            wall_seconds: started.elapsed().as_secs_f64(),
        })
    }

    /// Aplica `gates`, na ordem, ao estado: `prepare` + `run(1)`.
    pub fn execute(
        &self,
        state: &GpuState,
        gates: &[GpuGate],
        tuning: RunTuning,
    ) -> Result<RunStats> {
        let started = Instant::now();
        let prepared = self.prepare(state, gates, tuning)?;
        let mut stats = self.run(&prepared, 1)?;
        stats.wall_seconds = started.elapsed().as_secs_f64();
        Ok(stats)
    }

    /// Lê `count` amplitudes a partir de `first`.
    pub fn read_range(&self, state: &GpuState, first: u64, count: u64) -> Result<Vec<C32>> {
        let bytes = count * 8;
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("staging"),
            size: bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_buffer_to_buffer(&state.buffer, first * 8, &staging, 0, bytes);
        self.queue.submit(Some(encoder.finish()));
        self.map_read(&staging)
    }

    /// Lê o estado inteiro para `out` (que já existe e já foi tocado, como o `state` do
    /// C++): evita alocar e faltar páginas de um `Vec` novo de `2^n * 8` bytes.
    pub fn read_into(&self, state: &GpuState, out: &mut [C32]) -> Result<()> {
        assert_eq!(out.len(), 1usize << state.qubits, "tamanho do estado");
        let bytes = (out.len() as u64) * 8;
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("staging"),
            size: bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_buffer_to_buffer(&state.buffer, 0, &staging, 0, bytes);
        self.queue.submit(Some(encoder.finish()));

        let slice = staging.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv()
            .map_err(|e| GpuError::Readback(e.to_string()))?
            .map_err(|e| GpuError::Readback(e.to_string()))?;
        {
            let data = slice
                .get_mapped_range()
                .map_err(|e| GpuError::Readback(e.to_string()))?;
            bytemuck::cast_slice_mut::<C32, u8>(out).copy_from_slice(&data);
        }
        staging.unmap();
        Ok(())
    }

    pub fn read_state(&self, state: &GpuState) -> Result<Vec<C32>> {
        self.read_range(state, 0, 1u64 << state.qubits)
    }

    fn map_read<T: Pod>(&self, buffer: &wgpu::Buffer) -> Result<Vec<T>> {
        let slice = buffer.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv()
            .map_err(|e| GpuError::Readback(e.to_string()))?
            .map_err(|e| GpuError::Readback(e.to_string()))?;
        let data = slice
            .get_mapped_range()
            .map_err(|e| GpuError::Readback(e.to_string()))?;
        let out = bytemuck::pod_collect_to_vec(&data);
        drop(data);
        buffer.unmap();
        Ok(out)
    }

    /// Colapsa o qubit do bit `shift` do índice (ver `shaders::collapse_kernel`): com
    /// `CollapseMode::Measure` o ramo `bit` vai para a posição do bit em 0 (a medição do
    /// C++); com `InPlace` fica onde está (o `colapse`). `scale` é `1/norma`.
    pub fn collapse_bit(
        &self,
        state: &GpuState,
        shift: u32,
        bit: u8,
        scale: f32,
        mode: CollapseMode,
    ) -> Result<()> {
        if shift >= state.qubits {
            return Err(GpuError::Unsupported(format!(
                "bit {shift} fora de um estado de {} qubits",
                state.qubits
            )));
        }
        let params = [
            shift,
            u32::from(bit),
            match mode {
                CollapseMode::Measure => 0,
                CollapseMode::InPlace => 1,
            },
            state.qubits,
            scale.to_bits(),
            0,
            0,
            0,
        ];
        let params_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("collapse params"),
                contents: bytemuck::cast_slice(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("collapse"),
            layout: &self.layout_collapse,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: state.buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params_buffer.as_entire_binding(),
                },
            ],
        });
        let pairs = 1u64 << (state.qubits - 1);
        let groups = self.dispatch_size(pairs.div_ceil(u64::from(self.options.workgroup_size)));
        self.dispatch_once(&self.collapse_pipeline, &bind_group, groups);
        Ok(())
    }

    /// Multiplicação modular controlada (`hgm_core::modmult::plan_mod_mult`): cada
    /// amplitude de destino lê a de origem num buffer de destino separado, e os dois
    /// trocam de lugar. Aloca o segundo buffer (mesmo tamanho do estado) na primeira vez.
    pub fn mod_mult(&self, state: &mut GpuState, plan: &ModMultPlan) -> Result<()> {
        let gather_bytes = (u64::from(plan.modulus) * 4).max(16);
        let gather = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mod_mult gather"),
            size: gather_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue
            .write_buffer(&gather, 0, bytemuck::cast_slice(&plan.gather));
        let params = [
            state.qubits,
            plan.ctrl_shift,
            plan.reg_shift,
            plan.width,
            plan.modulus,
            0,
            0,
            0,
        ];
        let params_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mod_mult params"),
                contents: bytemuck::cast_slice(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        if state.scratch.is_none() {
            state.scratch = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("state scratch"),
                size: 8u64 << state.qubits,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let Some(scratch) = state.scratch.as_ref() else {
            return Err(GpuError::Unsupported("buffer auxiliar".to_owned()));
        };
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mod_mult"),
            layout: &self.layout_mod_mult,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: state.buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: scratch.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: gather.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: params_buffer.as_entire_binding(),
                },
            ],
        });
        let groups = self
            .dispatch_size((1u64 << state.qubits).div_ceil(u64::from(self.options.workgroup_size)));
        self.dispatch_once(&self.mod_mult_pipeline, &bind_group, groups);
        if let Some(mut scratch) = state.scratch.take() {
            std::mem::swap(&mut state.buffer, &mut scratch);
            state.scratch = Some(scratch);
        }
        Ok(())
    }

    /// Um dispatch num comando só (kernels pequenos: colapso e `modmult`).
    fn dispatch_once(
        &self,
        pipeline: &wgpu::ComputePipeline,
        bind_group: &wgpu::BindGroup,
        groups: (u32, u32),
    ) {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.dispatch_workgroups(groups.0, groups.1, 1);
        }
        self.queue.submit(Some(encoder.finish()));
    }

    /// Soma de `|a|^2` com o bit `bit` em 0 e em 1: parciais em `f32` por workgroup
    /// na GPU, fechamento em `f64` aqui.
    pub fn measure_sums(&self, state: &GpuState, bit: u32) -> Result<(f64, f64)> {
        const GROUPS: u32 = 1024;
        let params = ParamsRaw {
            op_start: 0,
            op_count: 0,
            c: 0,
            w: 0,
            s: bit,
            n: state.qubits,
            pad: [0; 2],
        };
        let mut params_bytes = bytemuck::bytes_of(&params).to_vec();
        params_bytes.resize(self.uniform_slot as usize, 0);
        let params_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("measure params"),
                contents: &params_bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let partial_bytes = u64::from(GROUPS) * 2 * 4;
        let partial = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("partials"),
            size: partial_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("partials staging"),
            size: partial_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = self.measure_pipeline.get_bind_group_layout(0);
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("measure"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: state.buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: partial.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &params_buffer,
                        offset: 0,
                        size: wgpu::BufferSize::new(std::mem::size_of::<ParamsRaw>() as u64),
                    }),
                },
            ],
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("measure"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.measure_pipeline);
            pass.set_bind_group(0, &bind_group, &[0]);
            pass.dispatch_workgroups(GROUPS, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&partial, 0, &staging, 0, partial_bytes);
        self.queue.submit(Some(encoder.finish()));
        let sums: Vec<f32> = self.map_read(&staging)?;
        let (mut zero, mut one) = (0.0f64, 0.0f64);
        let (pairs, _) = sums.as_chunks::<2>();
        for pair in pairs {
            zero += f64::from(pair[0]);
            one += f64::from(pair[1]);
        }
        Ok((zero, one))
    }

    /// Banda de memória medida com um kernel de cópia de `bytes` bytes (lê e escreve
    /// `bytes`), em GB/s (1e9 bytes por segundo), mediana de `repetitions`. Usa os
    /// timestamps da GPU se houver, senão o tempo de parede.
    pub fn copy_bandwidth(&self, bytes: u64, repetitions: usize) -> Result<f64> {
        let make = |label| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let (src, dst) = (make("src"), make("dst"));
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("copy"),
            layout: &self.layout_copy,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: src.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: dst.as_entire_binding(),
                },
            ],
        });
        let groups = (bytes / 16).div_ceil(u64::from(self.options.workgroup_size));
        let (x, y) = self.dispatch_size(groups);

        let mut seconds = Vec::new();
        for round in 0..repetitions + 1 {
            let timing = self.timestamps.then(|| TimestampQueries::new(&self.device));
            let started = Instant::now();
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("copy"),
                    timestamp_writes: timing.as_ref().map(TimestampQueries::writes),
                });
                pass.set_pipeline(&self.copy_pipeline);
                pass.set_bind_group(0, &bind_group, &[]);
                pass.dispatch_workgroups(x, y, 1);
            }
            if let Some(timing) = &timing {
                timing.resolve(&mut encoder);
            }
            self.queue.submit(Some(encoder.finish()));
            let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
            let elapsed = match timing {
                Some(t) => t.read(&self.device, &self.queue),
                None => started.elapsed().as_secs_f64(),
            };
            if round > 0 {
                seconds.push(elapsed); // a primeira rodada é aquecimento
            }
        }
        seconds.sort_by(f64::total_cmp);
        let median = seconds[seconds.len() / 2];
        Ok(2.0 * bytes as f64 / median / 1e9)
    }
}

/// Par de timestamps (início e fim) de uma passada de computação.
struct TimestampQueries {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    staging: wgpu::Buffer,
}

impl TimestampQueries {
    fn new(device: &wgpu::Device) -> Self {
        let set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: 2,
        });
        let make = |label, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: 16,
                usage,
                mapped_at_creation: false,
            })
        };
        Self {
            set,
            resolve: make(
                "ts resolve",
                wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            ),
            staging: make(
                "ts staging",
                wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            ),
        }
    }

    fn writes(&self) -> wgpu::ComputePassTimestampWrites<'_> {
        wgpu::ComputePassTimestampWrites {
            query_set: &self.set,
            beginning_of_pass_write_index: Some(0),
            end_of_pass_write_index: Some(1),
        }
    }

    fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.resolve_query_set(&self.set, 0..2, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.staging, 0, 16);
    }

    /// Segundos entre os dois timestamps. Chamar depois de `submit` + `poll`.
    fn read(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> f64 {
        let slice = self.staging.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        let ok = rx.recv().map(|r| r.is_ok()).unwrap_or(false);
        if !ok {
            return f64::NAN;
        }
        let Ok(data) = slice.get_mapped_range() else {
            return f64::NAN;
        };
        let ticks: Vec<u64> = bytemuck::pod_collect_to_vec(&data);
        drop(data);
        self.staging.unmap();
        let period = f64::from(queue.get_timestamp_period()); // ns por tick
        (ticks[1].saturating_sub(ticks[0])) as f64 * period * 1e-9
    }
}
