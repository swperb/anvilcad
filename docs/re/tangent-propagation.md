# Tangent propagation

What AnvilCAD's fillet and chamfer features do with edges that run smoothly on from the
ones selected, and how it was worked out. It extends
[fillet-curved-edges.md](fillet-curved-edges.md).

## How this was studied

REA (rea-agents 6.5.0) was run on published SOLIDWORKS information only, as described in
[fillet-curved-edges.md](fillet-curved-edges.md): each page captured with REA's browser
scenario command in a temporary headless Chromium profile (the browser was Google Chrome),
recording the page's accessibility tree as Evidence. No SOLIDWORKS software, installer,
library or binary was used or analysed.

| Source | Version | REA Evidence |
|---|---|---|
| Constant Size Fillets, SOLIDWORKS Design Help, <https://help.solidworks.com/2022/english/solidworks/sldworks/r_constant_size_fillets.htm> | SOLIDWORKS 2022 SP05 | `ev_3af5195c3df936a5a035767e2c1b9eb7625b49629a4721b8e4432b2cfa644a87` |
| Face Fillets, SOLIDWORKS Design Help, <https://help.solidworks.com/2019/English/SolidWorks/sldworks/r_Face_Fillets.htm> | SOLIDWORKS 2019 | `ev_92874d12cb9d4541be5492984be033e89bfe35c987383b03275cd04d375e7b5f` |
| Example: Tangent Propagation, SOLIDWORKS Design Help, <https://help.solidworks.com/2022/english/SolidWorks/sldworks/c_example_tangent_propagation.htm> | SOLIDWORKS 2022 | `ev_6564c977396ec20750e659a4a742c04cc76b31b7bf30fffc924ddf4fd2bb9254` |
| Chamfer PropertyManager, SOLIDWORKS Design Help, <https://help.solidworks.com/2021/English/SolidWorks/sldworks/r_chamfer_pm.htm> | SOLIDWORKS 2021 | `ev_a1506bf3486d15f9c128bc75f847876d2899a172ced50eecdb46808171c20969` |
| PropagateToTangentFaces Property (ISimpleFilletFeatureData2), SOLIDWORKS API Help, <https://help.solidworks.com/2022/english/api/sldworksapi/SolidWorks.Interop.sldworks~SolidWorks.Interop.sldworks.ISimpleFilletFeatureData2~PropagateToTangentFaces.html> | API Help 2022 SP05; property since SOLIDWORKS 2001Plus | `ev_bc3c1042e493de0d7e9fe3d966fdb986779813452fb52f1e3e53e757ea0499f6` |
| IChamferFeatureData2 Interface Members, SOLIDWORKS API Help, <https://help.solidworks.com/2022/english/api/sldworksapi/SolidWorks.Interop.sldworks~SolidWorks.Interop.sldworks.IChamferFeatureData2_members.html> | API Help 2022 SP05 | `ev_ddfca76df5b736827ce465ec565dc66c5837bda8fb2357d1d0caf6c1a6b72c4a` |

The findings below paraphrase those pages; no text was copied.

## What the published documentation says

Observed:

- Fillets have a tangent propagation option that extends the fillet to every face tangent
  to the selected one (Constant Size Fillets, Face Fillets). The API's fillet data has a
  matching on/off property, PropagateToTangentFaces: extend the fillet to all faces
  tangent to the selected face or edge.
- Chamfers have the same option, extending the chamfer to faces or edges tangent to the
  selection (Chamfer PropertyManager); the API's chamfer data has TangentPropagation.
- The example page starts from a block with one edge selected, marks the edges tangent to
  it, and shows the result with the option cleared and with it selected. Its content is
  pictures with captions; the captions say no more than that.

Silent or ambiguous:

- Whether the option is on by default is not stated on these pages.
- How far propagation goes is not defined beyond "tangent": whether it follows edges
  through a chain of tangent joins (AnvilCAD does), whether it crosses from a convex edge
  to a concave one, and what happens where a chain branches.
- What a part saved without the setting should do (the setting is not optional in the
  API, so the question does not arise there).

## What AnvilCAD does

- Fillet and chamfer features have a tangent propagation setting. New features have it on
  (AnvilCAD's choice; the pages do not give a default). Parts saved before the setting
  existed load with it off, so they rebuild as they did. The migrator reads
  PropagateToTangentFaces for fillets and TangentPropagation for chamfers.
- With it on, the edges blended are the selected ones and every edge reached from them
  through ends where one edge runs smoothly (with the same direction) into the next,
  provided that edge is a crease (its two faces are not tangent there) bending the same
  way, convex or concave, as the edge it is reached from. A propagated edge is blended
  in the same step as the selected edge it runs on from, so the blend is one smooth run.
  Its faces are named after that selected edge.
- The common case: after one edge of a block is rounded, its neighbour runs smoothly on
  into the round's end. Rounding the neighbour with propagation carries the round on round
  the first round's end and down the next straight edge, instead of stopping with a
  step. Smaller than the first round, the corner is a torus; equal to it, a sphere;
  between half of it and all of it, the torus would cut through itself, so the run is
  swept by the rolling ball instead. A round larger than the first cannot roll round it
  and is refused.
- Propagation needs the exact kernel. A part already handed to the faceted kernel blends
  the selected edges only; the setting's tooltip says so.
- Where the closed-form tools of a run cannot be joined or applied exactly (the chamfer
  cones and prisms round a slot's outline meet along a hyperbola, which the exact booleans
  do not yet trace), the run is swept instead, as a rolling-ball round or bevel.
