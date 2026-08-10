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
[dgm.cpp:529-783](../src/core/dgm.cpp#L529)

Três famílias de funções (`_1_*`, `_2_*`, `_3_*`) implementam o mesmo
cálculo (denso / diagonal principal / diagonal secundária) de formas
diferentes. `DGM::execute()` só despacha para `CpuExecution1(it)`
([dgm.cpp:361](../src/core/dgm.cpp#L361)) — as famílias `2` e `3`
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

**Onde:** `GenericExecute` ([dgm.cpp:28](../src/core/dgm.cpp#L28)) usa
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

## 6. [BUG CONFIRMADO] Segfault em `t_PAR_CPU` quando `cpu_region > qubits`

**Onde:** `src/cli/general.cpp` (defaults do `main()`) +
`PCpuExecution1` em [dgm.cpp:789](../src/core/dgm.cpp#L789).

`general.cpp` usa `cpu_region = 14` fixo como valor padrão,
independente de quantos qubits o usuário pedir na linha de comando. Se
`qubits < cpu_region` (ex: `general.out 10 1 2`, pedindo só 10 qubits),
dentro de `PCpuExecution1`:

```c
long reg_count = (1 << (qubits - region)) + 1;
```

com `qubits=10` e `region=14`, vira `1 << (10 - 14)` = **`1 << -4`** —
deslocamento por expoente negativo, comportamento indefinido em C/C++.
Na prática isso produz um `reg_count` absurdamente grande, e o laço
paralelo seguinte escreve em `state[pos]` muito além do vetor alocado →
**segmentation fault** (reproduzido: `general.out 10 1 2` crasha;
`general.out 16 1 2` não).

**Confirmado que não tem relação com a renomeação** — a aritmética é
idêntica antes e depois, só os nomes dos campos mudaram.

**Workaround imediato:** rodar com `qubits >= cpu_region` (o padrão de
`cpu_region` é 14, então `general.out 16 1 2` ou mais funciona).

**Como corrigir (não aplicado ainda):** validar `region <= qubits` no
começo de `PCpuExecution1` (e possivelmente em `HybridExecution`, que
tem uma lógica de região parecida), reduzindo `region` para `qubits`
quando necessário — parecido com o `if (count < region) region = count;`
que já existe logo acima, só que também cobrindo o caso do valor inicial
de `region` já vir maior que `qubits`.

## 7. Build de `kernel.cu` anormalmente lento (`nvcc`/`cicc`) numa máquina sem GPU

**Onde:** `src/core/kernel.cu`, build via WSL2 sem hardware NVIDIA.

Compilar `kernel.cu` com `nvcc` chegou a levar mais de 2 horas (processo
`cicc` preso em ~100% de CPU) mesmo depois de reduzir drasticamente o
número de instanciações de template (`GEWrapper2`/`GpuExecutionWrapper`,
normalmente ~260 combinações de `tam_block`/`rept`/`coalesc` — ver
[docs/04-gpu-cuda.md](04-gpu-cuda.md)) e de baixar a otimização pra
`-O0`. Mesmo com **uma única instanciação**, não terminou em 5 minutos —
ou seja, não é sobre volume de templates nem nível de otimização; é algo
mais fundamental (ambiente, WSL2 acessando `/mnt/c`, antivírus escaneando
arquivos temporários, ou algum problema específico dessa instalação do
`nvcc`/`cicc`) que não foi diagnosticado até o momento.

**Contorno aplicado:** `src/core/kernel_stub.cpp` — implementação das
mesmas funções `extern "C"` de `kernel.cu`, compilada com `g++` (sem
`nvcc` nenhum), que só imprime um aviso e retorna se alguém tentar usar
`t_GPU`/`t_HYBRID`. É o padrão do `makefile` agora (`GPU=stub`); use
`make GPU=real` para compilar o `kernel.cu` de verdade quando for
investigar isso com calma ou tiver acesso a uma máquina com GPU NVIDIA.
`kernel.cu` em si não foi alterado por causa disso.

**Como investigar no futuro:** testar compilar de dentro do sistema de
arquivos nativo do WSL (`~/...`, não `/mnt/c/...`) com uma única
instanciação isolada; testar com o Windows Defender desligado
temporariamente pra pasta do projeto; testar uma versão do CUDA Toolkit
diferente.

---

*Achados durante a leitura de documentação em 2026-08-06, com adições em
2026-08-10 durante os testes de build da Fase 1 da renomeação. Atualizar
esta lista conforme novos pontos forem encontrados ou os existentes forem
corrigidos.*
