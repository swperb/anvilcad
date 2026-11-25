# Sketch entity tools

What AnvilCAD's sketch entity tools do: the commands that draw rectangles, slots, polygons,
arcs, circles and points into a sketch, the relations each one adds, and the automatic
relations that sketching infers from the pointer. SOLIDWORKS users know the equivalent tools
well, so AnvilCAD follows their published behaviour where it is documented. This
specification was written for later work in `crates/anvil-sketch/src/sketch.rs` (geometry
builders and relations) and `crates/anvil-app/src/sketcher.rs` (the interactive tools).
AnvilCAD today has Line, Centerline, (corner) Rectangle, Circle, Centerpoint Arc and Point
tools, and the relations listed in `Constraint` in `sketch.rs`.

## How this was studied

- **Inputs.** Only published information was used:
  - Dassault's public online help for SOLIDWORKS 2025, with older pages (2012, 2014, 2016,
    2017) read to check that the wording had not changed;
  - the 2024 API help: `ISketchManager` methods, `ISketchSlot`, `ISketchArc`, three
    published API examples, and the related enumerations;
  - Wikipedia articles for the geometry of regular polygons and stadiums (slot outlines).

  No SOLIDWORKS software, installer, library or file was downloaded, installed, run or
  analysed. No SOLIDWORKS licence was accepted on the machine used.
- **Retrieval.** The help site renders its articles with JavaScript, and a plain request
  is refused. Each article was fetched over HTTPS with a browser user agent, and its text
  was read from the page's embedded `__NEXT_DATA__` JSON
  (`props.pageProps.helpContentData.helpText`), with HTML stripped by a script run with
  `python3 -I`. Link targets were read from the same JSON to find page names. A missing
  page still returns HTTP 200 with "page cannot be found" in its data, so each page's
  content was checked. Two web searches looked for public tutorials on the relations the
  tools add; they found nothing more specific than the help. The text was paraphrased; none
  of it is copied into this repository.
- **REA.** REA (`rea-agents` 6.5.0, with the `third_party/rea` submodule at `3fc0075`) was
  set up and its research method followed:
  - each finding below is marked **Observed** (stated in a cited source), **Inferred**
    (our reasoning from the sources), **Decision** (AnvilCAD's own choice where the
    sources are silent) or **Unknown**;
  - every observation cites its source.

  REA's analysis tools were **not** run. They inspect artifacts (binaries, packages,
  captures, files), and the only lawful inputs for this behaviour are prose documents.
  Pointing REA at SOLIDWORKS software is ruled out by
  [INTEROPERABILITY.md](../../INTEROPERABILITY.md). How sketch entities are stored in
  `.sldprt` files was not studied.
- **What the sources do not cover.** The help shows the relations each tool adds only in
  pictures and videos, which were not used. Its text lists the relation *kinds* a user can
  add, but not the relations each tool creates. Most of the relation sets below are
  therefore **Decisions**, chosen so that the result has exactly the degrees of freedom the
  shape needs and uses only relations AnvilCAD already has (or one new one, see
  "Arc-to-arc tangency").

## Sources

Base A is `https://help.solidworks.com/2025/English/SolidWorks/sldworks/`. Base B is
`https://help.solidworks.com/2024/English/api/sldworksapi/`, and the API member pages there
are `SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.<Interface>~<Member>.html`.
Base C is `https://help.solidworks.com/2024/English/api/swconst/`, with pages
`SOLIDWORKS.Interop.swconst~SOLIDWORKS.Interop.swconst.<enum>.html`. All were read on
2026-10-18.

