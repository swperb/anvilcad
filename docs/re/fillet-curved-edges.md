# Fillets on curved edges

What AnvilCAD's fillet feature should do on edges that are not straight, and how it was
worked out. It extends [shell-and-fillet.md](shell-and-fillet.md).

## How this was studied

REA (rea-agents 6.5.0) was run on published SOLIDWORKS information only. No SOLIDWORKS
software, installer, library or binary was used or analysed, and none is installed on the
machine the study was done on.

Each page was captured with REA's browser scenario command, which loads the public page in
a temporary headless Chromium profile (Chromium 155.0.8059.84, the browser was Google
Chrome) and records the page's accessibility tree as Evidence:

```bash
npx -y rea-agents@6.5.0 capture-browser-scenario ./scenario.json --format json
```

with a scenario of the form

```json
{
  "browser": { "mode": "launch", "executable_path": "<Google Chrome>" },
  "start_url": { "url": "<page>" },
  "actions": [ { "step_id": "settle", "action": "wait_for_timeout", "duration_ms": 3000 } ],
  "capture": { "at_end": ["accessibility", "url"] }
}
```

| Source | Version | REA Evidence |
|---|---|---|
| Constant Size Fillets, SOLIDWORKS Design Help, <https://help.solidworks.com/2022/english/solidworks/sldworks/r_constant_size_fillets.htm> | SOLIDWORKS 2022 SP05 (page footer) | `ev_3af5195c3df936a5a035767e2c1b9eb7625b49629a4721b8e4432b2cfa644a87`, capture complete |
| Fillet Overview, SOLIDWORKS Design Help, <https://help.solidworks.com/2021/English/SolidWorks/sldworks/c_fillet_overview.htm> | SOLIDWORKS 2021 | `ev_90b89e9aee2e27ccd19a0fdde662af591f4ec46578357648a0c9ea1397ce057c`, capture complete |
| Face Fillets, SOLIDWORKS Design Help, <https://help.solidworks.com/2019/English/SolidWorks/sldworks/r_Face_Fillets.htm> | SOLIDWORKS 2019 | `ev_92874d12cb9d4541be5492984be033e89bfe35c987383b03275cd04d375e7b5f`, capture complete |
| Conic Fillets, What's New in SOLIDWORKS, <https://help.solidworks.com/2014/english/WhatsNew/c_conic_fillets.htm> | SOLIDWORKS 2014 | `ev_d16710332fae420f094cc76fdd00da1b21d5a1311f327043551391d0798bf72c`, capture complete |
| Chamfer PropertyManager, SOLIDWORKS Design Help, <https://help.solidworks.com/2021/English/SolidWorks/sldworks/r_chamfer_pm.htm> | SOLIDWORKS 2021 | `ev_a1506bf3486d15f9c128bc75f847876d2899a172ced50eecdb46808171c20969`, capture complete |
| Chamfer Enhancements, What's New in SOLIDWORKS, <https://help.solidworks.com/2017/english/WhatsNew/c_chamfer_enhancements.htm> | SOLIDWORKS 2017 | `ev_2fd007313bc314c69549dd49f721615d298a00c695e3e57becf47db9ead0bc1c`, capture complete |
| swFeatureFilletProfileType_e Enumeration, SOLIDWORKS API Help, <https://help.solidworks.com/2022/english/api/swconst/SOLIDWORKS.Interop.swconst~SOLIDWORKS.Interop.swconst.swFeatureFilletProfileType_e.html> | API Help 2022 SP05 (page footer) | `ev_239e1c12fa4aca1d046ebd8dc6705f7b0c15b9c68d45e3e2e545d8b79f815712`, capture complete |
| ConicTypeForCrossSectionProfile Property (ISimpleFilletFeatureData2), SOLIDWORKS API Help, <https://help.solidworks.com/2022/english/api/sldworksapi/SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.ISimpleFilletFeatureData2~ConicTypeForCrossSectionProfile.html> | API Help 2022; property since SOLIDWORKS 2014 FCS | `ev_84cbc022e0483b05714c1fdd92538229abc786a810cd444b393c7e322a3a3d5d`, capture complete |
| CurvatureContinuous Property (ISimpleFilletFeatureData2), SOLIDWORKS API Help, <https://help.solidworks.com/2022/english/api/sldworksapi/SolidWorks.Interop.sldworks~SolidWorks.Interop.sldworks.ISimpleFilletFeatureData2~CurvatureContinuous.html> | API Help 2022; property since SOLIDWORKS 2001Plus FCS | `ev_a7b3c9b7e9562cba4fe41b8a890e5ea151f3c4015c21091a5bea27daef3db4d9`, capture complete |
| ISimpleFilletFeatureData2 Interface Members, SOLIDWORKS API Help, <https://help.solidworks.com/2022/english/api/sldworksapi/SolidWorks.Interop.sldworks~SolidWorks.Interop.sldworks.ISimpleFilletFeatureData2_members.html> | API Help 2022 | `ev_f4ab1471d1c700cef30aeeedd5795caaf6814abdbfd3d3cced10a0b46fc0b7fe`, capture complete |

