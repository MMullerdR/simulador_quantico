#!/usr/bin/env python3
"""Confere os vetores de referência de tests/golden/data com um simulador
INDEPENDENTE do C++ (Python + numpy), escrito a partir da especificação do
formato de circuito (docs/02) e não do código C++. Serve para dois fins:

  1. provar que os arquivos estão íntegros e legíveis sem o C++;
  2. confirmar que o t_CPU produziu o que um simulador independente produz,
     de modo que o porte em Rust compara com algo que já foi verificado por
     outro caminho (e não só "o que o C++ cuspiu").

O que é verificado independentemente (resumo por tipo no final):
  state    reexecuta os steps em Python e compara com o estado final;
           Shor/QFT/somador de Fourier: checa o resultado esperado;
  measure  reproduz o MT19937 (numpy) e a regra de sorteio do DGM::measure,
           conferindo os bits medidos, as probabilidades e o colapso;
  colapse, modmult  recalcula em Python;
  pts      reconstrói as portas de cada step e compara; confere que fusão e
           agendamento preservam o estado final;
  table    teoria dos números com math/pow do Python;
  text     geradores simples reimplementados; gramática de todos os steps.

Uso: python tests/golden/check_golden.py [pasta_data]
Sai com código 1 se algo falhar.
"""
import math
import os
import re
import sys
from collections import Counter

import numpy as np

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")

DATA = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(os.path.abspath(__file__)), "data")
FAILS = []
CHECKS = Counter()
WORST = {}


def fail(case, message):
    FAILS.append("%s: %s" % (case, message))


def ok(kind, case, passed, message, value=None):
    CHECKS[kind] += 1
    if value is not None:
        WORST[kind] = max(WORST.get(kind, 0.0), value)
    if not passed:
        fail(case, message)


# ----------------------------------------------------------------- leitura

def read_case(path):
    info = {"gates": {}, "lines": []}
    with open(path, encoding="utf-8") as handle:
        rows = [row.rstrip("\n") for row in handle]
    i = 0
    while i < len(rows):
        row = rows[i]
        key, _, rest = row.partition(" ")
        if key in ("steps", "pts", "lines"):
            count = int(rest)
            info[key + "_list"] = rows[i + 1:i + 1 + count]
            i += 1 + count
            continue
        if key == "gate":
            parts = rest.split()
            v = [float(x) for x in parts[1:]]
            info["gates"][parts[0]] = np.array([complex(v[0], v[1]), complex(v[2], v[3]),
                                                complex(v[4], v[5]), complex(v[6], v[7])])
        elif key in ("format", "name", "kind", "qubits", "iterations", "qft_degree", "mode", "absorbed", "seed"):
            info[key] = rest
        elif key in ("init", "final", "schedule", "measure", "bits", "values", "colapse", "modmult", "call"):
            info[key] = rest
        else:
            info["lines"].append(row)
        i += 1
    return info


def load_state(relative, qubits):
    raw = np.fromfile(os.path.join(DATA, relative), dtype="<f4")
    if raw.size != (1 << qubits) * 2:
        return None
    pairs = raw.reshape(-1, 2).astype(np.float64)
    return pairs[:, 0] + 1j * pairs[:, 1]


# ---------------------------------------------------------- simulador próprio

BASE = {
    "H": np.array([1, 1, 1, -1], dtype=complex) / math.sqrt(2),
    "X": np.array([0, 1, 1, 0], dtype=complex),
    "Z": np.array([1, 0, 0, -1], dtype=complex),
    "Y": np.array([0, 1j, -1j, 0], dtype=complex),
    "R1": np.array([1, 0, 0, np.exp(1j * math.pi)]),
    "R2": np.array([1, 0, 0, np.exp(1j * math.pi / 2)]),
    "R3": np.array([1, 0, 0, np.exp(1j * math.pi / 4)]),
}


def matrices(info):
    table = dict(BASE)
    table.update(info["gates"])
    return table


TOKEN_CONTROL = re.compile(r"^Control(\d+)\((\d)\)$")
TOKEN_TARGET = re.compile(r"^Target(\d+)\((.+)\)$")


