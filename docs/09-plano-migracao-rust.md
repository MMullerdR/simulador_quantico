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
│  └─ hgm-cli/     # binários shor / grover / general (clap)
├─ tests/golden/   # vetores de referência exportados do C++ no tag
├─ benches/        # criterion
└─ docs/
```

Dependência: `cli -> algos -> engine -> {cpu, gpu} -> core`. O backend entra no
motor por uma `trait Backend`, o que também permite plugar CUDA depois.

## 8. Fases

Tamanho relativo: P (pequeno), M (médio), G (grande). Não coloco datas: dependem
da disponibilidade de quem vai fazer, e eu não tenho como medi-la.

| Fase | O que entrega | Tam. | Portão para avançar |
|---|---|---|---|
| **0. Fechar o C++** | item 30 commitado; docs 06/07/08 atualizados; `make test GPU=real` verde; tag; branch de manutenção | P | tag criada e CI verde |
| **1. Vetores de referência** | **feita (2026-11-21), ver `tests/golden/README.md`**: 310 casos exportados do `v1.1.0` (estados, texto dos geradores, portas compiladas, fusão e agendamento, medição com semente, teoria dos números), conferidos por um simulador independente em Python | P | Rust consegue ler e comparar: o formato é texto e float32 binário, sem depender do C++ |
| **2. Spike de GPU (go/no-go)** | kernel do Hadamard/porta densa + fundida em WGSL, medição em `f32` com fechamento em `f64`; medir na A4500 contra o CUDA, e em pelo menos outro fornecedor se houver hardware | M | critério 3 da seção 1; se falhar, decidir entre CUDA opcional, CubeCL ou aceitar a perda |
| **3. Núcleo + CPU** | `hgm-core`, `hgm-cpu` (serial e paralelo), parser, fusão, agendamento, medição | G | paridade com os vetores + testes aleatórios + critério 2 |
| **4. GPU completa** | sessão residente, lotes de portas, agendamento, medição e colapso na GPU, emulação de multiplicação modular | G | paridade GPU x CPU nas tolerâncias de hoje |
| **5. Algoritmos e CLI** | Grover, Shor (com a correção do item 30 e a emulação), general; binários e `SimConfig` | M | os 3 executáveis reproduzem os resultados do smoke test |
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
