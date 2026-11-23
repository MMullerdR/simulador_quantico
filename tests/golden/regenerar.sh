#!/usr/bin/env bash
# Regenera os vetores golden com o C++ do tag v1.1.0 e compara com tests/golden/data.
# O C++ não está mais na main: o script clona o tag deste repositório numa pasta
# temporária, copia para lá os geradores (export_*.cpp) e roda os alvos de golden.mk.
# Precisa de git, make e g++ com OpenMP (GPU=stub: não usa nvcc).
#
#   tests/golden/regenerar.sh [PASTA_DE_TRABALHO]   # padrão: pasta temporária
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
work="${1:-$(mktemp -d)}"

if [ ! -d "$work/src" ]; then
	git clone --quiet --branch v1.1.0 "$repo" "$work"
fi
mkdir -p "$work/tests/golden"
cp "$here"/export_golden.cpp "$here"/export_runs.cpp "$here"/golden.mk "$work/tests/golden/"
rm -rf "$work/tests/golden/data"
( cd "$work" && make GPU=stub -f makefile -f tests/golden/golden.mk golden golden-runs )

if diff -r -q "$work/tests/golden/data" "$here/data" > /dev/null; then
	echo "OK: a saída é idêntica a tests/golden/data (pasta de trabalho: $work)"
else
	echo "DIFERE: veja 'diff -r $work/tests/golden/data $here/data'" >&2
	exit 1
fi
