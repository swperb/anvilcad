# Kernel evaluations

`crates/anvil-evals` checks the exact kernel (`anvil-brep`) against an independent analytic
model, so a change to the kernel cannot quietly make parts wrong, ugly or slow. It runs in
`cargo test` (the gate) and from the command line (the full corpus, random streams, images and
timing baselines). It needs no GPU, no SOLIDWORKS and no other kernel.

## How a case is judged

A case is a solid written as a constructive solid geometry tree of exact primitives (boxes,
cylinders, cones, spheres and tori, each placed by a frame) combined by union, subtraction and
intersection (`src/csg.rs`). The tree is built twice:

- **By the kernel**, the way a user's features would build it: boxes and cylinders are
  extrusions, cones, spheres and tori are revolutions, and the booleans are the kernel's own.
- **As the reference**, which answers exactly whether a point is inside and where a ray crosses
  the boundary (with the outward normal there). It shares no geometry code with the kernel.

The kernel's result is then checked three ways (`src/lib.rs`, `src/oracle.rs`, `src/raster.rs`):

| Check | What it catches |
|---|---|
| **Audit**: `check::validate`; every edge sample within 1e-5 mm of the surfaces of both faces that use it; the display mesh closed and finite, enclosing a positive volume, its triangles facing the way their faces do, its vertex normals agreeing with their triangles | Invalid topology, edges off their faces, inside-out faces, shading that lights a face from behind (this found the cone-tip normals fixed alongside it) |
| **Sag**: every display triangle within twice the chord of its face's true surface | Faces drawn flat across curvature (this found the cone slivers fixed alongside it) |
| **Volume**: the fine mesh's volume against the closed-form volume (to 5e-4) or against an unbiased quasi-Monte Carlo reference (exact inside lengths along Halton-placed rays under six random shifts; to 1e-3 plus four standard errors), allowing the mesh's own area times chord | Missing or extra pieces, faces bulging off their surfaces |
| **STEP round trip**: every result written with `anvil-step` and read back valid and of the same volume; a body that comes back as a mesh is a warning | Export or import losing or bending geometry (the rational conic edges included) |
| **Membership**: 300 points spread through the part, inside the kernel's result (by the mesh's winding number) exactly when inside the reference; points near a primitive's boundary are skipped | Material on the wrong side, cavities filled, cuts missing |
| **Pixels**: depth and normal images of the display mesh from four views, against the reference ray-cast, pixel by pixel; silhouettes and creases are not judged | Cracks, holes in the mesh, flipped or bent shading normals, faces drawn in the wrong place |
| **Panics**: building and tessellating run under `catch_unwind` | Any panic, which the kernel must never raise |

Each case ends as **pass**, **error** (the kernel refused with a `KernelError`: the user sees a
message and loses nothing) or **fail** (a wrong or invalid body, or a panic). A fail is never
acceptable. Each corpus case also says what it expects today: `Pass`, or `Graceful` for a known
limit, which may refuse but must not be wrong. The gate fails when a case breaks its
expectation, and also when a `Graceful` case starts passing, so the fix is recorded by promoting
it.

## Features

A case can also build a base tree and then apply feature steps to the result, the way a
user would: a fillet or chamfer on the edge through a given point (`Step`). Its `tree` is
then the exact shape the feature must produce, written in CSG (a round is a square prism
less a cylinder, a hole-rim round less a torus, a bevel a slab or a cone), and every check
above applies. Such a case also states the exact volume the feature takes off or adds, by
formula (Pappus for rounds on circular edges) and checks it on its own, since a small
feature's error hides in the part's total: a hole-rim round 4% too big fails this check.
Rounds much smaller than a millimetre change the volume less than the mesh resolves and
are judged by the other checks alone.

## The corpus