def parse_step(step, qubits):
    """Devolve [(target_bit, control_mask, control_value, gate_name)] de um step."""
    tokens = step.split(",")
    if len(tokens) != qubits:
        raise ValueError("step com %d tokens, esperado %d: %s" % (len(tokens), qubits, step[:60]))
    groups = {}
    ops = []
    for q, token in enumerate(tokens):
        bit = qubits - 1 - q
        m = TOKEN_CONTROL.match(token)
        if m:
            groups.setdefault(int(m.group(1)), {"c": [], "t": []})["c"].append((bit, int(m.group(2))))
            continue
        m = TOKEN_TARGET.match(token)
        if m:
            groups.setdefault(int(m.group(1)), {"c": [], "t": []})["t"].append((bit, m.group(2)))
            continue
        if token != "ID":
            ops.append((bit, 0, 0, token))
    for number in sorted(groups):
        group = groups[number]
        cmask = sum(1 << b for b, _ in group["c"])
        cval = sum(v << b for b, v in group["c"])
        for bit, gate in group["t"]:
            ops.append((bit, cmask, cval, gate))
    return ops


def apply_op(state, n, target_bit, cmask, cval, m):
    idx = np.arange(1 << n)
    pick = (((idx & cmask) == cval) & (((idx >> target_bit) & 1) == 0))
    i0 = idx[pick]
    i1 = i0 | (1 << target_bit)
    a0, a1 = state[i0].copy(), state[i1].copy()
    state[i0] = m[0] * a0 + m[1] * a1
    state[i1] = m[2] * a0 + m[3] * a1


def apply_fused(state, n, terms):
    idx = np.arange(1 << n)
    phase = np.zeros(1 << n, dtype=np.int64)
    for mask, value, steps in terms:
        phase += np.where((idx & mask) == value, steps, 0)
    state *= np.exp(2j * math.pi * (phase % 65536) / 65536.0)


def simulate(info, init, qubits, iterations):
    table = matrices(info)
    state = init.copy()
    for _ in range(iterations):
        for step in info["steps_list"]:
            for bit, cmask, cval, gate in parse_step(step, qubits):
                apply_op(state, qubits, bit, cmask, cval, table[gate])
    return state


def init_state(info, qubits):
    spec = info["init"].split()
    if spec[0] == "basis":
        state = np.zeros(1 << qubits, dtype=complex)
        state[int(spec[1])] = 1
        return state
    return load_state(spec[1], qubits)


def layout(modulus):
    width = modulus.bit_length()
    return width, 2 * width + 3


# ----------------------------------------------------------------- verificações

def check_state(name, info):
    qubits = int(info["qubits"])
    iterations = int(info.get("iterations", "1"))
    init = init_state(info, qubits)
    final = load_state(info["final"].split()[1], qubits)
    if init is None or final is None:
        return fail(name, "arquivo de estado ausente ou com tamanho errado")
    # a porta de teste AD não é unitária (anti-diagonal com um fator 0,6): com ela
    # no circuito a norma final não é 1 por construção, e só a do estado inicial vale
    uses_nonunitary = any("AD" in step.replace("ADD", "") for step in info["steps_list"])
    final_norm_ok = uses_nonunitary or abs(np.vdot(final, final).real - 1) < 1e-3
    ok("state-norm", name, abs(np.vdot(init, init).real - 1) < 1e-4 and final_norm_ok,
       "estado fora de norma 1 (init %.6f, final %.6f)" % (np.vdot(init, init).real, np.vdot(final, final).real))
    mine = simulate(info, init, qubits, iterations)
    diff = float(np.max(np.abs(mine - final)))
    ok("state-vs-python", name, diff < 5e-5, "difere do simulador Python em %.3g" % diff, diff)

    m = re.match(r"shor-round-N(\d+)-a(\d+)-deg\d+-c(\d)-x(\d+)$", name.split("-", 1)[1])
    if m:
        modulus, mult, ctrl, x = int(m.group(1)), int(m.group(2)), int(m.group(3)), int(m.group(4))
        width, q = layout(modulus)
        target = (mult * x) % modulus if ctrl else x
        index = (ctrl << (q - 1)) | (target << (width + 2))
        weight = abs(final[index]) ** 2
        # QFT aproximada perde um pouco de peso (docs/07, item 26)
        approx = "-deg0-" not in name
        ok("shor-round", name, weight > (0.7 if approx else 0.99),
           "peso %.4f no resultado esperado (reg1=%d) abaixo do esperado" % (weight, target))
    m = re.match(r"shor-qft-rqft-N15-deg(\d+)-x(\d+)$", name.split("-", 1)[1])
    if m:
        index = int(m.group(2)) << 1
        weight = abs(final[index]) ** 2
        ok("shor-qft-roundtrip", name, weight > (0.99 if m.group(1) == "0" else 0.5),
           "QFT seguida de RQFT devolveu peso %.4f ao estado inicial" % weight)
    m = re.match(r"shor-draper-add-N15-deg(\d+)-x(\d+)$", name.split("-", 1)[1])
    if m:
        index = (int(m.group(2)) + 7) << 1
        weight = abs(final[index]) ** 2
        ok("shor-draper-add", name, weight > (0.99 if m.group(1) == "0" else 0.5),
           "somador de Fourier: peso %.4f em x+7" % weight)


