# Measure, Section View and inspection tools

What AnvilCAD's inspection tools do: **Measure** (sizes of one entity and distances and
angles between two), **Section View** (cut the displayed model with one to three planes
without changing it), and, briefly, zebra stripes, curvature display, the geometry check
and interference checking between the bodies of a multibody part. SOLIDWORKS users know the
equivalent tools well, so AnvilCAD follows their published behaviour where that makes sense.
This specification was written before the implementation. Today AnvilCAD has mass
properties (`anvil_kernel::Solid::mass_properties`, `anvil_brep::mass`), a status bar that
reports them, face and edge picking in the viewport (`crates/anvil-app/src/app.rs`), an
exact B-rep kernel (`crates/anvil-brep`) with booleans and a validity check, and multibody
exact results in `crates/anvil-model/src/rebuild.rs`.

## How this was studied

- **Inputs.** Only published information was used:
  - Dassault's public online help for SOLIDWORKS 2025, with older pages (2011, 2012, 2013,
    2017, 2019) where they say more than the 2025 page, the 2013, 2018 and 2021 "What's
    New" pages, and the eDrawings 2021 and 2025 help;
  - the 2024 API help and its programming guide.

  No SOLIDWORKS software, installer, library or file was downloaded, installed, run or
  analysed. No SOLIDWORKS licence was accepted on the machine used.
- **Retrieval.** The help site renders its articles with JavaScript and refuses a plain
  request. Each article was fetched over HTTPS with a browser user agent, and its text read
  from the page's embedded `__NEXT_DATA__` JSON (`props.pageProps.helpContentData.helpText`)
  by a script run with `python3 -I`. A missing page still returns HTTP 200 with "page cannot
  be found" in its data, so each page was checked. Link targets were read from the same
  JSON to find related pages, and page names were also found by web search restricted to
  `help.solidworks.com`. The text was paraphrased; none of it is copied into this
  repository. Pictures on the pages were not available to the script, so nothing here is
  taken from them.
- **REA.** REA (`rea-agents` 6.5.0, with the `third_party/rea` submodule at `3fc0075`) was
  set up and its research method followed:
  - each finding below is marked **Observed** (stated in a cited source), **Inferred**
    (our reasoning from the sources), **Decision** (AnvilCAD's own choice where the
    sources are silent) or **Unknown**;
  - every observation cites its source.

  REA's analysis tools were **not** run. They inspect artifacts (binaries, packages,
  captures, files), and the only lawful inputs for an interactive behaviour like this are
  prose documents. Pointing REA at SOLIDWORKS software is ruled out by
  [INTEROPERABILITY.md](../../INTEROPERABILITY.md). Measurements and section views are not
  stored in part files as features, so no sample file was studied.
- **eDrawings.** Several Measure details (which values each pair of selections reports) are
  published only for eDrawings, Dassault's viewer, not for SOLIDWORKS itself. They are used
  as evidence of the family's conventions and marked as such; they are not proof of what
  SOLIDWORKS's own Measure shows.

## Sources

Base A is `https://help.solidworks.com/2025/English/SolidWorks/sldworks/`. Base B is
`https://help.solidworks.com/2024/English/api/sldworksapi/`, and member pages there are
named `SolidWorks.Interop.sldworks~SolidWorks.Interop.sldworks.<Interface>~<Member>.html`.
Base C is `https://help.solidworks.com/2024/English/api/swconst/`. All were read on
2026-10-18.

