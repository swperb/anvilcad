#!/usr/bin/env bash
# Type-checks the workspace for Linux and Windows from any host, with zig as the C compiler.
#
# Usage: scripts/cross-check.sh [TARGET...] [-- CARGO_CHECK_ARGS...]
#   scripts/cross-check.sh                          # both targets, --workspace --all-targets
#   scripts/cross-check.sh x86_64-pc-windows-msvc   # one target
#   scripts/cross-check.sh -- -p anvil-app          # other cargo check arguments
#
# Rust alone cross-checks fine, but mlua-sys compiles Lua's C sources in its build script,
# which needs a C compiler and headers for the target. zig (https://ziglang.org, `brew
# install zig`, `snap install zig --classic`, `winget install zig.zig`) has both for every
# target. This script writes small wrappers that call `zig cc -target ...` (dropping the
# `--target=` flag the cc crate adds, which zig does not accept) and points cargo at them.
#
# For x86_64-pc-windows-msvc, Lua is compiled against zig's MinGW headers and archived with
# `zig lib`, because zig ships no MSVC headers. That is enough for `cargo check`, which
# links nothing, but it is not an MSVC build: build and test on Windows itself (the Windows
# VM, `scripts/winvm/winvm check`, or CI) for that.
set -euo pipefail

cd "$(dirname "$0")/.."

targets=()
extra=()
while [ $# -gt 0 ]; do
  case "$1" in
    --) shift; extra=("$@"); break ;;
    -h|--help) sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) targets+=("$1"); shift ;;
  esac
done
[ ${#targets[@]} -gt 0 ] || targets=(x86_64-unknown-linux-gnu x86_64-pc-windows-msvc)
[ ${#extra[@]} -gt 0 ] || extra=(--workspace --all-targets)

if ! command -v zig >/dev/null 2>&1; then
  echo "cross-check: zig is not installed; see https://ziglang.org/download/ (or brew install zig)" >&2
  exit 2
fi

bin="$PWD/target/cross-check/bin"
mkdir -p "$bin"

# The zig target triple for a Rust target, and the archiver the cc crate expects for it.
zig_target() {
  case "$1" in
    x86_64-unknown-linux-gnu) echo x86_64-linux-gnu ;;
    aarch64-unknown-linux-gnu) echo aarch64-linux-gnu ;;
    x86_64-pc-windows-msvc | x86_64-pc-windows-gnu) echo x86_64-windows-gnu ;;
    aarch64-pc-windows-msvc) echo aarch64-windows-gnu ;;
    x86_64-apple-darwin) echo x86_64-macos ;;
    aarch64-apple-darwin) echo aarch64-macos ;;
    *) return 1 ;;
  esac
}

failed=()
for t in "${targets[@]}"; do
  zt=$(zig_target "$t") || { echo "cross-check: no zig target known for $t" >&2; exit 2; }
  if ! rustup target list --installed 2>/dev/null | grep -qx "$t"; then
    echo "cross-check: adding the Rust target $t"
    rustup target add "$t"
  fi

  cc="$bin/cc-$t"
  cat >"$cc" <<EOF
#!/bin/sh
# Written by scripts/cross-check.sh: zig cc for $t, without the cc crate's --target= flag.
for a in "\$@"; do
  shift
  case "\$a" in --target=*) ;; *) set -- "\$@" "\$a" ;; esac
done
exec zig cc -target $zt "\$@"
EOF
  ar="$bin/ar-$t"
  case "$t" in
    *-msvc) printf '#!/bin/sh\nexec zig lib "$@"\n' >"$ar" ;;
    *) printf '#!/bin/sh\nexec zig ar "$@"\n' >"$ar" ;;
  esac
  chmod +x "$cc" "$ar"

  lower=$(echo "$t" | tr '-' '_')
  echo "== cargo check --target $t ${extra[*]}"
  if env "CC_$lower=$cc" "AR_$lower=$ar" cargo check --target "$t" "${extra[@]}"; then
    echo "== $t: ok"
  else
    echo "== $t: FAILED" >&2
    failed+=("$t")
  fi
done

if [ ${#failed[@]} -gt 0 ]; then
  echo "cross-check: failed for ${failed[*]}" >&2
  exit 1
fi
echo "cross-check: all targets ok (${targets[*]})"
