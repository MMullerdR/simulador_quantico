# Algoritmo de Shor — mapeado ao código

Arquivos, todos em [`crates/hgm-algos/src`](../crates/hgm-algos/src): [`shor.rs`](../crates/hgm-algos/src/shor.rs)
(orquestração do algoritmo — o mais denso do projeto),
[`shor_circuits.rs`](../crates/hgm-algos/src/shor_circuits.rs) (construção dos circuitos: QFT, somadores,
multiplicadores modulares) e [`number_theory.rs`](../crates/hgm-algos/src/number_theory.rs) (matemática
clássica pura: GCD, frações contínuas, inverso modular). O C++ equivalente (`lib_shor.cpp`,
`lib_shor_circuits.cpp`, `lib_shor_number_theory.cpp`) está no tag `v1.1.0`. O algoritmo fatora um
número `N` explorando o período da função `f(x) = a^x mod N`. Esta implementação usa duas técnicas
específicas que vale muito a pena entender antes de ler o código, porque sem elas ele parece
"mágico":

1. **Estimação de fase iterativa/semiclássica** (1 único qubit de controle, reaproveitado, em vez
   de um registrador inteiro de fase).
2. **Somador quântico no domínio de Fourier** (Draper adder) para fazer a exponenciação modular
   sem "somadores" booleanos tradicionais.

O executável `shor QUBITS ...` não recebe `N`: o número de qubits precisa ser um dos valores
mapeados internamente (15→57, 17→119, 19→253, 21→485, 23→1017, 25→2045, 27→2863), e `shor()` recebe o
número já escolhido. `shor(config, number, &ShorOptions)` aceita qualquer `number >= 3` que caiba
em `(32 - 3) / 2 = 14` bits.

## 1. Layout de qubits

[`Layout::for_modulus(N)`](../crates/hgm-algos/src/shor_circuits.rs) (`n` = número de bits de `N`):

```rust
width = n;
qubits   = 2*n + 3;
ctrl     = 0;           // 1 qubit: ancilla de estimação de fase (reaproveitado)
reg1     = 1;           // n qubits: 1..n           -> registrador acumulador
over     = n + 1;       // 1 qubit: n+1             -> "overflow" do somador
reg2     = n + 2;       // n qubits: n+2..2n+1      -> registrador de trabalho
over_bool = qubits - 1; // 1 qubit: 2n+2            -> flag auxiliar de overflow
```

Estado inicial: `reg1` é inicializado no valor `|1>` (`simulator.set_basis(1 << (width + 2))`,
que — usando a convenção de bits do [01](01-arquitetura-geral.md) — liga o qubit `n`, o bit menos
significativo de `reg1`). Isso é o ponto de partida padrão do Shor: computar `a^x mod N` a partir
de `1`. Com `N = 15`, são 11 qubits; com `N = 2863`, 27.

## 2. Por que só 1 qubit de "QFT" (`ctrl`)?

A versão "livro-texto" do Shor precisa de um registrador de `2n` qubits em superposição para fazer
a estimação de fase (QPE) via QFT inversa completa. Isso é caro em qubits. Este código usa a
variante **semiclássica** (técnica de Griffiths–Niu / "Kitaev phase estimation" com reaproveitamento
de qubit):

- Um único qubit (`ctrl`) é colocado em superposição (`H`), usado como controle de uma
  exponenciação modular controlada, medido, e **reiniciado** para a próxima rodada — em vez de
  manter `2n` qubits vivos simultaneamente.
- Como o qubit é medido a cada rodada, o "giro de fase" que normalmente a QFT inversa faria entre
  qubits diferentes do registrador de fase é substituído por uma **porta de fase
  clássica-condicionada** (`gen_rot`, dependente dos bits *já medidos*) aplicada na rodada.

Esse laço é o coração de `run_rounds` ([`shor.rs`](../crates/hgm-algos/src/shor.rs)):

