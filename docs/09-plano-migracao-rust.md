# Plano de migração para Rust

Status: **proposta**, nada disto foi executado além da Fase 0 (e a Fase 0 ainda
espera commit e tag). Este documento tem três partes: o que existe hoje, se a
migração é viável (e o que ainda não dá para afirmar), e o plano em fases com
portões de decisão.

## 1. Objetivo e critérios de sucesso

Reescrever o HybriD-GM em Rust para que **o mesmo código rode em qualquer GPU e
em qualquer sistema operacional**, sem depender do CUDA Toolkit, e que fique
estruturado para receber mudanças.

Critérios de aceitação (propostos; a pessoa dona do projeto confirma ou muda os
números):

| # | Critério | Como se mede |
|---|---|---|
| 1 | Paridade de resultado: os 4 caminhos (serial, paralelo, GPU, híbrido) dão o mesmo vetor de estado que o `t_CPU` em C++ no tag final | vetores de referência exportados do C++ + circuitos aleatórios contra o oráculo (tolerâncias de hoje: 1e-3 circuito x emulação, 1e-5 GPU x CPU, igualdade exata CPU serial x paralela) |
| 2 | CPU não pode regredir | mesmos comandos de `docs/08`: Rust >= 90% da velocidade do C++ no tag (meta proposta) |
| 3 | GPU portátil com desempenho aceitável | no portão do spike (fase 2): tempo na RTX A4500 até 1,5x o do CUDA atual (meta proposta); abaixo disso, decide-se se mantém um backend CUDA opcional |
| 4 | Build com um comando em Windows, Linux e macOS | `cargo build --release`, sem `nvcc`, sem `makefile` |
| 5 | Compatibilidade declarada **só** para hardware testado | tabela "verificado / não testado" por GPU e sistema (ver riscos) |

## 2. Estado de partida (medido no repositório)

- ~4,9 mil linhas de C++/CUDA (`src/` 4,3 mil, `include/` 0,6 mil); o maior é
  `kernel.cu` (844) e `dgm_par_exec.cpp` (837).
- ~1,1 mil linhas de teste em C++ e ~0,2 mil de shell; **o `t_CPU` serve de
  oráculo** para todo o resto. Isso é o maior ativo da migração: dá para provar
  paridade, não só "parece certo".
- 4 backends (`t_CPU`, `t_PAR_CPU`, `t_GPU`, `t_HYBRID`), CUDA só da NVIDIA.
- Dependências de plataforma que a migração elimina: `nvcc` (item 7 de
  [07](07-bugs-e-pontos-de-atencao.md)), `rand()` do MinGW (item 18), CRLF em
  arquivos, `makefile` com `-arch=sm_86` fixo, OpenMP.
- Interface por texto: os circuitos são strings (`"Control1(1),ID,Target1(X)"`),
  geradas e re-interpretadas a cada passo.

## 3. Viabilidade

**Veredito: viável.** O projeto é pequeno, bem coberto por teste e com oráculo.
Três pontos pedem cuidado e estão tratados no plano: o desempenho da GPU
portátil, o modo híbrido e a memória de GPU. Nenhum deles é conhecido por
bloquear a migração, mas **os dois primeiros só se confirmam medindo** (spike da
fase 2). Não há como afirmar de antemão que o wgpu iguala o CUDA nesta carga.

### 3.1 Mapa C++ -> Rust