| # | Topic | URL | Version |
|---|---|---|---|
| S1 | Sketch entities index | A + `c_sketch_entities_top.htm` | 2025 |
| S2 | Rectangles overview | A + `c_Rectangles.htm` | 2025 |
| S3 | Rectangle PropertyManager | A + `HIDD_DVE_SKETCH_RECTANGLES.htm` (also 2012, 2014, 2016) | 2025 |
| S4 | Corner rectangles | A + `t_Sketching_Corner_Rectangles.htm` | 2025 |
| S5 | Center rectangles | A + `t_Sketching_Center_Rectangles.htm` | 2025 |
| S6 | 3 point corner rectangles | A + `t_Sketching_3_Point_Corner_Rectangles.htm` | 2025 |
| S7 | 3 point center rectangles | A + `t_Sketching_3_Point_Center_Rectangles.htm` | 2025 |
| S8 | Parallelograms | A + `t_Sketching_Parallelograms.htm` | 2025 |
| S9 | Construction lines and centerlines in rectangles | A + `t_add_construction_lines_and_centerlines.htm` | 2025 |
| S10 | Resizing rectangles; line properties in rectangles | A + `t_Changing_the_Size_or_Shape_of_Rectangles.htm`, `t_changing_line_properties_rectangles.htm` | 2025 |
| S11 | Slots overview | A + `c_Slots.htm` | 2025 |
| S12 | Slot PropertyManager | A + `HIDD_DVE_SKETCH_SLOT.htm` (also 2012) | 2025 |
| S13 | Straight slot | A + `t_Sketch_Creating_Straight_Slot.htm` | 2025 |
| S14 | Centerpoint straight slot | A + `t_Sketch_Creating_Centerpoint_Straight_Slot.htm` | 2025 |
| S15 | 3 point arc slot | A + `t_Sketch_Creating_3_Point_Arc_Slot.htm` | 2025 |
| S16 | Centerpoint arc slot | A + `t_Sketch_Creating_Centerpoint_Arc_Slot.htm` | 2025 |
| S17 | Detailing for sketch slots | A + `c_detailing_for_sketch_slots.htm` | 2025 |
| S18 | Sketching polygons | A + `t_Sketching_Polygons.htm` | 2025 |
| S19 | Polygon PropertyManager | A + `HIDD_DVE_SKETCH_POLYGON.htm` (also 2012, 2014, 2016) | 2025 |
| S20 | Editing and dragging polygons | A + `t_Changing_Polygon_Properties.htm`, `t_Modifying_Polygons_By_Dragging.htm` | 2025 |
| S21 | Arcs overview | A + `c_Arcs.htm` | 2025 |
| S22 | Arc PropertyManager | A + `HIDD_DVE_SKETCH_ARC.htm` | 2025 |
| S23 | Centerpoint arcs | A + `t_Sketching_Centerpoint_Arcs.htm` | 2025 |
| S24 | Tangent arcs | A + `t_Sketching_Tangent_Arcs.htm` (also 2014, 2016) | 2025 |
| S25 | 3 point arcs | A + `t_Sketching_3_Point_Arcs.htm` | 2025 |
| S26 | Autotransitioning between line and arc | A + `t_Autotransitioning.htm` | 2025 |
| S27 | Circles overview | A + `c_Circles.htm` | 2025 |
| S28 | Circle PropertyManager | A + `HIDD_DVE_SKETCH_CIRCLE.htm` (also 2017) | 2025 |
| S29 | Sketching circles | A + `t_Sketching_Circles.htm` | 2025 |
| S30 | Perimeter circles | A + `t_Sketching_Perimeter_Circles.htm` (also 2014, 2016) | 2025 |
| S31 | Automatically adding relations (tangent circles) | A + `t_Automatically_Adding_Relations.htm` | 2025 |
| S32 | Dragging circles | A + `t_Modifying_Circles_By_Dragging.htm` | 2025 |
| S33 | Sketching points | A + `t_Sketching_Points.htm` | 2025 |
| S34 | Point PropertyManager | A + `HIDD_DVE_SKETCH_POINT.htm` | 2025 |
| S35 | Ellipses and partial ellipses | A + `c_ellipses.htm`, `t_Sketching_Ellipses.htm`, `t_Sketching_Partial_Ellipses.htm`, `HIDD_DVE_SKETCH_ELLIPSE.htm` | 2025 |
| S36 | Parabolas | A + `t_Sketching_Parabolas.htm`, `HIDD_DVE_SKETCH_PARABOLA.htm`, `t_Modifying_Parabolas.htm` | 2025 |
| S37 | Conics | A + `c_Conics.htm`, `t_Sketching_Conics.htm`, `HIDD_DVE_SKETCH_BCONIC.htm` | 2025 |
| S38 | Splines | A + `c_Splines.htm`, `HIDD_DVE_SKETCH_SPLINE.htm` | 2025 |
| S39 | Sketch text | A + `c_Sketch_Text.htm`, `t_Sketching_Text.htm`, `HIDD_DVE_SKETCH_TEXT.htm` | 2025 |
| S40 | Inferencing | A + `c_Inferencing.htm` | 2025 |
| S41 | Using automatic relations | A + `t_Using_Automatic_Relations.htm` | 2025 |
| S42 | Sketch relations overview | A + `c_Sketch_Relations_Overview.htm` | 2025 |
| S43 | Description of sketch relations | A + `c_Description_of_Sketch_Relations.htm` | 2025 |
| S44 | Sketch snaps | A + `c_Sketch_Snaps.htm` | 2025 |
| S45 | Quick snaps | A + `c_Quick_Snaps.htm` | 2025 |
| S46 | Relations/Snaps options | A + `r_relations_snaps_options.htm` | 2025 |
| S47 | Sketch options (numeric input) | A + `HIDD_OPTIONS_SKETCH.htm` | 2025 |
| S48 | Sketching lines | A + `t_Sketching_Lines.htm` | 2025 |
| S49 | Geometric relations | A + `c_Geometric_Relations.htm` | 2025 |
| S50 | `ISketchManager` members | B + `SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.ISketchManager_members.html` | 2024 |
| S51 | `ISketchManager::CreateCornerRectangle`, `CreateCenterRectangle`, `Create3PointCornerRectangle`, `Create3PointCenterRectangle`, `CreateParallelogram` | B + member pages | 2024 |
| S52 | `ISketchManager::CreateSketchSlot` | B + member page | 2024 |
| S53 | `ISketchSlot` members (`Length`, `LengthType`, `Width`, `CenterArcDirection`, `CreationType`, `GetCenterPoint`, `GetCenterPointHandle`, `GetSlotPoints`) | B + `SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.ISketchSlot_members.html` and member pages | 2024 |
| S54 | Insert and Resize Sketch Slot example; Get Sketch Slot Data example | B + `Insert_and_Resize_Sketch_Slot_Example_VB.htm`, `Get_Sketch_Slot_Data_Example_VB.htm` | 2024 |
| S55 | `ISketchManager::CreatePolygon`; Create Polygon example | B + member page, `Create_Polygon_Example_VB.htm` | 2024 |
| S56 | `ISketchManager::CreateArc`, `Create3PointArc`, `CreateTangentArc`; Create Tangent Arc example | B + member pages, `Create_Tangent_Arc_Example_VB.htm` | 2024 |
| S57 | `ISketchManager::CreateCircle`, `CreateCircleByRadius`, `CreatePoint`, `CreateLine`, `CreateCenterLine` | B + member pages | 2024 |
| S58 | `ISketchManager::CreateEllipse`, `CreateEllipticalArc`, `CreateParabola`, `CreateConic`, `CreateSpline2`; `IModelDoc2::InsertSketchText` | B + member pages | 2024 |
| S59 | `ISketchManager::AddToDB` (inference and automatic relations in API creation) | B + member page | 2024 |
| S60 | `ISketchArc` members | B + `SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.ISketchArc_members.html` | 2024 |
| S61 | `swSketchSlotCreationType_e`, `swSketchSlotLengthType_e` | C | 2024 |
| S62 | `swTangentArcTypes_e` | C | 2024 |
| S63 | `swConstraintType_e` | C | 2024 |
| S64 | `swSketchSegments_e` | C | 2024 |
| S65 | `swUserPreferenceToggle_e` (`swSketchAutomaticRelations`, `swSketchInference`) | C | 2024 |
| N1 | Regular polygon: circumradius, apothem, side and area | `https://en.wikipedia.org/wiki/Regular_polygon` | not Dassault |
| N2 | Stadium: area and perimeter | `https://en.wikipedia.org/wiki/Stadium_(geometry)` | not Dassault |
| S66 | Ellipse tools, partial ellipses, changing ellipse properties | A + `c_ellipses.htm`, `t_Sketching_Ellipses.htm`, `t_Sketching_Partial_Ellipses.htm`, `t_Changing_Ellipse_Properties.htm` | 2025 |
| S67 | Ellipse PropertyManager | A + `HIDD_DVE_SKETCH_ELLIPSE.htm` | 2025 |
| S68 | `ISketchManager::CreateEllipse`, `CreateEllipticalArc` (parameters and `Direction`) | B + member pages | 2024 |
| S69 | `ISketchEllipse` members (`GetCenterPoint2`, `GetMajorPoint2`, `GetMinorPoint2`, `GetStartPoint2`, `GetEndPoint2`, `GetRotationDir`) | B + `SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.ISketchEllipse_members.html` | 2024 |
| S70 | Splines overview; B-splines | A + `c_Splines.htm`, `c_b_splines.htm` | 2025 |
| S71 | Spline PropertyManager | A + `HIDD_DVE_SKETCH_SPLINE.htm` | 2025 |
| S72 | Creating multiple point splines; two-point splines with tangency | A + `t_Creating_Multiple_Point_Splines.htm`, `t_Creating_Two_Point_Splines_with_Tangency.htm` | 2025 |
| S73 | Relations with splines; curvature continuous splines | A + `c_Relations_with_Splines.htm` | 2025 |
| S74 | Spline handles, spline points, tangency and curvature control | A + `c_Spline_Handles.htm`, `c_Spline_Points.htm`, `t_Adding_Tangency_Control_to_Splines.htm`, `t_Adding_Curvature_Control_to_Splines.htm` | 2025 |
| S75 | Control polygons | A + `c_Control_Polygons.htm` | 2025 |
| S76 | Style splines; sketching a style spline; Style Spline PropertyManager | A + `c_about_style_splines.htm`, `t_sketch_style_spline.htm`, `r_style_spline_pm.htm` | 2025 |
| S77 | Spline tools; fixed spline length | A + `c_spline_tools.htm`, `t_set_fixed_length_for_spline.htm` | 2025 |
| S78 | `ISketchManager::CreateSpline2`; `ISketchSpline` members | B + member pages | 2024 |
| N3 | Ellipse: area and centroid of an elliptic segment | `https://en.wikipedia.org/wiki/Ellipse` | not Dassault |

## Common behaviour

### Tool families and the property panel

- **Observed (S2, S11, S21, S27).** Rectangles, slots, arcs and circles each come as a
  family of tools under one toolbar flyout. The panel for a new entity of the family lets
  the user switch between them. The flyout remembers the last tool used.
- **Observed (S3, S12, S19, S22, S28, S34).** Every entity's panel shows its existing
  relations, its status (fully defined, under defined, and so on), relations that can be
  added, a "For construction" option, and editable parameters (coordinates, radius, angle,
  diameter).
- **Decision.** AnvilCAD keeps one tool per shape in its toolbar, grouped as families, and
  remembers the last tool used in each family. The relation and parameter panel is outside
  this specification.

### Sketch entities in the tree

- **Inferred (S1, S50).** None of these tools makes a feature in the feature tree. Every
  entity belongs to the sketch it was drawn in ("Sketch1" and so on), and is reached by
  editing that sketch.
- **Unknown.** The default names SOLIDWORKS gives individual entities. They were not
  studied.

### Inferencing and automatic relations

- **Observed (S40).** While a tool is active, dotted inference lines and the pointer's
  shape show relations with existing geometry (horizontal, vertical, coincident, midpoint
  and others). If the user clicks while a relation is shown, that relation is added.
- **Observed (S41).** The relations shown and added this way include horizontal,
  vertical, coincident, midpoint, perpendicular and tangent. More than one can apply at
  once.
- **Observed (S41, S46, S65).** "Automatic relations" can be turned off (Tools > Sketch
  Settings, or the Relations/Snaps options; `swSketchAutomaticRelations` in the API).
  Holding Ctrl while sketching suppresses them for that entity.
- **Observed (S40, S46).** Clearing "Enable snapping" also clears automatic relations.
- **Observed (S44).** Sketch snaps (all on by default except Grid) snap to:
  - end points and sketch points, and arc centres;
  - centre points of circles, arcs and partial ellipses;
  - midpoints of lines, arcs and the sides of rectangles and polygons;
  - quadrant points of circles, arcs and ellipses;
  - intersections, tangents, perpendiculars, parallels;
  - horizontal or vertical alignment with existing lines and points;
  - lengths in grid increments (with Shift held), the grid itself, and angles.
- **Observed (S46).** Lines snap to angles in steps set by "Snap angle". With no other
  lines the steps are measured from the sketch x axis. When a line starts at the end of
  another line, they are measured from that line.
- **Observed (S59).** Entities made through the API with `AddToDB` set skip inferencing,
  automatic relations and grid snapping.
- **Unknown.** The angle and distance tolerances inside which a relation is inferred, and
  the default snap angle.
