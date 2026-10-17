# Roadmap

AnvilCAD is building toward a complete, free and open-source engineering stack: CAD, CAM,
PDM and PLM that work together, that anyone can run, study and extend, and that interoperate
with commercial systems without needing them installed (see
[INTEROPERABILITY.md](INTEROPERABILITY.md)). Items move to the README's "What works today"
when they are in the code and tested.

## CAD

1. Kernel: surfaces of revolution, so revolves, shells and curved-edge fillets build
   exactly; fillets on curved-to-curved edges, spherical corner blends, variable-radius
   fillets, draft; tangential contacts between surfaces; traced intersections through a
   cone's apex.
2. Features: a hole feature with standard sizes (ISO and ANSI clearance, tapped,
   counterbored and countersunk holes), mirror, sweep, loft, multi-body parts, sheet metal
   (base and edge flanges, bends, unfold to a flat pattern).
3. Assemblies with mates; drawings with views, dimensions and title blocks fed by custom
   properties.
4. STEP: assembly placement, colours and names; AP242 product structure and PMI.
5. A scripting and plugin API.

## CAM

Inspired by what integrated CAM systems offer, built from published research. Detailed
plans live in `docs/cam/`.

1. 2.5D milling: adaptive (constant engagement) clearing, facing, contour and profile
   finishing, drilling cycles; a tool and feeds-and-speeds library keyed to the material
   library; setups, work offsets and stock.
2. Feature recognition from the part: holes, pockets, bosses and faces become operations
   automatically, with the rules editable.
3. Simulation: stock removal, gouge checks against the part, holder and shank collisions,
   rapid moves through material.
4. Post processors as shared, declarative data with an authoritative verifier, a
   community registry with human review, and optional AI-assisted edits and AI review of
   edits (advisory only).
5. 3-axis surfacing (parallel, waterline, scallop, pencil), rest machining, then 4- and
   5-axis indexed and simultaneous machining, and turning and mill-turn.

## PDM (product data management)

Open formats and an open API, usable on one laptop or a team server.

1. A vault: versioned, content-addressed storage of parts, assemblies, drawings and their
   derived files, with check-in and check-out, revisions, where-used and references, and
   an audit log. Works offline and syncs.
2. Metadata from custom properties, searchable; part numbering schemes; data cards defined
   in plain files.
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
