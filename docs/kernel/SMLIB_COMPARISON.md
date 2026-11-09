# The SMLib kernel as a behavioural comparator

NVIDIA released the SMLib geometry kernel as part of USD BRep under the Apache-2.0 licence
(<https://github.com/NVIDIA-Omniverse/usd-brep>). AnvilCAD uses it only as a **reference for
behaviour**: what a mature kernel does with coincident faces, tangent contacts, tolerances,
validity and failure. Nothing from it is copied, built, linked or translated into AnvilCAD; the
kernel in `crates/anvil-brep` stays AnvilCAD's own Rust code (see CLAUDE.md). This note was
written by reading a shallow clone at commit `3fd5b9548d19aa8c12c29bc9637a96d42a06f8a5`
(22 October 2026); paths below are relative to that repository.

**O** marks something observed in its source, documentation or tests at the cited path, **I**
an inference, and **Silent** a question its code and documents do not answer.

## How AnvilCAD compares

| Behaviour | SMLib | AnvilCAD (`anvil-brep`) | Status |
|---|---|---|---|
| One tolerance band for "same point", "on surface" and "same surface" | Zone tolerance 1e-5 per object, intersection tolerance 2e-5, vertex tol > edge tol > face tol (O, `source/SMLib/inc/SmMath.h:38-61`, `SmTol.h:391-407`) | Vertices, faces and on-surface tests now agree at 1e-5 mm in booleans (`MERGE = ON`, coincident faces at `ON`) | **Matched** in this change; it was split between 1e-6 and 1e-5, which broke booleans for offsets between them |
| Coincident faces are merged and imprinted; kept or dropped by which side the material is on | `FixFoundFaceCoincidence`, keep/delete table (O, `source/SMLib/src/SmMerge.cpp:2110-2274, 3299-3325`); applies to any coincident surface, not only planes | Planes: yes. Coincident curved faces that overlap: refused with "not supported yet" | Partly; `cyl/coaxial-stack` in the corpus tracks it |
| Tangent contacts in surface intersections are traced | Tangent edges intersected by default; open intersection loops closed and gaps merged until stable (O, `SmTopologyIntersector.h:105-128`, `SmMerge.cpp:2920-3102`) | Touching contacts are noted and divide nothing; isolated tangent points inside a crossing curve (equal-radius cross holes) fail to close | Gap; `cyl/steinmetz-*`, `cyl/cross-holes-equal` |
| Non-manifold results (bodies touching at an edge or a point, zero-thickness walls) | Allowed; "manifold solid" is a query (O, `SmBrep.cpp:11268`). **Silent**: no test unions two cubes that share only an edge | Refused with a plain "zero-thickness geometry" error, by design: AnvilCAD's solids are manifold | Different on purpose |
| Nested voids | One body with a void region (O, `source/SmPyLib/tests/test_omni_solid.py:5141`) | Refused ("an empty pocket inside the part"); on the roadmap as sealed hollows | Gap |
| Inputs are left alone on failure | Not transactional: operand A may be modified on failure (O, `source/SM_API/inc/SmApiBrep.h:41-56`) | Every operation takes `&Solid` and returns a new result; inputs cannot change | **Better** |
| Failures say so | Some wrappers report success after a failed stage (O, `SmApiBrep.cpp:1128-1150`, `KNOWN_ISSUES.md`) | Every operation returns `KernelError`, and every returned solid has passed `check::validate` | **Better** |
| Validity checking | `AssertValid`, three levels, roughly 150 named checks (pointers, orientation, gaps between edge, pcurve, vertex and surface, short edges, zero-area faces, loops in UV, shells nested) (O, `SmBrep.cpp:30866-31016, 33396-33403, 34923-34952`) | `check::validate` (closed, oriented, one shell, Euler) plus the evaluation audit (`anvil-evals`: edges on surfaces, mesh closed, outward, normals agree) | Partly; the audit is the evaluation gate, not yet a library check |
| Mass properties | Adaptive integration over each trimmed face, caller-chosen relative accuracy around 1e-6 (O, `source/SMLib/src/SmFace.cpp:5764-6160`) | From the display mesh (`mass::properties`) | Gap: exact integration is a good next step |
| Far-from-origin models | Moved to the origin for the boolean and back (O, `SmMerge.cpp:2356-2386`) | Not moved; the corpus passes at 1e4 and 1e5 mm from the origin | Watch; add if the far cases ever fail |
| Determinism across platforms | Fused multiply-add turned off on aarch64 because it changed marginal booleans (O, `CHANGELOG.md:12`) | Rust does not fuse floating-point operations unless asked, so results match across targets | Matched |

## Test strategy worth matching

SMLib's Python tests check exact values: closed-form volumes (inclusion-exclusion of boxes, the
tricylinder `8(2 - sqrt 2) r^3`), bounding boxes to six places, face, edge and vertex counts,
manifoldness, every operand order, and a 1e-6 axis perturbation
(O, `source/SmPyLib/tests/test_omni_solid.py`). Many of its older C++ boolean tests check only that
the call succeeded and the result is valid (O, `tests/prog_test`); AnvilCAD's evaluations do not
follow that pattern. Every case is compared with an analytic reference: volume, point membership
and rendered pixels (see [EVALS.md](EVALS.md)).