def mt_draws(seed, count):
    generator = np.random.MT19937()
    generator._legacy_seeding(seed)       # init_genrand(seed), igual ao std::mt19937
    return generator.random_raw(count)


def check_measure(name, info):
    qubits = int(info["qubits"])
    state = load_state(info["init"].split()[1], qubits)
    final = load_state(info["final"].split()[1], qubits)
    seed = int(info["seed"])
    parts = info["measure"].split()
    order = [int(v) for v in parts[1:]]
    bits = [int(v) for v in info["bits"].split()]
    values = [float(v) for v in info["values"].split()]
    draws = mt_draws(seed, 2 * len(order))
    good = True
    for k, qubit in enumerate(order):
        shift = qubits - 1 - qubit
        idx = np.arange(1 << qubits)
        prob = np.abs(state) ** 2
        zero = float(prob[((idx >> shift) & 1) == 0].sum())
        one = float(prob[((idx >> shift) & 1) == 1].sum())
        # std::uniform_real_distribution<double>: generate_canonical com 2 sorteios de 32 bits
        sample = (int(draws[2 * k]) + int(draws[2 * k + 1]) * 2.0 ** 32) / 2.0 ** 64
        if sample >= 1.0:
            sample = math.nextafter(1.0, 0.0)
        sample = float(np.float32(sample))          # random_sample é float no DGM::measure
        bit = 0 if zero > sample else 1
        if bit != bits[k]:
            good = False
        kept = zero if bit == 0 else one
        if abs(kept - values[k]) > 1e-5 * max(1.0, kept):
            good = False
        norm = math.sqrt(kept)
        new = np.zeros_like(state)
        keep = ((idx >> shift) & 1) == bit
        new[idx[((idx >> shift) & 1) == 0]] = state[idx[keep]] / norm
        state = new
        bits[k] = bit if bit == bits[k] else bits[k]
    ok("measure-rng-bits", name, good, "bits, probabilidades ou sorteio diferem do MT19937 reproduzido em Python")
    diff = float(np.max(np.abs(state - final)))
    ok("measure-collapse", name, diff < 1e-5, "colapso difere em %.3g" % diff, diff)


def check_colapse(name, info):
    qubits = int(info["qubits"])
    state = load_state(info["init"].split()[1], qubits)
    final = load_state(info["final"].split()[1], qubits)
    qubit, value = [int(v) for v in info["colapse"].split()]
    shift = qubits - 1 - qubit
    idx = np.arange(1 << qubits)
    keep = ((idx >> shift) & 1) == value
    norm = math.sqrt(float((np.abs(state[keep]) ** 2).sum()))
    new = np.where(keep, state / norm, 0)
    diff = float(np.max(np.abs(new - final)))
    ok("colapse", name, diff < 1e-5, "difere em %.3g" % diff, diff)


def check_modmult(name, info):
    qubits = int(info["qubits"])
    ctrl, reg_first, width, mult, modulus = [int(v) for v in info["modmult"].split()]
    state = load_state(info["init"].split()[1], qubits)
    final = load_state(info["final"].split()[1], qubits)
    reg_shift = qubits - reg_first - width
    ctrl_shift = qubits - 1 - ctrl
    reg_mask = (1 << width) - 1
    new = np.zeros_like(state)
    for d in range(1 << qubits):
        src = d
        if (d >> ctrl_shift) & 1:
            y = (d >> reg_shift) & reg_mask
            if y < modulus:
                x = (pow(mult, -1, modulus) * y) % modulus      # destino y = mult*x, origem x
                src = (d & ~(reg_mask << reg_shift)) | (x << reg_shift)
        new[d] = state[src]
    diff = float(np.max(np.abs(new - final)))
    ok("modmult", name, diff == 0.0, "permutação difere em %.3g" % diff, diff)