| Hoje | Em Rust | Risco |
|---|---|---|
| `float complex` | `num_complex::Complex32` (mesmo layout de 2 `f32`; sobe para a GPU com `bytemuck`, sem cópia) | baixo |
| OpenMP (`parallel for`, regiões) | `rayon`; as regiões são subconjuntos com stride (não contíguos), então o acesso disjunto exige um módulo `unsafe` pequeno e testado | médio |
| `#pragma omp simd` | auto-vetorização + `multiversion`/detecção em tempo de execução (`std::simd` ainda é instável) | médio |
| truques de bit (`__builtin_popcountl`, `ctzl`) | `count_ones`, `trailing_zeros` | baixo |
| `std::mt19937` + `uniform_*_distribution` | `rand_mt` (mesma sequência do MT19937); as distribuições do C++ não são portáveis, então reproduzir sequências bit a bit é desejável, não garantido | baixo |
| `sin/cos` da tabela de fase | `libm` (resultado igual em todo sistema operacional) | baixo |
| `getenv` espalhado (`GPU_FUSE`, `HYBRID_PIN`...) | struct `SimConfig` preenchida na CLI; o núcleo não lê o ambiente | baixo |
| `printf` + `exit(1)` em erro de usuário | `Result<_, SimError>` (`thiserror`); só a CLI encerra o processo | baixo |
| variáveis `static` e `g_rng` globais | contexto passado explicitamente (`OnceLock` só para a tabela de fase) | baixo |
| CUDA (`kernel.cu`) | WGSL via `wgpu` (ver 4) | **alto** |
| memória fixada do híbrido (`cudaHostRegister`) | sem equivalente no wgpu (ver riscos) | **alto** |

### 3.2 Pontos de atenção que o código atual já ensina

- **Aritmética de ponto flutuante.** O CUDA contrai `a*b+c` em FMA por padrão;
  o Rust e o g++ sem `-march=native` não. Por isso CPU x GPU sempre foi
  comparado com tolerância, e continuará sendo. CPU serial x paralela segue
  exigindo igualdade exata (o Rust não faz fast-math por padrão, então isso se
  mantém).
- **Taxa de "Found factors" não valida circuito** (item 30): os testes do Shor
  portados precisam usar equivalência com a emulação e a fase medida, não essa
  taxa.
- **Falha de alocação** em Rust aborta o processo por padrão: usar alocação
  fallible (`try_reserve_exact`) e verificar a memória antes, devolvendo erro.

## 4. Decisão da GPU

Resumo das opções (a coluna "portátil" é sobre o alvo "toda GPU, todo sistema"):

| Opção | Portátil | Observação |
|---|---|---|
| **`wgpu` + WGSL** | sim: Vulkan (Linux/Windows/Android), Metal (macOS/iOS), DX12 (Windows) | **recomendada.** API segura, mantida, um kernel para todos |
| CUDA via `cudarc` | não, só NVIDIA | boa como backend **opcional** atrás de uma feature, se o spike mostrar perda grande |
| Vulkan direto (`ash`/`vulkano`) | Linux/Windows; macOS só por MoltenVK | mais controle, muito mais código |
| OpenCL | irregular; descontinuado no macOS | descartada |
| CubeCL (kernels escritos em Rust) | multi-backend | promissora, mas jovem; vale uma olhada no spike, não como aposta principal. **Não verifiquei a maturidade atual** |

### 4.1 O que o kernel atual precisa e como o wgpu responde