## Detailed notes

The rest of this page records what was read, for whoever extends the comparison.
Topology model (context for everything below). **O**: a radial-edge, non-manifold structure:
Brep > Region > Shell > Faceuse/Loopuse/Edgeuse/Vertexuse, with mate and radial pointers, an
explicit infinite region, void/non-void region flags, wire edges and shell (isolated) vertices
(`source/SMLib/inc/SmBrep.h`, check tables in `source/SMLib/src/SmBrep.cpp:30866-31016`).
"Manifold solid" is a query, not a type: at least 2 regions, no shell vertices, every edge
manifold (`SmBrep::IsManifoldSolid`, `source/SMLib/src/SmBrep.cpp:11268`).

### 1. Boolean operations

### Operations (O)
- `SmBooleanOperationType` (`source/SMLib/inc/SmCoreTypes.h:297-327`): UNION, INTERSECTION,
  DIFFERENCE, EXCLUSIVE_OR, MERGE (combine topology, keep all pieces = partition/non-regularised
  union), PARTIAL_MERGE, IMPRINT (imprint A∩B curves on A and stop), IMPRINT_CLASSIFY (imprint and
  tag faces that would be deleted with `SM_AI_BOOLEAN_DELETED` instead of deleting),
  EXTRACT_SEPARATE (split the merged result into one Brep per connected face set), SLICE (solid
  cut by an oriented sheet; a non-oriented sheet is auto-oriented, a non-sheet operand returns
  `SM_ERR`; `source/SMLib/src/SmMerge.cpp:2537-2546`).
- API wrappers (`source/SM_API/inc/SmApiBrep.h:41-125, 386-430`): `SmApiBoolean`, `...Union`,
  `...Difference`, `...Intersection`, `...Merge`, `SmApiMergeBreps` (n-ary left fold),
  `SmApiBooleanWithCurves` (also returns the intersection edges), `SmApiBoolean2d` (planar
  regions incl. XOR; non-coplanar input rejected before either operand is consumed,
  `tests/SM_API_test/src/TestSmBrep.cpp:231`), `SmApiBooleanWithOptions` (cookie-cutter,
  imprint, post-process, classify, keep-other flags), `SmApiNonManifoldBoolean`,
  `SmApiPiecewiseMerge`, `SmApiBooleanLists`, `SmApiBooleanTrees` (postfix CSG tree).
- Option semantics (`source/SMLib/inc/SmMerge.h:56-104`): cookie-cutter removes from A only and
  "may not be manifold"; post-process (default TRUE in options) removes "topological" edges and
  vertices, i.e. merges faces that share one surface across a non-seam edge after the op.
- Mixed dimension: solids are booleaned against wire Breps and shell vertices
  (`ProcessWiresAndShellVertices`, `SmMerge.h:488`; tests `tests/prog_test/src/smMergeTest.cpp:989`
  "Union/Intersection/Difference of Box and Wires").

