#!/usr/bin/env bash
# Build the extension module and drop it next to the Python sources.
#
# There is no maturin step: the crate is a plain cdylib whose module entry point
# is `PyInit_nodez`, so renaming the shared library to `nodez.so` is all Python
# needs to import it.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
profile="${1:-debug}"

case "$profile" in
    debug)   cargo build -p nodez-py ;;
    release) cargo build -p nodez-py --release ;;
    *) echo "usage: $0 [debug|release]" >&2; exit 2 ;;
esac

cp "$here/../target/$profile/libnodez_py.so" "$here/python/nodez.so"
echo "built $here/python/nodez.so"
