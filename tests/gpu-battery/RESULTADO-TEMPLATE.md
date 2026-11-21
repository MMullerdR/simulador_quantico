# Resultado da bateria: <GPU> em <sistema>

Copie este arquivo para `resultados/<gpu>-<sistema>-<data>.md` e preencha. Regras
em [docs/11-bateria-testes-gpu.md](../../docs/11-bateria-testes-gpu.md).

Status geral: **verificada / parcial / não testada** (apague o que não vale)

## Ambiente

(cole a saída de `tests/gpu-battery/coleta-ambiente.sh`)

Backend usado (C++/CUDA, Rust/Vulkan, Rust/Metal, Rust/DX12):
Limites do adaptador (Rust): tamanho máx. de buffer, memória de workgroup, threads
por workgroup, timestamp query disponível?:

## Corretude

| ID | Resultado (OK / FALHA / não rodou) | Maior diferença medida | Observação |
|---|---|---|---|
| C1 | | | |
| C2 | | | |
| C3 | | | |
| C4 | | | |
| C5 | | | |
| C6 | | | |
| C7 | | | |
| C8 | | | |
| C9 | | | |

## Desempenho (mediana de >= 3 execuções; "pulado" = não coube na memória)

| ID | Caso | Tempo (s) | Mín - máx | vs. baseline C++ (A4500) | Tempo / banda medida |
|---|---|---|---|---|---|
| P1 | general 24 e 26, CPU paralela | | | | |
| P2 | general 26, GPU | | | | |
| P3 | general 26, híbrido | | | | |
| P4 | grover 24, GPU | | | | |
| P5 | shor 21, CPU e GPU | | | | |
| P6 | shor 25 e 27, GPU (circuito) | | | | |
| P7 | shor 25 e 27, GPU (emulação) | | | | |
| P8 | banda de memória (GB/s) | | | | |

## Robustez

| ID | Resultado | Observação |
|---|---|---|
| R1 | | |
| R2 | | |
| R3 | | |
| R4 | | |

## Falhas e anomalias

(o que falhou, comando exato, saída, e se reproduz)

## O que não foi verificado

(liste explicitamente)
