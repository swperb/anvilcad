# Fillet corners

What AnvilCAD's fillet feature does where the edges it rounds meet at a vertex, and how it
was worked out. It extends [fillet-curved-edges.md](fillet-curved-edges.md) and
[shell-and-fillet.md](shell-and-fillet.md). It was written for #114.

## How this was studied

The study used published information only. No SOLIDWORKS software, installer, library or
binary was used or analysed, and none is installed on the machine the study was done on.

REA (rea-agents 6.10.1, from the official npm package) was set up for this study with
`npx rea-agents setup --client claude_code`. Following [fillet-curved-edges.md](fillet-curved-edges.md),
each public help page below was to be captured with REA's browser scenario command:

```bash
REA_BROWSER_NO_SANDBOX=true npx -y rea-agents@6.10.1 capture-browser-scenario ./scenario.json --format json
```

`REA_BROWSER_NO_SANDBOX` is needed because the study ran as root in a container, where
Chromium cannot start its sandbox. **None of the captures completed.** The study's network
policy refused `help.solidworks.com` (the proxy answered HTTP 403), and the policy change
that allows it had not reached the proxy when the work went ahead. So there is no REA
Evidence for these pages. What this document records as observed comes from a web search
restricted to `help.solidworks.com`: the search engine's excerpts of the pages, not the
pages read in full. Each such finding should be checked against a REA capture of its page
once the host is reachable. Until then, treat it as a reading of an excerpt.

| Source | Version | How read |
|---|---|---|
| Constant Size Fillets, SOLIDWORKS Design Help, <https://help.solidworks.com/2022/english/solidworks/sldworks/r_constant_size_fillets.htm> | SOLIDWORKS 2022 | search excerpt |
| Fillet PropertyManager, SOLIDWORKS Design Help, <https://help.solidworks.com/2016/english/solidworks/sldworks/hidd_dve_fillet_dlg2.htm> | SOLIDWORKS 2016 | search excerpt |
| Fillet Overview, SOLIDWORKS Design Help, <https://help.solidworks.com/2021/English/SolidWorks/sldworks/c_fillet_overview.htm> | SOLIDWORKS 2021 | search excerpt; also captured with REA for [fillet-curved-edges.md](fillet-curved-edges.md) |
| FilletXpert Overview, SOLIDWORKS Design Help, <https://help.solidworks.com/2022/english/SolidWorks/sldworks/c_FilletXpert_Overview.htm> | SOLIDWORKS 2022 | search excerpt |
| FilletXpert Corner PropertyManager, SOLIDWORKS Design Help, <https://help.solidworks.com/2020/English/SolidWorks/sldworks/HIDD_FILLET_MGR_CORNER.htm> | SOLIDWORKS 2020 | search excerpt |
| Choosing Alternative Fillet Corners, SOLIDWORKS Help, <https://help.solidworks.com/2016/english/SolidWorks/sldworks/t_Choosing_Alternative_Fillet_Corners.htm> | SOLIDWORKS 2016 | search excerpt |
| RoundCorners Property (ISimpleFilletFeatureData2), SOLIDWORKS API Help, <https://help.solidworks.com/2026/english/api/sldworksapi/SolidWorks.Interop.sldworks~SolidWorks.Interop.sldworks.ISimpleFilletFeatureData2~RoundCorners.html> | API Help 2026 | search excerpt |
| FilletXpertMakeCorner Method (IFeatureManager), SOLIDWORKS API Help, <https://help.solidworks.com/2019/English/api/sldworksapi/SolidWorks.Interop.sldworks~SolidWorks.Interop.sldworks.IFeatureManager~FilletXpertMakeCorner.html> | API Help 2019 | search excerpt |

