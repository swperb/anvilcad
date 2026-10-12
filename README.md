# AnvilCAD

Free and open source parametric 3D CAD, written in Rust, with its own geometry kernel.
The workflow follows SOLIDWORKS conventions: a FeatureManager tree, sketches on planes or
faces, fully defined sketches, extrudes and revolves, equations, custom properties and
templates.

![AnvilCAD showing the sample part](docs/screenshot.png)

Status: early (0.1). The core modelling loop works end to end. Large parts of a full CAD
system are still ahead; see the roadmap.

## What works today

**Sketching**
- Line (chained), centerline, rectangle, circle, centerpoint arc
- Snapping to existing points and the origin, automatic horizontal/vertical relations
- Relations: coincident, horizontal, vertical, parallel, perpendicular, equal, tangent,
  concentric, midpoint, point on line/circle, fix
- Smart Dimension: length, diameter, radius, point distance, angle
- Constraint solver with degree-of-freedom reporting (under / fully / over defined, with
  conflicting relations highlighted)
- Drag points and circles while relations stay satisfied
- Construction geometry

**Features**
- Extruded Boss/Base and Extruded Cut: Blind, Mid Plane, Through All, Through All - Both,
  reverse direction
- Revolved Boss and Revolved Cut about any sketch line
- Linear Pattern and Circular Pattern (about any axis)
- Fillet (constant radius) and Chamfer (equal distance) on straight edges between flat
  faces and on circular edges where a flat face meets a cylinder (boss tops, hole rims,
  boss roots); convex edges are rounded off, concave edges are filled in
- Shell to a uniform wall thickness, with any faces opened
- Reference planes offset from planes or faces
- Sketch on Front/Top/Right planes, reference planes, or any planar model face
- Edit, suppress, rename, delete (with dependents), roll back

**Parameters and properties**
- Global variables with equations (`width = 80`, `depth = width / 2`, functions like
  `sqrt`, `sin` in degrees, `min`, `max`); type `=width` into any dimension to link it
- Material library with mass, volume, surface area, center of mass, bounding box
- Custom properties (text, number, date, yes/no)
- Part templates (`.anvprtdot`)

**Files**
- Native part format `.anvprt` (versioned JSON, written atomically)
- Export to STEP AP214, STL (binary) and OBJ
- SOLIDWORKS files are identified and explained (see below)

**Application**
- Undo/redo for every change, unsaved-changes prompts
- Shaded view with edges, standard views, zoom to fit, face highlighting and picking

## Migrating from SOLIDWORKS

`anvil-migrate` converts SOLIDWORKS parts to AnvilCAD parts with their feature tree:
sketches with relations and dimensions, extrudes, cuts, revolves, patterns, reference
planes, global variables and equations, material and custom properties.

It runs on a Windows machine with SOLIDWORKS installed. SOLIDWORKS reads each file through
its public API, which is the same approach commercial history-based translators use, so
every release SOLIDWORKS can open is supported. Files are opened read-only and never saved.
If SOLIDWORKS is already running, the tool attaches to it and leaves any documents you have
open untouched.

```bash
anvil-migrate -o D:\migrated D:\vault\projects
```

How a part is translated:

1. Features are read in tree order. Sketches keep their geometry, relations and driving
   dimensions; sketches on faces stay attached to the AnvilCAD face where possible.
2. After each solid feature, SOLIDWORKS is rolled back to that point and its exact volume is
   compared with AnvilCAD's rebuild. When a translation does not match, plausible variants
   are tried (for example the opposite direction).
3. Features AnvilCAD cannot rebuild yet (sweeps, lofts, variable-radius fillets and so on), or whose
   translation does not match, become an **imported body**: SOLIDWORKS' own geometry at that
   point in the tree. Translation then continues parametrically on top of it, so later
   sketches, cuts and patterns stay editable.
4. Global variables become AnvilCAD variables, and equations driving dimensions are linked
   when they can be expressed (otherwise the value is kept and the report says so).

Every file gets a status in `migration-report.csv` / `.json`:

| Status | Meaning |
|---|---|
| Parametric | Every geometric feature was rebuilt as an editable AnvilCAD feature |
| Hybrid | Some features came across as imported geometry; the report lists which and why |
| Failed | The file could not be read |
| Skipped | Assemblies and drawings (not migrated yet) |

