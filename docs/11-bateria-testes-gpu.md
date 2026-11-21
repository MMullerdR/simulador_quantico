# Bateria de testes em outras GPUs

Status: **preparada, ainda não executada em GPU de outro fornecedor.** Este
documento define o que será rodado, como medir, como registrar e o que conta
como aprovado, para que a bateria possa ser feita depois sem decisões novas. É
parte da Fase 0/1 de [09](09-plano-migracao-rust.md).

## 1. O que está pronto e o que ainda não pode rodar

| Item | Estado |
|---|---|
| Definição da bateria (seções 3 a 6) | pronta |
| Coleta de identificação da máquina: [coleta-ambiente.sh](../tests/gpu-battery/coleta-ambiente.sh) | pronta; testada em Windows (Git Bash) |
| Baseline do C++/CUDA (tempos de `docs/08` + `make test GPU=real`): [run-baseline-cpp.sh](../tests/gpu-battery/run-baseline-cpp.sh) | pronto; só vale em máquina NVIDIA |
| Modelo de registro: [RESULTADO-TEMPLATE.md](../tests/gpu-battery/RESULTADO-TEMPLATE.md) | pronto |
| Registro das GPUs disponíveis (seção 7) | **vazio**, a preencher |
| Execução da bateria em AMD/Intel/Apple | **impossível hoje**: o C++ só tem CUDA, e o programa em Rust que roda a bateria (spike e depois o simulador) ainda não existe |

Ou seja: a bateria só roda de fato em GPU não-NVIDIA depois da Fase 2 do plano.
Até lá, o que existe serve para (a) fixar os critérios, (b) gerar a referência do
C++ na NVIDIA e (c) identificar as máquinas.

## 2. Como usar

1. Em cada máquina, a partir da raiz do repositório:

```bash
tests/gpu-battery/coleta-ambiente.sh > ambiente.md
```

2. Máquina NVIDIA (baseline do C++, no tag `v1.1.0`):

```bash
make GPU=real
tests/gpu-battery/run-baseline-cpp.sh
```

   Saída em `tests/gpu-battery/resultados/baseline-cpp-<data>/`: `tempos.csv`,
   `ambiente.md`, `make-test.log`. `QUICK=1` roda só 3 casos pequenos (confere se o
   script funciona); `REPS` muda as repetições (padrão 3).

3. Quando o Rust existir (Fase 2 em diante), a mesma bateria da seção 3 roda por
   `cargo`, e o resultado vai para um arquivo do modelo `RESULTADO-TEMPLATE.md`,
   um por GPU e sistema.

## 3. Corretude (C): contra o oráculo

Referência: vetores exportados do C++ em `v1.1.0` (Fase 1) e o `t_CPU` de cada
build. Tolerâncias são as que os testes de hoje já usam.

| ID | Caso | Origem hoje | Tolerância |
|---|---|---|---|
| C1 | Portas de 1 qubit (H, X, Y, Z, R1-R3) sem e com controles, alvo em cada posição de bit (dentro e fora da região coalescida) | `tests/test_gates.cpp` | igual ao oráculo em CPU; GPU até 1e-5 |
| C2 | Circuitos aleatórios GPU x CPU em várias combinações de região/bloco/repetição (inclui `repeat_count=1`) | `tests/test_cpu_parallel.cpp` | máx. \|Δ amplitude\| < 1e-5 |
| C3 | Portas de fase fundidas (QFT, somadores): corridas longas, até 64 termos | `test_cpu_parallel`, `test_qft_addf` | < 1e-5 |
| C4 | Medição: probabilidades por qubit e colapso | `measure` / `GpuMeasureSums` | probabilidade relativa < 1e-6 contra soma em `f64` no host (limite proposto, ainda não usado em teste) |
| C5 | Sessão residente: várias execuções e medições seguidas sem voltar ao host | `GPU_RESIDENT` | igual a C2 |
| C6 | Multiplicação modular emulada (N = 15, 21, 35, 55; 3 permutações com H entre elas) | `tests/test_shor_emulate.cpp` | < 1e-5 |
| C7 | Algoritmos de ponta a ponta: `general 24` (amplitude 0,000244 a 2e-6), Grover (>= 2/3 de acertos em 3 a 10 execuções), Shor 15 e 21 | `tests/smoke_test.sh` | como no smoke test |
| C8 | Falta de memória: pedir estado maior que a GPU | `GpuMinFreeBytes` | erro limpo, sem travar nem derrubar o driver |
| C9 | Shor por **fase medida** (fase a até 1/2^(2w) de um múltiplo de 1/r), não por "Found factors" | item 30 de [07](07-bugs-e-pontos-de-atencao.md) | esperado ~24-27% das execuções (N = 57, QFT exata, 150 execuções); por acaso seria ~1% |

