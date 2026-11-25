# Kernel evaluation results

**91 cases: 88 pass, 3 refused, 0 fail, 0 panic. Every case met its expectation.**

Each case is a solid built by AnvilCAD's exact kernel and checked against an independent reference: its topology and mesh are audited, its volume compared, 300 points tested for inside and outside, its STEP export read back, and its images compared pixel by pixel. **pass** means every check agreed; **refused** means the kernel declined with an error, so the user sees a message and loses nothing; a **fail** would be a wrong or invalid body, which is never accepted. A case marked *may refuse* is a known limit. How each check works is in [EVALS.md](EVALS.md).

This page is generated: `cargo run --release -p anvil-evals -- --publish docs/kernel/RESULTS.md`. It comes out the same, byte for byte, from the same commit.

Timings are from the committed reference run `crates/anvil-evals/baselines/cloud-linux-x86_64.json` (a 4-core cloud Linux container, release build, best of 3), not from the run that made this page, since they vary from machine to machine. A dash means the case is newer than that run.

## box

| Case | Note | Reference volume (mm³) | Result | Expected | Build ms | Mesh ms |
|---|---|---:|---|---|---:|---:|
| `box/overlap-union` |  | 15 (exact) | pass | pass | 0.36 | 0.04 |
| `box/overlap-subtract` |  | 7 (exact) | pass | pass | 0.36 | 0.03 |
| `box/overlap-intersect` |  | 1 (exact) | pass | pass | 0.42 | 0.02 |
| `box/nested-union` |  | 64 (exact) | pass | pass | 0.11 | 0.03 |
| `box/nested-intersect` |  | 1 (exact) | pass | pass | 0.08 | 0.01 |
| `box/nested-subtract-void` | one body with a sealed void: two shells | 63 (exact) | pass | pass | 0.09 | 0.00 |
| `box/identical-union` |  | 1 (exact) | pass | pass | 0.09 | 0.01 |
| `box/identical-intersect` |  | 1 (exact) | pass | pass | 0.08 | 0.01 |
| `box/identical-subtract` | subtracting a solid from itself leaves nothing | 0 (exact) | pass | pass | 0.07 | 0.00 |
| `box/boss-on-face` |  | 236 (exact) | pass | pass | 0.14 | 0.03 |
| `box/shared-face` |  | 2 (exact) | pass | pass | 0.11 | 0.02 |
| `box/shared-partial-face` |  | 1.25 (exact) | pass | pass | 0.22 | 0.05 |
| `box/touching-edge` | bodies that would share only an edge are refused: the kernel's solids are manifold | 2 (exact) | refused | may refuse | 0.09 | 0.00 |
| `box/touching-vertex` |  | 2 (exact) | pass | pass | 0.10 | 0.02 |
| `box/through-slot` |  | 72 (exact) | pass | pass | 0.47 | 0.03 |
| `box/cut-in-two` |  | 32 (exact) | pass | pass | 0.51 | 0.03 |
| `box/flush-side-notch` | the tool's side lies in the plate's side: coplanar faces of opposite sense | 480 (exact) | pass | pass | 0.49 | 0.03 |
| `box/flush-top-pocket` |  | 428 (exact) | pass | pass | 0.15 | 0.03 |
| `box/rotated-45-intersect` |  | 16 (exact) | pass | pass | 0.43 | 0.02 |
| `box/rotated-45-subtract` |  | 48 (exact) | pass | pass | 0.43 | 0.04 |
| `box/thin-wall` |  | 4.992 (exact) | pass | pass | 0.30 | 0.03 |
| `box/foil` | a 1 um sheet left by a cut | 0.1 (exact) | pass | pass | 0.27 | 0.02 |

Why cases did not pass:

- `box/touching-edge`: not supported yet: zero-thickness geometry (bodies would touch along an edge)

## near

| Case | Note | Reference volume (mm³) | Result | Expected | Build ms | Mesh ms |
|---|---|---:|---|---|---:|---:|
| `near/boss-gap-1e-3` |  | 247.984 (exact) | pass | pass | 0.01 | 0.02 |
| `near/boss-overlap-1e-3` |  | 248 (exact) | pass | pass | 0.33 | 0.04 |
| `near/side-offset-1e-3` | the boss overhangs the plate's side by a hair | 283.991 (exact) | pass | pass | 0.19 | 0.04 |
| `near/boss-gap-1e-4` |  | 247.998 (exact) | pass | pass | 0.01 | 0.02 |
| `near/boss-overlap-1e-4` |  | 248 (exact) | pass | pass | 0.28 | 0.03 |
| `near/side-offset-1e-4` | the boss overhangs the plate's side by a hair | 283.999 (exact) | pass | pass | 0.19 | 0.03 |
| `near/boss-gap-1e-5` |  | 248 (exact) | pass | pass | 0.15 | 0.03 |
| `near/boss-overlap-1e-5` |  | 248 (exact) | pass | pass | 0.14 | 0.03 |
| `near/side-offset-1e-5` | the boss overhangs the plate's side by a hair | 284 (exact) | pass | pass | 0.15 | 0.03 |
| `near/boss-gap-1e-7` | a gap below the kernel's tolerance: merged or kept apart, either is right | 248 (exact) | pass | pass | 0.19 | 0.03 |

