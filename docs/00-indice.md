# Documentação do HybriD-GM (Quantum-Simulator)

Este é um simulador de computação quântica baseado em vetor de estado (state-vector), capaz de
rodar em CPU serial, CPU paralela, GPU (qualquer uma que o `wgpu` suporte: Vulkan, Metal, DX12) ou
em modo híbrido GPU+CPU. Ele não usa nenhuma biblioteca de "circuito quântico" pronta — tudo, desde
o parser de portas até a aplicação da matriz na memória, é escrito à mão em Rust (os kernels de GPU
em WGSL), com muita manipulação de bits para performance.

> **Nota:** os documentos 01 a 08 descrevem a versão em Rust (2.0.x). A implementação original em
> C/C++/CUDA saiu da `main` na 2.0.1 e continua no tag `v1.1.0` e na branch `cpp-legacy`; os
> documentos citam o que o C++ fazia de diferente onde isso ajuda a entender uma decisão.

## Estrutura de pastas

```
crates/              workspace Rust (ver crates/README.md)
 ├─ hgm-core/        portas, parser, Pt/Op, otimizador, StateVector, medição, modmult, Mt19937, trait Backend
 ├─ hgm-cpu/         backends de CPU: serial e paralelo por regiões (rayon, AVX2)
 ├─ hgm-gpu/         GPU em wgpu/WGSL: planejador de passadas, kernels, contexto, GpuBackend
 ├─ hgm-engine/      Simulator, escolha do backend, HybridBackend
 ├─ hgm-algos/       general, Grover, Shor, teoria dos números, geradores de circuito
 ├─ hgm-cli/         os executáveis general, grover e shor
 └─ hgm-golden/      leitor dos vetores de referência (só testes)
tests/golden/        vetores de referência gerados pelo C++ do tag v1.1.0 (oráculo de paridade)
tests/gpu-battery/   bateria de testes de GPU (docs/11) e o baseline do C++
benches/             benchmarks C++ x Rust
docs/                esta documentação
```

Esta documentação foi escrita para dar entendimento *profundo* de como o simulador funciona por
dentro, não apenas listar o que cada arquivo faz. Ordem sugerida de leitura:

1. [01-arquitetura-geral.md](01-arquitetura-geral.md) — visão geral: representação do estado, o
   `Simulator` e a trait `Backend`, os quatro backends de execução (CPU serial/paralela, GPU,
   híbrido).
2. [02-linguagem-de-circuitos.md](02-linguagem-de-circuitos.md) — o "mini-idioma" de texto usado
   para descrever portas e como ele vira `Pt`/`Op` executáveis.
3. [03-motor-de-execucao-cpu.md](03-motor-de-execucao-cpu.md) — o algoritmo central: como uma
   matriz 2x2 é aplicada sobre um vetor de 2^n amplitudes usando truques de bits; regiões, rayon e
   AVX2.
4. [04-gpu-wgpu.md](04-gpu-wgpu.md) — como o mesmo algoritmo é levado para a GPU (wgpu/WGSL):
   memória de workgroup, "coalescimento", passadas, fase fundida, estado residente, blocos e o
   híbrido.
5. [05-algoritmo-grover.md](05-algoritmo-grover.md) — Grover explicado linha a linha em cima do
   código real.
6. [06-algoritmo-shor.md](06-algoritmo-shor.md) — Shor explicado: layout de registradores, QFT,
   somador em Fourier, estimação de fase semi-clássica, pós-processamento clássico.
7. [07-bugs-e-pontos-de-atencao.md](07-bugs-e-pontos-de-atencao.md) — pontos de atenção do Rust e
   o histórico dos 31 itens do C++ com o que aconteceu com cada um (útil antes de mexer em algo).
8. [08-performance.md](08-performance.md) — como medir, Rust × C++, onde o tempo vai e quando cada
   backend vale a pena.
9. [09-plano-migracao-rust.md](09-plano-migracao-rust.md) — plano de migração para Rust
   (viabilidade, escolha da GPU portátil, riscos, boas práticas, fases e o tag do C++).
10. [10-simulador-em-blocos.md](10-simulador-em-blocos.md) — o simulador em blocos operacionais,
    com diagramas e dois exemplos (Bell e uma rodada do Shor).
11. [11-bateria-testes-gpu.md](11-bateria-testes-gpu.md) — bateria de testes para GPUs de outros
    fornecedores: casos, tolerâncias, critérios e modelo de registro (preparada, ainda não
    executada).
12. [12-versao-rust.md](12-versao-rust.md) — a versão em Rust: como compilar e usar, limites,
    diferenças em relação ao C++, desempenho medido contra o `v1.1.0` e a tabela do que foi
    verificado e do que não foi testado.

## Mapa mental rápido

```
main() (hgm-cli: general / grover / shor)
   |
   v
hgm-algos: general / grover / shor        <- monta o circuito como strings
   |                                         ("hadamard", "cnot", "qft", ...)
   v
Simulator::compile_steps() / run()        <- parser (hgm-core): strings -> Program (Vec<Op>)
   |                                         + fusão de fase (se ligada)
   v
Backend::execute()   (trait de hgm-core)
   |
   +-- 0  CpuSerial    -> SerialBackend      (hgm-cpu, um core, sem paralelismo)
   +-- 1  CpuParallel  -> ParallelBackend    (hgm-cpu, rayon + AVX2, regiões)
   +-- 2  Gpu          -> GpuBackend         (hgm-gpu, wgpu/WGSL, estado residente)
   +-- 3  Hybrid       -> HybridBackend      (hgm-engine, GPU se cabe, senão CPU paralela)
```

Todos os quatro caminhos implementam **a mesma operação matemática** (aplicar uma porta 2x2
controlada sobre um vetor de estado), só que com estratégias de paralelização e acesso à memória
diferentes. Entender bem o caminho serial (o mais simples) é a chave para entender todos os outros.