### Algorithm outline (O, `source/SMLib/src/SmMerge.cpp:2425-2515`)
1. Intersect every face pair, insert the curves into both topology graphs, relate matching
   pieces (`IntersectInsertRelate`).
2. Classify each face along each intersection edge against the other body's radial sectors.
3. Classify the remaining faces by containing region in the other body.
4. Classify wires and shell vertices. 5. Copy kept faces of B into A. 6. Delete faces.
7. Propagate void/non-void shell status. 8. Re-find the infinite region if needed.
9. Optional post-process face merging. 10-11. Output and delete B as the op dictates.
- If both inputs are far from the origin they are translated to the origin for the operation and
  back afterwards, to cut numerical noise (`SmTemporaryBrepTransform`, `SmMerge.cpp:2356-2386, 2528`;
  the test helper does the same when |bbox centre|^2 > 1e7, `smMergeTest.cpp:2184+`).
- `SmApiBoolean` picks `ManifoldBoolean` when both inputs are manifold solids, otherwise
  `NonManifoldBoolean` (`source/SM_API/src/SmApiBrep.cpp:231-238`).

### Coincident (coplanar / co-cylindrical) faces (O)
- Detected during classification as a face "coincident" to a face of the other body.
  `FixFoundFaceCoincidence` (`SmMerge.cpp:2110-2274`) merges the two surfaces
  (`MergeCoincidentSurfaces`) and projects each face's edges onto the other so the pair shares a
  compatible set of edges; then the pair is related as one.
- Keep/delete rule (`SmMerge.cpp:3299-3325`), with *same side* = the two materials overlap at
  that face, *non-solid* = a sheet involved:
  UNION, INTERSECTION: keep if same side or non-solid, else delete both (opposed faces vanish).
  DIFFERENCE, SLICE: delete if same side and solid, else keep. XOR: delete. MERGE: keep both.
- Coincidence is surface-level, not only planar: `CoverOtherCoincidentSurface` with a tolerance
  (`SmMerge.cpp:1005-1043`). Tests: mirrored copies sharing trimmed coplanar faces, with exact
  volumes and bounds (`source/SmPyLib/tests/test_omni_solid.py:2492`).
- A changelog fix shows a union whose edge overlap lay on two coincident face pairs used to crash
  or be non-deterministic (`CHANGELOG.md:22`).

### Tangent / touching contacts (O unless marked)
- `m_bIntersectTangentEdges` default TRUE: tangent edges are added to the intersection set;
  `m_bIgnoreTangency` makes curve classification "work harder to remove" near-coincident
  tangency cases (`source/SMLib/inc/SmTopologyIntersector.h:105-128`).
- Sphere cut tangent to a planar face yields a face with a one-vertex loop; deleting that face in a
  later intersection was a crash bug, now tested with exact volume and bounds
  (`test_omni_solid.py:2540`, `CHANGELOG.md:24`). Box-union-tangent-cylinder fixtures:
  `TestFiles/pt_TestFiles/Boolean/BoxUCylTan{1,2}.txt` (`smMergeTest.cpp:964`).
- Open intersection loops: a "single vertex" (common vertex with one intersection edge) triggers
  `CloseIntersectionLoop`, then `FixGaps` merges nearby single vertices, iterating until stable;
  only in manifold mode. Afterwards `m_bIntersectionLoopClosed` records whether all loops closed
  (`SmMerge.cpp:2920-3102`). **Silent**: what the op does with an unclosed loop beyond recording it.
- Edge-on-face, vertex-on-face: handled in stitching/merge by `EdgeFaceCoincidence` (imprint an
  edge into a face, possibly splitting it) and vertex merging (`SmTopologyIntersector.h:470-520`).
  **Silent** on a dedicated boolean rule.

### Disjoint, nested, empty, non-manifold results
- Disjoint union (O): one Brep with two material regions, census `{solid 2, void 0}`
  (`test_omni_solid.py:5153`).