| Recurso do `kernel.cu` | Em WGSL/wgpu | Observação |
|---|---|---|
| memória compartilhada (`__shared__`, região de `2^12` amplitudes = 32 KiB) | `var<workgroup>` | o limite **padrão** do wgpu é 16 KiB; é preciso pedir o limite do adaptador. Em GPU com menos, `gpu_region_bits` cai em tempo de execução |
| `__syncthreads()` | `workgroupBarrier()` | direto |
| `atomicAdd` em memória compartilhada (corpo fundido v2) | `atomic<u32>` em `workgroup` | direto |
| `__constant__ op[300]` | buffer de armazenamento só de leitura | direto |
| `cuFloatComplex`/`cuCmulf` | `vec2<f32>` e produto escrito à mão | sem tipo complexo; sem diferença de custo esperada |
| somas em `double` (medição) | WGSL não tem `f64` portátil | somar por workgroup em `f32` (soma em árvore) e fechar no host em `f64`; **precisa de teste de precisão**: hoje a medição usa `double` |
| índices de 64 bits | WGSL só tem `u32` | GPU limitada a <= 32 qubits (2^32 amplitudes). Cabe: nenhuma GPU comum guarda 2^33 amplitudes de 8 bytes |
| buffer de estado grande | limite de tamanho de buffer/binding por adaptador (padrões do wgpu bem abaixo do que as GPUs aceitam) | pedir os limites do adaptador; acima do limite, dividir o estado em vários buffers. Shor 27 = 1 GiB; a emulação dobra isso |
| `#pragma unroll` | sem equivalente | confiar no compilador do driver; **pode custar desempenho**, medir |
| medição de tempo (`cudaEvent`) | `TIMESTAMP_QUERY` (recurso opcional) | disponível na maioria dos adaptadores desktop |
| ncu/nsys | cada fornecedor tem o seu (Nsight, RGP, Xcode) | o `ncu` já não funcionava na qflex (ERR_NVGPUCTRPERM) |

Nenhum desses itens inviabiliza a porta. Os que podem custar desempenho
(unroll, FMA, sem warp intrinsics) só se conhecem medindo. O kernel é limitado por
banda de memória, o que ajuda a paridade, mas **isso é uma expectativa, não um
resultado medido**.

### 4.2 Resultado do spike (fase 2, 2026-11-21)

Medido só na qflex: RTX A4500, Vulkan, driver 580.178.04, `wgpu` 30.0.2, rustc 1.100.0.
Código em `crates/hgm-gpu` (`examples/spike.rs`, `gpu_grover_bench` em `crates/hgm-algos/examples`,
`tests/parity.rs`). **Uma GPU, um fornecedor, um sistema.**

| Medida | Rust (wgpu/WGSL) | C++ CUDA `v1.1.0` |
|---|---|---|
| Banda de cópia (P8), buffer de 512 MiB | 599 GB/s | não medida |
| Custo de uma porta dentro de uma região (H, 26 qubits, 7800 portas) | 0,181 ms | 0,181 ms (4,7 ms por camada de 26 H, inclinação entre 90 e 300 iterações) |
| `general 26` (H x3), só kernels | 16,5 ms (3 passadas, com agendamento por comutação); 26,9 ms (9 passadas, sem); 140 ms (uma porta por dispatch, sem região) | não medido isolado |
| `general 26`, processo inteiro como o `general.out` (estado no host, cópia para a GPU e de volta) | 0,92 a 0,97 s | 0,58 a 0,70 s (baseline: 0,61 s) |
| mesmo, com o estado **residente** na GPU (lê 1 amplitude) | 0,24 a 0,28 s | não existe no `general.out` |
| Grover 24 qubits (2274 iterações), tempo total | 8,0 s (7,95 a 8,27 s variando coalescida e lote; região de 11 bits: 8,9 s), probabilidade do valor 0,9994 | 8,34 s (inclui as 23 medições qubit a qubit; o Rust não mediu) |

Corretude: 13 circuitos aleatórios (3 a 22 qubits, várias regiões e coalescidas, inclusive
só passadas globais) dão no máximo 2,0e-7 de diferença para a CPU serial (tolerância
1e-5); a soma de medição (`f32` por workgroup, fechada em `f64`) tem erro relativo
< 1e-6 contra `f64` no host em 3 tamanhos; 50 execuções do mesmo circuito dão o
mesmo estado bit a bit (R1 de `docs/11`).

O que o kernel precisa para andar como o CUDA, e que o spike confirmou: a **região em memória
de workgroup** (5x mais rápida que uma porta por dispatch no Hadamard), o agendamento por
comutação (`schedule_for_batching` do núcleo, 9 para 3 passadas) e controles fora da região
tratados como condição sobre o endereço-base da região (sem isso o Grover não cabe em
região).

**Achados que limitam o desenho** (nenhum bloqueia, todos têm custo):