| # | Topic | URL | Version |
|---|---|---|---|
| S1 | Measure tool dialog and options | A + `HIDD_MEASURE.htm` | 2025 |
| S2 | Using the Measure tool | A + `t_using_the_measure_tool.htm` | 2025 |
| S3 | Measure units and precision | A + `HIDD_MEASURE_OPTIONS_SELECTOR.htm` | 2025 |
| S4 | Projected surface area | A + `t_measure_proj_surf_area_bodies.htm` | 2025 |
| S5 | Dual units in Measure | A + `t_dual_dims_measure_tool.htm` | 2025 |
| S6 | Angular rotation between coordinate systems | A + `t_measuring_angle_rotation_coordsys.htm` | 2025 |
| S7 | Status bar | A + `r_status_bar.htm` | 2025 |
| S8 | Distance mates to cylinders (center, min, max, custom) | A + `c_assy_distance_mates_between_cylinders.htm` | 2025 |
| S9 | Measure tool (older wording: one entity gives its size, two give the smallest distance) | `https://help.solidworks.com/2012/English/SolidWorks/sldworks/HIDD_MEASURE.htm` | 2012 |
| S10 | Measure tool (2019 wording) | `https://help.solidworks.com/2019/English/SolidWorks/sldworks/HIDD_MEASURE.htm` | 2019 |
| S11 | What's New: Measure (point-to-point, arc conditions, history) | `https://help.solidworks.com/2013/English/WhatsNew/c_wn_measure_tool.htm` | 2013 |
| S12 | What's New tutorial: distance between two circles | `https://help.solidworks.com/2013/English/WhatsNew/t_wntut_measuring_distance_two_circles.htm` | 2013 |
| S13 | What's New tutorial: point-to-point mode | `https://help.solidworks.com/2013/English/WhatsNew/t_wntut_measuring_point_to_point.htm` | 2013 |
| S14 | What's New: Measure dialog enhancements | `https://help.solidworks.com/2018/English/WhatsNew/c_measure_tool_enhancements.htm` | 2018 |
| E1 | eDrawings: types of measurements (per-selection table) | `https://help.solidworks.com/2025/English/eDrawings/c_Types_of_Measurements.htm` | eDrawings 2025 |
| E2 | eDrawings: Measure pane (plane-cylinder gives normal distance) | `https://help.solidworks.com/2021/English/eDrawings/r_measure_pane.htm` and `https://help.solidworks.com/2021/English/WhatsNew/c_wn2021_edrawings_measure.htm` | eDrawings 2021 |
| E3 | eDrawings: measuring entities (measurements not saved) | `https://help.solidworks.com/2025/English/eDrawings/t_measuring_entities.htm` | eDrawings 2025 |
| S15 | Section View PropertyManager (models) | A + `HIDD_DVE_DYNAMIC_SECTION.htm` | 2025 |
| S16 | Section views in models (task) | A + `t_section_views_models.htm` | 2025 |
| S17 | Section view offset options | A + `c_section_view_offset_options.htm` | 2025 |
| S18 | Section view selection (truncated faces) | A + `c_section_view_selection.htm` | 2025 |
| S19 | Intersection zones in section views | A + `t_zonal_section_view_example.htm` | 2025 |
| S20 | Section View PropertyManager (older wording: keep cap color has no effect while open) | `https://help.solidworks.com/2013/English/SolidWorks/sldworks/HIDD_DVE_DYNAMIC_SECTION.htm` and the 2017 page at the same path | 2013, 2017 |
| S21 | Section views in models (legacy) | `https://help.solidworks.com/2011/English/SolidWorks/sldworks/LegacyHelp/Sldworks/Display/Section_Views_in_Models.htm` | 2011 |
| S22 | Zebra stripes | A + `c_zebra_stripes.htm`, `HIDD_ZEBRA_PROPERTIES.htm`, `t_viewing_part_zebra_stripes.htm` | 2025 |
| S23 | Curvature display | A + `c_Curvature.htm`, `t_setting_curvature_values.htm`, `t_rendering_curved_surfaces_color.htm`, `t_displaying_curvature_radius_values.htm` | 2025 |
| S24 | Check entity | A + `HIDD_CHECK_ENTITY.htm` | 2025 |
| S25 | Interference Detection PropertyManager | A + `HIDD_DVE_INTERFERENCE.htm` | 2025 |
| S26 | Interference detection for multibody parts | A + `t_intererence_detection_multibody_parts.htm` | 2025 |
| A1 | `IMeasure` interface and members | B + `IMeasure.html`, `IMeasure_members.html` | 2024 |
| A2 | `IMeasure` members (`Angle`, `ArcLength`, `ArcOption`, `Area`, `CenterDistance`, `ChordLength`, `DeltaX`, `Diameter`, `Distance`, `IsConcentricSpheres`, `IsIntersect`, `IsParallel`, `IsPerpendicular`, `Length`, `LengthDecimalPlaces`, `Normal`, `NormalDistance`, `Perimeter`, `Projection`, `ProjectionOption`, `Radius`, `SetProjectionEntity`, `SpericalCenterDistance`, `TotalArea`, `TotalLength`, `X`) | B + `IMeasure~<member>.html` | 2024 |
| A3 | `IMeasure::Calculate` | B + `IMeasure~Calculate.html` | 2024 |
| A4 | Example: measure selected entities | B + `Measure_Selected_Entities_Example_VB.htm` | 2024 |
| A5 | `IModelDocExtension::CreateMeasure` | B + `IModelDocExtension~CreateMeasure.html` | 2024 |
| A6 | `swMeasureArcCircleOption_e`, `swMeasureProjectOnOption_e` | C + `SOLIDWORKS.Interop.swconst~SOLIDWORKS.Interop.swconst.<enum>.html` | 2024 |
| A7 | `ISectionViewData` members | B + `ISectionViewData_members.html` | 2024 |
| A8 | `ISectionViewData` members (`FirstColor`, `FirstOffset`, `FirstPlane`, `FirstReverseDirection`, `FirstRotationX`, `GetDynamicCenterTransform`, `GetFirstPlaneParameters`, `GraphicsOnlySection`, `KeepCapColor`, `SectionedZones`, `ShowSectionCap`, `ZonalSection`) | B + `ISectionViewData~<member>.html` | 2024 |
| A9 | `IModelViewManager::CreateSectionViewData`, `CreateSectionView`, `GetSectionViewData`, `RemoveSectionView` | B + `IModelViewManager~<method>.html` | 2024 |
| A10 | Examples: create section view in model; get section view data | B + `Create_Section_View_in_Model_Example_VB.htm`, `Get_Section_View_Data_Example_VB.htm` | 2024 |
| A11 | `swZonalSectionViewZones_e` | C + `SOLIDWORKS.Interop.swconst~SOLIDWORKS.Interop.swconst.swZonalSectionViewZones_e.html` | 2024 |
| A12 | API units (metres, radians) | `https://help.solidworks.com/2024/English/api/sldworksapiprogguide/Overview/Units.htm` | 2024 |
| A13 | `IBody2::Check3`, `IModelView::DisplayZebraStripes`, `IModelDoc2::ViewDisplayCurvature`, `IAssemblyDoc::InterferenceDetectionManager`, `IModelDocExtension::CreateMassProperty2` | B + `<Interface>~<member>.html` | 2024 |

That is 26 user-help topics (about 40 pages), 3 eDrawings topics (4 pages) and 13 API
topics (about 45 pages).

## Measure

### Opening, selecting and clearing

- **Observed (S1, S2).** Measure is a non-modal dialog opened from the Tools toolbar or
  *Tools > Evaluate > Measure*. It stays open while the user switches documents, and its
  results update when the active document already has a selection.
- **Observed (S2).** It can be used while other tools are running.
- **Observed (S2).** Clicking an item that is already selected removes it from the
  measurement. Clicking empty space clears every selection. Right-click *Select* suspends
  measuring; clicking in the dialog resumes it.
- **Observed (S1, S10, S14).** The selection box lists up to six selections. Values can be
  copied with one click ("Quick Copy"), the font can be enlarged, and the dialog can be
  pinned so that Esc does not close it.