def pt_list(info):
    items = []
    for row in info["pts_list"]:
        p = row.split()
        if p[0] == "fused":
            count = int(p[1])
            terms = [(int(p[2 + 3 * k]), int(p[3 + 3 * k]), int(p[4 + 3 * k])) for k in range(count)]
            items.append(("fused", terms))
        else:
            v = [float(x) for x in p[6:14]]
            m = np.array([complex(v[0], v[1]), complex(v[2], v[3]), complex(v[4], v[5]), complex(v[6], v[7])])
            positions = [int(x) for x in p[14:]]
            items.append(("pt", int(p[1]), int(p[2]), int(p[3]), int(p[4]), int(p[5]), m, positions))
    return items


def check_pts(name, info):
    qubits = int(info["qubits"])
    mode = info["mode"]
    steps = info["steps_list"]
    table = matrices(info)
    items = pt_list(info)
    expected_ops = [parse_step(step, qubits) for step in steps]
    total_ops = sum(len(ops) for ops in expected_ops)
    declared = int(info.get("absorbed", "0"))
    plain = all(it[0] == "pt" for it in items)

    if mode == "parse":
        # cada step vira uma fatia contígua de PTs, na ordem que o C++ escolhe; o
        # conteúdo de cada fatia tem que ser o mesmo conjunto de portas
        position = 0
        good = len(items) == total_ops
        for ops in expected_ops:
            part = items[position:position + len(ops)]
            position += len(ops)
            got = Counter((it[1], it[2], it[3]) for it in part if it[0] == "pt")
            want = Counter((bit, cmask, cval) for bit, cmask, cval, _ in ops)
            if got != want:
                good = False
        ok("pts-parse", name, good, "lista de PT difere das portas reconstruídas dos steps")
        # a máscara e as posições de controle são coerentes
        coherent = all((it[2] & it[3]) == it[3] and bin(it[2]).count("1") == it[4]
                       and sorted(it[7]) == sorted(b for b in range(qubits) if (it[2] >> b) & 1) for it in items)
        ok("pts-control-fields", name, coherent, "control_mask/value/count/posições incoerentes")
    else:
        # 'absorbed' conta as portas engolidas por portas fundidas; cada porta fundida
        # criada ocupa uma posição da lista, então:
        #   portas do circuito = itens da lista - fundidas + absorbed
        fused_count = sum(1 for it in items if it[0] == "fused")
        if "fuse" in mode:
            good = len(items) - fused_count + declared == total_ops
        else:
            good = len(items) == total_ops
        ok("pts-absorbed", name, good,
           "itens %d, fundidos %d, absorbed %d não fecham com %d portas do circuito" % (len(items), fused_count, declared, total_ops))

    # em todos os modos: a lista, aplicada a um estado qualquer, tem que dar o mesmo estado do circuito
    rng = np.random.default_rng(12345)
    start = rng.normal(size=1 << qubits) + 1j * rng.normal(size=1 << qubits)
    start /= np.linalg.norm(start)
    reference = start.copy()
    for ops in expected_ops:
        for bit, cmask, cval, gate in ops:
            apply_op(reference, qubits, bit, cmask, cval, table[gate])
    mine = start.copy()
    for it in items:
        if it[0] == "fused":
            apply_fused(mine, qubits, it[1])
        else:
            apply_op(mine, qubits, it[1], it[2], it[3], it[6])
    diff = float(np.max(np.abs(mine - reference)))
    ok("pts-semantics", name, diff < 1e-4, "lista (%s) não equivale ao circuito: diferença %.3g" % (mode, diff), diff)


def check_table(name, info):
    for row in info["lines"]:
        left, _, right = row.partition(" = ")
        fn, *args = left.split()
        args = [int(a) for a in args]
        out = [int(v) for v in right.split()]
        if fn == "modular_pow":
            want = [pow(args[0], args[1], args[2])]
        elif fn == "mul_inv":
            want = [pow(args[0], -1, args[1])]
        elif fn == "quantum_gcd":
            want = [math.gcd(args[0], args[1])]
        elif fn == "revert_bits":
            want = [int(format(args[0], "0%db" % args[1])[::-1], 2)]
        elif fn == "qft_degree_for_width":
            want = [math.ceil(math.log2(args[0])) + 2 if args[0] > 1 else 2]
        elif fn == "frac_approx":
            num, den, width = args
            n, d = out
            # denominador limitado por 2^width e fração próxima da entrada; não há
            # fórmula fechada independente do algoritmo, então só se confere o contrato
            good = 0 < d < (1 << width) + 1 and abs(n / d - num / den) <= 1.0 / (1 << width)
            ok("table-frac_approx", name, good, "frac_approx fora do contrato: %s" % row)
            continue
        else:
            fail(name, "função desconhecida na tabela: " + fn)
            continue
        ok("table-" + fn, name, out == want, "%s: C++ deu %s, Python %s" % (left, out, want))