1. **Estado máximo de 27 qubits por binding** nesta GPU: o limite de binding é 2147483644
   bytes, 4 a menos que os 2 GiB de 28 qubits. O C++ chega a 31 qubits na GPU. Passar disso
   exige dividir o estado em vários buffers/bindings (`GpuError::StateTooLarge` hoje).
2. **Vários dispositivos vivos no mesmo processo, destruídos ao mesmo tempo, travaram ou
   derrubaram o processo** (7 de 10 execuções com 4 threads; também com uma `Instance`
   compartilhada, aí com falha de segmentação). Um único contexto compartilhado entre threads
   não teve o problema. Regra do `hgm-gpu`: um `GpuContext` por processo. Não investiguei se
   a causa é o wgpu ou o driver.
3. O caminho de cópia do estado inteiro (host <-> GPU) do wgpu ficou 1,5 a 1,6x acima do
   `cudaMemcpy` do `general.out` (subida 0,18 s, descida 0,24 s para 512 MiB depois de evitar
   uma cópia extra). Some quando o estado fica residente, que é o desenho planejado, mas só
   existe como medida, não como Backend.
4. A tentativa de uma segunda implementação (lavapipe, Vulkan por software do Mesa, na
   própria qflex) falhou ao abrir o dispositivo ("Parent device is lost"). O CI (run
   44906182960, `dfa7a1f`) mostrou o que os executores usam: **macOS** "Apple Paravirtual
   device" (Metal, GPU integrada virtualizada), os 4 testes de paridade passam em 8 a 12 s;
   **Windows** "Microsoft Basic Render Driver" (DX12, `Cpu`, ou seja, software/WARP), passam,
   mas levam ~890 s mesmo com o circuito reduzido a 14 qubits e 5 repetições. Isolado com
   tempos por fase (run 44909816577, `05c817b`): ~135 s para criar o contexto e **751 s na
   primeira execução do kernel de região** (circuito de 3 qubits, uma passada); daí em diante
   cada execução leva de 4 a 100 ms. É custo único, do primeiro uso no WARP, não do tamanho do
   circuito. A sonda `examples/software_probe.rs` (workflow manual `gpu-software-probe`,
   removido depois; está em `722bed6`; run 44913362168) mostrou que depende do **tamanho do tile**: contexto mais primeira execução
   levam 1,7 s com região de 4 bits, 2,9 s com 6 (workgroup 256; 0,8 s com workgroup 64),
   6,6 s com 8, e com 12 não terminaram em 40 min (cancelados; o run anterior deu ~890 s,
   então varia muito). Não verifiquei por que o WARP é tão sensível ao tile. O `hgm-gpu`
   passa a limitar a região a 8 bits em adaptadores de software (`DeviceType::Cpu`); GPU de
   verdade não é afetada; **Linux** sem adaptador (os testes de GPU não rodam). Logo
   Metal e DX12 foram exercitados só em adaptadores virtualizados/por software, e nenhuma
   GPU real que não seja a A4500 foi verificada.

**Não está no spike:** portas de fase fundida, emulação de `modmult`, sessão residente com
medição e colapso na GPU, a implementação da trait `Backend`, Shor, estados acima de 27
qubits, o limite por submissão no Windows (os dispatches já saem em comandos de até 512, mas
não foi testado lá) e qualquer GPU que não seja a A4500.

**Leitura contra o critério 3** (tempo na A4500 até 1,5x o do CUDA): nos kernels e no Grover 24
a razão ficou perto de 1,0; no `general` com cópia completa do estado ficou em 1,5 a 1,6x, por
causa da cópia. Minha recomendação é **seguir para a fase 4** com os itens 1 e 2 acima como
riscos declarados e o item 4 como lacuna, mas a decisão do portão é sua.

## 5. Riscos e mitigação

