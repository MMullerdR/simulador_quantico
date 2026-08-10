# Algoritmo de Shor — mapeado ao código

Arquivos: [lib_shor.cpp](../src/algorithms/lib_shor.cpp) (orquestração do
algoritmo — o mais denso do projeto), [lib_shor_circuits.cpp](../src/algorithms/lib_shor_circuits.cpp)
(construção dos circuitos: QFT, somadores, multiplicadores modulares) e
[lib_shor_number_theory.cpp](../src/algorithms/lib_shor_number_theory.cpp)
(matemática clássica pura: GCD, frações contínuas, inverso modular). Fatora
um número `N` explorando o período da função
`f(x) = a^x mod N`. Esta implementação usa duas técnicas específicas que vale
muito a pena entender antes de ler o código, porque sem elas ele parece
"mágico":

1. **Estimação de fase iterativa/semiclássica** (1 único qubit de controle,
   reaproveitado, em vez de um registrador inteiro de fase).
2. **Somador quântico no domínio de Fourier** (Draper adder) para fazer a
   exponenciação modular sem "somadores" booleanos tradicionais.

## 1. Layout de qubits

```c
qubits = 2*n + 3;         // n = número de bits de N
qft_qb   = 0;              // 1 qubit: ancilla de estimação de fase (reaproveitado)
reg1     = 1;               // n qubits: 1..n           -> registrador acumulador
over     = n+1;             // 1 qubit: n+1              -> "overflow" do somador
reg2     = n+2;             // n qubits: n+2..2n+1       -> registrador de trabalho
over_bool= qubits-1;        // 1 qubit: 2n+2             -> flag auxiliar de overflow
```

Estado inicial: `reg1` é inicializado no valor `|1>` (`setMemoryValue(1<<(n+2))`,
que — usando a convenção de bits do [01](01-arquitetura-geral.md) — liga o
qubit `n`, o bit menos significativo de `reg1`). Isso é o ponto de partida
padrão do Shor: computar `a^x mod N` a partir de `1`.

## 2. Por que só 1 qubit de "QFT" (`qft_qb`)?

A versão "livro-texto" do Shor precisa de um registrador de `2n` qubits em
superposição para fazer a estimação de fase (QPE) via QFT inversa completa.
Isso é caro em qubits. Este código usa a variante **semiclássica** (técnica
de Griffiths–Niu / "Kitaev phase estimation" com reaproveitamento de qubit):

- Um único qubit (`qft_qb`) é colocado em superposição (`H`), usado como
  controle de uma exponenciação modular controlada, medido, e **reiniciado**
  para a próxima rodada — em vez de manter `2n` qubits vivos simultaneamente.
- Como o qubit é medido a cada rodada, o "giro de fase" que normalmente a
  QFT inversa faria entre qubits diferentes do registrador de fase é
  substituído por uma **porta de fase clássica-condicionada**
  (`genRot`, dependente dos bits *já medidos*) aplicada no início da rodada
  seguinte, antes da próxima medição.

