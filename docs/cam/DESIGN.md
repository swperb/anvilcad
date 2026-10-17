# AnvilCAD CAM: design

`crates/anvil-cam` is AnvilCAD's computer-aided manufacturing library. This first version
covers 2.5D milling from the top of the part: facing, adaptive clearing, contour finishing
and drilling, planned from features recognised on the part, written for a machine by a
declarative post processor, and checked by a G-code verifier and a stock simulator before
it is called safe. It is a library with tests; there is no user interface for it in the
application yet.

It is inspired by what integrated CAM systems such as CAMWorks and SolidCAM offer
(feature recognition, a technology database, automatic operation plans, simulation), and
built from published methods. See [ADAPTIVE.md](ADAPTIVE.md) for the clearing algorithm and
the patents checked, [POSTS.md](POSTS.md) for post processors, the verifier and the
registry, and [ROADMAP.md](ROADMAP.md) for what comes next.

## Modules

| Module | What it does |
|---|---|
| `toolpath` | The machine-independent tool path: rapid, line and arc (helical) moves tagged by purpose (cut, entry, link, lead, retract), feeds, spindle, coolant, tool changes, dwell, drilling cycles. Expansion of cycles into moves, exact lengths and feed time. |
| `geom` | Regions (outer loop with islands), loops of lines and arcs, exact point-to-boundary distance with a bucket index, and offsets through `cavalier_contours` (guarded so a degenerate shape is an error, not a crash). |
| `raster` | The material raster used by adaptive clearing and its verifier, and an exact Euclidean distance transform (Felzenszwalb and Huttenlocher). |
| `adaptive` | Engagement-limited clearing and the independent engagement verifier. |
| `facing` | Parallel passes across the stock top, entering from outside the stock, zig-zag or one way, in step-downs. |
| `profile` | Contour finishing with cutter radius compensation done in CAM (the program holds tool-centre coordinates and needs no G41/G42), arcs kept exact, tangent lead-in and lead-out arcs placed only where they clear the walls, several radial passes and step-downs, climb milling. |
| `drill` | Drilling cycles from holes: drill, chip-break and full-retract peck by depth, through holes past the bottom by the drill point plus 1 mm, holes grouped and ordered nearest-first. |
| `features` | Hole and pocket recognition from the part body. |
| `tool` | Tools (flat, ball, bull nose, drill, spot drill), holders, tool library, cutting data by material group, chip-thinning factor. |
| `job` | Jobs, setups (work offset, program zero, stock, safe height), operation lists, building a setup into one tool path, and an automatic operation plan from the part. |
| `sim` | Height-field stock simulation with gouge, rapid, link, shank and holder checks. |
| `post` | Post processor definitions and the G-code generator. |
| `verify` | The authoritative G-code verifier. |
| `safety` | Certification: verifier plus simulator on the program text. |
| `registry` | Content hashes, version pins, the deterministic review of post edits, and how AI findings combine with it. |
| `ai` | Optional (`ai` feature): AI-proposed post edits and AI review of edits through the Claude API. |

