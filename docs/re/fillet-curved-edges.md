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

Silent or ambiguous:

- The pages do not define the fillet's geometry between two curved faces. AnvilCAD uses the
  usual definition (general practice, not from these pages): the round is swept by a ball
  of the fillet's radius rolling in contact with both faces, so on a circular edge between
  faces round about the same axis its cross-section in each half-plane through that axis
  is the same arc, and the round is part of a torus.
- How an edge's run is chosen when tangent propagation meets a branch (three or more
  tangent edges at a vertex) is not described.
- The default overflow choice is not described beyond "the software picks".

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
- A run mixing convex and concave edges, and edges between faces that are not round about
  a common axis (a hole drilled into a curved wall, an ellipse or spline edge), still go to
  the faceted kernel: their rolling-ball round is not a torus, and AnvilCAD does not yet
  build freeform rounds. Overflow choices and round corners are not offered yet.