| Risco | Probabilidade/impacto | Mitigação |
|---|---|---|
| wgpu bem mais lento que CUDA | média / alto | **spike da fase 2 com portão go/no-go**; plano B: backend CUDA opcional (`cudarc`) atrás de uma `trait Backend` |
| Híbrido perde a aceleração da memória fixada | alta / médio | em GPU discreta o híbrido hoje já se reduz a `t_GPU` quando o estado cabe; reavaliar o ganho do híbrido na fase 6 e, se não compensar, manter só o caso "não cabe na GPU" |
| Estado maior que o limite de buffer da GPU | média / médio | dividir em buffers; senão, cair no host como hoje (`GpuModMult` já devolve `false`) |
| Precisão da medição sem `f64` | baixa / médio | soma em árvore + fechamento em `f64` no host; teste contra o `f64` de hoje |
| `unsafe` do acesso por regiões | média / alto | módulo único, `// SAFETY:` em cada bloco, teste de propriedade contra o oráculo, Miri em estados pequenos |
| Bugs de driver em fornecedor que ninguém testou | média / médio | tabela "verificado / não testado"; só declarar compatibilidade onde houve execução. CI hospedado só tem rasterizador por software (lavapipe, WARP): serve para corretude, não para desempenho |
| Escopo crescer ("já que estou reescrevendo...") | alta / alto | **paridade primeiro, refatoração depois** (ver 6); mudanças de comportamento viram itens separados |
| Resultados diferentes nos últimos bits derrubam testes | média / baixo | política de tolerância explícita por par de backends, documentada |
| Perder o conhecimento dos itens de `docs/07` | média / médio | cada item 1-30 vira caso de teste ou nota no código Rust |

## 6. Boas práticas adotadas

**Processo**

1. **Paridade antes de melhoria.** Primeiro uma porta fiel, com os mesmos
   algoritmos e a mesma ordem de operações; só depois trocar a IR de texto por
   tipos, limpar API etc. Misturar as duas coisas impede saber quem quebrou o
   quê.
2. **Teste diferencial.** Antes de apagar o C++, exportar **vetores de
   referência** (circuitos pequenos, até ~12 qubits, em binário compacto) a
   partir do tag. Os testes em Rust comparam com eles, então o C++ pode sair do
   repositório sem perder o oráculo.
3. Testes de propriedade (`proptest`) com circuitos aleatórios contra o `t_CPU`,
   mutações propositais para provar que o teste pega erro (como já se faz hoje).
4. Portões de decisão por fase: não se avança sem o critério da fase anterior.

**Código**

5. Workspace com crates pequenos e dependências em um só sentido (ver 7).
6. **IR tipada** (`Gate`, `Op`, `Circuit`) no lugar de strings; o parser de texto
   continua só como entrada (compatibilidade com `docs/02` e com os testes), e
   um teste garante que gerador tipado e parser produzem a mesma lista de portas.
7. Erros com `Result`; bibliotecas nunca chamam `exit`/`panic` por entrada do
   usuário.
8. `#![forbid(unsafe_code)]` em todo crate, exceto o módulo de regiões da CPU.
   Cada `unsafe` com comentário `SAFETY:`.
9. Sem estado global: configuração (`SimConfig`) e gerador de números aleatórios
   passados explicitamente; variáveis de ambiente lidas só na CLI.
10. Logs com `tracing` no lugar de `printf`/`HYBRID_DEBUG`.
11. Alocação fallible e checagem de memória (host e GPU) antes de começar.
12. Determinismo: `libm` para a tabela de fase, semente explícita, sem
    fast-math.

**Ferramentas e CI**

13. `rustfmt`, `clippy -D warnings`, `cargo deny` (licenças e avisos),
    `rust-toolchain.toml` fixando a versão, `Cargo.lock` versionado.