```rust
for round in (0..2 * width).rev() {                // 2n rodadas
    let base_pow_mod     = modular_pow(base, 1 << round, number)?;          // a^(2^round) mod N
    let base_inv_pow_mod = modular_pow(base_inverse, 1 << round, number)?;  // (a^-1)^(2^round) mod N

    let mut steps = vec![hadamard_phase];                                   // H(ctrl)
    steps.extend(c_mult_mod(&layout, base_pow_mod, number, qft_degree, gates));   // reg2 += a^(2^i)·reg1
    steps.extend(c_swap_r(qubits, ctrl, reg1, reg2, width));                      // troca reg1 <-> reg2
    steps.extend(cr_mult_mod(&layout, base_inv_pow_mod, number, qft_degree, gates)); // desfaz com o inverso
    steps.push(hadamard_phase);                                             // H(ctrl)
    if measured_phase_bits != 0 { steps.push(gen_rot(qubits, ctrl, measured_phase_bits, gates)); }

    simulator.run_steps(&steps, 1, 1)?;
    let bit = simulator.measure(ctrl)?.bit;            // mede e efetivamente "reseta" o qubit
    measured_phase_bits = (measured_phase_bits << 1) | u64::from(bit);   // a fase, em binário
}
```

Ou seja: a cada uma das `2n` rodadas, `ctrl` funciona como o "controle" de uma multiplicação
modular controlada por `a^(2^i) mod N`, exatamente como no QPE tradicional — mas em vez de deixar
as fases se acumularem em um registrador multi-qubit e só ler tudo no final com uma QFT inversa, o
bit é lido *na hora* e a correção de fase equivalente é aplicada manualmente via `gen_rot`.

A base `a` é sorteada com o **mesmo gerador** das medições (`simulator.rng_mut()`, o `Mt19937`),
coprima com `N`; `shor_with_base` deixa quem chama escolher a base (para os testes). Fixar
`HGM_SEED` torna a execução inteira reproduzível.

## 3. `c_mult_mod` / `cr_mult_mod` — multiplicação modular controlada

[`shor_circuits.rs`](../crates/hgm-algos/src/shor_circuits.rs). A ideia (técnica de
Vedral–Barenco–Ekert / Beckman et al.): para multiplicar `reg2` por `a` módulo `N`, controlado por
`ctrl`:

1. Aplica `H` em `over` e em todos os qubits de `reg2`. `reg2` começa em `|0>` e a QFT de `|0>` é a
   superposição uniforme, então esse `H` é a QFT de `reg2` (a `qft` completa é calculada só para
   registrar as portas `R<n>` no catálogo e não entra nos passos, como no C++); depois dela, somar um
   valor fixo vira só um conjunto de rotações de fase — seção 4.
2. Para cada bit `i` do `reg1` (do mais significativo ao menos), aplica `c2_add_mod(...)` — soma
   modular de `(a*2^i) mod N` em `reg2`, controlada simultaneamente por `ctrl` **e** pelo bit `i`
   de `reg1` — implementando a soma "deslocar e somar" de uma multiplicação binária, mas tudo
   dentro do domínio de Fourier.
3. Aplica `rqft` (QFT inversa) para voltar `reg2` à base computacional.

`cr_mult_mod` é o mesmo processo com `c2_sub_mod` no lugar de `c2_add_mod`, e **precisa receber o
inverso multiplicativo modular** do multiplicador (`mul_inv(a, N)`, Euclides estendido) — usado
para "desfazer" a parte que sobrou em `reg1` depois da troca, um padrão comum em aritmética
reversível ("compute, copy/swap, uncompute"). Receber `a` em vez de `a^-1` foi o bug do item 30
de [07](07-bugs-e-pontos-de-atencao.md) (`reg2` saía com `x*(1 - a^2)`); o Rust nasceu com o
inverso, e o teste `circuit_and_emulation_measure_the_same_bits_for_every_base` o pega (as bases
2, 7, 8 e 13 não são o próprio inverso módulo 15).

`shor_round_steps` junta as três peças (`c_mult_mod`, `c_swap_r`, `cr_mult_mod`) de uma rodada,
sem a medição.

## 4. `qft`/`rqft` e o somador de Draper (`add_f`/`sub_f`)

Todas em `shor_circuits.rs`. A QFT aqui é a sequência padrão de `H` + portas de fase controladas
(`R2`, `R3`, ...) entre pares de qubits do registrador — a construção de circuito de QFT de
livro-texto. `qft` e `rqft` são espelhos exatos um do outro (mesmo circuito, só muda o sinal da
fase, o prefixo do nome da porta — `"R"` vs `"R'"`, necessário porque o nome vira chave no
catálogo de portas da execução (`GateCatalog`, ver
[07](07-bugs-e-pontos-de-atencao.md) item 5) — e um `reverse()` final), por isso compartilham a
implementação interna `qft_impl(..., inverse, qft_degree, gates)`, e cada uma é só um wrapper de
uma linha. (Existiu no C++ uma variante `QFT2`/`ApplyQFT`, código morto, removida ainda lá;
item 14 de [07](07-bugs-e-pontos-de-atencao.md); o Rust não a portou.)