- **Observed (S1, S7).** When Measure is closed, common measurements of the current
  selection (for example an edge's length) appear in the status bar.
- **Observed (S9).** One selected entity reports its size (an edge's length, a face's
  area). Two selected entities report the **smallest distance** between them.
- **Observed (S1).** Callouts in the graphics area show the result next to the selection.
- **Observed (E3, for eDrawings).** Measurements are not saved with the file.
  **Inferred (S1).** In SOLIDWORKS too a measurement is transient: keeping one needs a
  sensor (*Create Sensor*, S1) or a reference dimension.
- **Decision.** AnvilCAD's Measure is a tool, not a feature. Nothing is added to the
  feature tree and nothing is saved, apart from the tool's own settings (see *Settings
  persistence*).

### What each selection reports

The table lists what AnvilCAD reports. Each row says where the behaviour comes from. Every
length is in model units (millimetres today); every angle in degrees in the UI. "Distance"
is always the minimum distance between the two selected entities as bounded sets (a face is
its trimmed region, an edge its trimmed curve), never between their underlying infinite
surfaces or curves.

| Selection | Reports | Source |
|---|---|---|
| One vertex or sketch point | X, Y, Z of the point | Observed (S1, A2 `X`/`Y`/`Z`) |
| One straight edge | Length | Observed (S9, A2 `Length`); E1 for eDrawings |
| One circular edge (full circle) | Diameter, radius, centre X/Y/Z, length (circumference) | Observed (A2 `Diameter`, `Radius`, "selected circle"); centre from E1 (eDrawings) |
| One circular arc edge | Radius, diameter, centre X/Y/Z, arc length, chord length | Observed (A2 `ArcLength`, `ChordLength`, `Radius`); centre from E1 |
| Any other edge (ellipse, spline) | Length | Inferred (A2 `Length` "of the selected entity") |
| One face | Area and perimeter | Observed (S9, A2 `Area`, `Perimeter`) |
| One cylindrical face | Area, perimeter, radius, diameter | Observed (A2 `Radius`/`Diameter` "arc or cylinder") |
| One spherical face | Area, perimeter, radius | Inferred (A2 `SpericalCenterDistance`, `IsConcentricSpheres` show spheres are recognised); Decision |
| Two vertices | Distance; dX, dY, dZ | Observed (S1 "Show XYZ Measurements"; E1) |
| Vertex and edge | Minimum distance; for a straight edge also the perpendicular distance to its line | E1 (eDrawings: perpendicular and closest distance) |
| Vertex and face | Minimum distance; for a planar face also the normal distance to its plane | E1 (eDrawings: perpendicular distance); S9 (smallest distance) |
| Two planar faces, parallel | Distance; normal distance; angle 0 | Inferred (E1 "normal or closest distance", A2 `NormalDistance`, `IsParallel`) |
| Two planar faces, not parallel | Minimum distance (0 if they touch); angle | E1 (angle if not parallel); A2 `Angle`, `IsIntersect` |
| Edge and face | Minimum distance; for a straight edge parallel to a planar face, the normal distance; angle between edge and face | E1 (eDrawings) |
| Two straight edges | Minimum distance; angle; dX, dY, dZ; total length | E1 (eDrawings); A2 `TotalLength` |
| Two arcs, circles or cylindrical faces | Distance per the arc/circle option (centre by default); dX, dY, dZ between the measured points; total length or total area | S1, S8, S12, A2 `CenterDistance`; E1 |
| Planar face and cylindrical face | Distance per the arc/circle option (centre: axis to plane) | E2 (eDrawings: normal distance); S8 (center is the default for a cylinder and a plane) |
| Two faces or two edges of any kind | Total area or total length as well | Observed (A2 `TotalArea`, `TotalLength`) |
| Two coordinate systems | Roll, pitch, yaw | Observed (S6) |

Details and conventions behind the table:

- **Observed (A2).** The API reports each quantity separately and returns −1 for a
  quantity that does not apply to the selection. `Calculate` returns false when the
  combination of selected types is not valid (A3).
  - **Inferred.** −1 is a sentinel. A real distance, length or area is never negative, but
    `X`, `Y`, `Z`, `DeltaX`, `DeltaY` and `DeltaZ` can be −1 legitimately (a point at
    x = −1 m), so `anvil-migrate` cannot tell them apart from "not applicable". Deltas
    being reported as −1 for "not applicable" also suggests the deltas are not signed,
    but that is not stated (see *Unknown*).
  - **Decision.** AnvilCAD's result type holds each quantity as an `Option`, never a
    sentinel.
- **Observed (A2).** `NormalDistance` is the distance normal to "the selected plane"
  between an entity and a plane; `Normal` is the same between a face and a plane;
  `Projection` is the distance projected onto the projection entity.
  - **Decision.** AnvilCAD reports a **normal distance** whenever one selection is planar
    (a planar face or reference plane) and the other is a point, a straight edge parallel
    to it, a planar face parallel to it, or a cylinder whose axis is parallel to it. It is
    the perpendicular distance from the plane to the other entity's point, line, plane or
    axis. For two parallel planar faces this is the perpendicular gap, whatever the faces'
    extents. The minimum distance can be larger when the faces do not overlap when one is
    projected onto the other.
- **Angles.**
  - **Observed (A2).** `Angle` is the angle between the two selected entities, in radians.
    `IsParallel` and `IsPerpendicular` are reported.
  - **Observed (E1, for eDrawings).** For an edge and a face, "parallel" means the edge is
    perpendicular to the face's normal.
  - **Decision.** Two straight edges: the angle between their direction lines, 0° to 90°.
    Edge and planar face: 90° less the angle between the edge's line and the face's normal
    line, 0° to 90°. Two planar faces: the angle between their normal lines, 0° to 90°, so
    two faces of a box report 90° and a 30° wedge reports 30°. Whether SOLIDWORKS ever
    reports the obtuse angle is **Unknown**.