## scale

| Case | Note | Reference volume (mm³) | Result | Expected | Build ms | Mesh ms |
|---|---|---:|---|---|---:|---:|
| `scale/tiny-0.01-union` |  | 0.000015 (exact) | pass | pass | 0.42 | 0.03 |
| `scale/tiny-0.01-subtract` |  | 0.000007 (exact) | pass | pass | 0.35 | 0.04 |
| `scale/huge-1000-union` |  | 15000000000 (exact) | pass | pass | 0.36 | 0.03 |
| `scale/huge-1000-subtract` |  | 7000000000 (exact) | pass | pass | 0.35 | 0.02 |
| `scale/far-1e4-union` |  | 15 (exact) | pass | pass | 0.36 | 0.03 |
| `scale/far-1e4-subtract` |  | 7 (exact) | pass | pass | 0.35 | 0.02 |
| `scale/far-1e5-union` |  | 15 (exact) | pass | pass | 0.37 | 0.03 |
| `scale/far-1e5-subtract` |  | 7 (exact) | pass | pass | 0.34 | 0.02 |
| `scale/far-1e4-hole` |  | 437.168 (exact) | pass | pass | 1.39 | 5.80 |

## cyl

| Case | Note | Reference volume (mm³) | Result | Expected | Build ms | Mesh ms |
|---|---|---:|---|---|---:|---:|
| `cyl/through-hole` |  | 437.168 (exact) | pass | pass | 3.06 | 6.85 |
| `cyl/blind-hole` |  | 462.301 (exact) | pass | pass | 2.15 | 4.44 |
| `cyl/hole-flush-both-ends` | the tool's caps lie in the plate's top and bottom | 437.168 (exact) | pass | pass | 2.57 | 7.10 |
| `cyl/hole-tangent-inside` | a wall of zero thickness along a line is refused: the kernel's solids are manifold | 437.168 (exact) | refused | may refuse | 4.50 | 0.00 |
| `cyl/hole-breaks-out` |  | 449.452 (exact) | pass | pass | 4.06 | 4.84 |
| `cyl/hole-on-edge` | a hole centred on the plate's vertical edge leaves a quarter notch | 484.292 (exact) | pass | pass | 2.96 | 1.55 |
| `cyl/overlapping-holes` |  | 470.851 (exact) | pass | pass | 6.62 | 14.98 |
| `cyl/coaxial-stack` | two cylinders of the same radius on one axis share a cylindrical face | 100.531 (exact) | pass | pass | 8.26 | 10.64 |
| `cyl/coaxial-seams-apart` | the cylinders' seams lie at different angles, so each cap circle crosses the other's seam | 37.6991 (exact) | pass | pass | 10.24 | 7.01 |
| `cyl/coaxial-inside` | a slice of the same radius cut from the middle leaves two cylinders | 75.3982 (exact) | pass | pass | 9.29 | 8.17 |
| `cyl/hole-continues-hole` | a hole drilled from each side that meet in the middle | 524.602 (exact) | pass | pass | 11.93 | 7.50 |
| `cyl/coaxial-tube` |  | 125.664 (exact) | pass | pass | 6.86 | 29.13 |
| `cyl/coaxial-step` |  | 150.796 (exact) | pass | pass | 4.93 | 20.28 |
| `cyl/tangent-outside` | touching along a line: two bodies or a clear error are both fine | 562.832 (exact) | pass | pass | 2.21 | 10.72 |
| `cyl/cross-holes-equal` | equal-radius cross holes meet at points where their surfaces are tangent | 791.339 (exact) | pass | pass | 11.08 | 19.25 |
| `cyl/cross-holes-unequal` |  | 855.081 ± 0.02 (sampled) | pass | pass | 31.47 | 104.67 |
| `cyl/steinmetz-2` | the two ellipses of equal cylinders cross where the surfaces are tangent | 42.6667 (exact) | pass | pass | 5.95 | 4.92 |
| `cyl/steinmetz-3` | three equal cylinders: six ellipse arcs meeting at tangent points | 37.4903 (exact) | pass | pass | 24.49 | 4.29 |
| `cyl/angled-hole` |  | 477.784 ± 0.006 (sampled) | pass | pass | 2.70 | 9.90 |
| `cyl/tee-unequal` |  | 260.949 ± 0.05 (sampled) | pass | pass | 23.34 | 57.24 |
| `cyl/tee-equal` | equal-radius tee: the intersection curve has two singular points | 292.838 ± 0.06 (sampled) | pass | pass | 6.97 | 19.23 |

Why cases did not pass:

- `cyl/hole-tangent-inside`: not supported yet: zero-thickness geometry (the result would touch itself along a curve)