- **Decision.** AnvilCAD's inference, applied to every tool below unless it says otherwise:
  1. **Coincident.** A click within the pick radius of an existing sketch point uses that
     point; the new entity shares it, so no separate relation is stored. This is what the
     sketcher does today. A click on a line or a circle or arc (but not on a point) adds
     `PointOnLine` or `PointOnCircle` for the new point.
  2. **Midpoint.** A click on a line's midpoint marker adds `Midpoint`.
  3. **Horizontal and vertical.** A new line within 3° of horizontal or vertical gets
     `Horizontal` or `Vertical` (today's `auto_hv`). This applies to lines drawn by the Line
     and Centerline tools and to the first edge of a 3 point rectangle or parallelogram.
  4. **Tangent.** An arc or line whose free end is drawn from the end of an arc, and leaves
     it within 3° of tangent, gets `Tangent`. Arcs from the Tangent Arc tool always do.
  5. **Suppression.** Holding Ctrl while clicking suppresses 2 to 4 (not 1, since sharing
     a point is how the user connects geometry). A sketcher setting turns them off.

  Perpendicular, parallel, and horizontal or vertical alignment with *other* points are
  left for later; they are not needed to reproduce any of the tools below.

### Numeric input and Shift

- **Observed (S3, S47).** With on-screen numeric input turned on, rectangle tools offer
  "Add dimensions", which dimensions the new rectangle. Numeric input is not available for
  slots, which instead have their own "Add dimensions" option (S12).
- **Observed (S2, S4–S8).** Holding Shift while drawing a rectangle makes a square, and
  makes a parallelogram's sides equal.
- **Observed (S44).** For lines, Shift turns on length snapping instead.
- **Decision.** Shift with a rectangle tool adds `Equal` between two adjacent sides.
  Numeric input is left for later.

## Rectangles

### Tools and picks

- **Observed (S2, S3).** There are five rectangle tools:
  - Corner Rectangle: axis-aligned, from two opposite corners;
  - Center Rectangle: axis-aligned, from its centre;
  - 3 Point Corner Rectangle: at any angle, from a corner;
  - 3 Point Center Rectangle: at any angle, from its centre;
  - Parallelogram.
- **Observed (S4).** Corner Rectangle: click the first corner, drag (or click) the
  opposite corner.
- **Observed (S51).** `CreateCornerRectangle` takes the upper-left and lower-right
  corners.
- **Observed (S5).** Center Rectangle: click the centre, then drag to a corner. The
  rectangle is drawn with centerlines.
- **Observed (S6).** 3 Point Corner Rectangle: click the first corner; the second click
  sets the length and angle of the first edge; the third sets the other edges.
- **Observed (S7).** 3 Point Center Rectangle: click the centre; the second click sets
  *half* the length of a centreline, and its direction; the third sets the other edges.
- **Observed (S8).** Parallelogram: click the first corner; the second click sets the
  length and angle of the first edge; the third sets the angle and length of the other
  edges.
- **Inferred (S6, S7, S8).** With picks p1, p2, p3, let u be the unit vector from p1 to
  p2 and n the unit vector at +90° to u:
  - 3 Point Corner: corners p1, p2, p2 + h n, p1 + h n, where h = (p3 − p2) · n. The
    signed h puts the rectangle on the side of the first edge where p3 is.
  - 3 Point Center: the centre is c = p1, half-length a = |p2 − p1|, half-width
    b = |(p3 − c) · n|. The corners are c ± a u ± b n. So p2 is the midpoint of one of the
    short or long sides, not a corner.
  - Parallelogram: corners p1, p2, p3, p1 + (p3 − p2). The third pick is the corner after
    p2.
- **Decision.** A rectangle with zero length or zero width is not created (the tool waits
  for another pick), as today.

### Construction lines

- **Observed (S3, S9).** The panel's "Add construction lines" option adds centerlines
  either "From Corners" (corner to corner, the diagonals) or "From Midpoints" (joining the
  midpoints of opposite sides).
- **Observed (S51).** `CreateCenterRectangle` and `Create3PointCenterRectangle` return
  the edges *and diagonals*. `CreateCornerRectangle` and `Create3PointCornerRectangle`
  return only the edges.
- **Observed (S3).** Center rectangles also have a centre point, whose coordinates the
  panel shows.
- **Inferred (S5, S51).** By default the two centre rectangle tools add construction
  diagonals, and the corner tools add no construction lines.
- **Unknown.** Whether "From Midpoints" lines on a centre rectangle also meet at the
  centre point, and whether the option persists between uses.

### Relations each tool adds

- **Observed (S3).** The panel lists the relations "inferenced automatically" for a new
  rectangle, but no source lists which ones the tool adds itself.
- **Decision.** The relations below, in AnvilCAD's terms, give each shape exactly the
  freedoms it should have. Sides are numbered l0 (p1 to p2 for the 3 point tools; bottom
  for the axis-aligned tools), l1, l2, l3, going round, each sharing its corner points with
  its neighbours. DOF counts are of the new geometry alone.

| Tool | Relations | Free | DOF |
|---|---|---|---|
| Corner | `Horizontal` l0, l2; `Vertical` l1, l3 (today's `add_rectangle`) | x, y, width, height | 4 |
| Center | as Corner; construction diagonals d0 (corner 0 to 2), d1 (corner 1 to 3); centre point m with `Midpoint`(m, d0) | m, width, height | 4 |
| 3 Point Corner | `Perpendicular`(l0, l1), `Parallel`(l0, l2), `Parallel`(l1, l3) | x, y, angle, length, width | 5 |
| 3 Point Center | as 3 Point Corner; diagonals and `Midpoint`(m, d0) | m, angle, length, width | 5 |
| Parallelogram | `Parallel`(l0, l2), `Parallel`(l1, l3) | x, y, angle, two lengths, included angle | 6 |

- **Decision.** Only `Midpoint`(m, d0) is added, not one for d1 as well: the diagonals of
  a parallelogram bisect each other, so a second relation would be redundant and make the
  sketch read as over-defined.
- **Decision.** "From Midpoints" adds construction lines k0 (midpoint of l0 to midpoint
  of l2) and k1 (midpoint of l1 to midpoint of l3), with `Midpoint` from each end of k0 and
  k1 to its side. On a centre rectangle the centre point gets `Midpoint`(m, k0) instead of
  the diagonal relation. This adds no freedom.
- **Decision.** Inference also applies: a corner clicked on an existing point shares it
  (today's behaviour), and the first edge of a 3 point tool within 3° of horizontal or
  vertical gets `Horizontal` or `Vertical` (taking up the angle freedom).
- **Decision.** Shift adds `Equal`(l0, l1) (square, or rhombus for the parallelogram).
- **Decision.** "Add dimensions" adds the two side lengths (`Length` on l0 and l1). For a
  parallelogram it also adds the angle between l0 and l1 (`RayAngle`).

## Slots

### Tools and picks

- **Observed (S11, S12, S61).** Four slot tools:

| Tool | API `swSketchSlotCreationType_e` | Picks (S13–S16) |
|---|---|---|
| Straight Slot | `swSketchSlotCreationType_line` = 0 | start point; length; width |
| Centerpoint Straight Slot | `swSketchSlotCreationType_center_line` = 1 | centre; length; width |
| 3 Point Arc Slot | `swSketchSlotCreationType_3pointarc` = 3 | arc start; arc end; third point on the arc; width |
| Centerpoint Arc Slot | `swSketchSlotCreationType_arc` = 2 | arc centre; radius (and start); length (end); width |

- **Observed (S54).** The published example makes a straight slot with points (−50, 0)
  and (50, 0) mm, width 50 mm, length type centre to centre, and no dimensions.
- **Inferred (S12, S13, S54).** For a straight slot the first two points are the centres
  of the two end arcs. The slot runs along the line between them.
- **Inferred (S14).** For a centerpoint straight slot the first pick is the slot's centre
  and the second is the centre of one end arc. The other end arc's centre is its mirror
  through the slot's centre.
- **Inferred (S15, S16).** For arc slots the picks fix a *centre arc*, built like a
  3 point arc or a centerpoint arc (below). The ends of the centre arc are the centres of
  the slot's end arcs.
- **Observed (S52, S53).** Arc slots run clockwise or counter-clockwise
  (`CenterArcDirection`, −1 or +1).
- **Inferred.** The width pick sets the width to twice the distance from the pointer to
  the centre line or centre arc.
- **Unknown.** Whether the first two picks of a straight slot still mark the arc centres
  when "Overall Length" is chosen, or the slot's outer ends instead.

### Geometry

- **Inferred (S12, S53, N2).** A straight slot is a stadium. With end-arc centres c1 and
  c2, centre-to-centre length L = |c2 − c1| and width w:
  - two semicircular arcs of radius w/2 about c1 and c2;
  - two lines parallel to c1c2, offset ±w/2, joining the arcs' ends;
  - area L·w + π(w/2)² and perimeter 2L + πw (N2);
  - overall length L + w.
- **Inferred (S12, S15, S16).** An arc slot about a centre O with centre-arc radius R,
  sweep θ (centre to centre) and width w is:
  - an outer arc of radius R + w/2 and an inner arc of radius R − w/2, both about O and
    sweeping θ;
  - two semicircular end arcs of radius w/2 about the centre arc's ends, tangent to both;
  - area θ·R·w + π(w/2)².
- **Observed (S53, S54).** Every slot has a centre point (`GetCenterPoint`,
  `GetCenterPointHandle`).
- **Observed (S12).** The panel's parameters are the slot centre's x and y, width and
  length, and for arc slots also the arc radius and angle.
- **Decision.** Each slot also gets a construction line from c1 to c2 (straight slots) or
  a construction arc from c1 to c2 about O (arc slots). The slot's centre point is the
  construction line's midpoint, or the construction arc's centre O.
- **Decision.** The tool refuses (waits for another pick) when L is zero, w is zero, or,
  for an arc slot, w ≥ 2R (the inner arc would vanish) or θ is zero.

### Length type and dimensions

- **Observed (S12, S61).** "Center to Center" (`swSketchSlotLengthType_CenterCenter` = 0)
  makes a straight slot's length dimension measure between the arc centres. "Overall
  Length" (`swSketchSlotLengthType_FullLength` = 1) makes it measure the slot's full
  length. Arc slot angles are always measured centre to centre.
- **Observed (S12, S52).** "Add Dimensions" puts the length and arc dimensions on a new
  slot (`AddDimension` in the API).
- **Observed (S53).** `ISketchSlot::Length` reports the slot's length and `Width` its
  width, and `LengthType` can be changed on an existing slot.
- **Unknown.** Which dimension carries the width (a radius or diameter of an end arc, or a
  linear width), and whether an arc slot's dimensions are its radius and angle only.
- **Decision.** "Add dimensions" adds:
  - straight slot, centre to centre: `Length` on the construction line;
  - arc slot: `Radius` on the construction arc, and the centre-to-centre angle as a
    `RayAngle` between two construction radii, from O to c1 and from O to c2 (they share
    their points with the slot, so they add no freedom);
  - straight slot, overall length: `Distance` between two construction points e1, e2 where the centre
    line meets the end arcs' far sides (each with `PointOnLine` on the centre line and
    `PointOnCircle` on its end arc);
  - width: `Diameter` on one end arc, which reads as the slot width.

  The default is centre to centre, matching the published example (S54).

### Slot relations

- **Observed (S12, S43, S63).** Two relations exist only for slots: Fix Slot
  (`swConstraintType_FIXEDSLOT` = 74) and Equal Slots (`swConstraintType_SAMESLOTS` = 75,
  same width and length).
- **Unknown.** The relations a slot holds internally. The sources show none in text.
- **Decision (straight slots).** With end arcs A1, A2, side lines s1, s2, construction
  line k (c1 to c2) and slot centre m:
  - `Tangent`(s1, A1), `Tangent`(s1, A2), `Tangent`(s2, A1), `Tangent`(s2, A2);
  - `Equal`(A1, A2);
  - `Midpoint`(m, k).

  The side lines share their end points with the arcs, and the arc centres are k's end
  points. Count: points c1, c2, m and four side-line ends give 14 unknowns; the two arcs'
  implicit equal-radius conditions, four tangencies, one equal and the midpoint (two) give
  9 equations, leaving 5 freedoms: c1, c2 and the width. That is exactly a stadium.
- **Decision (arc slots).** Arc slots need tangency between two arcs, which AnvilCAD does
  not have; see "Arc-to-arc tangency" below. With it: the inner arc, outer arc and
  construction arc share centre O; each end arc is centred on an end of the construction
  arc, shares its end points with the inner and outer arcs, and is tangent to both; the
  end arcs are `Equal`.
- **Decision.** AnvilCAD does not group a slot into one entity yet. Fix Slot and Equal
  Slots are left out; Equal Slots can be built from `Equal` on the end arcs and on the
  construction lines.

## Polygon

- **Observed (S18, S19).** A polygon is regular, with 3 to 40 sides. The user sets the
  number of sides in the panel, clicks the centre and drags out the polygon. "New Polygon"
  starts another one.
- **Observed (S19).** Its size is set by a construction circle. "Inscribed circle" draws
  the circle *inside* the polygon, so it touches the sides. "Circumscribed circle" draws it
  *outside*, so it passes through the corners. The panel shows the centre's x and y, the
  circle's diameter and an angle of rotation, and changing one updates the others.
- **Observed (S55).** `CreatePolygon` takes the centre, the coordinates of a *vertex*, the
  number of sides, and `Inscribed` (true for an inscribed construction circle).
- **Observed (S19).** A polygon shows no relations of its own, and relations can be added
  only to its points and lines. It is edited through "Edit Polygon" (S20).
- **Observed (S20).** Dragging a side resizes it; dragging a vertex or the centre moves
  it.
- **Inferred (N1).** With n sides, circumradius R and apothem r = R cos(π/n):
  - side s = 2R sin(π/n) = 2r tan(π/n);
  - area ½ n R² sin(2π/n) = n r² tan(π/n);
  - an inscribed circle of diameter D has r = D/2; a circumscribed one has R = D/2.

  Note that SOLIDWORKS's "inscribed" names the *circle* (inside the polygon), not the
  polygon. A hexagon with an *inscribed* circle of 20 mm has sides of 11.547 mm; one with a
  *circumscribed* circle of 20 mm has sides of 10 mm.
- **Unknown.**
  - The default number of sides and default circle type.
  - Which line the rotation angle is measured on, and from which axis.
  - Whether the dragged point is always a vertex (as in the API) or, for an inscribed
    circle, the midpoint of a side.
- **Decision.**
  - Default: 6 sides, inscribed circle. The panel shows sides, circle type, diameter and
    angle.
  - The dragged point is a vertex v0, as in the API, for both circle types. Vertex k is at
    c + R (cos(φ + 2πk/n), sin(φ + 2πk/n)), where φ is the direction from the centre to v0.
  - The angle shown is φ, measured counter-clockwise from the sketch x axis.
- **Decision (relations).** AnvilCAD has no polygon entity, so the polygon is ordinary
  lines held regular by relations. Sides l0 … l(n−1), each running from v_k to v_(k+1):
  - `Angle`(l_k, l_(k+1)) = 360°/n for k = 0 … n−2 (the exterior angle; the last one
    follows from the polygon closing);
  - `Equal`(l0, l_k) for k = 1 … n−3 (the last two sides follow);
  - a construction circle with centre point c;
  - circumscribed: `PointOnCircle` for v0, v1 and v2; inscribed: `Tangent` for l0, l1 and
    l2.

  This is 2n − 4 shape equations on 2n coordinates, plus 3 to tie the circle to the
  polygon, which leaves 4 freedoms (centre x, y, diameter, rotation). It is exact for every
  n from 3 to 40. A single `Diameter` on the circle and one `Horizontal` on a side then
  fully define it.

## Arcs

### Centerpoint arc

- **Observed (S23).** Click the centre, click the start point (setting the radius), then
  click the end point.
- **Observed (S56).** `CreateArc` takes the centre, start and end points and a direction:
  +1 counter-clockwise, −1 clockwise, going from start to end.
- **Observed (S22, S60).** The arc's parameters are its centre, start and end, radius and
  subtended angle. `ISketchArc::GetRotationDir` reports its direction.
- **Inferred (S23, S56).** The direction follows the way the pointer travels round the
  centre after the start point is placed.
- **Decision.** AnvilCAD stores arcs counter-clockwise from `a` to `b` (`Geom::Arc`).
  The tool sums the signed angle the pointer sweeps about the centre after the start pick.
  If the total is positive, the arc runs from the start to the end. If it is negative, the
  start and end swap, so the stored arc still runs counter-clockwise. The end point is
  projected onto the circle through the start (today's behaviour). Today the tool always
  runs counter-clockwise from the start; this replaces that.

### Tangent arc

- **Observed (S24).** The Tangent Arc tool starts at the end point of a line, arc,
  ellipse or spline and drags to the arc's end.
- **Observed (S24).** The tool reads the user's intent from how the pointer moves away
  from the end point. There are four intent zones and eight possible results. Moving off
  in a *tangent* direction makes a tangent arc; moving off in a *normal* direction makes a
  normal arc (one that leaves at right angles). Going back to the end point and moving away
  again chooses afresh.
- **Observed (S26).** The Line tool switches to a tangent arc if the pointer goes back to
  the end of the last line and away again, and back to a line after the arc. Pressing A
  toggles between line and arc without going back.
- **Observed (S62).** The API names four tangent arc types: `swForward` = 1,
  `swLeft` = 2, `swBack` = 3, `swRight` = 4.
- **Inferred (S24, S62).** The four zones are forward, back, left and right of the
  entity's direction at its end. Forward and back give tangent arcs, left and right give
  normal arcs. Each zone can curve either way, which gives the eight results.
- **Decision (zones).** Let t be the unit direction leaving the entity's end (for a line,
  from its other end towards this one; for an arc, its tangent at this end pointing away
  from the arc). When the pointer first leaves a capture circle round the end point (the
  pick radius), the angle between t and the pointer's direction picks the zone:
  - within ±45° of t: forward (the arc leaves along t);
  - within ±45° of −t: back (the arc leaves along −t);
  - otherwise: left or right (the arc leaves along ±n, n = t turned +90°, on the side the
    pointer went).

  Coming back inside the capture circle resets the zone.
- **Decision (geometry).** With start P, leaving direction τ (t, −t, n or −n), its left
  normal ν (τ turned +90°) and end Q, let d = Q − P:
  - signed radius ρ = |d|² / (2 d·ν), centre C = P + ρν, radius |ρ|;
  - ρ > 0 curves left (counter-clockwise), ρ < 0 curves right (clockwise; stored with start
    and end swapped).

  If d·ν = 0 the arc would be a straight line, so the tool waits.
- **Decision (relations).** The arc shares its start point with the entity.
  - Tangent arc from a line: `Tangent`(line, arc).
  - Normal arc from a line: `PointOnLine`(arc centre, line).
  - From an arc: arc-to-arc tangency (below) for tangent arcs; for normal arcs, a
    construction line from the old arc's centre to the shared point with the new centre on
    it.

### 3 point arc

- **Observed (S21, S25).** Click the start, click the end, then drag to set the radius and
  click. The arc runs from start to end through the third point.
- **Observed (S56).** `Create3PointArc` takes three points.
- **Inferred.** The arc is the circle through the three points, keeping the part that
  contains the third. Three points in a line give no arc.
- **Decision.** If the points are collinear, or two coincide, the tool waits. Relations
  come only from inference at the picks.

## Circles

- **Observed (S29).** Circle: click the centre, then click to set the radius.
- **Observed (S57).** `CreateCircle` takes the centre and a point on the circle;
  `CreateCircleByRadius` takes the centre and radius.
- **Observed (S30).** Perimeter Circle: click on the perimeter, drag left or right, and
  right-click to finish.
- **Observed (S31).** A circle drawn with the Perimeter Circle tool against an existing
  circle while the tangent marker shows becomes tangent to it.
- **Observed (S28).** The circle panel has a choice between centre and perimeter creation.
- **Observed (S32).** Dragging the edge resizes; dragging the centre moves.
- **Unknown.** How many points the perimeter circle takes and what the right-click does.
  No `ISketchManager` method for perimeter circles exists (S50).
- **Decision.** AnvilCAD's Perimeter Circle is a three-point circle: the circle through
  three clicks. The third click finishes it; a right-click after two picks makes the circle
  with those two points as a diameter. Inference applies at each click; a click on a
  circle or arc with the tangent marker showing adds tangency (arc-to-arc tangency, below).
- **Decision.** The existing Circle tool keeps its behaviour.

## Point

- **Observed (S33).** Click to place a point. The tool stays active for more points.
- **Observed (S34).** The point's panel shows its x and y.
- **Decision.** AnvilCAD's Point tool already does this. It should also apply inference: a
  click on a line or circle puts the point on it (`PointOnLine` or `PointOnCircle`), and a
  click on a line's midpoint adds `Midpoint`. Clicking an existing point still adds
  nothing.

## Arc-to-arc tangency

- **Observed (S43).** Tangent applies to an arc, ellipse or spline and a line or arc. A
  relation to an arc applies to its full circle.
- **Decision.** AnvilCAD's `Tangent` is between a line and a circle or arc only. Arc
  slots, tangent arcs from arcs, and tangent perimeter circles need tangency between two
  circles or arcs: |c_a − c_b| = r_a + r_b (outside) or |r_a − r_b| (inside). A new
  relation, say `TangentCircles { a, b, inside }`, does this; the side is chosen from the
  geometry when the relation is made, so the solver cannot jump to the other one.

## Ellipses

Studied on 2026-11-25 for issue #69 from S43 and S66–S69, retrieved and handled as in "How
this was studied" (same retrieval script, `python3 -I`, text paraphrased). REA
(`rea-agents` 6.10.1) was set up and its method followed: Observed, Inferred, Decision and
Unknown are kept apart and every observation cites its source. REA's analysis tools were
not run, for the reason given above: the inputs are prose pages.

### Tools and picks

- **Observed (S66).** Ellipse: click the centre, drag and click to set the major axis,
  drag and click again to set the minor axis.
- **Observed (S66).** Partial Ellipse: click the centre, click to define one axis, click
  to define the second axis; the outline stays shown as a guide, and the pointer is then
  dragged round it to set the extent and clicked to finish. The help calls this "similar
  to creating a centerpoint arc".
- **Inferred (S66).** For a partial ellipse the help says "one axis" and "the second
  axis", not major and minor, so the first axis drawn need not be the longer one.
- **Unknown.** Whether the click that sets the second axis also places the partial
  ellipse's start point, or the start is the point where the pointer begins its drag
  round the outline. The help's wording allows both; its video was not used.
- **Decision.** AnvilCAD's Ellipse tool takes three clicks: the centre C, the end of the
  first axis P1, and a third click Q. The second radius is Q's distance from the line
  C–P1, and the second axis end P2 is C plus that radius square to the first axis, on Q's
  side. The first axis need not be the longer for either tool. The Partial Ellipse tool
  takes the same three clicks and then a fourth: the start is the point of the ellipse in
  the direction of Q from C, and the end is the point of the ellipse in the direction of
  the fourth click. The direction follows the pointer's sweep round the centre, as for the
  centerpoint arc (below): a clockwise sweep swaps start and end so the stored arc still
  runs counter-clockwise. A pick that leaves a zero radius, or a fourth click on the start,
  makes nothing and the tool waits.

### Geometry and stored data

- **Observed (S68).** `CreateEllipse` takes the centre, a point on the ellipse on its
  major axis and a point on the ellipse on its minor axis. `CreateEllipticalArc` takes the
  same three and a start and an end point, with `Direction` +1 for counter-clockwise from
  start to end and −1 for clockwise.
- **Observed (S69).** An ellipse segment exposes its centre, major point, minor point,
  start point, end point and rotation direction as sketch points.
- **Observed (S67).** Its parameters are the centre's x and y, Radius 1 and Radius 2, and
  for a partial ellipse its start and end coordinates and its angle.
- **Inferred (S67, S69).** The ellipse is held by sketch points: the centre and one point
  on each axis. Its two radii are the distances from the centre to those points; its
  rotation is the direction of the first.
- **Decision.** `Geom::Ellipse { c, axis1, axis2 }` is the whole ellipse with centre `c`,
  and `axis1`, `axis2` the ends of its two semi-axes, both on the ellipse.
  `Geom::EllipseArc { c, axis1, axis2, a, b }` is the part running counter-clockwise from
  `a` to `b`. The solver keeps `axis2 − c` square to `axis1 − c`, and keeps `a` and `b` on
  the ellipse. A whole ellipse has 5 degrees of freedom and a partial one 7. The kernel's
  elliptic segment is `center + u r1 cos t + v r2 sin t` with u the unit vector towards
  `axis1` and v square to it (counter-clockwise), so a sketch ellipse maps to it with
  rotation angle(u), `a` = r1, `b` = r2. If the second axis point lies clockwise of the
  first (a mirrored ellipse) the same curve results, since v is always u turned +90°.
- **Observed (S67).** "Add construction lines" turns the major and minor axes into
  construction lines. **Decision.** Not in this slice; the axis end points are drawn and
  can be dimensioned and related without the lines.

### Relations

- **Observed (S43).** Tangent applies to an ellipse and a line or arc; Coincident to a
  point and an ellipse (the point lies on it); Symmetric to two ellipses about a
  centerline; Fix to any entity, leaving an elliptical segment's end points free to move
  round the full ellipse. A relation to an elliptical segment is to its full ellipse.
- **Observed (S67).** Add Relations offers only the relations possible for the selected
  entity.
- **Decision.** In this slice:
  - `PointOnCircle` also puts a point on an ellipse or partial ellipse (its full
    ellipse). Its residual is (√((x/r1)² + (y/r2)²) − 1)·√(r1 r2), in the ellipse's own
    axes: zero on the curve, the plain radial distance for a circle.
  - `Tangent` between a line and an ellipse: at a shared end point, the line runs along
    the ellipse's tangent there; with no shared point, the line's distance from the centre
    equals the ellipse's support distance √(r1²(n·u)² + r2²(n·v)²) for the line's unit
    normal n.
  - `TangentCircles` between an ellipse and a circle, arc or ellipse needs a shared end
    point (a partial ellipse's end); the curves' normals there are parallel. Without a
    shared point the tool refuses with a plain message, since two ellipses touching away
    from their ends has no closed form the solver can use well.
  - `Concentric` and `Coincident` use the centre point; `Fix` on an ellipse fixes its
    centre and both axis points, leaving a partial ellipse's ends free, as S43 says.
  - Symmetric ellipses, Equal ellipses and the construction axes are left for later.

### Dimensions

- **Observed (S67).** Radius 1 and Radius 2 are the ellipse's sizes.
- **Unknown.** How Smart Dimension treats a click on an ellipse itself; the dimension
  help (see `sketch-dimension.md`) does not cover ellipses.
- **Decision.** A dimension on an ellipse is the distance from its centre to one axis
  point: picking the ellipse with the Dimension tool dimensions the semi-axis whose end is
  nearer the pick. Picking the centre and an axis point gives the same dimension.

### Editing tools

- **Decision.** Move, Copy, Rotate, Scale, Mirror, Linear and Circular Pattern move the
  ellipse's points and keep it; Mirror swaps a partial ellipse's ends so it stays
  counter-clockwise. Trim, Extend, Split, Sketch Fillet, Sketch Chamfer and Offset refuse
  an ellipse with a plain message in this slice: an offset of an ellipse is not an
  ellipse, and trimming needs ellipse intersections, which come later.

### Features and export

- **Inferred (S66, N3).** An ellipse with radii r1 and r2 encloses π r1 r2; its centroid is
  its centre.
- **Decision.** Ellipses reach the exact kernel's elliptic segments (`anvil-brep`
  `Segment::Ellipse`), so extrusions of ellipses are exact, and STEP export writes
  `ELLIPSE` curves. Where the exact kernel does not yet take an ellipse (revolve, loft,
  drafted extrude; issue #143) the feature is built on the faceted kernel, as for any
  other feature the exact kernel refuses, and the part says it is no longer exact. Shell
  and thin features, which offset the profile, refuse an ellipse with a plain message.

### Testable statements

28. An ellipse with centre (0, 0), first axis end (30, 0) and third click (12, 20) has
    second axis end (0, 20), radii 30 and 20, area 600π = 1884.956 and 5 DOF.
29. Extruded 10, that ellipse gives a body of volume 6000π = 18849.556 with centroid
    (0, 0, 5) and an elliptic side face whose STEP export holds an `ELLIPSE` with semi-axes
    30 and 20.
30. An ellipse with centre (5, 5), first axis end at 45° and radii 20 and 10 keeps its
    area 200π when rotated, and its radii stay 20 and 10 when its centre is dragged.
31. A partial ellipse with centre (0, 0), radii 30 and 20, running counter-clockwise from
    (30, 0) to (−30, 0), closed by the line between them, encloses 300π = 942.478 with its
    centroid at (0, 4·20/(3π)) = (0, 8.488).
32. A point with `PointOnCircle` on that ellipse, dragged to (0, 50), settles at (0, 20)
    (or wherever the solver's least move puts it, always on the curve).
33. A horizontal line made `Tangent` to an ellipse of radii 30 and 20 about the origin,
    first axis along x, settles at y = 20 (or −20, on the side it started); a vertical one
    at x = ±30.

## Splines

Studied on 2026-11-25 for issue #69 from S70–S78, in the same way. Implemented for issue
#207 from this section: the Spline and Control Point Spline tools, the relations below,
exact extrusion and STEP export. "Implementation decisions (#207)" at its end records the
choices made where the sources are silent; REA (`rea-agents` 6.10.1) was set up again and
its method kept (Observed, Inferred, Decision and Unknown apart). No new sources were
read and REA's analysis tools were not run, since the inputs are prose pages.

### Two kinds

- **Observed (S70).** There are two kinds: B-splines, defined by through points
  ("spline points") with handles, and style splines, single-span Bézier curves defined by
  control vertices that form a control polygon (hull). A B-spline has one span between
  each pair of through points; a style spline has no through points and one span.
- **Observed (S70).** Style splines are recommended where curvature continuity matters;
  they can be inferred tangent or equal-curvature to neighbours, dimensioned on their
  polygon's sides, mirrored and made self-symmetric.
- **Observed (S76).** A style spline's curve type is a Bézier curve or a B-spline of degree
  3, 5 or 7; inserting a control vertex raises a Bézier curve's degree. Rational style
  splines take a weight per control vertex.
- **Decision.** AnvilCAD's names: **Spline** (through points) and **Control Point Spline**
  (control vertices). Both are stored as non-rational B-splines in the sketch: a through-
  point spline as a cubic interpolating its points, a control-point spline as a clamped
  B-spline of its vertices (degree 3 by default, the Bézier case when it has degree + 1
  vertices). Rational weights are left out.

### Picks

- **Observed (S72).** Spline: click to place the first point, click each further point,
  and double-click to finish. A spline has two or more points.
- **Observed (S76).** Style spline: each click adds a control vertex; Alt with the final
  double-click adds a tangent relation at the last vertex. Snapping the second vertex to an
  inference line from a neighbouring entity makes the spline tangent there, and snapping
  the third makes it equal-curvature there.
- **Decision.** Both tools add points on each click and finish on a double-click or
  Enter; Escape abandons the spline. A spline started or ended at the end of a line or arc
  shares that point (Coincident, by sharing).

### Handles, tangency and curvature

- **Observed (S71, S74).** At each through point a handle sets the tangent direction and
  two tangent weights (one each side); dimensions can go to handles. A curvature control
  sets the radius of curvature at a point. "Proportional" keeps the shape when an end is
  dragged.
- **Observed (S73).** Relations available on a spline's end points include Coincident,
  Horizontal, Vertical, Midpoint, Fix and Merge; on its end handles Tangent and Equal
  Curvature; between handles Parallel, Horizontal and Vertical.
- **Observed (S43, S73).** Equal Curvature between two splines makes the radius of
  curvature and its direction match at their shared end (curvature continuity). It needs
  the two splines to share an end point. Torsion Continuity adds an equal rate of change
  of curvature.
- **Decision.** For a control-point spline, tangency at an end holds the end's first two
  control points on the line of the tangent, and equal curvature at a shared end is
  expressed on the first three control points of each, which is linear-plus-one-equation
  in the vertices and solves well. For a through-point spline the end tangent direction is
  a handle the user can relate; its weight is the derivative's length. Torsion continuity
  is left out.

### Dimensions and errors

- **Observed (S77).** A spline's length can be dimensioned (a fixed length that holds
  while its ends or shape change).
- **Observed (S43).** A spline end can be tangent to a line or arc.
- **Decision.** A spline whose points coincide, or a control-point spline with fewer than
  degree + 1 vertices, is not created.

### Features and export (when implemented)

- **Decision.** A closed profile containing a spline extrudes to a body with a B-spline
  side face (a linear extrusion of the curve); STEP export writes
  `B_SPLINE_CURVE_WITH_KNOTS`. Revolve, sweep and loft follow the kernel as it gains them
  (#143).

### API names (S78)

- `CreateSpline2` (superseded by `CreateSpline3`) takes a flat array of x, y, z values, at
  least two points, and a flag for natural ends. `ISketchSpline` has `GetPoints2`,
  `GetPointCount`, `IsStyleSpline`, `CurveType`, `CurveDegree`, `IsRationalCurve`,
  `GetControlVertexWeights`, `GetSplineHandles` and `InsertPoint`.

### Unknowns

1. The default end conditions of a through-point spline (natural, or "simulated natural"
   as the API's flag says) and its parametrisation (chord length or other).
2. What "Maintain Internal Continuity" and "Relax Spline" compute.
3. How tangent weights map onto derivative lengths.

### Implementation decisions (#207)

Each of these is a **Decision**; the sources above are silent on them or they are
AnvilCAD's own design.

- **Curve.** Both kinds are cubic and held as a chain of cubic Bézier spans
  (`anvil_sketch::spline`). The kernel's profiles take a span as a segment of its own
  (`Seg2::Cubic`, `anvil_brep::profile::Segment::Cubic`), so a spline is one entity in the
  sketch and one segment per span in a profile. This answers Unknown 1 for AnvilCAD: a
  spline through points is the C2 cubic interpolant, parametrised by chord length, with
  natural ends (no second derivative there). Two points make a straight span.
- **Control point spline.** The clamped cubic B-spline of its vertices with uniform
  knots, so it has (vertices − 3) spans; four vertices make one Bézier span (S70, S76).
  Degrees 5 and 7 and rational weights are left out; inserting a vertex is left out.
- **Handles.** A spline through points gets a handle at an end only when a relation
  needs that end's direction (Tangent, or Equal Curvature) or the user drags one in. The
  handle is a sketch point, the end span's inner control point: the end leaves towards
  it, with derivative 3 × (handle − end) ÷ chord length. This answers Unknown 3 for
  AnvilCAD: the weight is the handle's distance. A handle adds two freedoms, so a
  tangency on it leaves the weight free, as S71 shows the weights to be. Handles at
  interior points and curvature controls (S74) are left out.
- **Tangent.** Line to spline, at a shared end: the spline's end direction is along the
  line. Arc, partial ellipse or spline to spline, at a shared end: their normals there
  are parallel (`TangentCircles`). Away from a shared end the tools do not offer it.
  For a control point spline the solver turns the end's second vertex (the end's first
  two control points are then on the line of the tangent); for a spline through points,
  its handle.
- **Equal Curvature** (`Constraint::EqualCurvature`) between a spline and a line, arc,
  partial ellipse or another spline sharing an end: tangent there, and equal curvature
  vectors along the normal (residual scaled by the curves' size squared, so it is a
  length like the solver's other residuals). It includes tangency, as S43 describes.
- **Coincident** (a point on a spline) holds the point at zero signed distance from the
  curve, positive on the left of its travel, so the residual is smooth across it.
- **Length.** The Dimension tool on a spline adds its length (`ArcLength`, S77); the
  solver holds it.
- **Fix** on a spline fixes every point that shapes it: through points or vertices, and
  handles.
- **Tools.** Each click adds a point; a click on the last point (the second click of a
  double-click) or Enter finishes; Escape abandons; too few points leave the tool waiting
  with a message. A click on an existing point shares it, so a spline drawn from a line's
  end closes a profile with it. No relations are inferred beyond that. While the sketch is
  edited a control point spline shows its control polygon and a spline its handles, as
  thin dashed lines.
- **Errors.** A spline whose neighbouring points coincide, or whose ends meet, is not
  created; nor is a control point spline of fewer than four vertices. Closed (periodic)
  splines are left out.
- **Editing tools.** Move, Copy, Rotate, Scale, Mirror and the patterns carry a spline's
  points (a similarity maps either kind onto the spline of the moved points). Trim,
  Extend, Split, Sketch Fillet, Sketch Chamfer and Offset refuse a spline with a plain
  message, as for ellipses.
- **Features.** Extrusions of profiles with splines are exact: each span's side is a
  cubic-by-linear B-spline face. Revolve, loft, drafted extrude and sweep paths refuse a
  spline in the exact kernel (#143), so the part is built on the faceted kernel and says
  which feature made it so. Shell and thin features, which offset the profile, refuse a
  spline with a plain message.
- **STEP.** Each span's edges export as `B_SPLINE_CURVE_WITH_KNOTS` of degree 3 with four
  control points, and the side faces as `B_SPLINE_SURFACE_WITH_KNOTS`; reading the file
  back gives B-spline curves again.
- **Scripts.** `anvil.sketch{ splines = ..., control_splines = ... }` and the MCP
  `add_sketch` tool take lists of points.

### Testable statements (splines)

34. The control point spline on (0, 0), (0, 30), (40, 30), (40, 0) is one Bézier span; it
    passes through (20, 22.5) at its middle and, closed by its base, encloses 720 (3/5 of
    its box) with centroid height 9 × 30 / 28 = 9.643. Extruded 10 it is a body of volume
    7200, and its STEP export holds two cubic `B_SPLINE_CURVE_WITH_KNOTS`.
35. The clamped uniform cubic B-spline of five vertices has two spans joined at
    p1/4 + p2/2 + p3/4 with equal first and second derivatives.
36. The spline through (0, 0), (20, 10), (40, 0) has natural ends; closed by its base it
    encloses 250 (each span is y = 15u − 5u³ over x = 20u).
37. A spline through (0, 0), (20, 15), (40, 5) with its points fixed and its start
    tangent to the line from (−30, 0): its start handle comes onto y = 0 ahead of the
    start, the points stay put and 1 DOF (the handle's weight) is left.
38. A control point spline of four free vertices has 8 DOF; a spline through three free
    points has 6.
39. A control point spline of three vertices, or a spline repeating a point, is refused.

## Other entities (recommend deferring)

- **Ellipse.** Specified in "Ellipses" below (S66–S69) and implemented.
- **Parabola.** **Observed (S36, S58).** Click the focus, drag to size it, then click and
  drag along it to set its extent. Its parameters are start, end, focus (centre) and apex.
  **Decision.** Defer.
- **Conic.** **Observed (S37).** Click two end points, then the top vertex (where the end
  tangents meet), then drag to set Rho, the ratio of the curve's height to the top vertex's
  height. Rho decides whether the curve is elliptical, parabolic or hyperbolic. "Auto
  tangency" makes it tangent at both ends. **Observed (S58).** The API's `CreateConic`
  takes a focus, an apex and two end points, which does not match the help's description;
  the difference is not explained. **Decision.** Defer.
- **Spline.** Specified in "Splines" below (S70–S78) and implemented.
- **Text.** **Observed (S39, S58).** Text is placed along selected curves or edges, or
  horizontally from the origin, with a font, alignment, flips, width factor and spacing,
  and can then be extruded or cut. **Decision.** Defer; it needs a font engine and
  outline conversion.

## Testable statements

Lengths in mm, angles in degrees. "DOF" is the degrees of freedom the solver reports for
the new geometry alone, with nothing else in the sketch except the origin.

1. A corner rectangle from (0, 0) to (20, 10) has corners (0, 0), (20, 0), (20, 10),
   (0, 10), area 200 and 4 DOF.
2. A center rectangle with centre (0, 0) and corner (15, 10) has corners (±15, ±10), area
   600, two construction diagonals of length 36.056 that do not count as profile
   geometry, and 4 DOF. Moving the centre point to (5, 5) while the width and height stay
   fixed moves every corner by (5, 5).
3. A 3 point center rectangle with centre (0, 0), second pick (6, 8) and a third pick at
   distance 5 from the line through the first two has corners (2, 11), (10, 5), (−2, −11)
   and (−10, −5): length 20, width 10, area 200, 5 DOF.
4. A 3 point corner rectangle with picks (0, 0), (30, 40) and a third pick 20 from that
   edge has area 1000, sides 50 and 20, and 5 DOF.
5. A parallelogram with picks (0, 0), (20, 0), (25, 10) has fourth corner (5, 10), area 200
   and 6 DOF.
6. Any rectangle drawn with Shift held has equal adjacent sides; a corner square from
   (0, 0) dragged to (10, 7) is solved to sides of equal length.
7. A straight slot with end-arc centres (0, 0) and (30, 0) and width 10 has:
   - end arcs of radius 5;
   - side lines from (0, ±5) to (30, ±5);
   - area 30·10 + π·25 = 378.540 and perimeter 60 + 10π = 91.416;
   - overall length 40;
   - 5 DOF.
   Extruded 5, it has volume 1892.699.
8. Changing that slot's width to 20 (by its `Diameter` dimension) keeps the centres and
   gives area 30·20 + π·100 = 914.159.
9. A centerpoint straight slot with centre (0, 0) and second pick (15, 0) has end-arc
   centres (−15, 0) and (15, 0); its centre point is the midpoint of the construction line.
10. With "Overall Length" and dimensions on, a slot whose overall length is set to 40 and
    width to 10 has a centre-to-centre length of 30.
11. A centerpoint arc slot with centre (0, 0), radius 20, a 90° centre-to-centre sweep
    and width 6 has an outer arc of radius 23, an inner arc of radius 17, end arcs of
    radius 3, and area (π/2)·20·6 + 9π = 69π = 216.770.
12. A 3 point arc slot through (−20, 0), (20, 0) and (0, 20), width 4, has a centre arc of
    radius 20 about (0, 0) sweeping 180°, and area π·20·4 + 4π = 84π = 263.894.
13. An arc slot with width ≥ twice its radius is not created.
14. A 6-sided polygon with a *circumscribed* circle of diameter 20 has sides of 10 and area
    259.808.
15. A 6-sided polygon with an *inscribed* circle of diameter 20 has sides of 11.547 and
    area 346.410.
16. A 4-sided polygon with an inscribed circle of diameter 20 is a 20 × 20 square (area
    400); with a circumscribed circle of diameter 20 it has sides of 14.142 and area 200.
17. A 3-sided polygon with a circumscribed circle of diameter 20 has sides of 17.321 and
    area 129.904.
18. A polygon of any side count from 3 to 40 has 4 DOF; 2 or 41 sides are rejected.
    Changing the circle's diameter from 20 to 30 on a circumscribed hexagon gives sides of
    15.
19. A centerpoint arc with centre (0, 0), start (10, 0) and the pointer swept
    counter-clockwise to (0, 10) runs counter-clockwise from (10, 0) to (0, 10) (90°). The
    same picks swept clockwise give the 270° arc, stored counter-clockwise from (0, 10) to
    (10, 0).
20. A tangent arc from the end (10, 0) of a line from (0, 0) to (10, 0), ending at
    (10, 10) in the forward zone, has centre (10, 5), radius 5 and sweep 180°. Ending at
    (15, 5) instead gives centre (10, 5), radius 5, sweep 90° and length 7.854.
21. A normal arc from the same line end, leaving upward (left zone) and ending at (15, 5),
    has centre (15, 0) and radius 5; the centre lies on the line.
22. After a tangent arc is made from a line, dragging the arc's end keeps the arc tangent
    to the line (the solver keeps the centre's distance from the line equal to the radius).
23. A 3 point arc with start (−10, 0), end (10, 0) and third point (0, 5) has centre
    (0, −7.5), radius 12.5, sweep 106.260 and length 23.182. With third point (0, 10) it
    has centre (0, 0) and radius 10.
24. Three collinear points make no 3 point arc and no perimeter circle.
25. A perimeter circle through (0, 0), (10, 0) and (0, 10) has centre (5, 5) and radius
    7.071.
26. A line drawn from (0, 0) to (10, 0.3) (1.7° off horizontal) gets `Horizontal`; one to
    (10, 1) (5.7°) gets none; the same first line held with Ctrl gets none.
27. A point clicked on a line's midpoint marker gets `Midpoint`, and stays at the midpoint
    when the line's end is dragged.

## API names

For `anvil-migrate`, which reads a user's own licensed SOLIDWORKS through its published API
(names and values only):

- **Creation (S50–S58).** `ISketchManager`: `CreateCornerRectangle`,
  `CreateCenterRectangle`, `Create3PointCornerRectangle`, `Create3PointCenterRectangle`,
  `CreateParallelogram`, `CreateSketchSlot`, `CreatePolygon`, `CreateArc`,
  `Create3PointArc`, `CreateTangentArc`, `CreateCircle`, `CreateCircleByRadius`,
  `CreatePoint`, `CreateLine`, `CreateCenterLine`, `CreateEllipse`, `CreateEllipticalArc`,
  `CreateParabola`, `CreateConic`, `CreateSpline3`, `CreateConstructionGeometry`;
  `IModelDoc2::InsertSketchText`. `AddToDB` (S59) turns off inference while creating.
- **Slots (S52, S53, S61).** `ISketchSlot`: `CreationType`, `LengthType`, `Length`,
  `Width`, `CenterArcDirection` (−1 clockwise, 1 counter-clockwise), `GetCenterPoint`,
  `GetCenterPointHandle`, `GetSlotPoints`. `swSketchSlotCreationType_e`: `line` 0,
  `center_line` 1, `arc` 2, `3pointarc` 3. `swSketchSlotLengthType_e`: `CenterCenter` 0,
  `FullLength` 1. `ISketch::GetSketchSlots`, `GetSketchSlotCount` (S54).
- **Arcs (S56, S60, S62).** `ISketchArc`: `GetCenterPoint2`, `GetStartPoint2`,
  `GetEndPoint2`, `GetRadius`, `GetRotationDir`, `IsCircle`. `swTangentArcTypes_e`:
  `swForward` 1, `swLeft` 2, `swBack` 3, `swRight` 4.
- **Segment types (S64).** `swSketchSegments_e`: `swSketchLINE` 0, `swSketchARC` 1,
  `swSketchELLIPSE` 2, `swSketchSPLINE` 3, `swSketchTEXT` 4, `swSketchPARABOLA` 5. Full
  circles are arcs (`ISketchArc::IsCircle`).
- **Relations (S63).** `swConstraintType_e` values that map onto AnvilCAD relations:
  `HORIZONTAL` 4, `VERTICAL` 5, `TANGENT` 6, `PARALLEL` 7, `PERPENDICULAR` 8,
  `COINCIDENT` 9, `CONCENTRIC` 10, `SYMMETRIC` 11, `ATMIDDLE` 12 (midpoint),
  `ATINTERSECT` 13, `SAMELENGTH` 14 (equal), `FIXED` 17, `HORIZPOINTS` 25, `VERTPOINTS` 26,
  `COLINEAR` 27, `CORADIAL` 28, `MERGEPOINTS` 42, `FIXEDSLOT` 74, `SAMESLOTS` 75.
- **Preferences (S65).** `swUserPreferenceToggle_e`: `swSketchAutomaticRelations`,
  `swSketchInference`.
- **Not available.** No API method or enumeration for perimeter circles was found, and
  `CreatePolygon` returns plain sketch segments with no polygon object.
- **Inferred.** A polygon or rectangle read through the API arrives as lines and relations,
  so `anvil-migrate` can map it line by line. A slot arrives as an `ISketchSlot` plus its
  segments; mapping its `Width`, `Length` and `LengthType` lets AnvilCAD rebuild it with
  the relations above.

## Recommended scope

In order, given the current code (`Geom` has lines, circles and counter-clockwise arcs; the
sketcher has Line, Centerline, Rectangle, Circle, Centerpoint Arc and Point):

1. **Inference basics** in the sketcher: `PointOnLine`, `PointOnCircle` and `Midpoint` on
   clicks, and Ctrl to suppress. Everything below uses them.
2. **Rectangle family**: Center, 3 Point Corner, 3 Point Center and Parallelogram as
   builders in `sketch.rs` next to `add_rectangle`, with construction diagonals or midpoint
   lines, Shift for squares, and the relations table above.
3. **Centerpoint arc direction** from the pointer's sweep, then **3 point arc** and
   **tangent arc** (from lines first; zones, tangent and normal arcs), and line-to-arc
   autotransition later.
4. **Straight slots** (both tools), with centre-to-centre and overall length and the
   relations above. They need only existing relations.
5. **Polygon**, built from lines with the angle and equal recipe.
6. **Arc-to-arc tangency** (`TangentCircles`), then **arc slots**, tangent arcs from arcs,
   and the **perimeter circle**.
7. Leave out for now: ellipses, partial ellipses, parabolas, conics, splines and text
   (they need new geometry in the kernel, or a font engine); grouped slot and polygon
   entities, Fix Slot and Equal Slots; numeric input; grid, angle and length snaps.

## Unknown or not covered

1. Which relations each tool adds internally; the help shows them only in pictures. The
   relation sets above are AnvilCAD decisions.
2. The inference tolerances and the default snap angle.
3. Whether a straight slot's first two picks mark the arc centres or the outer ends when
   "Overall Length" is chosen, and which dimension carries a slot's width.
4. A polygon's default side count and circle type, which line its angle is measured on,
   and whether the dragged point is always a vertex.
5. How many picks a perimeter circle takes.
6. Whether "From Midpoints" lines meet at a centre rectangle's centre point, and whether
   the construction line choice persists between uses.
7. Why the API's `CreateConic` takes a focus and an apex while the help describes end
   points, a top vertex and Rho.
8. The default names of individual sketch entities.