The shapes themselves come from the definition of a rolling-ball round and from general
solid-modelling practice, not from SOLIDWORKS. The closed-form answers used to check them
come from [brep-bench](https://github.com/bmander/brep-bench)'s published cases
`fillet/box-all-edges`, `fillet/notched-cube-inside-corner`, `fillet/octahedron-all-edges`
and `fillet/rectangular-pyramid-all-edges`, which derive each corner from that definition.

The findings below paraphrase the excerpts; no text was copied.

## What the published documentation says

Observed (in the excerpts):

- Where fillets converge at a vertex, the documentation recommends making the larger ones
  first (Fillet Overview).
- A constant size fillet has an option, Round corners, that makes the fillets of adjacent
  edges meet in a smooth transition instead of a sharp junction. It needs at least two
  adjacent edges selected (Constant Size Fillets, Fillet PropertyManager). In the API it
  is a true/false property that applies only when the fillet is not curvature continuous
  (RoundCorners).
- Setback fillets: a vertex is picked with a setback distance for each edge there, where
  the fillets begin to blend into the faces at the corner (Constant Size Fillets).
- The FilletXpert's corner tool works at a vertex where exactly three filleted edges meet.
  Where all three are convex, or all concave, or all have the same radius, only one
  corner shape exists; otherwise there are alternatives to choose from, two edges being
  of one convexity and one of the other (FilletXpert Corner PropertyManager, Choosing
  Alternative Fillet Corners, FilletXpertMakeCorner).

Inferred:

- With every edge at a corner rounded with one radius and bending the same way, there is
  one corner, with no choice for the user. That fits the rolling-ball definition: the ball
  that rolls along each edge comes to rest touching every face at the corner, and the
  corner is the patch of its sphere between the rounds.
- The Round corners option and setbacks are about other cases: fillets that meet where not
  every edge is rounded, and a corner whose transition the user shapes. Neither is part of
  this change.

Unknown:

- The excerpts do not say what shape SOLIDWORKS gives the corner, only that there is one.
  The sphere patch is the rolling-ball definition, not a published statement.
- What SOLIDWORKS does where four or more rounds meet, and where no one ball touches every
  face (the apex of a pyramid on a rectangle).
- Whether the default (Round corners off) changes the corner where every edge is rounded.

## What AnvilCAD does

Inputs: a fillet feature (constant radius `r`, circular profile) and its edges.

- **Which corners are closed.** A corner is closed by the ball where every edge at the
  vertex is among the fillet's edges (with tangent propagation, among the edges it
  reaches), at least three of them. A vertex where some edge is left sharp keeps the
  rounds as they meet, as before.
- **The corner.** The ball of radius `r` that touches every face at the vertex, on the
  material's side at a convex corner and on the empty side at a concave one. Its patch
  closes the corner: each round's cylinder ends on the plane through the ball's centre
  square to its edge, running into the patch tangentially. At a box corner the patch is an
  eighth of the ball, at an octahedron's corner a square patch between four rounds, and at
  an inside corner the empty eighth of the ball is filled.
- **What it needs.** Flat faces and straight edges at the corner, all of its edges convex
  or all concave, and a point at `r` from every face's plane. On a rectangular pyramid's
  apex there is no such point. The rounds would have to split the apex into a short
  ridge, which the exact kernel does not make yet, so the fillet is refused there rather
  than built with the wrong corner, and the part goes to the faceted kernel.
- **It must fit.** Along an edge with a closed corner at each end, the two patches take
  `(c - v) . e` each from the edge's length (`c` the ball's centre, `v` the vertex, `e`
  the edge's direction away from it). Where they would overlap, the fillet is refused and
  says so.
- **Face names.** Each patch is a face of the fillet feature. The corner is named after
  the first selected edge at it, `k`: face `8 + 8k` at that edge's start vertex and
  `9 + 8k` at its end, indices the edge's own round and side faces (`2 + 8k` to `6 + 8k`)
  leave free. An edge reference that stands for several edges can name more corners than
  that; those take the first free index from `8 + 8n` for `n` selected edges.

How it is built: the edges are rounded as before, one run after another, and then one more
exact boolean per corner. The tool is the cone at the ball's centre spanned by the faces'
normals (pointing out of the side being rounded), cut off just past the vertex, less the
ball. It is cut from the part at a convex corner and added at a concave one. Inside that
cone the rounded part is the ball and nothing else, so the boolean changes nothing but the
corner. The vertex is the furthest point of the corner from the centre along the cone's
axis (a small linear programme, worked in `crates/anvil-brep/src/corner.rs`), so the cap
just past it reaches nothing beyond the corner.

## Not done

- A corner where no one ball touches every face (a pyramid's apex on a rectangle) is
  refused: the split corner is not built.
- Corners with convex and concave edges, corners next to curved faces or curved edges,
  fillets of different radii meeting, conic and curvature-continuous profiles, chamfers'
  corners, the Round corners option, setbacks and the FilletXpert's alternative corners
  are not offered. These corners are left as the rounds make them, or refused as above.
- The faceted kernel's fillets keep their own corners.