14. `criterion` para benchmarks, com os mesmos casos de `docs/08`.
15. CI em matriz Linux/Windows/macOS: build + testes de CPU + testes de GPU em
    rasterizador por software; desempenho e fornecedores reais em máquinas
    reais (qflex = NVIDIA A4500).
16. Shaders WGSL validados com `naga` nos testes (pega erro de shader sem GPU).
17. Features de Cargo: `gpu` (padrão), `cuda` (opcional, futura).
18. `rustdoc` + `docs/` em português continuam sendo a documentação.

## 7. Estrutura proposta

```
hybrid-gm/
├─ Cargo.toml                  # workspace
├─ rust-toolchain.toml
├─ crates/
│  ├─ hgm-core/    # tipos, IR de circuito, matrizes, classificação, fusão de fase,
│  │               # agendamento por comutação; sem I/O e sem GPU
│  ├─ hgm-cpu/     # backends serial e paralelo (rayon); único módulo com unsafe
│  ├─ hgm-gpu/     # wgpu + WGSL: kernel, sessão residente, medição, emulação modmult
│  ├─ hgm-engine/  # Simulator: escolhe o backend, híbrido, medição, SimConfig
│  ├─ hgm-algos/   # general, grover, shor (teoria dos números, circuitos)
│  └─ hgm-cli/     # binários shor / grover / general (argumentos posicionais lidos à mão, como nos `*.out`)
├─ tests/golden/   # vetores de referência exportados do C++ no tag
├─ benches/        # criterion
└─ docs/
```

Dependência: `cli -> algos -> engine -> {cpu, gpu} -> core`. O backend entra no
motor por uma `trait Backend`, o que também permite plugar CUDA depois. A trait
mora em `hgm-core` (não em `hgm-engine`) para que `hgm-cpu` e `hgm-gpu` a
implementem sem dependência circular. `hgm-golden` é um crate só de testes (leitor
de `tests/golden`), usado como `dev-dependency`.

**Esqueleto (2026-11-21, compilado e testado só no qflex, Linux x86-64, rustc 1.100.0):** o workspace existe com `hgm-core`
(catálogo de portas, parser com a ordenação por passo do C++, `Pt` e classificação,
`StateVector`, `Backend`), `hgm-cpu` (backend serial, porta de `CpuExecution1_1/2/3`),
`hgm-engine` (`Simulator` mínimo), `hgm-algos` (teoria dos números do Shor),
`hgm-gpu` e `hgm-cli` (espaços reservados). Testes de paridade lêem
`tests/golden`: parser (8 casos `pts` modo `parse`), estados (144 casos `state`) e a
tabela de teoria dos números. Fusão de fase, agendamento, medição, backend paralelo,
GPU e os geradores de circuito **não** foram portados. Verificado no qflex: `cargo
test --workspace` (36 testes, todos passam; os de paridade reprovam mutações
propositais no parser, no executor e numa matriz), `cargo clippy -- -D warnings` e
`cargo fmt --check` limpos. **Não verificado:** Windows e macOS (o CI de
`.github/workflows/rust.yml` fará isso no primeiro push) e qualquer GPU.

## 8. Fases

Tamanho relativo: P (pequeno), M (médio), G (grande). Não coloco datas: dependem
da disponibilidade de quem vai fazer, e eu não tenho como medi-la.