def py_hadamard(q, reg, width):
    return ",".join("H" if reg <= i < reg + width else "ID" for i in range(q))


def check_text(name, info):
    lines = info["lines_list"]
    q = None
    for step in lines:
        tokens = step.split(",")
        if q is None:
            q = len(tokens)
        good = len(tokens) == q and all(
            t == "ID" or TOKEN_CONTROL.match(t) or TOKEN_TARGET.match(t) or re.match(r"^[A-Za-z][A-Za-z0-9_'\-]*$", t) for t in tokens)
        if not good:
            return ok("text-grammar", name, False, "step fora da gramática: %s" % step[:80])
    ok("text-grammar", name, True, "")
    call = info.get("call", "")
    m = re.match(r"Hadamard qubits=(\d+) reg=(\d+) width=(\d+)$", call)
    if m:
        ok("text-reimpl", name, lines == [py_hadamard(*[int(g) for g in m.groups()])], "Hadamard difere")
    m = re.match(r"Oracle1 qubits=(\d+) value=(\d+)$", call)
    if m:
        n, v = int(m.group(1)), int(m.group(2))
        want = ["Target1(X)"] + ["Control1(%d)" % ((v >> (n - 1 - i)) & 1) for i in range(1, n)]
        ok("text-reimpl", name, lines == [",".join(want)], "Oracle1 difere")
    m = re.match(r"ControledZ qubits=(\d+)$", call)
    if m:
        n = int(m.group(1))
        want = ["ID"] + ["Control1(0)"] * (n - 2) + ["Target1(Z)"]
        ok("text-reimpl", name, lines == [",".join(want)], "ControledZ difere")
    m = re.match(r"Toffoli qubits=(\d+) c1=(\d+) c2=(\d+) target=(\d+) cv=(\d+)$", call)
    if m:
        n, c1, c2, t, cv = [int(g) for g in m.groups()]
        tokens = ["ID"] * n
        tokens[c1] = "Control1(%d)" % (cv >> 1)
        tokens[c2] = "Control1(%d)" % (cv & 1)
        tokens[t] = "Target1(X)"
        ok("text-reimpl", name, lines == [",".join(tokens)], "Toffoli difere")


# ----------------------------------------------------------------------- main

def main():
    index_path = os.path.join(DATA, "INDEX.txt")
    with open(index_path, encoding="utf-8") as handle:
        rows = handle.read().split("\n")
    declared = int(rows[1].split()[1])
    paths = [r for r in rows[2:] if r]
    ok("index", "INDEX.txt", declared == len(paths), "INDEX declara %d casos e lista %d" % (declared, len(paths)))
    kinds = Counter()
    for relative in paths:
        info = read_case(os.path.join(DATA, relative))
        name = info["name"]
        kind = info["kind"]
        kinds[kind] += 1
        try:
            if kind == "state":
                check_state(name, info)
            elif kind == "measure":
                check_measure(name, info)
            elif kind == "colapse":
                check_colapse(name, info)
            elif kind == "modmult":
                check_modmult(name, info)
            elif kind == "pts":
                check_pts(name, info)
            elif kind == "table":
                check_table(name, info)
            elif kind == "text":
                check_text(name, info)
            else:
                fail(name, "tipo desconhecido " + kind)
        except Exception as error:      # um caso quebrado não pode esconder os outros
            fail(name, "exceção: %r" % (error,))

    print("casos lidos: %d  %s" % (len(paths), dict(kinds)))
    for key in sorted(CHECKS):
        extra = "" if key not in WORST else "  pior diferença %.3g" % WORST[key]
        print("  %-24s %4d verificações%s" % (key, CHECKS[key], extra))
    if FAILS:
        print("\nFALHAS (%d):" % len(FAILS))
        for message in FAILS:
            print("  " + message)
        sys.exit(1)
    print("\nOK: todas as verificações independentes passaram")


if __name__ == "__main__":
    main()