**QFT aproximada (item 26 de [07](07-bugs-e-pontos-de-atencao.md)).** `qft_impl` descarta as
rotações controladas cujo nível (distância entre controle e alvo + 1, ângulo `2*pi/2^nível`) passa
de `qft_degree`: elas mudam pouco o resultado e são a maior parte das portas. O grau vem de
`ShorOptions::qft_degree` (`SHOR_AQFT=N` no executável): `None` usa o padrão
`ceil(log2(largura)) + 2` (`qft_degree_for_width`), e `Some(0)` é o circuito com a QFT exata. A
taxa de sucesso medida a 19 e 21 qubits (no C++) ficou igual à do circuito exato; **não** foi
medida a 25 qubits ou mais. (No C++ o grau era uma variável global, `set_qft_degree`; no Rust é
um argumento, e não há estado global.)

O ponto chave: **depois de uma QFT, somar uma constante `value` a um registrador vira apenas uma
sequência de portas de fase em cada qubit** (o "somador de Draper", puramente em portas diagonais
de fase — daí porque `add_f`/`sub_f` só criam portas do tipo `DiagPri`, o tipo mais barato de
executar, ver [03](03-motor-de-execucao-cpu.md)). `add_f` e `sub_f` são, pelo mesmo motivo que
`qft`/`rqft`, espelhos exatos (sinal da fase e prefixo `"ADD_"`/`"SUB_"` do nome da porta) e
compartilham `add_sub_f_ops`. Ela calcula, para cada qubit do registrador, qual produto de rotações
corresponde a somar/subtrair `value` naquela posição binária, registra essa porta combinada sob um
nome único (`"ADD_<value>_<dígito>"` / `"SUB_..."`) via `GateCatalog::add_gate`, e monta o step.
`c_add_f`/`c2_add_f` (e os análogos `sub`) são as mesmas portas com um ou dois controles extra
amarrados.

`c2_add_mod`/`c2_sub_mod` combinam `add_f`/`sub_f` com o qubit `over` para implementar soma
**modular** (soma `a`, subtrai `N`, verifica overflow via `over_bool`, soma `N` de volta
condicionalmente) — o gadget clássico de soma modular reversível. Ao contrário de `qft`/`rqft` e
`add_f`/`sub_f`, essas duas **não** compartilham implementação: a sequência de passos do "somar mod
N" e do "desfazer a soma mod N" é genuinamente diferente (compute/uncompute), não um simples
espelhamento de sinal.

As rotações são calculadas em `f64` e arredondadas uma vez para `f32` (`phase`); o C++ multiplicava
`float complex` fator a fator (`cpowf`), então as matrizes diferem por alguns `1e-7`. A estrutura
dos passos, que é o que decide o circuito, é igual (e os testes de `hgm-algos` comparam os textos
dos passos com os vetores de `tests/golden`).

## 5. Pós-processamento clássico (fora de qualquer circuito quântico)

Depois das `2n` rodadas, `measured_phase_bits` contém a fase medida (em binário, invertida bit a bit
por `revert_bits`). `factors_from_phase` é **matemática clássica pura**, sem nenhum qubit
envolvido:

1. `quantum_frac_approx` — aproximação por frações contínuas, para transformar a fase medida (um
   número entre 0 e 1) na fração `c/q` mais simples que a aproxima — `q` é o candidato a período de
   `f(x)=a^x mod N`. Reproduz a aritmética mista do C++ (`float` e `double`) para que a paridade
   com os vetores de referência seja exata.
2. Um pequeno laço tenta múltiplos de `q` até achar um período `r` tal que `a^r mod N == 1` (o
   período real).
3. `i = a^(r/2) mod N`; os fatores candidatos são `gcd(N, i+1)` e `gcd(N, i-1)` (`quantum_gcd`,
   Euclides). Se algum gcd não-trivial (`1 < fator < N`) for encontrado, esse é um fator de `N`.
