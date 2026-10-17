# Roadmap

AnvilCAD is building toward a complete, free and open-source engineering stack: CAD, CAM,
PDM and PLM that work together, that anyone can run, study and extend, and that interoperate
with commercial systems without needing them installed (see
[INTEROPERABILITY.md](INTEROPERABILITY.md)). Items move to the README's "What works today"
when they are in the code and tested.

## CAD

1. Kernel: fillets on edges between faces not round about a common axis (freeform
   rolling-ball rounds), tangent propagation to unselected edges, overflow choices and
   round corners, spherical corner blends, variable-radius fillets, draft; shells of freeform
   faces, sealed hollows (a second shell inside a solid), and walls thick enough to
   change a part's shape; freeform faces that touch other surfaces without crossing them, and
   results that would touch themselves at a single point.
2. Features: a hole feature with standard sizes (ISO and ANSI clearance, tapped,
   counterbored and countersunk holes), mirror, sweep, loft, multi-body parts, sheet metal
   (base and edge flanges, bends, unfold to a flat pattern).
3. Assemblies with mates; drawings with views, dimensions and title blocks fed by custom
   properties.
4. STEP: assembly placement, colours and names; AP242 product structure and PMI.
5. A scripting and plugin API.

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