| Fase | O que entrega | Tam. | Portão para avançar |
|---|---|---|---|
| **0. Fechar o C++** | item 30 commitado; docs 06/07/08 atualizados; `make test GPU=real` verde; tag; branch de manutenção | P | tag criada e CI verde |
| **1. Vetores de referência** | **feita (2026-11-21), ver `tests/golden/README.md`**: 310 casos exportados do `v1.1.0` (estados, texto dos geradores, portas compiladas, fusão e agendamento, medição com semente, teoria dos números), conferidos por um simulador independente em Python | P | Rust consegue ler e comparar: o formato é texto e float32 binário, sem depender do C++ |
| **2. Spike de GPU (go/no-go)** | kernel do Hadamard/porta densa + fundida em WGSL, medição em `f32` com fechamento em `f64`; medir na A4500 contra o CUDA, e em pelo menos outro fornecedor se houver hardware | M | critério 3 da seção 1; se falhar, decidir entre CUDA opcional, CubeCL ou aceitar a perda. **Feito no código (2026-11-21), só na A4500/Vulkan: ver §4.2** (paridade com o CUDA nos kernels e no Grover 24; limites e lacunas listados lá); decisão do portão pendente |
| **3. Núcleo + CPU** | `hgm-core`, `hgm-cpu` (serial e paralelo), parser, fusão, agendamento, medição | G | paridade com os vetores + testes aleatórios + critério 2. **Feita no código (2026-11-21), verificada só no Linux x86-64 (qflex); ainda sem commit.** Paridade: estado até 5,7e-7 do C++ (4,3e-6 com fusão), paralelo idêntico ao serial em circuitos aleatórios, medição/colapso/`modmult` exatos nos vetores. Critério 2: com cargas sintéticas de 18 a 26 qubits o Rust ficou mais rápido que o C++ do `v1.1.0` (26 qubits, 16 threads: 0,43 s contra 0,76 s no Hadamard, 0,56 contra 0,89 s na mistura), com o mesmo checksum. Ressalvas: 3 cargas sintéticas, uma máquina, sem Windows/macOS, sem GPU. A fusão de fase na CPU ajuda a carga mista e atrapalha a de fases puras, então fica desligada por padrão (o C++ também só funde no caminho de GPU). O modo híbrido (`parent_mask`/`op_search_end` do `computeRegion`) não foi portado: é da fase 6 |
| **4. GPU completa** | sessão residente, lotes de portas, agendamento, medição e colapso na GPU, emulação de multiplicação modular | G | paridade GPU x CPU nas tolerâncias de hoje. **Em andamento.** Feito (4a, 2026-11-21, A4500): `GpuBackend` (trait `Backend`, estado no host, uma ida e volta por execução, agendamento por comutação com lote 300 como o `OPS_BLOCK`), `BackendKind::Gpu`, tipo de execução 2 nos três executáveis (`general`, `grover`, `shor`), contexto único por processo. `general 10/14/18` com a mesma amplitude da CPU, Grover 12 qubits 8/8 e Shor 15 6/8 no smoke test. **Falta:** fase fundida na GPU, `modmult` emulado, sessão residente (medição e colapso na GPU, sem ida e volta entre medições), estados acima de 27 qubits |
| **5. Algoritmos e CLI** | Grover, Shor (com a correção do item 30 e a emulação), general; binários e `SimConfig` | M | os 3 executáveis reproduzem os resultados do smoke test. **Feita e enviada (9a125bb, 2026-11-21); CI verde em Linux, Windows e macOS (testes de paridade e smoke test dos executáveis nos 3).** `hgm-algos`: Grover (blocos de iterações), Shor (circuito e emulação, com o `a^-1` do item 30) e `general`; `hgm-cli`: três executáveis com os argumentos posicionais dos `*.out`, `HGM_SEED` fixa a semente, tipos de execução 2 e 3 recusados. Paridade com o C++: 88 casos de texto dos geradores (linhas idênticas, matrizes até 3e-7), 49 circuitos regenerados pelo Rust e simulados contra o estado do C++ (até 5,5e-6) e **108 execuções completas semeadas** (40 Grover, 68 Shor) com o mesmo valor ou os mesmos fatores, na mesma ordem, no serial e no paralelo. Isso exigiu portar a `uniform_int_distribution` do libstdc++ (sorteio da base). `tests/smoke_test_rust.sh` porta a parte de CPU do smoke test (amplitudes, Grover >= 7/10, Shor >= 1/16) e passou. Mutação (três rodadas de mutações propositais em Grover/Shor, cada sobrevivente analisado): os que sobraram viraram testes, exceto 3 do Shor que são equivalentes porque a correção de fase do C++ é uma operação nula (**item 31 de `docs/07`**, reproduzido de propósito) e uma mutação do grau padrão da QFT, que ganhou teste direto; também são equivalentes a ordem dos bits medidos e o limite do período no Shor. Não rodei uma 4ª rodada depois desses testes novos. Ressalvas: só CPU, e o smoke test de GPU/híbrido não foi portado; as taxas de sucesso do Shor/Grover no smoke test variam por execução (limiares fracos de propósito) |
| **6. Híbrido** | reavaliar com medição se compensa sem memória fixada; implementar a versão que compensar | M | decisão registrada com números |
| **7. Plataformas e release** | CI em 3 sistemas, tabela verificado/não testado, benchmarks, docs, `v2.0.0` | M | critérios 1-5 |