- Nested (O): A minus an interior B gives one solid with one void cavity, volume 936 for 10^3-4^3
  (`test_omni_solid.py:5141`); a solid floating in a void merges to nested shells
  (`tests/SM_API_test/src/TestSmBrep.cpp:3641-3650`). The validator requires inner shells to be
  geometrically inside their outer shell (section 3).
- Empty result: 2D booleans may return a Brep with zero faces, which tests accept
  (`test_omni_solid.py:2454`). `SmApiBoolean2d` with an empty operand returns success and does not
  assign the result (`SmApiBrep.h:107`). **Silent** for 3D: no explicit "empty solid" status.
  **I**: a 3D empty intersection returns success with a Brep that has no material region.
- Single edge / single vertex contact: MERGE of overlapping boxes is reported as not a manifold
  solid (`test_omni_solid.py:2618`). `NonManifoldBoolean` allows spine edges (edges with more than
  two faces); the manifold path "will not create spine edges" and may instead leave coincident
  manifold/lamina edges (`SmMerge.cpp:2389-2435`; the two docblocks contradict each other about
  which path does which, so treat with care). **Silent**: no test unions two cubes sharing only an
  edge or a vertex. **I**: the result is a valid non-manifold Brep (radial edge) rather than an
  error, because nothing rejects non-manifold output and `IsManifoldSolid` is just a query.
- Rejected (O): NULL operand, A == B, EXTRACT_SEPARATE through the single-result API all return
  `SM_ERR_INVALID_INPUT` with inputs untouched (`SmApiBrep.cpp:186-200`). `merge_breps` rejects
  empty, singleton, None and duplicate lists without consuming inputs
  (`test_omni_solid.py:2573-2610`).
- Contact query (O): `SmApiBrepRelationship` returns separate / touching / interpenetrating /
  A contains B / B contains A plus gap or wall clearance (`source/SM_API/inc/SmApiQueries.h:55`);
  it samples points, so a shallow overlap with no sample inside reads as touching
  (`KNOWN_ISSUES.md`, "brep_relationship").

### 2. Tolerance model

- Compiled model is the **old per-object model** (`SM_USE_OLDTOL`, `source/SMLib/inc/SmConfig.h:227`).
  A "consistent" global model (one zone tolerance on the context/Brep, derived from a model-size
  estimate) is designed and documented but not enabled (`source/SMLib/inc/SmTol.h:335-505`).
- Three tolerance kinds (O, `SmTol.h:343-383`): **ZoneTol3d** (radius of an object's tolerant
  neighbourhood), **XSectTol3d** = ZoneTol(a)+ZoneTol(b) (two objects intersect if neighbourhoods
  touch; also the minimum distance between distinct points), **ApproxTol3d** = ZoneTol/2 (max
  deviation of approximated geometry). Geometry methods take explicit tolerances; topology code
  fetches them from `SmTol`.
- Defaults, unitless model units (O, `source/SMLib/inc/SmMath.h:38-61`): ZoneTol 1e-5, XSect 2e-5,
  Approx 5e-6, numeric zero 1e-12, knot-parameter zero 1.2e-8, zero angle 1e-4 deg, parallel-vector
  angle 0.5 deg, small-topology ratio 1000 ("pinhole in a battleship": small objects get 1000x
  smaller tolerances when flagged), tolerance gap gain 1.1. Size-estimate table: 0.5-500 -> 1e-5
  default, each decade-pair shifts one order (`SmTol.h:511-521`).
- Old-model rule (O, `SmTol.h:391-407`): vertex tol > edge tol > face tol, so a point near an edge
  end also classifies to the vertex, a point near a face boundary also to the edge.
- Growth / tolerant modelling (O): `SmTol::UpdateObjectTolerance` sets tol = max(1.001*gap,
  existing) and only ever increases ("update only if larger"), for vertex, edge and adjacent face
  (`source/SMLib/src/SmTol.cpp:1225-1330`; some paths use 2.001*gap). `SmBrep::ValidateAndUpdateTolerances`
  grows vertex/edge tols to cover vertex-edge, vertex-face, vertex-pcurve, edge-pcurve, edge-face
  gaps; a bad pcurve with a good 3D edge is fixed by rebuilding the pcurve instead; gaps larger
  than bbox size / 800 are not fixed (`SmBrep.cpp:33407-33436`).
