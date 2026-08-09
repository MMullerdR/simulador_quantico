# Bugs e Pontos de Atenção

Lista viva de problemas/riscos encontrados lendo o código. Objetivo: não
tomar decisões de "incrementar o algoritmo X" sobre uma base que já tem um
bug de corretude escondido. Atualizar esta lista conforme formos mexendo.

## 1. [BUG PROVÁVEL] `genRot` sempre gera/reusa a porta `"Rot_0"` — afeta o Shor

**Onde:** [lib_shor.cpp:103-128](../src/algorithms/lib_shor.cpp#L103)

```c
string genRot(int qubits, int reg, long value){
    ...
    rot = 1;
    while (value){
        if (value&1) rot *= cpowf(eps, -2*M_PI*I/pow(2.0, k));
        value = value >> 1;   // <- 'value' é consumido bit a bit aqui
        k++;
    }

    if (rot != 1){
        Gates g;
        name = "Rot_" + int2str(value);   // <- 'value' já é 0 nesse ponto!
        g.addGate(name, 1.0, 0.0, 0.0, rot);
        func[reg] = name;
        return concatena(func, qubits);
    }
    return "";
}
```

`value` é passado por valor e é **usado como contador** no `while` — no fim
do laço ele sempre vale `0`, independente do valor original. Ou seja, o nome
gerado é sempre `"Rot_0"`, não importa qual `res` foi passado.

Isso interage mal com `Gates::addGate`
([gates.cpp:33](../src/core/gates.cpp#L33)):

```c
bool Gates::addGate(string name, float complex* matrix){
    if (Gates::list.find(name) != Gates::list.end()) return false;  // não sobrescreve!
    Gates::list[name] = matrix;
    return true;
}
```

Na primeira vez que `genRot` é chamado com uma correção não-trivial, a porta
`"Rot_0"` é criada com a matriz de rotação certa. Em **todas as chamadas
seguintes**, mesmo que a rotação correta (`rot`) seja calculada
corretamente a partir do `res` daquela rodada, `addGate` recusa silenciosamente
sobrescrever `"Rot_0"` — e o step aplica, no qubit `qft_qb`, a matriz da
**primeira** correção de fase aplicada no processo, não a da rodada atual.

**Efeito esperado:** a estimação de fase do Shor (a parte mais delicada do
algoritmo) provavelmente aplica correções de fase erradas a partir da
segunda vez que `genRot` gera uma correção não-trivial em uma mesma
chamada de `Shor()`. Isso pode ser uma causa (talvez a principal) de o Shor
falhar em encontrar fatores mais frequentemente do que a taxa de falha
teórica do algoritmo.

**Como corrigir (não aplicado ainda, só diagnóstico):** gerar o nome a
partir do valor original, antes do laço consumir `value` — por exemplo,
guardando `long value_orig = value;` no topo da função e usando
`int2str(value_orig)` no nome. Também vale considerar usar `res` (o valor
completo acumulado) em vez de reconstruir a partir de `value`, para garantir
nomes realmente únicos por chamada.

## 2. Código morto: `CpuExecution2_*` e `CpuExecution3_*`

**Onde:** [dgm.h:137-143](../include/dgm.h#L137),
[dgm.cu:529-783](../src/core/dgm.cu#L529)

Três famílias de funções (`_1_*`, `_2_*`, `_3_*`) implementam o mesmo
cálculo (denso / diagonal principal / diagonal secundária) de formas
diferentes. `DGM::execute()` só despacha para `CpuExecution1(it)`
([dgm.cu:361](../src/core/dgm.cu#L361)) — as famílias `2` e `3`
não são chamadas de lugar nenhum no projeto atual. Parecem experimentos de
otimização anteriores (a família `3`, com os vetores `gap`/`max`, é uma
tentativa de pular blocos contíguos de bits livres de forma mais eficiente
que a família `1`). Não são um bug, mas são peso morto — vale decidir se
mantém como referência histórica, documenta como "não usado" (feito aqui),
ou remove.

## 3. `PT::destructor()` provavelmente nunca libera memória

**Onde:** [common.cpp:11-15](../src/core/common.cpp#L11)

```c
void PT::destructor(){
    if ((mat_size != 1) && !matrix) free(matrix);
    if (!ctrl_pos) free(ctrl_pos);
    if (!ctrl_rest) free(ctrl_rest);
}
```

As condições estão invertidas: `!matrix` só é verdadeiro quando `matrix ==
NULL` — ou seja, `free(matrix)` só roda quando `matrix` já é `NULL` (o que
não libera nada de útil e depender de `free(NULL)` ser um no-op). O mesmo
vale para `ctrl_pos`/`ctrl_rest`. O provável objetivo original era `if
(matrix) free(matrix);` (liberar quando o ponteiro **existe**). Na prática
isso é um vazamento de memória lento (cada `PT` alocado nunca libera sua
matriz/arrays de controle), pouco crítico para execuções curtas, mas pode
importar se o objetivo for rodar circuitos muito grandes/repetidos.

## 4. `DGM::freeMemory()` chamado sobre estado que não foi alocado por `DGM`

**Onde:** `GenericExecute` ([dgm.cu:28](../src/core/dgm.cu#L28)) usa
`dgm.setMemory(state)` (que não copia, só aponta `state` para o ponteiro
recebido). Se o chamador espera manter posse desse ponteiro depois, é
preciso ter cuidado: `DGM::freeMemory()`/o destrutor da `DGM` chamam
`free(state)` sobre esse mesmo ponteiro. Vale revisar caso a caso quem é
"dono" do buffer antes de usar essas funções em código novo.

## 5. `Gates::list` é `static` (compartilhado entre todas as instâncias)

**Onde:** [gates.h:29](../include/gates.h#L29),
[gates.cpp:7](../src/core/gates.cpp#L7)

Não é um bug isoladamente, mas é a causa raiz de por que o problema do item
1 se manifesta como está: como `Gates::list` é global/estático, portas
registradas com o mesmo nome em execuções diferentes (ou até rodadas
diferentes do mesmo `Shor()`) colidem. Vale ter isso em mente ao criar
qualquer porta nova dinamicamente (sempre garantir nomes realmente únicos,
ou aceitar que o cache seja intencional quando o valor for de fato o mesmo).

---

*Achados durante a leitura de documentação em 2026-08-06. Atualizar esta
lista conforme novos pontos forem encontrados ou os existentes forem
corrigidos.*