## 9. O tag do C++ (Fase 0)

- Nome proposto: **`v1.1.0`** (anotado). Já existe `v1.0.0`
  ("Reestruturação e documentação do projeto"), então `v1.1.0` é "o último C++,
  com as otimizações e a emulação"; o Rust nasce como `v2.0.0`.
- Branch de manutenção `cpp-legacy` a partir do tag, para correções no C++ sem
  mexer na linha do Rust.
- O que "fecha" o C++:

| Ponto | Estado |
|---|---|
| Item 30 (`CRMultMod` com `a` no lugar de `a^-1`) | corrigido no working tree; `make test GPU=real` na qflex passou (43 linhas OK, nenhuma FAIL); **falta commit** |
| docs 06/07/08 alinhados com a correção | feito, **falta commit** |
| Item 13 (multi-GPU) | arquivado: nunca testado em hardware real, fora de escopo da migração |
| Item 7 (`nvcc` lento) | não reproduzido; desaparece com a migração |
| CI do GitHub (`test.yml`, backend stub) | verde em `9a78318` |
| Não verificado e a registrar como limite do C++ | emulação a 31-33 qubits (o buffer auxiliar dobra a memória); medição de fase de ponta a ponta do `Shor()` já corrigido (o experimento de 300 execuções se perdeu); desempenho do Shor com a correção não remedido |

- **Nada disso é commitado, e o tag não é criado nem enviado ao GitHub, sem a
  confirmação explícita de quem manda no projeto.**

## 10. Fora de escopo (por enquanto)

- Multi-GPU (arquivado, item 13).
- WebAssembly/WebGPU no navegador: o wgpu permite, mas a memória do WebAssembly
  (4 GiB) limita o estado a ~28 qubits; só como demonstração futura.
- Backend CUDA nativo, a menos que o spike o justifique.
- Mudar algoritmo ou formato de circuito durante a porta (paridade primeiro).

## 11. Decisões tomadas (2026-11-21)

1. **Tag `v1.1.0` e branch `cpp-legacy`: aprovados.**
2. **Metas:** mantidas como piso de não regressão (CPU >= 90% do C++; GPU até 1,5x
   o CUDA). A expectativa de que o Rust seja mais rápido que o C++ **não** é
   adotada como meta: Rust e C++ compilam por backends de otimização equivalentes
   (LLVM no `rustc`, GCC ou LLVM no C++) e, para este tipo de laço, o desempenho
   esperado é parecido. Ganhos possíveis vêm de outras coisas (menos cópia de
   strings com a IR tipada, sem recompilação de shader por plataforma, melhor
   agendamento de threads), e só contam se forem medidos.
3. **Onde mora o Rust:** no mesmo repositório, apenas com as divisões (workspace
   em `crates/`, C++ preservado no tag e na branch `cpp-legacy`).
4. **Hardware:** há acesso a GPUs de outros fornecedores além da NVIDIA; falta
   registrar **quais** (modelo e sistema operacional) para a tabela "verificado /
   não testado" e para o spike da Fase 2.