`src/corpus.rs` holds the configurations that break kernels in practice, with closed-form
volumes wherever one exists: overlapping, nested, identical and touching boxes; faces that share
a plane in the same and opposite senses; a boss sketched 1e-7 to 1e-3 mm into, onto or beside a
plate; parts from 0.01 to 1000 times the usual size and 1e4 to 1e5 mm from the origin; holes
that are blind, through, flush, tangent to a side, breaking out, on an edge and overlapping;
coaxial, tangent and crossing cylinders (the Steinmetz solids); spheres cut through the centre,
in a bore, on a corner and tangent to a face; cones and tori cut by planes through their
singular circles; and chains (25 holes in a plate, a 12-step staircase, a ball drilled three
ways) that double as timing cases.

## Random cases

`src/fuzz.rs` draws trees of two to four primitives. Positions and sizes come from a half-unit
grid most of the time and axes are mostly aligned, so faces coincide, axes coincide and
surfaces touch far more often than uniform random numbers would make them. Every case is
reproducible from its seed and index (`fuzz/<seed>-<index>`). Cases whose reference is empty or
too thin to judge are skipped.

## Running it

```bash
# The gate (also part of cargo test --workspace).
cargo test -p anvil-evals

# The whole corpus, with comparison images for every case.
cargo run -p anvil-evals -- --images target/evals/img --report target/evals/corpus.json

# 1000 random cases of up to four primitives from seed 7; only problems are printed.
cargo run -p anvil-evals -- --fuzz 1000 --seed 7 --prims 4 --quiet --failed-images --images target/evals/fuzz

# Show a case's tree and how its reference volume converges.
cargo run -p anvil-evals -- --fuzz 300 --seed 1 --filter 1-136 --probe
```

An image has one row per view: the reference, the kernel's mesh, and their difference with bad
pixels red (coverage), blue (depth) or green (normal). Depth is not judged where the surface is
seen nearly edge-on, where a chord's sag shows divided by the cosine.

To look at the results in the real app, `scripts/kernel-gallery.sh [OUT_DIR] [anvil-evals
arguments]` builds each case, writes it to STEP, opens it in the headless harness
(`anvilcad-agent`, see [AGENT_HARNESS.md](../AGENT_HARNESS.md)) and saves a screenshot, beside the
oracle comparison images. On Linux without a GPU it needs Mesa's lavapipe
(`mesa-vulkan-drivers`).

## Timing baselines

`--repeats N` times each case N times and keeps the fastest. `--save-baseline FILE` writes a
report to compare with later; `--baseline FILE` compares and fails on any case that is more than
`--slower` percent (default 30) and 5 ms slower in building or tessellating, or that passed in
the baseline and does not now.

```bash
cargo run --release -p anvil-evals -- --repeats 5 --save-baseline target/evals/before.json
# ...change the kernel...
cargo run --release -p anvil-evals -- --repeats 5 --baseline target/evals/before.json
```

Baselines are per machine. `crates/anvil-evals/baselines/` keeps the ones recorded for a
change's pull request; compare on the same machine they came from. Timings are not part of
`cargo test`, because CI machines vary too much.

## Known kernel defects the evaluations have found and not yet fixed

- A display mesh that is not watertight where an intersection edge runs into a torus's seam
  circle at a shallow angle (`fuzz/2-28` at `--prims 4`, `fuzz/13-66`): about one random case
  in a thousand.
- A face that fails to tessellate ("boundary crosses itself in parameter space") after a torus
  is cut by another torus (`fuzz/2-344` at `--prims 4`).
- About one random result in twenty-five comes back from a STEP round trip as a mesh rather
  than exact (recorded as a warning, since nothing is lost or wrong): the importer cannot yet
  rebuild a sphere face that is a whole sphere or a band round it, some faces that wrap round
  their axis with one boundary loop, and a few faces whose loops it closes wrongly.
- Unbounded intersection curves between two cones, or a cone and a cylinder, are still refused
  (about one random case in seven hundred); plane and cone are solved exactly.

## What it does not cover yet

- Freeform (B-spline) faces, sweeps, lofts, shells and drafts, and fillets that meet at a
  corner: the reference model has no exact counterpart for them yet. Their own tests in `anvil-brep` and `anvil-model`
  check them against exact values.
- Results with internal voids or non-manifold contact, which the kernel refuses by design today.
- Self-intersection of a single face, which neither this audit nor SMLib's checker tests.
