#!/usr/bin/env bash
# Renders every kernel evaluation result in the real app, headlessly, for a person to look
# over: each corpus case is built, written to STEP, opened in anvilcad-agent and
# screenshotted. Needs no display (see docs/AGENT_HARNESS.md; on Linux without a GPU,
# install mesa-vulkan-drivers).
#
#   scripts/kernel-gallery.sh [OUT_DIR] [anvil-evals arguments...]
#
# OUT_DIR defaults to target/evals/gallery. Extra arguments go to anvil-evals, for
# example --filter cyl/ or --fuzz 50 --seed 3.
set -euo pipefail
cd "$(dirname "$0")/.."
out="${1:-target/evals/gallery}"
shift || true
mkdir -p "$out/step"
cargo build --release -q -p anvil-evals -p anvil-app --bin anvil-evals --bin anvilcad-agent
# The evaluation result decides nothing here: failing cases are worth seeing too.
./target/release/anvil-evals --quiet --step "$out/step" --images "$out/oracle" "$@" || true
shopt -s nullglob
count=0
for f in "$out"/step/*.step; do
  name="$(basename "$f" .step)"
  # Open the part, dismiss the import summary, and take the shot.
  if printf '%s\n' \
    "{\"cmd\":\"open\",\"path\":\"$f\"}" \
    '{"cmd":"click","label":"OK","role":"Button"}' \
    "{\"cmd\":\"screenshot\",\"path\":\"$out/$name.png\"}" \
    '{"cmd":"quit"}' |
    ./target/release/anvilcad-agent --size 900x600 2>/dev/null | grep -q '"path"'; then
    count=$((count + 1))
  else
    echo "could not render $name" >&2
  fi
done
echo "rendered $count parts into $out (oracle comparisons in $out/oracle)"