`verified` means the migrated part's volume matches SOLIDWORKS within 1%. Imported
geometry is a fine triangle mesh, so curved imported faces carry a small faceting error;
`--precise-geometry` tightens it at the cost of speed.

Limitations today: parts only (assemblies and drawings are next), the active configuration
only, and edits to features above an imported body do not flow through that body.

Native files (`.sldprt` etc.) are also recognised directly by the AnvilCAD app, which
explains how to migrate them.

## Architecture

```
crates/
  anvil-kernel   geometry kernel: math, profiles, triangulation, B-rep solids,
                 booleans (BSP), extrude/revolve, mass properties, picking
  anvil-sketch   2D sketcher: entities, constraints, solver, profile extraction
  anvil-model    parametric part document: feature tree, rebuild, equations,
                 materials, custom properties, file format
  anvil-io       STEP / STL / OBJ export, SOLIDWORKS file identification
  anvil-app      desktop application (egui + OpenGL)
  anvil-migrate  anvil-migrate: SOLIDWORKS to AnvilCAD converter (Windows; talks to
                 SOLIDWORKS through late-bound COM automation)
```

Kernel design notes:
- Solids are boundary representations made of planar convex polygons. Every polygon
  carries a face tag naming the model face it belongs to (feature id plus a stable index
  derived from the sketch entity) and the exact surface it lies on (plane, cylinder,
  surface of revolution).
- Booleans use BSP trees (iterative, no recursion limits), then T-junctions are repaired so
  results are watertight.
- Curved faces are faceted at 0.01 mm chord tolerance. Exact curved-surface booleans are the
  main kernel milestone ahead.
- The sketch solver minimises constraint residuals with a damped minimum-norm Gauss-Newton
  method, so under-defined sketches move as little as possible when edited.

## Building

Linux and macOS: `cargo run --release -p anvil-app`

Windows with MSVC (Visual Studio Build Tools installed): `cargo run --release -p anvil-app`

Windows without Visual Studio (GNU toolchain):

```bash
rustup toolchain install stable-x86_64-pc-windows-gnu
winget install BrechtSanders.WinLibs.POSIX.UCRT   # GNU binutils
bash scripts/setup-windows-gnu.sh                 # one time
bash scripts/build.sh run --release -p anvil-app
```

Run the tests with `cargo test --workspace` (or `bash scripts/build.sh test --workspace`).
The app crate's tests drive the real UI with simulated mouse input, so sketching,
dimensioning, dragging, face picking, extruding and undo are covered without a window.

Command line: `anvilcad [FILE.anvprt | --sample] [--screenshot OUT.png] [--edit-sketch ID]`

## Using it

| Action | Input |
|---|---|
| Rotate | Middle mouse drag, or arrow keys |
| Pan | Ctrl + middle drag, or right drag |
| Zoom | Mouse wheel (about the cursor), Shift + middle drag |
| Standard views | Ctrl+1 Front, 2 Back, 3 Left, 4 Right, 5 Top, 6 Bottom, 7 Isometric, 8 Normal To |
| Zoom to fit | F |
| Undo / Redo | Ctrl+Z / Ctrl+Y |
| Save / Open / New | Ctrl+S / Ctrl+O / Ctrl+N |
| Rebuild | Ctrl+B |
| Cancel tool / clear selection | Esc |
| Delete selection | Delete |

Typical flow: select a plane in the tree (or click a planar face), Sketch tab > Sketch,
draw and dimension, Exit Sketch, then Features > Extruded Boss/Base. Help > Open Sample
Part loads a model that uses most features.

## Roadmap

1. Kernel: exact NURBS/analytic B-rep with exact booleans; fillets on curved-to-curved
   edges, spherical corner blends, variable-radius fillets, draft
2. More features: holes (Hole Wizard style), mirror, sweep, loft, multi-body parts
3. Assemblies with mates; drawings with views, dimensions and title blocks fed by
   custom properties
4. STEP import; then SOLIDWORKS format research for the five-year compatibility window
5. A scripting and plugin API

## License

MIT. See [LICENSE](LICENSE).
