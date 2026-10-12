#!/usr/bin/env bash
# One-time setup for building AnvilCAD with the x86_64-pc-windows-gnu Rust toolchain
# (no Visual Studio needed).
#
# Requirements:
#   - rustup with the stable-x86_64-pc-windows-gnu toolchain
#   - GNU binutils (as + dlltool) from a MinGW-w64 distribution, e.g. WinLibs:
#       winget install BrechtSanders.WinLibs.POSIX.UCRT
#
# rustup's bundled MinGW lacks an assembler, which crates using raw-dylib need, and a few
# Windows import libraries. This script generates those import libraries with GNU dlltool.
# Rust's own self-contained linker and C runtime are still used for the final link.
set -euo pipefail

TOOLS="${ANVIL_TOOLS:-$HOME/.anvilcad-tools}"
source "$(dirname "$0")/mingw-path.sh"

command -v as >/dev/null || { echo "error: GNU 'as' not found. Install WinLibs (see above)."; exit 1; }
command -v dlltool >/dev/null || { echo "error: GNU 'dlltool' not found."; exit 1; }

TC="$(rustc --print sysroot)/lib/rustlib/x86_64-pc-windows-gnu"
mkdir -p "$TOOLS/lib"
SYS="${SYSTEMROOT:-C:/Windows}/System32"
for dll in shlwapi imm32 dwmapi uxtheme propsys; do
  if [ -f "$TC/lib/self-contained/lib$dll.a" ]; then continue; fi
  def="$TOOLS/lib/$dll.def"
  {
    echo "LIBRARY $dll.dll"
    echo "EXPORTS"
    objdump -p "$SYS/$dll.dll" \
      | awk '/\[Ordinal\/Name Pointer\] Table/{f=1;next} f&&/^\s*\[/{print "  "$NF} f&&/^$/{exit}'
  } > "$def"
  dlltool -d "$def" -D "$dll.dll" -l "$TOOLS/lib/lib$dll.a"
  echo "generated lib$dll.a"
done
echo "Setup complete."
