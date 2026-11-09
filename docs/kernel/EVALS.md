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
| **Volume**: the fine mesh's volume against the closed-form volume (to 5e-4) or against the reference integrated along a grid of exact ray spans (to 3e-3 plus twice the grid's own convergence error) | Missing or extra pieces, faces bulging off their surfaces |
| **Membership**: 300 points spread through the part, inside the kernel's result (by the mesh's winding number) exactly when inside the reference; points near a primitive's boundary are skipped | Material on the wrong side, cavities filled, cuts missing |
| **Pixels**: depth and normal images of the display mesh from four views, against the reference ray-cast, pixel by pixel; silhouettes and creases are not judged | Cracks, holes in the mesh, flipped or bent shading normals, faces drawn in the wrong place |
| **Panics**: building and tessellating run under `catch_unwind` | Any panic, which the kernel must never raise |

Each case ends as **pass**, **error** (the kernel refused with a `KernelError`: the user sees a
message and loses nothing) or **fail** (a wrong or invalid body, or a panic). A fail is never
acceptable. Each corpus case also says what it expects today: `Pass`, or `Graceful` for a known
limit, which may refuse but must not be wrong. The gate fails when a case breaks its
expectation, and also when a `Graceful` case starts passing, so the fix is recorded by promoting
it.

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
pixels red (coverage), blue (depth) or green (normal).

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

## What it does not cover yet

- Freeform (B-spline) faces, sweeps, lofts, fillets, chamfers, shells and drafts: the reference
  model has no exact counterpart for them yet. Their own tests in `anvil-brep` and `anvil-model`
  check them against exact values.
- Results with internal voids or non-manifold contact, which the kernel refuses by design today.
- Self-intersection of a single face, which neither this audit nor SMLib's checker tests.