- Known failure of growth (O, `CHANGELOG.md:27`): a stale drop distance doubled an edge tol to 2.4
  and later 16, which made a following DIFFERENCE return no solid. **I**: cap growth and test it.
- Fillet radius below 8x Brep tol (8e-5) is rejected (`CHANGELOG.md:44`).
- Sewing (O): `StitchAndOrient` default tol = 2 x Brep tol, capped at 1e-3 (100 x 1e-5) when the
  caller passes none (`SmBrep.cpp:26934-26960`). `SmApiStitchIntoShell` takes an absolute max
  separation; `SmApiStitchAdvanced` exposes tol, squeeze small edges, split edges at nearby
  vertices, manifold-only gluing (only lamina pairs) and remove sliver faces; all report stitched
  count, remaining lamina edges, max vertex gap, max edge gap (`SmApiBrep.h:513-560`,
  `source/SMLib/inc/SmStitch.h:55-100`).

### 3. Validity checking

Entry points (O): `SmBrep::AssertValid` (levels 0-2; "never changes the Brep except to refresh
cached data", `SmBrep.cpp:35039`), wrapped as `SmApiUsdGeometryValidate` / `bin/brep_geometry_validator`
(`source/SM_API_USD/inc/SmApiUsdGeometryValidate.h`); `SmBrep::ValidatePointers`
(`SmBrep.cpp:31163`); `SmBrep::ValidateAndUpdateTolerances`; `SmBrep::ContainsCoincidentTopology`
(`SmBrep.cpp:30407`); `SmTrimmingTools::CheckFace`; per-class `AssertValid` on Region, Shell,
Face, Loop, Loopuse, Edge, Edgeuse, Vertex. Each failure carries owner type, check name, detail.
Separately, a schema validator checks USD BrepArray data (`TestFiles/brep_validator/Test_BA_*`).

Checks enumerated from the label tables:
- **Brep** (`SmBrep.cpp:34923-34952`): infinite region present, first, and correct; outer shell
  is a face shell and closed; inner shells geometrically inside their outer shell; multi-region
  Brep has faces, no empty region, infinite region has a closed inner shell; region list matches a
  topology traversal; no coincident geometry; warning when every face is its own shell ("send to
  Stitch"); lamina-only Brep has one shell per face; stored tol consistent.
- **Coincident topology** (`SmBrep.cpp:30377-30382`): coincident vertex, edge or face pairs; vertex
  near an edge or face but not connected; edge near a face but not connected (all "send to Stitch").
- **Pointers/orientation** (`SmBrep.cpp:30870-31016`, 147 checks): every use has its mate; mated
  faceuses opposite; outer loopuse SAME, inner loopuses OPPOSITE; edgeuse opposite to its mate;
  even number of edgeuse mates; radial chain closed; loopuse CCW walk closed; tolerance at least
  effective zero; face/edge/vertex tol relative to Brep tol; edge endpoints inside face domain.
- **Gaps** (`SmBrep.cpp:33396-33403`): edge-vertex and vertex-face at each edgeuse end, edge-surface,
  edge-pcurve, pcurve-vertex at start and end, each against tolerance.
- **Edge** (`source/SMLib/src/SmEdge.cpp:6956-6980`): curve present, interval inside the curve's
  natural interval; length >= edge tol (short edges invalid); edgeuses of manifold/spine edges
  are loop uses; radial sector angles increase monotonically over 0-360 deg; all uses map to the
  same two vertices; edge within XSectTol of every adjacent surface; closed edge has one vertex,
  open has two; edge-vertex gaps < edge + vertex tol.
- **Edgeuse / pcurve** (`source/SMLib/src/SmEdgeuse.cpp:5878-5914`): pcurve owned by the use; no
  jump across a periodic boundary; non-zero length; pcurve interval equals the edge interval and
  lies in the curve's interval; radial pair on the same shell with different orientations; surface
  normals consistently point into the sector (orientation consistency across an edge); warning on a
  0-deg sector with opposed normals; loop neighbours joined through one vertex; sharp pcurve turn
  near a pole flagged; stale gap cache flagged; radial pair evaluates to the same binormal point.
- **Face** (`source/SMLib/src/SmFace.cpp:19904-19955`): UV domain non-zero and inside the surface
  domain; every face vertex classifies back to the face; single-edge loop closes within tol;
  consecutive edge end/start gap < 2 x edge tol, loop start/end likewise; surface poles marked by a
  vertex; seams marked by an edge, no illegal seam crossing, suspicious edge near a missing seam;
  zero-area face degenerating to a point or curve; coincident and partly coincident edges or
  vertices in one face; inner loops inside the outer loop and outside each other; loops disjoint
  in UV (no shared vertex, edge or crossing). `CheckFace` (`source/SMLib/src/SmTrimmingTools.cpp:2686-2760`):
  face has loops, loop orientation computable, not reversed, non-zero area. Note it builds missing
  pcurves as a side effect.
- **Loop / Loopuse** (`source/SMLib/src/SmLoop.cpp:1537-1543`, `SmLoopuse.cpp:1455-1460`): loop
  orientation vs surface natural boundary; edge loops have edgeuses, vertex loops none; both
  loopuses use the same edges in matching order; edgeuse gaps <= XSectTol.
- **Vertex** (`source/SMLib/src/SmVertex.cpp:2528-2530`): vertex-face gap <= XSectTol for every
  face; vertexuse pointers consistent. **Shell** (`SmShell.cpp:1516-1550`): shell kind consistent
  with its contents (vertex, wire or face shell).
- **Silent**: no check for surface or face *self-intersection* (beyond UV loop crossing), and no
  explicit "normals point outward" global check; outwardness follows from region void flags plus
  sector-normal consistency. Repair of orientation is `SmApiUnifyNormals` (ray classification).

### 4. Failure handling

- Status codes (O, `source/SMLib/inc/SmMessages.h:30-110`): `SM_SUCCESS` 1000, `SM_ERR` 1001,
  out of memory, null pointer, `SM_ERR_INVALID_INPUT` 1007, assert/assert-valid failure,
  outside domain, not within tolerance, not converging, bad intersections, bad coincident
  vertices, degenerate surface, tessellation failed face, and more. `SER(x)` returns the first
  non-success status up the stack (`SmMessages.h:265, 648`). Python raises `RuntimeError` naming
  the function and status (`test_omni_solid.py:2462-2467`).
- **Not transactional** (O): boolean operand A is modified in place and "remains allocated but may
  be modified on failure"; B is consumed on success; result NULL on failure (`SmApiBrep.h:41-56`).
  Input validation happens before any mutation, and tests assert inputs are untouched for invalid
  input (`test_omni_solid.py:650`, `TestSmBrep.cpp:1044, 1120, 1632, 2177`).
- Swallowed failures (O): `SmApiStitch` ignores `StitchAndOrient`'s status and always returns
  success (`SmApiBrep.cpp:1128-1150`); `heal_brep` can report success when a repair stage failed;
  `stitch_brep_with_face` crashes; `shell_brep` can return a non-solid (`KNOWN_ISSUES.md`).
- Mass properties reject non-solids and non-finite accuracy/density rather than returning
  zeros (`test_omni_solid.py:5329-5440`).

### 5. Other robustness features

- **Healing** `SmApiHealBrep` -> `HealBrep(SM_HO_ALL)` (`source/SM_API/src/SmApiHeal.cpp:27`). Stages
  (`SmBrep.h:1087-1126`, `source/SMLib/inc/SmHealData.h`): fix back-pointers; cache edge, vertex,
  face gap properties; fix tolerance sizes (too small and too large); find/fix coincident vertices
  (bucketed); find/fix degenerate faces (removed) and degenerate edges (deleted); move seam; split
  edges at seams; fix bad sheets; split faces at seams; fix bad loops (add pole vertex loops, flip
  misoriented loops, reorder outer loop first, split multi-outer-loop faces); build pcurves; fix
  infinite region. Coincident edges, missed edge intersections, bad gaps are TODO; uncontained
  edges is a stub. Heal is skipped by the validator because it "can repair the defects being looked
  for, and ... can crash on invalid Breps".
- **Degenerate edges**: poles are represented by one-vertex loops, not degenerate edges; degenerate
  edges are deleted by the healer (`SmHealData.h:210-213, 280-286`, `SmFace.cpp:19938`).
- **Small edges / slivers**: an edge shorter than its tolerance is invalid (`SmEdge.cpp:6965`);
  stitch can squeeze small edges and remove laminar sliver faces (`SmStitch.h:80-95`).
- **Mass properties** (O): `SmApiBrepComputeMassProperties` (area, volume, mass, centroid,
  moments and products of inertia about a given origin) via `SmFace::ComputePreciseProperties`:
  per face, nested adaptive Runge-Kutta ODE integration over iso-curves of the trimmed UV domain,
  volume as the signed region between face and an origin (bbox midpoint), summed over faces
  (`source/SMLib/src/SmFace.cpp:5764-6160`, `source/SM_API/src/SmApiQueries.cpp:801-850`).
  Accuracy is relative and caller-chosen; below 1e-8 not recommended; tests show 1e-6 relative on
  a sphere at 1e-7 requested and a thin half shell within 1e-2 at default (`test_omni_solid.py:5360-5384`).
  Planar faces with straight edges have an exact pyramid special case (compiled only under
  `USE_VOLUMES`). Area-only and volume-only queries skip unneeded integrals.
- Deterministic builds: aarch64 FMA contraction disabled because it changed marginal boolean
  outcomes (`CHANGELOG.md:12`). **I**: cross-platform determinism is a stated goal.

### 6. Test strategy

- `tests/prog_test` (C++ kernel regression, `tests/prog_test/README.md`): 18 suites: topology,
  healing, booleans, offset, fillets, local-ops, sweeps, primitives, tessellation, unit,
  trimmed-surfaces, brep-import, ssi-analytic, ssi-advanced (coincidence, tangency, seams,
  degenerate solutions), cci-advanced, section-advanced, silhouette-advanced, stitch (opt-in).
  Rules: every case must define an expected observable result, keep negative controls, and keep
  the first failure. Many older boolean cases check only success plus `Dump`/`AssertValid`.
- `tests/SM_API_test`: one file per API area (Brep, Booleans bounds example, distance, error
  codes with controlled fake geometry, heal, fillets, intersectors, polygons, primitives,
  surfaces, trimmed surfaces, import/export).
- `source/SmPyLib/tests/test_omni_solid.py` (7.7k lines): exact-value checks: volumes against
  closed forms (inclusion-exclusion for 3 boxes, tricylinder `8(2-sqrt2)`), tight bounding boxes to
  6 places, face/edge/vertex counts, `is_manifold_solid`, material census, operand order, every
  rotation order of an operation, 1e-6 axis perturbation, and that invalid input leaves inputs intact.
- `tests/usd_test`, `tests/SM_API_USD_test`, `tests/python/roundtrip_*`: USD round trip, compared
  line by line on an ASCII dump, with known lossy columns listed; level-2 AssertValid after import.
- `TestFiles/`: `pt_TestFiles` (native `.smb/.smp/.sms` fixtures: Boolean, Fillet (real parts such as bolt, bottle, blade hub),
  Shell, Defeature, Tess, Solids, SSI surface pairs like plane-sphere, torus-torus, two parallel
  planes), `brep_validator` (one USD file per schema defect, numbered BA.000-BA.764, plus
  non-manifold cubes, cube with void, empty), `occt_breps` (31 OCCT `.brep` conversion fixtures:
  seams, sparse pcurves, two-turn cylinders), `usd_TestFiles` (materials, a healer-crash regression).