Two further API pages were tried and not found at the 2022 help (the server reported the
file does not exist): the swConicTypeForCrossSectionProfile_e enumeration and the
ConicRhoOrRadius property. The type property's page links to swFeatureFilletProfileType_e
instead, which was captured.

The findings below paraphrase those pages; no text was copied.

## What the published documentation says

Observed (stated on the pages):

- A fillet makes a rounded face, inside or outside, on edges, a set of faces, or edge
  loops (Overview).
- A constant size fillet has one radius for its edges (Constant Size Fillets).
- Tangent propagation carries a fillet on to every face tangent to the one selected
  (Constant Size Fillets). Selecting one edge of a smooth run therefore rounds the whole
  run.
- Fillets on single closed edges (circles, splines, ellipses are the examples given) meet
  other edges under an "overflow" choice: keep the model's edge and let the fillet adjust,
  or keep the fillet smooth and let the model edge change; by default the software picks
  (Constant Size Fillets).
- With round corners, fillets along adjacent edges blend smoothly where the edges meet
  rather than meeting in a sharp junction (Constant Size Fillets).
- Recommendations: larger fillets before smaller ones where several meet at a vertex; one
  feature for edges of equal radius (Overview).
- A fillet's profile is its cross-sectional shape: circular, or conic shapes weighted by a
  Rho value (Face Fillets, Conic Fillets). Face fillets can instead follow a hold line,
  and fillets can be made curvature continuous at their boundaries (Face Fillets).

- The API names four cross-section profile types (swFeatureFilletProfileType_e): 0
  circular (elliptical for asymmetric fillets), 1 conic rho, which takes a ratio from 0.5
  to 0.95 giving the weight of the cross-section, 2 conic radius, which takes the radius
  of curvature at the shoulder point of the cross-section, and 3 a chamfer's cross-section.
  A fillet's data carries the type, one value for the conic's rho or radius
  (DefaultConicRhoOrRadius, "the conic rho, conic radius, or circular radius"), and a
  separate on/off curvature-continuous setting, smoother because there is no jump in
  curvature at the boundary (ISimpleFilletFeatureData2 pages). The 2014 page's example
  conic is a hyperbola.

- Chamfers come in several kinds: angle and distance, two distances (equal when
  symmetric), offset face, face face (with hold lines or a chord width) and vertex
  chamfers (Chamfer PropertyManager). An offset face chamfer is found by offsetting the
  faces next to the edge, taking where the offset faces meet, and dropping a normal from
  there to each face; the page says this gives predictable results between faces that
  are not flat.

Silent or ambiguous:

- The pages do not define where a circular profile's arc lies between two curved faces.
  AnvilCAD uses the usual definition (general practice, not from these pages): the round
  is swept by a ball of the fillet's radius rolling in contact with both faces, so each
  cross-section square to the ball's path is a circular arc of the radius between the two
  contact points. On a circular edge between faces round about the same axis that makes
  the round part of a torus.
- How an edge's run is chosen when tangent propagation meets a branch (three or more
  tangent edges at a vertex) is not described.
