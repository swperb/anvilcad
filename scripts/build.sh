#!/usr/bin/env bash
# Runs cargo with the windows-gnu setup from setup-windows-gnu.sh.
# Usage: scripts/build.sh [cargo args...]   e.g. scripts/build.sh run --release
set -euo pipefail
TOOLS="${ANVIL_TOOLS:-$HOME/.anvilcad-tools}"
source "$(dirname "$0")/mingw-path.sh"
# Use Rust's bundled linker/CRT (not the external MinGW one) and our extra import libs.
export RUSTFLAGS="${RUSTFLAGS:-} -C link-self-contained=yes -L native=$TOOLS/lib"
exec cargo "$@"