Observação: C9 ainda não existe como teste automatizado (a medição do item 30 foi
feita com um patch descartável). Entra na Fase 1/5.

CPU serial x paralela segue exigindo **igualdade exata**. GPU x CPU é sempre por
tolerância (a GPU contrai multiplicação e soma em FMA; a CPU não).

## 4. Desempenho (P)

Mesmos comandos de `docs/08`, mediana de pelo menos 3 repetições, com uma
execução de aquecimento descartada na GPU. Casos que não cabem na memória da GPU
são **pulados e registrados como pulados**, não omitidos.

| ID | Caso | Comando de hoje (`outputs/`) |
|---|---|---|
| P1 | Hadamard 24 e 26 qubits, CPU paralela | `general.out 24 1 <threads>`, `general.out 26 1 <threads>` |
| P2 | Hadamard 26, GPU | `general.out 26 2 1` |
| P3 | Hadamard 26, híbrido | `general.out 26 3 1` |
| P4 | Grover 24, GPU | `grover.out 24 2 1` |
| P5 | Shor 21, CPU paralela e GPU | `shor.out 21 1 <threads>`, `shor.out 21 2 1` |
| P6 | Shor 25 e 27, GPU (circuito) | `shor.out 25 2 1`, `shor.out 27 2 1` |
| P7 | Shor 25 e 27, GPU com emulação | `SHOR_EMULATE=1 shor.out 25 2 1` (e 27) |
| P8 | **Banda de memória medida** (kernel de cópia) | a criar no spike: serve para normalizar GPUs diferentes, já que o simulador é limitado por banda |

Para comparar GPUs diferentes, reportar também **tempo x banda medida (P8)**, não
só tempo absoluto.

## 5. Robustez (R)

| ID | Caso | Por que |
|---|---|---|
| R1 | 50 execuções seguidas do mesmo circuito: resultado idêntico entre elas (ou dentro da tolerância) | pega corrida de dados e driver instável |
| R2 | Estado grande com muitos lotes de portas (ex.: Shor 27) sem o sistema cancelar o kernel | Windows (TDR, ~2 s por comando) e macOS têm vigilância de tempo por submissão; lotes longos precisam ser divididos |
| R3 | GPU também usada pela tela (Intel integrada, Apple, Windows) | mesma vigilância; registrar se a interface trava |
| R4 | Perda do dispositivo (driver reiniciado) | deve virar erro, não pane |

## 6. Critérios de aprovação por GPU

Uma GPU só é marcada **"verificada"** se: C1-C8 passam; R1 passa; e P-medições
foram registradas (ou pulados por memória, anotado). Marcar **"parcial"** com a
lista do que falhou ou não coube. **"Não testada"** é o padrão: nenhuma GPU é
declarada compatível sem execução registrada. C9 passa a ser exigido quando existir
como teste automatizado.

## 7. GPUs disponíveis (a preencher)

Não sei ainda quais GPUs de outros fornecedores o projeto tem à disposição.
Preencher uma linha por GPU e sistema. A única máquina com informação registrada:

| Máquina | GPU | Sistema | API provável | Status |
|---|---|---|---|---|
| qflex (UFPEL) | NVIDIA RTX A4500 | Linux | CUDA (C++), Vulkan (Rust) | baseline C++: ver `tests/gpu-battery/resultados/` |
| máquina de desenvolvimento (a que rodou o `coleta-ambiente.sh`) | Intel UHD Graphics (integrada, driver 32.0.101.7085, Vulkan 1.4) | Windows 11 | Vulkan / DX12 | não testada; memória compartilhada com o sistema, sujeita a R2/R3 |
| (acrescentar) | | | | não testada |

## 8. Riscos específicos desta bateria

- **GPU integrada** (como a Intel UHD acima): pouca banda, pouca memória útil e
  limite de memória compartilhada (workgroup) possivelmente menor que os 32 KiB da
  região padrão; o simulador precisa reduzir `gpu_region_bits` em tempo de execução
  (ver [09](09-plano-migracao-rust.md), seção 4.1).
- **Sem Rust instalado** na máquina usada para escrever isto: nenhum código Rust
  foi compilado ainda, então nada aqui afirma que o spike funciona.
- **Comparação justa** entre fornecedores exige a mesma versão do código e o mesmo
  commit; o `coleta-ambiente.sh` registra o commit.
