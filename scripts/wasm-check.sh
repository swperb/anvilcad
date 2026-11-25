#!/usr/bin/env bash
# Keeps AnvilCAD's kernel crates buildable for the browser (wasm32-unknown-unknown).
#
# Usage: scripts/wasm-check.sh [--probe]
#   scripts/wasm-check.sh           # cargo check the kernel crates for wasm32
#   scripts/wasm-check.sh --probe   # also build scripts/wasm-probe, print its size, run it
#                                   # in Node on the test fixtures and compare with native
#
# The checked crates are the ones a browser build would ship: anvil-kernel, anvil-sketch,
# anvil-brep, anvil-step, anvil-model and anvil-io. anvil-app does not build for wasm32 yet
# (see docs/WEB.md). The probe needs Node.js (any version with WebAssembly) and, for the
# compressed sizes, gzip and optionally brotli. This script is run by hand; it is not a CI
# job.
set -euo pipefail

cd "$(dirname "$0")/.."

probe=0
case "${1:-}" in
  --probe) probe=1 ;;
  -h|--help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
  "") ;;
  *) echo "unknown argument: $1 (try --help)" >&2; exit 2 ;;
esac

target=wasm32-unknown-unknown
if ! rustup target list --installed 2>/dev/null | grep -qx "$target"; then
  echo "The $target target is not installed. Install it with:" >&2
  echo "  rustup target add $target" >&2
  exit 1
fi

crates=(anvil-kernel anvil-sketch anvil-brep anvil-step anvil-model anvil-io)
args=()
for c in "${crates[@]}"; do args+=(-p "$c"); done
echo "== cargo check --target $target ${crates[*]}"
cargo check --target "$target" "${args[@]}"

[ "$probe" = 1 ] || exit 0

if ! command -v node >/dev/null; then
  echo "The probe runs in Node.js, which was not found on PATH." >&2
  exit 1
fi

out=target/wasm-probe
echo "== building scripts/wasm-probe for $target and natively"
(cd scripts/wasm-probe && CARGO_TARGET_DIR="../../$out" cargo build --release --target "$target")
(cd scripts/wasm-probe && CARGO_TARGET_DIR="../../$out" cargo build --release --example native)

wasm="$out/$target/release/anvil_wasm_probe.wasm"
raw=$(wc -c <"$wasm" | tr -d ' ')
gz=$(gzip -9c "$wasm" | wc -c | tr -d ' ')
line="== probe size: $raw bytes raw, $gz gzip -9"
if command -v brotli >/dev/null; then
  br=$(brotli -c -q 11 "$wasm" | wc -c | tr -d ' ')
  line="$line, $br brotli -q 11"
fi
echo "$line"

files=(crates/anvil-model/tests/fixtures/v0.1.0/*.anvprt crates/anvil-step/tests/data/*.stp)
echo "== WebAssembly (Node $(node --version))"
node scripts/wasm-probe/run.mjs "$wasm" "${files[@]}" | tee "$out/wasm.txt"
echo "== native"
"$out/release/examples/native" "${files[@]}" | tee "$out/native.txt"

# Rebuilt parts must agree exactly. Triangle counts of imported STEP meshes may differ by a
# few, because wasm32 uses Rust's own libm for sin, cos and friends where native uses the
# system's, so the last bits of some angles differ; only body counts are compared there.
strip() { sed -E 's/ ms=[0-9.]+//; s/ triangles=[0-9]+//' "$1"; }
if diff <(strip "$out/wasm.txt") <(strip "$out/native.txt"); then
  echo "== WebAssembly and native results agree"
else
  echo "== WebAssembly and native results differ (see the diff above)" >&2
  exit 1
fi