- **Perimeter.**
  - **Decision.** A face's perimeter is the total length of all its boundary loops,
    outer and inner (a plate with a hole includes the hole's circumference). Seam edges
    that a closed surface needs only for parametrisation (for example a cylinder's seam)
    are not counted, because they are not boundaries. SOLIDWORKS's own treatment of inner
    loops and seams is **Unknown**.
- **Circles and cylinders.**
  - **Observed (A2).** `CenterDistance` is the distance between cylinder axes, circle
    centres, or both.
  - **Inferred.** For two cylinders it is defined only when the axes are parallel (the
    distance between two parallel lines). For a circle and a cylinder it is the distance
    from the circle's centre to the axis.
- **Coordinates and deltas.**
  - **Observed (S1).** X, Y and Z, and the deltas, are relative to the part origin, or to a
    chosen coordinate system (*XYZ Relative To*). That option appears only when the
    document has more than one coordinate system.
  - **Observed (S1).** With *Show XYZ Measurements* cleared, only the minimum distance is
    shown in the graphics area.
  - **Inferred (S1, S9).** For two entities other than two points, dX, dY and dZ are the
    components of the vector between the two closest points (or between the two points
    that the arc/circle option measures).
  - **Decision.** AnvilCAD shows dX, dY and dZ as absolute values in the dialog, and keeps
    the signed vector (second selection minus first) in its result for scripting.
- **Total length and area.**
  - **Observed (A2).** `TotalLength` and `TotalArea` are the sums for the two selected
    entities.
  - **Decision.** With more than two edges or faces selected, AnvilCAD sums all of them and
    does not report a distance.
- **Spheres.**
  - **Observed (A2).** The API reports the distance between two spherical faces' centres
    and whether two spheres are concentric.
  - **Decision.** The centre distance of two spherical faces is reported like that of two
    circles.

### Arc/circle options

- **Observed (S1, S11, S12).** When arcs, circles or cylindrical faces are selected, the
  distance shown can be *Center to Center*, *Minimum Distance*, *Maximum Distance* or
  *Custom Distance*. Custom sets a condition (center, minimum, maximum) for each of the two
  arcs separately. The callout has a drop-down to switch between them.
- **Observed (S2).** These options apply only when point-to-point mode is off.
- **Observed (S8, for distance mates).** *Center to Center* is the default for a cylinder
  and another cylinder, axis, edge, line, vertex, point or plane. *Maximum Distance* is
  not offered when the distance is less than the sum of the radii.
- **Observed (A4).** The API example sets `ArcOption` to 0 (center to center), with 1 for
  minimum and 2 for maximum.
- **Inferred (S8, S12).** Measuring along the line joining the centres, with centre
  distance d and radii r1 and r2, each condition moves the measured point along that line:
  centre adds 0, maximum adds the radius (the far side), minimum subtracts it (the near
  side). So the distance is d + s1·r1 + s2·r2, with s = −1 for minimum, 0 for centre and
  +1 for maximum:
  - minimum: d − r1 − r2;
  - maximum: d + r1 + r2;
  - minimum/centre: d − r1;
  - centre/maximum: d + r2.

  This holds exactly for coplanar circles and for cylinders with parallel axes, which is
  the case the sources describe. For a cylinder and a plane parallel to its axis, d is the
  axis-to-plane distance and only the cylinder carries a radius term: minimum is d − r,
  maximum is d + r.
- **Decision.** Where the two circles overlap or one contains the other (d < r1 + r2), the
  minimum is reported as 0 for overlapping circles and as r_big − r_small − d for nested
  ones. The maximum is always d + r1 + r2. For circles that are not coplanar and
  cylinders whose axes are not parallel, AnvilCAD reports the true minimum or maximum
  distance between the curves or surfaces, found numerically, and the centre distance
  between the centres (circles) or the closest points of the two axes (cylinders).
- **Observed (A6).** The custom combinations are named first-to-second, for example
  `swMeasureArcCircle_CustomMinimumToCenter` = 8. **Inferred.** The first word is the
  condition on the first selection.

### Projected On

- **Observed (S1, A6).** *Projected On* is *None* (0 in the API), *Screen* (1) or
  *Select Face/Plane* (2). With a face or plane, the dialog adds
  *Projection* (the distance projected onto that plane) and *Normal* (the distance along
  its normal). With *None*, neither is calculated.
- **Inferred.** If the vector between the two measured points is v and the plane's unit
  normal is n, Normal = |v·n| and Projection = |v − (v·n)n|, so
  Projection² + Normal² = Distance².
- **Inferred.** *Screen* uses the view plane: Normal is the component along the view
  direction and Projection the component in the screen plane. Its value changes as the
  view rotates.
- **Observed (S4).** With bodies, faces or components selected and a face or plane
  chosen, *Projected Area* is the area of the selection's silhouette on that plane.
  **Inferred.** It is the area of the union of the projected faces, not the sum, so a
  closed box projected along one axis gives one face's area, not two.

### Point-to-point mode

- **Observed (S1, S11, S13).** Point-to-point mode measures between any two clicked
  points on the model, not just vertices. It snaps to faces and edges, and to midpoints,
  quadrant points and intersections. Hovering an edge offers its midpoint, as in a sketch.
  It reports the straight-line distance between the two points (and the deltas).
- **Decision.** AnvilCAD's points come from the viewport pick: the hit point on a face, or
  the nearest point on an edge, with snapping to vertices, edge midpoints and circle
  quadrants and centres first.

### Units and precision

- **Observed (S3).** *Use document settings* (default) or *Use custom settings*: a length
  unit, scientific notation, decimal or fractions (fractions with a denominator, such as 8
  for steps of 1/8 in) for inch-based units, decimal places for lengths, an angular unit
  with its decimal places, and an accuracy slider (faster to more accurate).
- **Observed (S5).** A second unit can be shown alongside the first (dual units).
- **Observed (A2, A12).** Through the API every length is in metres and every angle in
  radians, whatever the display units. `LengthDecimalPlaces` and `AngleDecimalPlaces`
  set the displayed precision only.
- **Decision.** AnvilCAD computes in millimetres and radians at full precision and rounds
  only for display. A fraction is shown rounded to the nearest step and reduced (2/8 is
  shown as 1/4). The default display is 3 decimal places for lengths and 2 for angles.
  The accuracy slider is not needed while every supported case is computed analytically
  or to the kernel's tolerance.

### Settings persistence

- **Observed (S11).** Measure keeps its settings from the last time it was used, and a
  session history lists earlier measurements (*Measurement History*, S1).
- **Decision.** AnvilCAD keeps the settings in the user's preferences. History is
  deferred.

### Other selection sources

- **Observed (S18).** In a section view, the cut faces and edges can be selected and
  measured, and section properties calculated for a cap face. Faces truncated by the
  section are measured as truncated.
- **Observed (S1).** Measure also works in sketches and drawings.

## Section View

### What it is

- **Observed (S16, S21).** A section view shows a part or assembly as if cut by planes or
  faces the user chooses, to reveal the inside. It is a display state, not a feature. It is
  switched on and off from the View toolbar (*View > Display > Section View*); the state is
  kept when the document is saved and reopened. Editing it again is a right-click command
  (*Section View Properties*).
- **Observed (S16).** It can be saved as a named view, or as an annotation view that a
  drawing can place. It cannot be applied to a mesh body.
- **Observed (S16, S21).** Zebra stripes are not available while a section view is active.
- **Inferred (S16).** The model itself is unchanged: mass properties and features are not
  affected.

### Section method

- **Observed (S15, S16).** *Planar* (one, two or three planes or planar faces) or *Zonal*
  (pick zones formed by the planes and the model's bounding box). Transparent sectioning
  needs *Zonal*.
- **Observed (A11).** In the zonal method, the zones are the 2, 4 or 8 regions made by one,
  two or three planes, numbered by which side of each plane they are on (zone 1 is on the
  front of all of them; with two planes, zone 2 is behind plane 1 and in front of plane 2;
  zone 3 in front of plane 1 and behind plane 2; zone 4 behind both). The bit values are
  1, 2, 4, …, 128. The one-plane row of the API table names "plane 2" for zone 2, which
  reads as a typing error for plane 1.
- **Inferred (S15, S16, A11).** In the planar method with two or three planes, the part
  removed is the region that is on the cut side of **every** plane (a quarter for two
  planes, an eighth for three), leaving an L-shaped or stepped view. Zonal, by contrast,
  can remove several of those regions. The help shows this only in pictures that were not
  available, so it is marked Inferred.
- **Decision.** AnvilCAD implements the planar method with that rule: a point is hidden
  when it is on the removed side of all active section planes.

### Each section plane

- **Observed (S15, S16, A7).** Section 1 is always present. Section 2 is enabled by a
  check box, and Section 3 appears only once Section 2 is on. Each section has:
  - a plane or planar face, or one of the buttons for the Front, Top and Right planes;
  - *Reverse Section Direction*, which flips which side is cut away;
  - *Offset Distance* from the plane or face;
  - *X Rotation* and *Y Rotation*, which tilt the section plane about two axes;
  - its own colour (*Edit Color*).
- **Observed (A10).** The API example sets offsets in metres (−0.01, 0.01) and rotations in
  radians (0.2967 = 17°, 0.1745 = 10°). **Observed (A12).** That is the API's general
  rule.
- **Observed (A8).** The API can read back each transformed plane as a centre point and a
  normal vector (`GetFirstPlaneParameters`).
- **Observed (S15, S16).** In Section 1 only, *Plane about axis* places the plane through
  an axis, temporary axis, hole, or cylindrical face or edge, optionally starting at a
  vertex or point, and rotated about the axis by an angle (or by dragging a handle).
  Reverse direction flips it across the plane.
- **Observed (S17).** *Offset Method* is *Reference Plane* (default) or *Selected Plane*.
  With Reference Plane, the offset is measured along the normal of the section plane as
  rotated. With Selected Plane, it is measured along the normal of the plane chosen in
  Section 1, before rotation.
- **Inferred (S17).** With Reference Plane, rotating the plane by θ and offsetting by t
  puts the plane at distance t from the point it rotates about. With Selected Plane, the
  rotated plane passes through the point moved t along the original normal, so its own
  distance from the rotation point is t·cos θ.
- **Unknown.** Which in-plane axes X Rotation and Y Rotation use, in which order they
  apply, about which point they turn (the API's `GetDynamicCenterTransform` relates the
  plane to "the center of the model", A8), and which side a new section removes before
  any reversal.
  - **Decision.** The plane rotates first about its own x axis, then about its rotated y
    axis. The pivot is the projection of the model's bounding-box centre onto the
    (unrotated, unoffset) plane. The default removes the side the plane's normal points
    to, so viewing along the normal's opposite (from the Front for the Front plane) shows
    the cap face-on. Angles and offsets are signed.

### Section options

- **Observed (S15, S20).** *Show section cap* fills the cut with a cap; clearing it shows
  the hollow inside. *Keep cap color* keeps the section colour on the cap after the
  PropertyManager closes. Older help (S20) adds that it has no effect while the
  PropertyManager is open, where the cap is always coloured.
- **Observed (A8).** `KeepCapColor` applies only when `ShowSectionCap` is true and
  `GraphicsOnlySection` is false.
- **Observed (S15, A8).** *Graphics-only section* is faster: the model is not rebuilt, the
  cut faces and edges cannot be selected, and the cap colour is always kept (Keep cap
  color is forced on). Pixels lying exactly in a section plane are not hidden.
- **Observed (S15, A7).** Bodies (in a multibody part) or components can be included in or
  excluded from sectioning (*Section by Body or Component*, *Exclude selected* or
  *Include selected*). With the zonal method, chosen bodies can be shown transparent at a
  set transparency.
- **Inferred (S15, S18).** Without graphics-only, the cut is computed geometrically: the
  cut faces exist, can be selected and measured (S18), and caps are real faces with
  section properties.
- **Decision.** When not graphics-only, the cap colour is the body's colour unless Keep cap
  color is set; while the section editor is open, caps always use the section colour.

### Errors and limits

- **Observed (S16).** A mesh body cannot be sectioned.
- **Observed (A9).** `CreateSectionView` reports whether the view was created, not whether
  valid section bodies were made, unless `Redraw` is set.
- **Decision.** If a section plane misses the body, the body is shown whole (all on the
  kept side) or not at all (all on the removed side). This is not an error. A plane
  given by a non-planar face is refused with a message.

## Zebra stripes (brief)

- **Observed (S22).** Zebra stripes imitate reflections of long light strips on a shiny
  surface. They show wrinkles, and they tell the continuity between adjacent faces:
  stripes that break at the boundary mean contact only (G0), stripes that meet but turn
  sharply mean tangency (G1), and stripes that flow smoothly mean curvature continuity
  (G2).
- **Observed (S22).** Settings: number of stripes, stripe width, accuracy, stripe and
  background colours, horizontal or vertical stripes, or an environment image from a
  file. Zebra stripes can be applied to selected faces only.
- **Observed (S16).** Not available with an active section view.
- **Inferred.** Stripes are a function of the reflected view direction, so they move as
  the view rotates and are drawn per pixel, not stored in the model.

## Curvature display (brief)

- **Observed (S23).** Faces are coloured by curvature, defined as 1/radius in model units.
  The default scale runs from 0.001 (black) through blue and green to 1.0 (red). A plane
  has curvature 0. Hovering shows the curvature and radius at the pointer. The scale has
  five editable values, with colours interpolated between them, and the help lists radius
  and curvature pairs (radius 0.125 gives 8, radius 4 gives 0.25, radius 500 gives 0.002).
- **Unknown.** Which curvature a doubly curved face shows (maximum principal, mean or
  Gaussian).
  - **Decision.** AnvilCAD shows the larger absolute principal curvature, so a cylinder of
    radius r shows 1/r, like the help's radius table.

## Check (brief)

- **Observed (S24).** *Tools > Evaluate > Check* checks all solids and/or surfaces,
  selected faces or edges, or features. It looks for invalid faces, invalid edges, short
  edges (shorter than a given length), open surfaces, and reports the minimum radius of
  curvature, maximum edge gap and maximum vertex gap. A stringent option re-runs every
  check instead of reusing earlier results. Results list errors with their count; selecting
  one highlights it, and faulty items can be isolated.
- **Decision.** AnvilCAD's Check runs `anvil_brep::check::validate` on each exact body and
  adds the short-edge threshold, minimum radius of curvature (smallest cylinder, sphere,
  torus minor or circle radius; numerically for splines) and the largest edge-to-vertex
  and edge-to-face gaps.

## Interference between bodies (brief)

- **Observed (S25, S26).** Interference detection on a multibody part lists every pair of
  bodies that overlap, with the volume of each overlap. Selecting a result highlights it.
  Bodies can be excluded. Options include treating coincidence as interference, showing
  ignored interferences, making interfering bodies transparent, ignoring hidden bodies,
  and the display of non-interfering bodies (wireframe, hidden, transparent, current).
  Interferences can be ignored, and stay ignored on later runs.
- **Observed (S25).** Surface-body interferences have no volume. *Treat coincidence as
  interference* reports coincident entities as interferences; **Inferred:** without it,
  bodies that only touch are not reported. Curved geometry that is coincident or nearly
  so can give false positives because facets collide.
- **Decision.** AnvilCAD intersects each pair of exact bodies with the kernel's boolean
  (`BoolOp::Intersect`) and reports pairs whose intersection has positive volume, with
  that volume. A pair of bodies that only touch is reported only with the coincidence
  option, with volume 0.

## Testable statements

Unless stated otherwise, the box is 100 × 50 × 20 mm occupying [0, 100] × [0, 50] × [0, 20],
and values are in millimetres and degrees.

Measure, one entity:

1. A vertex at (10, 20, 30) reports X = 10, Y = 20, Z = 30.
2. The box's edge along X reports length 100. Its top face reports area 5000 and
   perimeter 300.
3. A circular edge of diameter 20 reports diameter 20, radius 10, length 62.832 and its
   centre's coordinates.
4. A 90° arc of radius 10 reports arc length 15.708, chord length 14.142 and radius 10.
5. The side face of a cylinder of radius 5 and height 20 reports area 628.319,
   radius 5, diameter 10 and perimeter 62.832 (two circles; the seam is not counted).
6. A 40 × 40 square face with a central hole of diameter 10 reports area 1521.460
   (1600 − 25π) and perimeter 191.416 (160 + 10π).

Measure, two entities:

7. Vertices (0, 0, 0) and (30, 40, 0) report distance 50, dX 30, dY 40, dZ 0.
8. The box's top and bottom faces report distance 20, normal distance 20, angle 0, parallel.
9. Two adjacent faces of the box report distance 0 and angle 90.
10. A 10 × 10 face at z = 0 spanning [0, 10]² and a parallel 10 × 10 face at z = 5 spanning
    [20, 30] × [0, 10] report normal distance 5 and minimum distance 11.180 (√125).
11. Vertex (0, 0, 30) and the face z = 0 spanning [−10, 10]² report distance 30.
12. Vertex (20, 0, 10) and the face z = 0 spanning [0, 10]² report minimum distance 14.142
    (√200) and normal distance 10.
13. A straight edge from (0, 0, 0) to (10, 0, 10) and the face z = 0 report angle 45.
14. A straight edge along X at z = 0 and one along Y at z = 10 (skew) report distance 10
    and angle 90.
15. Two coplanar circles of diameters 10 and 6 with centres 50 apart report: centre 50,
    minimum 42, maximum 58; custom minimum/centre 45, centre/maximum 53, minimum/maximum 48,
    maximum/minimum 52.
16. A cylindrical face of radius 5 whose axis is parallel to a plane at 30 from it reports
    centre 30, minimum 25, maximum 35.
17. Concentric circles of radii 10 and 4 (d = 0) report centre 0, minimum 6, maximum 14.
18. Vertices (0, 0, 0) and (30, 40, 50) report distance 70.711; projected on the XY plane,
    projection 50 and normal 50.
19. Selecting the box's top face, bottom face and one side face (three faces) reports total
    area 12000 (5000 + 5000 + 2000) and no distance.
20. The silhouette of the box projected onto the XY plane has projected area 5000.
21. Two coordinate systems, the second turned 30° about Z, report roll 0, pitch 0, yaw 30.
22. Selecting an item twice removes it; clicking empty space empties the selection.

Units:

23. 25.4 mm shows as 1.000 in with three decimal places.
24. 0.3 in with fractions at denominator 8 shows as 1/4 in.
25. Through the `anvil-migrate` mapping, 50 mm corresponds to an API value of 0.05 and
    90° to 1.5707963.

Section View (box centred on the origin: [−50, 50] × [−25, 25] × [−10, 10], volume 100000,
unless stated otherwise):

26. One section on the Right plane (x = 0), offset 0: the shown part has volume 50000 and a
    cap of area 50 × 20 = 1000. Reversing the direction shows the other half, also 50000.
27. The Front plane (z = 0) with X Rotation 90° gives a plane through the X axis
    perpendicular to the Front plane (y = 0): shown volume 50000, cap area 100 × 20 = 2000,
    whichever way the rotation turns.
28. On the box [0, 100] × [0, 50] × [0, 20], a Right-plane section offset 40 removing
    x > 40 shows volume 40000 with cap area 1000; reversed, 60000.
29. On the same box, adding a Top-plane section offset 25 removing y > 25 removes only the
    region x > 40 and y > 25: shown volume 70000; caps of area 500 (on x = 40) and
    1200 (on y = 25).
30. Adding a third, Front-plane section offset 10 removing z > 10 removes only
    x > 40, y > 25, z > 10: shown volume 85000.
31. Front plane rotated 30° about its X axis with offset 10: under Reference Plane the
    section plane is 10 from the pivot along its own normal; under Selected Plane it is
    8.660 (10·cos 30°).
32. Plane about axis on a cylinder whose axis is Z, starting at a vertex on +X and rotated
    30°, gives a plane containing the Z axis and the direction (0.866, 0.5, 0) (Decision:
    a positive angle turns counter-clockwise about the axis direction).
33. Turning a section view on and off, or saving and reopening, leaves the part's mass
    properties unchanged.
34. A graphics-only section offers no selectable cap faces; a non-graphics-only one offers
    a cap face whose measured area equals the cap areas above.

Curvature, check and interference:

35. A cylinder face of radius 5 shows curvature 0.2; a plane shows 0; a radius of 1000
    shows 0.001 (the black end of the default scale).
36. Check on a box with one edge filleted at radius 2 reports minimum radius of curvature 2.
37. Check with a short-edge threshold of 0.1 reports an edge of length 0.05 and not one of
    length 0.2.
38. Two 10 mm cubes at [0, 10]³ and [5, 15] × [0, 10] × [0, 10] in one part interfere with
    volume 500.
39. Cubes [0, 10]³ and [10, 20] × [0, 10] × [0, 10] (touching) report no interference
    by default, and one interference of volume 0 with the coincidence option on.

## API names

For `anvil-migrate` and scripting. Values only; the API works in metres and radians (A12).

- **Measure.** `IModelDocExtension::CreateMeasure` returns an `IMeasure` (A5).
  - Method: `IMeasure::Calculate(Entities)` (`null` measures the current selection;
    returns false for an invalid combination), `SetProjectionEntity(Entity)` (A3, A2).
  - Settings: `ArcOption` (`swMeasureArcCircleOption_e`), `ProjectionOption`
    (`swMeasureProjectOnOption_e`), `LengthDecimalPlaces`, `AngleDecimalPlaces`.
  - Results (−1 when not applicable): `X`, `Y`, `Z`, `Length`, `ArcLength`,
    `ChordLength`, `Radius`, `Diameter`, `Area`, `Perimeter`, `TotalLength`, `TotalArea`,
    `Distance`, `CenterDistance`, `SpericalCenterDistance` (spelled so in the API),
    `NormalDistance`, `Normal`, `Projection`, `DeltaX`, `DeltaY`, `DeltaZ`, `Angle`.
  - Flags: `IsParallel`, `IsPerpendicular`, `IsIntersect`, `IsConcentricSpheres`.
- **`swMeasureArcCircleOption_e` (A6).** `swMeasureArcCircle_CenterToCenter` = 0,
  `_MinimumDistance` = 1, `_MaximumDistance` = 2, `_CustomCenterToCenter` = 3,
  `_CustomMinimumToMinimum` = 4, `_CustomMaximumToMaximum` = 5,
  `_CustomCenterToMinimum` = 6, `_CustomCenterToMaximum` = 7,
  `_CustomMinimumToCenter` = 8, `_CustomMaximumToCenter` = 9,
  `_CustomMinimumToMaximum` = 10, `_CustomMaximumToMinimum` = 11.
- **`swMeasureProjectOnOption_e` (A6).** `swMeasureProjectOn_None` = 0,
  `swMeasureProjectOn_Screen` = 1, `swMeasureProjectOn_FaceOrPlane` = 2.
- **Section view (A7–A10).** `IModelViewManager::CreateSectionViewData`,
  `CreateSectionView(SectionData)`, `GetSectionViewData(ViewName)` (empty name for the
  active view; null when not sectioned), `RemoveSectionView`.
  - `ISectionViewData` per section (First/Second/Third): `…Plane` (plane or planar face),
    `…Offset`, `…RotationX`, `…RotationY`, `…ReverseDirection`, `…Color` (RGB);
    `Get…PlaneParameters(PlaneCenter, PlaneVector)`; `GetDynamicCenterTransform(Index)`.
  - Whole view: `ShowSectionCap`, `KeepCapColor`, `GraphicsOnlySection`, `Redraw`,
    `ZonalSection`, `SectionedZones` (`swZonalSectionViewZones_e`), `SelectiveSection`,
    `SelectiveSectioningList`, `ExcludeSelectedItems`, `TransparentSection`,
    `TransparencyList`, `TransparencyValue`, `SectionTransparentItemsTransparent`.
  - Plane selection for Section 1, 2, 3 uses selection marks 1, 2 and 4 (A8, A10).
- **`swZonalSectionViewZones_e` (A11).** `swZonalSectionViewZones_swZonalSectionViewZone_1`
  = 1, `_2` = 2, `_3` = 4, `_4` = 8, `_5` = 16, `_6` = 32, `_7` = 64, `_8` = 128.
- **Others (A13).** `IModelView::DisplayZebraStripes` (on/off),
  `IModelDoc2::ViewDisplayCurvature` (toggle), `IBody2::Check3` (returns an
  `IFaultEntity`), `IAssemblyDoc::InterferenceDetectionManager` (assemblies; an API entry
  for multibody-part interference was not found), `IModelDocExtension::CreateMassProperty2`.

## Recommended scope

AnvilCAD has face and edge picking in the viewport, exact bodies (planes, cylinders,
cones, spheres, tori, splines; lines, circles, ellipses, splines), booleans, a validity
check and mass properties. It has no vertex picking, no coordinate systems and no unit
system yet (everything is millimetres). In order:

1. **Measure core (library).** A pure function in the model or kernel layer that takes one
   or two picked entities (vertex, edge, face, reference plane) of an exact body and returns
   a result struct of `Option` fields: coordinates, length, arc length, chord, radius,
   diameter, centre, area, perimeter, totals, minimum distance with the two closest points,
   normal distance, centre distance, deltas, angle and the parallel/perpendicular flags.
   Analytic cases first (point, line, circle, plane, cylinder, sphere); numerical
   closest-point search for the rest using the existing `closest` routines. Tests 1–19.
2. **Measure in the app.** A non-modal Measure window from the Evaluate tab, using the
   existing face and edge picks plus new vertex picking; toggling a pick, clearing on empty
   space, a callout in the viewport, and the length or area of a single selection in the
   status bar when Measure is closed.
3. **Arc/circle options and Show XYZ** (tests 15–17), then **units and precision**
   (tests 23–24), then **projected on plane** and **point-to-point** with snapping
   (tests 18, 20).
4. **Section View, graphics-only, one plane.** Clip planes in the renderer's shader with a
   cap drawn per plane; Front, Top and Right buttons, a picked plane or planar face,
   offset, X and Y rotation, reverse, show cap and colour. Store it as view state in the
   document (not a feature). Then two and three planes with the "removed where on the cut
   side of all planes" rule.
5. **Exact section.** Intersect each body with the kept region (half-spaces built as a
   large box) using the boolean, so caps are real faces that Measure can pick, and the cap
   area can be tested exactly (tests 26–30, 34).
6. **Interference between bodies** of a multibody result via `BoolOp::Intersect`
   (tests 38–39) and **Check** built on `anvil_brep::check::validate` (tests 36–37).
7. **Curvature and zebra stripes** as shader display modes (test 35).

Leave out for now: measurement history, sensors, quick copy and font size, dual units,
accuracy slider, coordinate-system rotation (until coordinate systems exist), measuring
in drawings, zonal sections, transparent and selective (per-body) sectioning, plane about
axis, saving sections as named or drawing annotation views, and projected silhouette
area (it needs a union of projected faces).

## Unknown or not covered

1. Whether dX, dY and dZ are signed, and, for pairs other than two points, exactly which
   two points they are measured between.
2. Whether the angle between two faces or two edges is ever reported as obtuse, and how
   the angle between non-planar faces (a cylinder and a plane, say) is defined.
3. Whether a face's perimeter includes inner loops and seam edges.
4. What SOLIDWORKS's Measure shows for two parallel planar faces whose projections do not
   overlap (minimum distance, normal distance, or both). The per-selection table is
   published only for eDrawings (E1), not for SOLIDWORKS itself.
5. Minimum and maximum distances for circles that overlap, nest or are not coplanar, and
   for cylinders whose axes are not parallel.
6. Which axes X Rotation and Y Rotation of a section plane use, their order, the point
   they turn about, and which side a new section removes before reversal.
7. The exact region removed by a planar section with two or three planes. It is described
   only in pictures that were not available.
8. How "Graphics-only section" draws its cap, and what "Keep cap color" changes for a
   part (the help's table is for an assembly).
9. Which curvature a doubly curved face shows in the curvature display.
10. How Check computes edge and vertex gaps, and its default short-edge length.
11. The API for interference detection between bodies of a part.
12. How *Projected On: Screen* and projected area treat overlapping faces.
