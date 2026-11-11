# Roadmap

AnvilCAD is building toward a complete, free and open-source engineering stack: CAD, CAM,
PDM and PLM that work together, that anyone can run, study and extend, and that interoperate
with commercial systems without needing them installed (see
[INTEROPERABILITY.md](INTEROPERABILITY.md)). Items move to the README's "What works today"
when they are in the code and tested.

How CAD, CAM, PDM, PLM and ERP connect into one flow, what runs on the desktop and what on a
self-hosted or managed server, and the order to build them in are described in
[docs/PLATFORM.md](docs/PLATFORM.md). In short: AnvilCAD stays a complete desktop app that
never needs a server; the business systems are an open-source server and web app that
AnvilCAD connects to through panels, so the whole stack feels like one app.

## CAD

### Priorities

The order below comes from comparing AnvilCAD with the other open-source parametric CAD
projects on 2026-11-10, chiefly SolidCraft (Rust, Fusion-style, v0.40 release candidates)
and FreeCAD. SolidCraft has more commands than AnvilCAD today: more sketch entities, sheet
metal, surfacing, joints and motion, and more file formats. Its tracker also has over 500
open bug reports, many of them features that fail with no reason given. AnvilCAD aims to
match that breadth one working slice at a time, and to beat it on reliability, on the
SOLIDWORKS workflow and migration, and on going from design to manufacturing. Re-check the
comparison before relying on it months later (see [docs/POSITIONING.md](docs/POSITIONING.md)).
The work is split into issues tracked in [#104](https://github.com/swperb/anvilcad/issues/104).

1. **Never lose work.** Autosave and crash recovery: after a crash, offer to restore the
   open documents. A test round-trips every saved-file format version, so a file from an
   older release reopens and rebuilds to the same geometry ([#86](https://github.com/swperb/anvilcad/issues/86), [#87](https://github.com/swperb/anvilcad/issues/87)).
2. **Show that it is reliable.** Publish the `anvil-evals` results (corpus, random cases,
   image comparisons and timings) with every release, as a page anyone can read and rerun.
   Every feature failure tells the user why and what to try; none fails with a bare
   "operation failed" ([#89](https://github.com/swperb/anvilcad/issues/89), [#88](https://github.com/swperb/anvilcad/issues/88)).
3. **Sketching.** Splines and ellipses ([#69](https://github.com/swperb/anvilcad/issues/69)),
   then conics ([#102](https://github.com/swperb/anvilcad/issues/102)); text, with emboss and engrave
   ([#62](https://github.com/swperb/anvilcad/issues/62)). Slots, polygons, trim, extend,
   offset, sketch fillet and chamfer, and symmetric and collinear relations are already in
   the sketcher.
4. **Sheet metal.** Base and edge flanges, hems, bends with bend tables, unfold and refold,
   and the flat pattern exported as DXF ([#91](https://github.com/swperb/anvilcad/issues/91), [#92](https://github.com/swperb/anvilcad/issues/92)).
5. **Assemblies.** Subassemblies ([#93](https://github.com/swperb/anvilcad/issues/93)); interference and clearance checks
   ([#66](https://github.com/swperb/anvilcad/issues/66)); dragging components within their
   remaining freedom ([#94](https://github.com/swperb/anvilcad/issues/94)); component patterns and mirrors; limit, width and mechanical mates
   (gear, rack and pinion, screw) ([#95](https://github.com/swperb/anvilcad/issues/95)); choosing which mate breaks when a new one conflicts.
6. **Drawings** with views, sections, dimensions and title blocks fed by custom properties
   ([#70](https://github.com/swperb/anvilcad/issues/70)), exported as PDF and DXF ([#96](https://github.com/swperb/anvilcad/issues/96), [#97](https://github.com/swperb/anvilcad/issues/97),
   [#98](https://github.com/swperb/anvilcad/issues/98)).
7. **A CAM tab in the application**, so a part goes from design, through Check Design, to
   verified G-code in one app (item 1 under CAM below; [#99](https://github.com/swperb/anvilcad/issues/99), [#100](https://github.com/swperb/anvilcad/issues/100)).
8. **A browser build**, so anyone can try AnvilCAD without installing it, running the same
   kernels compiled to WebAssembly ([#101](https://github.com/swperb/anvilcad/issues/101)).

### Also planned

- Kernel: overflow choices and round corners, spherical corner blends, variable-radius
  fillets; shells of freeform faces, sealed hollows (a second shell inside a solid), and
  walls thick enough to change a part's shape; freeform faces that touch other surfaces
  without crossing them, and results that would touch themselves at a single point.
- Features: more hole options (up-to-surface and offset end conditions, far-side and
  under-head countersinks, slots, tapered taps, holes on curved faces); modelled and
  cosmetic threads; coil and pipe; offset, replace and split face; plastic-part features:
  mounting bosses ([#65](https://github.com/swperb/anvilcad/issues/65)), lips and grooves
  ([#64](https://github.com/swperb/anvilcad/issues/64)) and snap-fit hooks
  ([#63](https://github.com/swperb/anvilcad/issues/63)); moldability checks
  ([#68](https://github.com/swperb/anvilcad/issues/68)).
- Surfacing: surface loft, sweep, extrude and revolve, boundary and patch surfaces, trim,
  extend, stitch, offset and thicken, with zebra, curvature and draft analysis.
- Files: STEP assembly placement, colours and names; AP242 product structure and PMI;
  IGES in and out; glTF and mesh import; FreeCAD and Rhino import.
- SOLIDWORKS: native `.sldprt` geometry reading from a `docs/re/` specification, and
  migration of assemblies and drawings.
- A scripting and plugin API that reaches every feature
  ([#71](https://github.com/swperb/anvilcad/issues/71)); an MCP server that can drive the
  running app so a user can watch an agent work ([#103](https://github.com/swperb/anvilcad/issues/103)).

## CAM

Inspired by what integrated CAM systems offer, built from published research. Detailed
plans live in `docs/cam/`. A first library (`crates/anvil-cam`) does 2.5D milling from
recognised features to verified, simulated G-code; see the README. Next:

1. A CAM tab in the application: setups, stock, the operation list, tool library, tool
   path display, simulation playback with findings shown on the part, posting.
2. 2.5D milling: faster adaptive clearing with fewer links, trochoidal passes for narrow
   channels, rest machining with smaller tools, open slots and steps, spot drilling,
   tapping and boring, hole-making chains.
3. Feature recognition: open slots and steps, counterbores and countersinks, floor
   fillets and chamfers, tapped holes, features facing other directions for indexed
   setups, with the rules editable.
4. Post processors: the community registry service with signed index releases, pinning
   and reporting; more example posts and verifier coverage; a post editor that shows the
   diff and the certificate.
5. 3-axis surfacing (parallel, waterline, scallop, pencil), holder-aware tool selection,
   tool-maker cutting data, then 4- and 5-axis indexed and simultaneous machining, and
   turning and mill-turn.

## PDM (product data management)

Open formats and an open API, usable on one laptop or a team server.

1. A vault: versioned, content-addressed storage of parts, assemblies, drawings and their
   derived files, with check-in and check-out, revisions, where-used and references, and
   an audit log. Works offline and syncs.
2. Metadata from custom properties, searchable; part numbering schemes; data cards defined
   in plain files. The starting point exists: `anvil-props` converts SOLIDWORKS custom
   properties into `.anvilprops.json` files and a company-wide `properties.csv` index.
3. Life-cycle states (work in progress, in review, released, obsolete) with transitions,
   permissions and approvals.
4. An open REST API (OpenAPI described), command-line tools and file-based configuration,
   so other systems and scripts can integrate.
5. Interoperability: import from and export to SOLIDWORKS PDM through its published API,
   using the customer's own licensed client; STEP AP242 packages with metadata for exchange
   with any system.

## PLM (product life-cycle management)

Builds on the PDM.

1. Product structure and bills of materials: engineering BOM and manufacturing BOM,
   variants and options, effectivity.
2. Change management: change requests and change orders that tie edits to approvals and
   release.
3. Requirements and verification links, quality records (nonconformance and corrective
   action), suppliers and approved parts.
4. Manufacturing hand-off: CAM programs, setup sheets and inspection plans released with
   the parts they belong to.
5. Interoperability: OASIS OSLC links, ISO 10303-239 (PLCS) and STEP AP242 exchange, and the
   3DEXPERIENCE platform's published web services for customers who use it, with their own
   licences and credentials.

## ERP and the shop floor

Builds on the PLM, as phases 4 and 5 of [docs/PLATFORM.md](docs/PLATFORM.md).

1. Manufacturing hand-off to existing ERPs: released items, BOMs and routings sent to
   ERPNext and Odoo first, then others, and generic CSV and REST for the rest.
2. For shops with no ERP: inventory and stock locations, purchasing and suppliers, work
   orders from routings, costing from material, machine time and tooling, and quotes.
3. A shop-floor view: work instructions and setup sheets on a tablet, G-code downloads for
   released programs only, and results (quantities, times, measurements) recorded against
   the work order.
4. Accounting, payroll and tax stay out of scope; connect to the systems that do them.

## Deployment: local, self-hosted, managed

1. Local: the vault in a folder, inside AnvilCAD, with no server and no account.
2. Self-hosted: the server as one container or binary, SQLite or PostgreSQL, sign-in through
   the organisation's own identity provider, backups and upgrades that take one command.
3. Managed: the same server run as a service, with every record exportable in open formats
   and a one-step move to self-hosting.