Everything is in millimetres, the tool axis is +Z, and coordinates in a setup are program
coordinates (part coordinates less the setup's origin). Public functions return
`CamError` for bad input instead of panicking; the crate forbids `unsafe`, denies `unwrap`
and warns on `expect` and `panic`, like the exact kernel.

## Feature recognition

`features::recognize` reads the faceted part body (`anvil_kernel::Solid`), whose polygons
carry their face's name and exact surface. The rules are few, so they can be checked:

- **Holes**: a cylindrical face whose axis is parallel to Z, whose material is outside it
  (its facets face the axis) and which goes all the way round. It is through when a ray
  down the axis from just below the face leaves the part, and blind otherwise, with the
  depth to the bottom of the cylinder. Coaxial cylinders of different sizes are separate
  holes (the larger are counterbores).
- **Pockets**: every upward-facing planar face below the top of the part is a floor. The
  part is sliced 0.1 micrometres above each floor height (slices are chained exactly, with
  each edge crossing computed the same way from both sides, and fall back to the nearest end within a nanometre
). A floor inside a loop of the slice that bounds empty space is a closed pocket:
  that loop is its outline and the loops directly inside it are islands. A floor outside
  every loop is an open floor (a step), cleared from the stock outline around the section.
  Loops that are recognised holes are left to drilling. The pocket's top is the top of its
  walls; walls that are not vertical are reported, since a 2.5D path cannot follow them.

Not yet recognised: slots open on one side as their own feature (they appear as open
floors), chamfers and fillets between floor and wall, threads, and features on faces not
facing +Z.

## Technology database

Tools have a shape, diameter, flutes, flute length, shank, stick-out and holder (a stack of
cylinders). Cutting data come from the part's material in the library
(`anvil_model::material`), mapped to eight machinability groups (aluminium, copper alloys,
low-carbon steel, alloy steel, stainless, titanium, soft and hard plastics; unknown metals
fall back to alloy steel and unknown plastics to hard plastic). Each group has a surface
speed for carbide and for high-speed steel and a chip load as a fraction of the diameter.
The spindle speed is `Vc * 1000 / (pi * D)` capped by the machine, the feed is
`fz * teeth * rpm`, plunges run at 30% of the feed, and drills use a feed per revolution.
These are AnvilCAD's own conservative starting values, inside the ranges tool makers
publish, and are labelled as such; a tool maker's figures for the actual tool govern.

## Automatic operation plan

`job::plan_operations` proposes, from the part and the stock:

1. face the stock down to the top of the part if the stock is taller, with the largest end
   mill;
2. for each closed pocket with vertical walls: adaptive clearing with the largest end mill
   no wider than 0.6 times the pocket's narrowest gap (estimated from the largest inscribed
   circle and the gaps between islands and the outer wall), leaving 0.2 mm on the walls,
   then a wall finishing pass with the same tool;
3. drill each set of holes with a drill within 0.05 mm of their diameter.

Open floors and pockets with drafted walls are reported but not machined yet. The plan is
editable: it is a plain list of operations.

## Setups

A setup names a work offset (G54 to G59), places program zero on the part, describes the
stock as a block, sets a safe height and lists operations. Building it inserts the tool
changes, spindle starts and coolant, and shifts every operation into program coordinates.
More than one setup per job is supported as data; each is built and posted on its own.

## Simulation

`sim` models the stock and the finished part as height fields (material up to a height in
each cell), which is exact for what 2.5D and 3-axis machining from +Z can make. The tool
(flat, ball, bull nose or drill profile, then shank, then holder) is swept along every move
in steps of half a cell, lowering the stock under its cutting envelope. It reports, with the
tool position, the move and the depth: gouges into the part, rapids through stock, links
that cut, material above the flutes against the shank, and holder contact with stock or
part. Features narrower than a cell can be missed, so the cell should be well under the tool
radius (the tests use 0.25 mm).

## Dependencies and licences

Default build (pure Rust), all permissive:

| Crate | Licence | Used for |
|---|---|---|
| `cavalier_contours` 0.9 | MIT OR Apache-2.0 | Offsets of loops with arcs |
| `toml` 1.1 | MIT OR Apache-2.0 | Post definitions |
| `sha2` 0.11 | MIT OR Apache-2.0 | Content hashes |
| `thiserror` 2 | MIT OR Apache-2.0 | Error type |
| `serde`, `serde_json` | MIT OR Apache-2.0 | Data and the AI request bodies |

Their own dependencies are MIT, Apache-2.0, Zlib (`foldhash`), Unlicense OR MIT
(`memchr`), BSD-2-Clause OR Apache-2.0 OR MIT (`zerocopy`) and Unicode-3.0 (`unicode-ident`,
with MIT OR Apache-2.0).

With the optional `ai` feature: `ureq` 3 (MIT OR Apache-2.0) with `rustls` (Apache-2.0 OR
ISC OR MIT), `ring` (Apache-2.0 AND ISC; contains C and assembly, so this feature is not
pure Rust), `rustls-webpki` and `untrusted` (ISC), `webpki-roots` (CDLA-Permissive-2.0),
`subtle` (BSD-3-Clause), `bytes` (MIT) and others under MIT OR Apache-2.0.

No GPL or LGPL code was read or used. FreeCAD's CAM workbench, libarea, OpenCAMLib and
PyCAM were considered from their public descriptions and licences only.

## Testing

`cargo test -p anvil-cam` (add `--features ai` for the AI module's mocked tests). Besides
the unit tests for each module, `tests/end_to_end.rs` takes a part with a pocket, an island
and a through hole from solid to certified programs with all three bundled posts, checks
that a cutting move turned into a rapid fails certification, and that a flat-bottomed blind
hole is reported as a gouge.