4. Se falhar, `shor()` retorna um vetor vazio — é esperado que o algoritmo falhe uma fração das
   vezes (período ímpar, `a` não coprimo com `N`, erro de medição etc.); quem chama (`shor.rs` do
   `hgm-cli`) simplesmente reporta falha, não há retry automático.

`shor_detailed` devolve também a base usada e os bits medidos (`ShorOutcome`), para diagnóstico e
para os testes conferirem a estimação de fase: quando o período divide `2^(2n)` a fase é exata, o
numerador medido é múltiplo de `2^(2n)/r`, e qualquer bit invertido, bit fora de ordem ou rodada com
multiplicador errado desloca o pico (teste `measured_phase_sits_on_an_exact_peak_of_the_period`).

## 6. Dois pontos de atenção sobre a correção de fase

**`gen_rot` e o nome da porta (item 1 de [07](07-bugs-e-pontos-de-atencao.md), corrigido).** No C++,
toda correção de fase depois da primeira reaproveitava a matriz da primeira, porque o nome da porta
era fixo e `addGate` não sobrescreve. O Rust nomeia a porta pelos bits medidos
(`Rot_<bits>`), então cada correção tem a sua matriz. Se a rotação dá exatamente 1, `gen_rot`
devolve um passo só de `ID` — nunca uma string vazia, que o parser contaria como 0 qubits (item 20
de [07](07-bugs-e-pontos-de-atencao.md)).

**A correção de fase não faz nada (item 31 de [07](07-bugs-e-pontos-de-atencao.md), ABERTO e
reproduzido de propósito).** Cada rodada é montada como `H, (multiplicação modular), H, gen_rot` e
em seguida o qubit é medido. `gen_rot` é uma porta diagonal aplicada imediatamente antes de uma
medição na base computacional, que não muda nenhuma probabilidade: na estimação de fase semiclássica
a rotação teria que vir **antes** do segundo `H`. O Rust reproduz o comportamento do `v1.1.0` porque
a migração exige paridade, bit a bit, com o C++ (`tests/golden/data/runs.txt`); corrigir é uma mudança
de algoritmo. Para `N = 15` é irrelevante (os períodos 2 e 4 dividem `2^(2n)`, e os picos de fase são
exatos com ou sem a correção). Para outros `N` o efeito medido é fraco e inconclusivo, e a métrica
certa para avaliá-lo é a fase medida, não a taxa de "Found factors".

## 7. Desempenho em GPU: fusão de fases, estado residente e emulação

Cerca de 85% das portas do Shor são rotações controladas (diagonais) da QFT/RQFT e dos somadores.
Na GPU e no híbrido elas são fundidas em poucas portas de fase por `fuse_phase_runs` (padrão
ligado) e o estado fica residente na GPU: o `measure()` de cada uma das `2n` rodadas roda lá. Ver
[04](04-gpu-wgpu.md), seções 5 e 6, e os itens 27 e 28 de [07](07-bugs-e-pontos-de-atencao.md). Medido
na qflex (RTX A4500): o `shor 25` em GPU leva ~10,6 s no Rust (~9,1 s no C++ `v1.1.0`) e o `shor 27`
~51,7 s (~43,9 s no C++), no circuito completo ([12](12-versao-rust.md), seção 4).

**Emulação da multiplicação modular (item 29 de [07](07-bugs-e-pontos-de-atencao.md)).** Com
`SHOR_EMULATE=1` (`ShorOptions::emulate_mult`) a multiplicação modular controlada de cada rodada
(`c_mult_mod` + `c_swap_r` + `cr_mult_mod`) é aplicada como uma permutação dos índices do estado
(`Simulator::apply_mod_mult`, [`modmult.rs`](../crates/hgm-core/src/modmult.rs)) em vez das portas do
circuito: `|ctrl>|x> → |ctrl>|a·x mod N>` quando o controle vale 1 e `x < N`; o resto do espaço não
muda. Escrita como "gather" (cada destino é escrito uma vez, e os buffers trocam de lugar). Deixa de
ser um circuito, por isso é opcional; no `shor 27` em GPU: 51,7 s (circuito) → 0,85 s (emulação). O
circuito exato é o oráculo da emulação: o teste
`circuit_and_emulation_measure_the_same_bits_for_every_base` confere que, com QFT exata, mesma base
e mesma semente dão os mesmos bits.