## sphere

| Case | Note | Reference volume (mm³) | Result | Expected | Build ms | Mesh ms |
|---|---|---:|---|---|---:|---:|
| `sphere/octant` |  | 4.18879 (exact) | pass | pass | 1.66 | 0.92 |
| `sphere/half` | the cutting plane passes through the ball's poles | 16.7552 (exact) | pass | pass | 2.02 | 4.33 |
| `sphere/lens` |  | 10.472 (exact) | pass | pass | 2.21 | 4.27 |
| `sphere/pair-union` |  | 56.5487 (exact) | pass | pass | 3.09 | 15.64 |
| `sphere/dent` |  | 492.931 (exact) | pass | pass | 1.55 | 4.53 |
| `sphere/ball-on-corner` |  | 503.665 (exact) | pass | pass | 1.37 | 6.85 |
| `sphere/in-bore` | pieces meeting only in a circle are refused: the kernel's solids are manifold | 41.8879 (exact) | refused | may refuse | 2.90 | 0.00 |
| `sphere/tangent-to-face` | touching at a point: two bodies or a clear error are both fine | 76.1888 (exact) | pass | pass | 1.24 | 7.58 |
| `sphere/hollow` | one body with a sealed void: two shells | 255.516 (exact) | pass | pass | – | – |
| `sphere/nested-hollows` | three bodies, each inside the one around it's void (brep-bench boolean/nested-shells) | 2714.34 (exact) | pass | pass | – | – |

Why cases did not pass:

- `sphere/in-bore`: not supported yet: zero-thickness geometry (the result would touch itself along a curve)

## cone

| Case | Note | Reference volume (mm³) | Result | Expected | Build ms | Mesh ms |
|---|---|---:|---|---|---:|---:|
| `cone/through-box` |  | 123.319 ± 0.003 (sampled) | pass | pass | 3.25 | 5.73 |
| `cone/countersink` | a pointed cone whose cut stops at the plate's top | 491.622 (exact) | pass | pass | 2.39 | 2.39 |
| `cone/countersink-breaks-out` | the side wall, parallel to the cone's axis, cuts it in a hyperbola | 477.734 ± 0.008 (sampled) | pass | pass | 1.60 | 1.66 |
| `cone/sliced-along-axis` | a slice parallel to the axis: hyperbolas on the side, lines on the caps | 7.07754 ± 0.0007 (sampled) | pass | pass | 1.81 | 1.69 |

## torus

| Case | Note | Reference volume (mm³) | Result | Expected | Build ms | Mesh ms |
|---|---|---:|---|---|---:|---:|
| `torus/half` | the cutting plane passes through the tube's centre circle | 39.4784 (exact) | pass | pass | 5.18 | 17.96 |
| `torus/quarter-box` |  | 19.7392 (exact) | pass | pass | 4.63 | 7.81 |
| `torus/threaded-rod` |  | 82.1515 ± 0.04 (sampled) | pass | pass | 71.25 | 131.48 |
| `torus/ring-groove` | a groove whose centre circle lies on the cylinder's surface | 165.772 (exact) | pass | pass | 7.41 | 17.19 |

## blend

| Case | Note | Reference volume (mm³) | Result | Expected | Build ms | Mesh ms |
|---|---|---:|---|---|---:|---:|
| `blend/fillet-box-edge` |  | 237.854 (exact) | pass | pass | 0.87 | 2.27 |
| `blend/fillet-small-radius` |  | 239.995 (exact) | pass | pass | 0.75 | 2.09 |
| `blend/fillet-four-vertical-edges` | each round ends on the top and bottom faces; the next edge is blended on the result | 236.566 (exact) | pass | pass | 4.79 | 7.37 |
| `blend/chamfer-box-edge` |  | 235 (exact) | pass | pass | 0.30 | 0.02 |
| `blend/fillet-boss-root` | a concave round adds material along the root of a boss and ends flush with its sides | 236.644 (exact) | pass | pass | 0.89 | 3.00 |
| `blend/fillet-hole-rim` | a round on a circular edge is a torus | 436.446 ± 0.1 (sampled) | pass | pass | 9.22 | 14.88 |
| `blend/chamfer-hole-rim` |  | 435.466 (exact) | pass | pass | 8.68 | 7.07 |

## chain

| Case | Note | Reference volume (mm³) | Result | Expected | Build ms | Mesh ms |
|---|---|---:|---|---|---:|---:|
| `chain/plate-25-holes` |  | 11616.4 (exact) | pass | pass | 268.79 | 240.45 |
| `chain/plate-64-holes` | a pattern of holes: each cut must not slow down with the number before it | 29738.1 (exact) | pass | pass | 1065.69 | 618.49 |
| `chain/staircase-12` |  | 37 ± 0.0002 (sampled) | pass | pass | 8.58 | 0.21 |
| `chain/perforated-ball` |  | 397.328 ± 0.1 (sampled) | pass | pass | 233.94 | 196.19 |