Esse laço é o coração da função `Shor()`
([lib_shor.cpp:35](../src/algorithms/lib_shor.cpp#L35)):

```c
for (i = L; i >= 0; i--){                       // L = 2n-1, ou seja, 2n rodadas
    mod_a     = modular_pow(a, 2^i, N);
    mod_inv_a = modular_pow(inv_a, 2^i, N);

    func = { H(qft_qb) }
    func += CMultMod(...)   // multiplica reg2 por a^(2^i) mod N, controlado por qft_qb
    func += CSwapR(...)     // troca reg1 <-> reg2 (controlado por qft_qb)
    func += CRMultMod(...)  // desfaz o "empréstimo" usando o inverso modular
    func += H(qft_qb)
    if (res) func += genRot(qubits, qft_qb, res);  // correção de fase clássica

    dgm.executeFunction(func);
    m = dgm.measure(qft_qb);       // mede e efetivamente "reseta" o qubit
    res = (res << 1) | m;          // acumula os bits medidos (a fase, em binário)
}
```

Ou seja: a cada uma das `2n` rodadas, `qft_qb` funciona como o "controle" de
uma multiplicação modular controlada por `a^(2^i) mod N`, exatamente como no
QPE tradicional — mas em vez de deixar as fases se acumularem em um
registrador multi-qubit e só ler tudo no final com uma QFT inversa, o bit é
lido *na hora* e a correção de fase equivalente é aplicada manualmente na
rodada seguinte via `genRot`.

## 3. `CMultMod` / `CRMultMod` — multiplicação modular controlada

[lib_shor_circuits.cpp:45](../src/algorithms/lib_shor_circuits.cpp#L45). A ideia (técnica
de Vedral–Barenco–Ekert / Beckman et al.): para multiplicar `reg2` por `a`
módulo `N`, controlado por `ctrl`:

1. Aplica `QFT` em `reg2` (ver seção 4 abaixo — depois de QFT, somar um valor
   fixo vira só um conjunto de rotações de fase).
2. Para cada bit `i` do `reg1` (do mais significativo ao menos), aplica
   `C2AddMod(...)` — soma modular de `(a*2^i) mod N` em `reg2`, controlada
   simultaneamente por `ctrl` **e** pelo bit `i` de `reg1` — implementando a
   soma "deslocar e somar" de uma multiplicação binária, mas tudo dentro do
   domínio de Fourier.
3. Aplica `RQFT` (QFT inversa) para voltar `reg2` à base computacional.

`CRMultMod` é o mesmo processo com `C2SubMod` no lugar de `C2AddMod` (usa
`mul_inv(a,N)`, o inverso multiplicativo modular calculado por
`mul_inv` — algoritmo de Euclides estendido) — usado para "desfazer" a parte
que sobrou em `reg1` depois do swap, um padrão comum em aritmética
reversível ("compute, copy/swap, uncompute").

## 4. `QFT`/`RQFT`/`QFT2` e o somador de Draper (`AddF`/`SubF`)

Todas em [lib_shor_circuits.cpp](../src/algorithms/lib_shor_circuits.cpp). A
QFT aqui é implementada como a sequência padrão de `H` + portas de fase
controladas (`R2`, `R3`, ...) entre pares de qubits do registrador — a
construção de circuito de QFT de livro-texto. `QFT`
([lib_shor_circuits.cpp:332](../src/algorithms/lib_shor_circuits.cpp#L332))
e `RQFT` ([lib_shor_circuits.cpp:370](../src/algorithms/lib_shor_circuits.cpp#L370))
são espelhos exatos um do outro (mesmo circuito, só muda o sinal da fase, o
prefixo do nome da porta — `"R"` vs `"R'"`, necessário porque o nome vira
chave no `Gates::list` estático — e um `reverse()` final), por isso
compartilham a implementação interna `QFT_impl(qubits, reg, over, width,
inverse)` e cada uma é só um wrapper de uma linha. `QFT2`
([lib_shor_circuits.cpp:336](../src/algorithms/lib_shor_circuits.cpp#L336))
é uma variante sem o qubit `over`, usada isoladamente em `ApplyQFT` (função
de teste/benchmark) — não compartilha a implementação com `QFT`/`RQFT`.
`QFT`/`RQFT` (com o qubit `over` extra) são as versões usadas dentro de
`CMultMod`.

O ponto chave: **depois de uma QFT, somar uma constante `num` a um
registrador vira apenas uma sequência de portas de fase `Rk` em cada qubit**
(o "somador de Draper", puramente em portas diagonais de fase — daí porque
`AddF`/`SubF` só criam portas do tipo `DIAG_PRI`, o tipo mais barato de
executar, ver [03](03-motor-de-execucao-cpu.md)). `AddF`
([lib_shor_circuits.cpp:249](../src/algorithms/lib_shor_circuits.cpp#L249))
e `SubF` ([lib_shor_circuits.cpp:274](../src/algorithms/lib_shor_circuits.cpp#L274))
são, pelo mesmo motivo que `QFT`/`RQFT`, espelhos exatos (sinal da fase e
prefixo `"ADD_"`/`"SUB_"` do nome da porta) e compartilham a implementação
`AddSubF_impl(qubits, reg, over, value, width, controlled, subtract)`. Ela
calcula, para cada qubit do registrador, qual produto de rotações
corresponde a somar/subtrair `value` naquela posição binária, registra essa
porta combinada sob um nome único (`"ADD_" + value + "_" + i` /
`"SUB_..."`) via `Gates::addGate`, e monta o step. `CAddF`/`C2AddF` (e os
análogos `Sub`) são as mesmas portas com um ou dois controles extra
amarrados.

`C2AddMod`/`C2SubMod`
([lib_shor_circuits.cpp:114](../src/algorithms/lib_shor_circuits.cpp#L114))
combinam `AddF`/`SubF` com o qubit `over` para implementar soma **modular**
(soma `a`, subtrai `N`, verifica overflow via `over_bool`, soma `N` de volta
condicionalmente) — o gadget clássico de soma modular reversível. Ao
contrário de `QFT`/`RQFT` e `AddF`/`SubF`, essas duas **não** compartilham
implementação: a sequência de passos do "somar mod N" e do "desfazer a soma
mod N" é genuinamente diferente (compute/uncompute), não um simples
espelhamento de sinal.

## 5. Pós-processamento clássico (fora de qualquer circuito quântico)

Depois das `2n` rodadas, `res` contém a fase medida (em binário, invertida
bit a bit por `revert_bits`). O resto de `Shor()`
([lib_shor.cpp:137](../src/algorithms/lib_shor.cpp#L137)) é **matemática
clássica pura**, sem nenhum qubit envolvido:

1. `quantum_frac_approx` — aproximação por frações contínuas, para
   transformar a fase medida (um número entre 0 e 1) na fração `c/q` mais
   simples que a aproxima — `q` é o candidato a período de `f(x)=a^x mod N`.
2. Um pequeno laço tenta múltiplos de `q` até achar um período `r` tal que
   `a^r mod N == 1` (o período real).
3. `i = a^(r/2) mod N`; os fatores candidatos são `gcd(N, i+1)` e
   `gcd(N, i-1)` (`quantum_gcd`, Euclides). Se algum gcd não-trivial (`1 <
   fator < N`) for encontrado, esse é um fator de `N`.
4. Se falhar, `Shor()` retorna um vetor vazio — é esperado que o algoritmo
   falhe uma fração das vezes (período ímpar, `a` não coprimo com `N`, erro
   de medição etc.); o caller (`shor.cpp`) simplesmente reporta falha, não
   há retry automático dentro de `Shor()`.

## 6. Um ponto de atenção importante (já corrigido)

O passo de correção de fase (`genRot`, chamado com `res` = bits já medidos,
agora em [lib_shor_circuits.cpp:11](../src/algorithms/lib_shor_circuits.cpp#L11))
tinha um bug que fazia toda correção de fase depois da primeira reaproveitar
a matriz da primeira, comprometendo a corretude das rodadas seguintes — já
corrigido, ver [07-bugs-e-pontos-de-atencao.md](07-bugs-e-pontos-de-atencao.md),
item 1. Vale ler mesmo assim antes de mexer em `genRot`/`Gates::list`, pelo
motivo raiz explicado ali (nomes de porta como chave num cache estático).