- The default overflow choice is not described beyond "the software picks".
- Where a conic or curvature-continuous profile meets the faces is not given: whether its
  contacts are those of a circular fillet of the radius, or set some other way. Rho's
  meaning is described only as the cross-section's weight; "shoulder point" is not
  defined. AnvilCAD uses the usual definitions (general practice, not from these pages):
  the conic is tangent to both faces at the contacts, with the corner where those tangents
  meet as its middle control point, and rho is the fraction of the way the shoulder point
  (the conic's middle) lies from the chord's middle to that corner. A rational quadratic
  weighted w there has rho = w / (1 + w): 0.5 is a parabola, above it a hyperbola, which
  agrees with the published range and the 2014 example.
- The shape of the curvature-continuous profile is not given, only that the curvature
  does not jump at the boundary. Nor is it said how it combines with a conic type.
- The units of the conic radius in the API are not stated on these pages; the API gives
  lengths in metres elsewhere, and the migrator reads it so.

## What AnvilCAD does

- Straight edges between flat faces, and circular edges, whole or arcs, between faces
  round about the circle's axis (a flat face square to it, a cylinder or cone on it), are
  filleted (and chamfered) exactly.
- Edges selected together that run smoothly one into the next (the outline of a slot or a
  rounded rectangle) are blended as one run, so the rounds join without a step, as with
  tangent propagation. AnvilCAD rounds the edges selected; it does not yet add tangent
  edges that were not selected.
- An arc that does not run on into another edge is rounded past its end only where
  nothing lies beyond it, so the round ends cleanly on the faces there.
- Any other edge between two faces (a hole slanting through a plate, a hole drilled across
  a boss, a boss standing on a shaft, faces of any kind the kernel has) gets a rolling-ball
  round: its sections are solved exactly, square to the edge, and joined into a B-spline
  surface (rational across, cubic along) refined until it follows them to 1e-7 mm. The
  round meets both faces tangentially. Closed edges are rounded all the way round.
- Edges selected together that run smoothly one into the next, where any of them needs a
  rolling ball (the root of a slot-shaped boss standing on a shaft, say), get one round
  along the whole run: one ball rolls along the edges in order, touching the faces of each
  in turn, so the round is a single face, closed where the run closes.
- Chamfers on such edges are equal-distance chamfers, as on straight and round edges: in
  each plane square to the edge, the point on each face the chamfer's distance from the
  edge (as a straight chord), joined by a straight line, swept along the edge like the
  rolling-ball round. On flat faces this is the same bevel as the closed-form chamfer.
  This is not the published offset face chamfer, which sets back d cot(phi / 2) on faces
  meeting at angle phi; AnvilCAD keeps one meaning of a chamfer's size for every edge.
- A chamfer can instead be an offset face chamfer, as published: the faces offset by the
  distance meet at a point (in each section, where a ball of that radius touching both
  faces has its centre), and the bevel joins the feet of the normals from it, straight.
- Or an angle and distance chamfer: the distance along one face, then the bevel leaving
  that face at the angle, straight across to the other. The pages say the distance's
  direction can be flipped but not which face the angle is measured from; AnvilCAD
  measures it from the face the distance runs along, which is the edge's first face (by
  the faces' names) unless flipped. On flat faces meeting at phi it sets back
  d sin(a) / sin(phi + a) on the other face.
- Offset face and angle and distance chamfers need the exact kernel; a part already
  handed to the faceted kernel reports that rather than building them.
- A fillet's profile can be circular (the ball's arc), conic by rho, conic by its radius at
  the shoulder point, or curvature continuous. Every profile touches the faces where the
  ball of the fillet's radius would, so its set-back is the circular fillet's.
  - Conic rho: in each section the rational quadratic from contact to contact with its
    middle control point at the corner where the tangents meet and weight rho / (1 - rho).
    Rho must be from 0.5 to 0.95, as published.
  - Conic radius: the same conic, its weight in each section chosen so the radius of
    curvature at its middle is the one given. A radius no conic between the contacts can
    have is refused.
  - Curvature continuous: in each section a quartic from contact to contact, leaving each
    along the face's tangent, its second and fourth control points half way to the corner
    and its middle one placed so that its curvature at each contact is the face's own in
    that direction. Between flat faces it leaves them with no curvature, and its middle
    control point is the corner. The half-way legs are AnvilCAD's choice; the published
    pages do not give the shape.
  - These profiles are swept along any edge, straight ones included, like a rolling-ball
    round, and need the exact kernel; a part already handed to the faceted kernel reports
    that rather than building them. The migrator reads the profile type, the curvature
    continuous setting and the conic's value; a fillet with a chamfer's cross-section, or
    a conic type with curvature continuous both set, is refused.
- Where a swept round's edge runs into another round at a corner (both tangent to the
  face they share at one point), the rounds meet there; as for the closed-form rounds,
  a round whose edge runs smoothly on into the earlier round's edge stops at that end, and
  the earlier round is not carried round the corner until tangent propagation is offered.
- Runs mixing convex and concave edges, hold lines, overflow choices and round corners are
  not offered yet; those fall back to the faceted kernel or are refused.
