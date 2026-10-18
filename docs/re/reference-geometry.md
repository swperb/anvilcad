# Reference geometry

What AnvilCAD's reference geometry does: construction planes, axes, points and coordinate
systems that other features are placed on or measured from, plus the centre-of-mass point
and the origin. SOLIDWORKS users know these as its reference geometry tools, so AnvilCAD
follows their published behaviour where that makes sense, and `anvil-migrate` needs the
same vocabulary to carry them across. Today AnvilCAD has only one kind of reference
geometry: a plane offset from the Front, Top or Right plane, another plane or a planar face
(`PlaneFeature { base, offset, flip }` in `crates/anvil-model/src/document.rs`, rebuilt in
`crates/anvil-model/src/rebuild.rs`). This specification was written before the rest is
implemented.

## How this was studied

- **Inputs.** Only published information was used:
  - Dassault's public online help for SOLIDWORKS 2025, with the 2012, 2014–2016 and
    2020–2024 editions of some pages to date when options appeared;
  - the public SOLIDWORKS 2024 API help (interfaces, enumerations and its example
    programs);
  - two public third-party tutorials (T1, T2), used only where Dassault's pages are silent,
    and marked as such.

  No SOLIDWORKS software, installer, library or file was downloaded, installed, run or
  analysed. No SOLIDWORKS licence was accepted on the machine used.
- **Retrieval.** The help site renders its articles with JavaScript and refuses plain
  requests. Each article was fetched over HTTPS with a browser user agent, and its text
  read from the page's embedded `__NEXT_DATA__` JSON
  (`props.pageProps.helpContentData.helpText`) by a script run with `python3 -I`. Link
  targets were read from the same HTML to find related pages. A missing page still returns
  HTTP 200, so each page's content was checked for the "page cannot be found" text. Where
  the stripped text lost an icon that carried meaning (the rotation order on the coordinate
  system page), the icon's row in the raw HTML was read to identify it. The tutorials were
  read with a web fetch. All text was paraphrased; none of it is copied into this
  repository.
- **REA.** REA (`rea-agents` 6.5.0, with the `third_party/rea` submodule at `3fc0075`) was
  set up and its research method followed:
  - each finding below is marked **Observed** (stated in a cited source), **Inferred**
    (our reasoning from the sources), **Decision** (AnvilCAD's own choice where the
    sources are silent) or **Unknown**;
  - every observation cites its source.

  REA's analysis tools were **not** run. They inspect artifacts (binaries, packages,
  captures, files), and the only lawful inputs for this behaviour are prose documents.
  Pointing REA at SOLIDWORKS software is ruled out by
  [INTEROPERABILITY.md](../../INTEROPERABILITY.md). How reference geometry is stored in a
  `.sldprt` file was not studied.
- **What the sources do not give.** Dassault's pages describe reference geometry mostly by
  example pictures, which were not read. They rarely say which way a new plane's normal
  points, how an axis is directed, or how a coordinate system completes missing axes. Those
  rules below are AnvilCAD decisions, and are marked so.

## Sources

Base A is `https://help.solidworks.com/2025/English/SolidWorks/sldworks/`. Base B is
`https://help.solidworks.com/2024/English/api/sldworksapi/`. Base C is
`https://help.solidworks.com/2024/English/api/swconst/`. API member pages are
`B + SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.<Interface>~<Member>.html`
(some use the older `SolidWorks.Interop` casing); enumeration pages are
`C + SOLIDWORKS.Interop.swconst~SOLIDWORKS.Interop.swconst.<enum>.html`. All were read on
2026-10-18.

| # | Topic | URL | Version |
|---|---|---|---|
| S1 | Reference geometry overview | A + `c_Reference_Geometry_Overview.htm` | 2025 |
| S2 | Selecting reference geometry; updating reference geometry in templates | A + `c_selecting_reference_geometry.htm`, A + `t_updating_reference_geometry_templates.htm` | 2025 |
| S3 | Planes overview | A + `c_Planes_Overview.htm` | 2025 |
| S4 | Plane PropertyManager | A + `HIDD_DVE_CREATE_PLANE.htm` | 2025 |
| S5 | Plane PropertyManager, older editions (no "Parallel to screen" or "Flip Normal" in 2012; "Flip Normal" from 2014; "Parallel to screen" from 2015) | `https://help.solidworks.com/<year>/English/SolidWorks/sldworks/HIDD_DVE_CREATE_PLANE.htm` for 2012, 2014, 2015, 2016; the 2011 copy at `https://help.solidworks.com/2011/english/SolidWorks/sldworks/AllContent/SolidWorks/Core/Ref_Geom/HIDD_DVE_CREATE_PLANE.htm` matches 2012 | 2011–2016 |
| S6 | Creating planes | A + `t_Creating_Planes.htm` | 2025 |
| S7 | Planes normal to view; planes parallel to screen | A + `t_creating_planes_normal_to_view.htm`, A + `t_right_click_to_create_plane_parallel_to_screen.htm` | 2025 |
| S8 | Move, resize and copy planes | A + `c_Move_Resize_and_Copy_Planes.htm` | 2025 |
| S9 | Automatic sizing of planes and axes | A + `t_Automatic_Sizing_of_Planes_and_Axes.htm` | 2025 |
| S10 | Axes overview; creating axes; temporary axes | A + `c_Axis_Overview.htm`, A + `t_Creating_Reference_Axes.htm`, A + `t_Displaying_Temporary_Axes.htm` | 2025 |
| S11 | Axis PropertyManager | A + `hidd_dve_ref_axis.htm` (2012 edition compared, same options) | 2025, 2012 |
| S12 | Reference points overview; Point PropertyManager; single and multiple points | A + `c_Reference_Points.htm`, A + `HIDD_DVE_REFPOINT.htm`, A + `t_Creating_Single_Reference_Points.htm`, A + `t_Creating_Multiple_Reference_Points_Along_Curves.htm` | 2025 |
| S13 | Point PropertyManager, older edition (no "On point"; centre of face called centre of gravity) | `https://help.solidworks.com/2012/English/SolidWorks/sldworks/HIDD_DVE_REFPOINT.htm` (2014–2018 have "On point") | 2012–2018 |
| S14 | Coordinate systems overview; Coordinate System PropertyManager | A + `c_Coordinate_Systems.htm`, A + `HIDD_DVE_COORD_SYS.htm` | 2025 |
| S15 | Creating a coordinate system; with insufficient entities; referencing coordinate systems | A + `t_Creating_a_Coordinate_System.htm`, A + `t_Moving_a_Coordinate_System.htm`, A + `c_referencing_coordinate_systems.htm` | 2025 |
| S16 | Coordinate System PropertyManager, older editions (numeric position and rotation absent up to 2021, present from 2022) | `https://help.solidworks.com/<year>/English/SolidWorks/sldworks/HIDD_DVE_COORD_SYS.htm` for 2012, 2016–2023 | 2012–2023 |
| S17 | Centre of mass points: overview, adding, overriding, reference points, icons | A + `c_Center_of_Mass_Point_Ref_Geom.htm`, A + `t_add_com_point.htm`, A + `t_override_com_point.htm`, A + `t_create_com_ref_point.htm`, A + `r_Center_of_Mass_Icons_Ref_Geom.htm` | 2025 |
| S18 | Origin | A + `t_origin.htm` | 2025 |
| S19 | Where to start a sketch (default planes and views) | `https://help.solidworks.com/2024/English/SolidWorks/sldworks/c_Where_to_Start_a_Sketch.htm` | 2024 |
| S20 | `IFeatureManager::InsertRefPlane` | B + `IFeatureManager~InsertRefPlane` | 2024 |
| S21 | `swRefPlaneReferenceConstraints_e` | C + `swRefPlaneReferenceConstraints_e` | 2024 |
| S22 | `IRefPlaneFeatureData`: interface remarks, members, `Type`, `Type2`, `Constraint`, `Reference`, `AngleOrDistance`, `Distance`, `Angle`, `ProjectionType`, `SolutionIndex`, `ReversedReferenceDirection`, `Selections` | B + `IRefPlaneFeatureData`, `_members`, `~<Member>` | 2024 |
| S23 | `swRefPlaneType_e`, `swOnSurfacePlaneProjectType_e` | C + each | 2024 |
| S24 | Example: Insert Reference Plane (C#) | B + `Insert_Reference_Plane_Example_CSharp.htm` | 2024 |
| S25 | `IRefPlane` members, `Transform`, `CornerPoints`; example: Get the Normal and Origin of a Reference Plane Using Its Transform (VBA) | B + `IRefPlane_members`, `~Transform`, `~CornerPoints`; B + `Get_the_Normal_and_Origin_of_a_Reference_Plane_Using_Its_Transform_Example_VB.htm` | 2024 |
| S26 | Obsolete plane methods `IModelDoc2::CreatePlaneThru3Points3`, `CreatePlaneAtAngle3`, `CreatePlaneAtOffset3`, `CreatePlanePerCurveAndPassPoint3`, `CreatePlaneAtSurface3` | B + `IModelDoc2~<Method>` | 2024 |
| S27 | `IModelDoc2::InsertAxis2`; example: Create Revolve Features (VBA) | B + `IModelDoc2~InsertAxis2`; B + `Create_Revolve_Features_Example_VB.htm` | 2024 |
| S28 | `IRefAxisFeatureData` members and `Type`; `swRefAxisType_e`; `IRefAxis` members and `GetRefAxisParams` | B + `IRefAxisFeatureData_members`, `~Type`; C + `swRefAxisType_e`; B + `IRefAxis_members`, `IRefAxis~GetRefAxisParams` | 2024 |
| S29 | `IFeatureManager::InsertReferencePoint`; `swRefPointType_e`; `swRefPointAlongCurveType_e`; `IRefPointFeatureData` members, `Type`, `Distance`, `AlongCurveOption`; `IRefPoint::GetRefPoint`; example: Insert Reference Points (VBA) | B + each; C + each; B + `Insert_Reference_Points_Example_VB.htm` | 2024 |
| S30 | `IFeatureManager::InsertCoordinateSystem`, `CreateCoordinateSystemUsingNumericalValues`; `ICoordinateSystemFeatureData` members and `Transform`; example: Insert Coordinate System Feature at Center of Mass (VBA) | B + each; B + `Insert_Coordinate_System_Feature_at_Center_of_Mass_Example_VB.htm` | 2024 |
| S31 | `IFeatureManager::InsertCenterOfMass`, `InsertCenterOfMassReferencePoint`; `ICenterOfMass` members; example: Insert Center of Mass Feature (VBA) | B + each; B + `Insert_Center_of_Mass_Feature_Example_VB.htm` | 2024 |
| S32 | `IFeature::GetTypeName2` (feature type name table) | B + `IFeature~GetTypeName2` | 2024 |
| T1 | Javelin: how to create reference planes | `https://www.javelin-tech.com/blog/2025/07/how-to-create-solidworks-reference-planes/` | not Dassault, 2025 |
| T2 | GoEngineer: creating reference planes (offset, angle, mid, cylindrical surface) | `https://www.goengineer.com/blog/creating-reference-planes-in-solidworks` | not Dassault |

## Conventions used in this specification

- A **plane** is an origin point `o` and a unit normal `n`, with in-plane axes `x`, `y`
  such that `x × y = n`. This is AnvilCAD's `Frame` (`z` is the normal).
- A **line** is a point `p` and a unit direction `u`. An edge or sketch line is directed
  from its start to its end.
- `R(u, θ)` is the right-handed rotation by angle `θ` about the unit vector `u`.
- **Observed (S25).** A SOLIDWORKS reference plane's transform takes the Front Plane's
  canonical position (origin at (0, 0, 0), normal (0, 0, 1)) to the plane's own position;
  the plane's normal is the image of (0, 0, 1). So in SOLIDWORKS too, a plane has an origin
  and a signed normal.
- **Observed (S20, S22, S26).** The API works in metres and radians; the user interface
  shows document units and degrees.

## The origin and the default planes

- **Observed (S18).** The model origin is the model's (0, 0, 0). While a sketch is open, a
  sketch origin marks the sketch's (0, 0, 0). Dimensions and relations may attach to the
  model origin but not to a sketch origin. Origins can be hidden.
- **Observed (S19).** A new part has three default planes, Front, Top and Right, tied to
  the standard views: the Front view looks straight at the Front Plane. The plane chosen for
  the first sketch decides how the part sits relative to those views.
- **Observed (S25, S27).** The default planes are named "Front Plane", "Top Plane" and
  "Right Plane". **Observed (S2).** Templates can rename them and set which reference
  geometry is shown.
- **Observed (S25).** The Front Plane is the canonical plane: origin (0, 0, 0), normal
  +Z.
- **Inferred.** The Top Plane is y = 0 with normal +Y, and the Right Plane is x = 0 with
  normal +X. S19 ties them to the top and right views but no source states the normals.
  AnvilCAD already uses these frames (`PlaneRef::standard_frame`): Top has x = +X,
  y = −Z, normal +Y; Right has x = −Z, y = +Y, normal +X. **Decision.** Keep them.
- **Observed (S17).** In the tree the origin appears near the top, with the centre of mass
  just below it when one exists.
- **Unknown.** S32's type table does not list the origin. `anvil-migrate` already skips a
  type named `OriginProfileFeature` (`crates/anvil-migrate/src/export.rs`); that name comes
  from AnvilCAD's own code, not from a source read here.

## Planes

### The Plane command

- **Observed (S4, S6).** A plane is defined by up to three references (First, Second and
  Third Reference). Each reference is a selected entity plus a constraint. Which
  constraints are offered depends on the entity type. The software proposes the most likely
  constraint for a selection, and the user can change it.
- **Observed (S4, S6).** A message box reports the plane's status. The plane can be made
  only when it is "Fully defined".
- **Observed (S20).** The API makes the same plane from three (constraint, value) pairs and
  the entities selected with marks 0, 1 and 2.
- **Observed (S22).** Every plane made in SOLIDWORKS 2010 or later is constraint based
  (`Type2` returns `swRefPlaneConstraintBase`). Older planes have a fixed type
  (`swRefPlaneType_e`: three points, line and point, line and line, parallel, at angle,
  offset distance, normal to curve, on surface).
- **Observed (S8, S6).** Ctrl-dragging an existing plane makes an offset plane.
- **Observed (S3, S9).** Planes and axes are displayed with a size that follows the
  geometry they were made from, or the model's bounding box, and is 5% larger. Resizing by
  hand turns this off; "Autosize" turns it back on. **Decision.** Display size is not
  geometry; AnvilCAD may size its plane display the same way, and nothing depends on it.
- **Observed (S4, S6, S25).** New planes are named Plane1, Plane2, and so on, and can be
  renamed.

### Reference entities and constraints

The entity kinds a reference can be, and the constraints SOLIDWORKS offers for them:

| Constraint | Option name | Applies to | What it fixes |
|---|---|---|---|
| Coincident | "Coincident" | point, line, plane, face | the plane passes through it (contains a line; lies on a plane) |
| Parallel | "Parallel" | plane, planar face, line | orientation: same normal as the plane |
| Perpendicular | "Perpendicular" | plane, face, line, edge, curve | orientation: square to the reference |
| Project | "Project" | point, vertex, origin, coordinate system, with a non-planar surface | an on-surface plane at the projected point |
| Parallel to screen | "Parallel to screen" | vertex | normal along the current view direction |
| Tangent | "Tangent" | cylindrical, conical and other non-planar faces | the plane touches the surface |
| At angle | "At angle" | plane, planar face, cylindrical face (used with a line) | the angle to that reference |
| Offset distance | "Offset distance" | plane, planar face | parallel, at a distance |
| Mid plane | "Mid Plane" | two planar faces, reference planes or 3D sketch planes | halfway between them |

- **Observed (S4).** These constraints and their applicability are stated as in the table.
  At angle and Offset distance offer a **Number of planes to create**. **Flip Normal**
  reverses the plane's normal vector. Perpendicular to a curve has **Set origin on curve**.
  Project offers **Nearest location on surface** and **Along sketch normal**. Mid plane is
  chosen on both references.
- **Observed (S5).** "Flip Normal" is documented from 2014 and "Parallel to screen" from
  2015; the 2011 and 2012 pages have neither.
- **Observed (S21).** The API has a bit for each constraint and option, including
  `OptionFlip` and `OptionReferenceFlip`, which the help pages do not describe.
  **Inferred.** `OptionFlip` is the side flip of an offset or angle (the "flip" check box
  next to the value), and `OptionReferenceFlip` matches
  `IRefPlaneFeatureData::ReversedReferenceDirection`, which reverses the direction of one
  reference (S22). **Unknown.** Which UI control each bit drives.

### Which combinations define a plane

A plane has three degrees of freedom: two of orientation, one of position. Coincidence
with a point removes one; with a line, two; Parallel removes two; Perpendicular to a line
removes two; Perpendicular to a plane removes one; Offset distance from a plane removes
all three; At angle to a plane about a coincident line removes the one left by the line.

- **Observed (S4, T1, T2).** Each of these combinations is shown in a source:
  1. one plane or planar face with **Offset distance** `d` (S4, T1, T2);
  2. a plane or face **Parallel**, and a point **Coincident** (S4);
  3. an edge or curve **Perpendicular**, and a point **Coincident** (S4);
  4. a plane or planar face **At angle** `θ`, and an edge, axis or sketch line
     (**Coincident**) (S4, S24, T1, T2);
  5. a cylinder's axis **Coincident**, and a plane **At angle** (S4);
  6. two planar faces or planes, both **Mid Plane** (S4, T1, T2);
  7. a surface and a sketch point on it: **Tangent** (S4);
  8. a sketch point and a surface: **Project** (S4);
  9. a vertex: **Parallel to screen**, with an optional distance (S7);
  10. three points, all **Coincident** (T1; the legacy type `swRefPlaneThreePoint`, S23);
  11. a cylindrical face **Tangent** and a plane, or two cylindrical faces (T2).
- **Observed (S23, S26).** The legacy plane types also name "line and point"
  (`swRefPlaneLinePoint`) and "line and line" (`swRefPlaneLineLine`), so those
  combinations exist too.
- **Inferred.** Other combinations that remove exactly three degrees of freedom also
  define a plane: a line Coincident and a plane Perpendicular (the same as At angle 90°);
  a plane Perpendicular and two points Coincident; a line Parallel and a point Coincident
  is under-defined (one rotation is left) unless a second line or point is added.
- **Decision.** AnvilCAD accepts a set of references only when it removes exactly three
  degrees of freedom and has a unique answer up to the explicit flip options. Otherwise
  the feature shows "under defined" or "over defined" and is not created, matching S4's
  "Fully defined" rule. A combination whose references are degenerate (collinear points, a
  point on the line it is paired with, a line that is not parallel to the plane it is
  angled from, parallel planes for an intersection) is an error on the feature, not a
  panic.

### The geometry of each plane type

In each rule, `n₁` is the first plane reference's normal and `o₁` a point on it.

1. **Offset distance.** Origin `o₁ + d·n₁`, normal `n₁`, in-plane axes copied from the
   reference.
   - **Observed (S4).** The plane is parallel to the reference at distance `d`.
   - **Observed (S4, T2).** With **Number of planes** `N`, `N` planes are made; T2 says
     each is `d` from the one before. **Inferred.** The `k`th plane is at `k·d`
     (`k = 1 … N`).
   - **Decision.** A flip of the side puts the plane at `o₁ − d·n₁`, keeping the normal
     `n₁`. **Flip Normal** reverses the normal (and `x`, so that `x × y = n` still holds)
     without moving the plane. This splits today's single `flip` flag, which does both.
   - **Unknown.** Whether SOLIDWORKS's side flip also reverses the normal.
2. **Parallel through a point.** Origin: the point `P`, normal `n₁`.
3. **Coincident with three points** `P₁, P₂, P₃`.
   - **Decision.** Origin `P₁`, normal `(P₂ − P₁) × (P₃ − P₁)` normalized, `x` along
     `P₂ − P₁`. Error if the points are collinear (the cross product's length is below
     tolerance). **Unknown.** SOLIDWORKS's choice of normal sign for three points.
4. **Line and point** (line `p, u`; point `P` not on the line).
   - **Decision.** Origin `p`, normal `u × (P − p)` normalized, `x = u`.
5. **Two coplanar lines** (`p₁, u₁` and `p₂, u₂`).
   - **Decision.** If they intersect, normal `u₁ × u₂` normalized and origin the
     intersection. If they are parallel and distinct, normal `u₁ × (p₂ − p₁)` normalized
     and origin `p₁`. Skew or coincident lines are an error.
6. **At angle** `θ` (plane `o₁, n₁`; line `p, u` with `u · n₁ = 0`).
   - **Observed (S4).** If the line lies in the plane, the new plane rotates about the
     line. If the line is parallel to the plane but off it, the new plane moves to the line
     and rotates about it. **Observed (S4, T2).** With **Number of planes** `N`, each
     plane is a further `θ` from the one before.
   - **Observed (S24).** The API example makes a 45° plane from a face (At angle,
     π/4 rad) and an edge (Coincident).
   - **Decision.** The plane contains the line: origin `p` (the foot of `o₁` on the line),
     normal `R(u, θ)·n₁`, `x = u`. At `θ = 0` it is parallel to the reference through the
     line; at `θ = 90°` it is perpendicular to it. The `k`th of `N` planes uses `k·θ`.
     The flip negates `θ`. A line that is not parallel to the plane is an error.
   - **Unknown.** SOLIDWORKS's sign convention for `θ`. T2 says a new angle plane starts
     perpendicular to the face; the default angle value is not documented by Dassault.
7. **At angle to a cylinder's axis plane** (S4's example): the cylinder's axis is the
   Coincident line and a plane is At angle. Same rule as 6 with the axis as the line.
8. **Perpendicular to a curve through a point** (curve `C`, point `P`).
   - **Observed (S4).** The plane is perpendicular to the curve and passes through the
     point. With **Set origin on curve**, the plane's origin is on the curve; without it,
     on the point.
   - **Inferred.** The plane meets the curve where the curve's tangent is along the
     plane's normal and the plane passes through `P`: at a point `Q = C(t)` with
     `(P − Q) · C′(t) = 0`. When `P` is on the curve, `Q = P`. For a straight edge `Q` is
     the foot of the perpendicular from `P`.
   - **Decision.** Take `Q` as the closest point of the curve to `P` (it satisfies the
     condition). Normal: the unit tangent at `Q`, in the curve's direction. Origin `P`, or
     `Q` with **Set origin on curve**. Error if no such point exists on the curve's
     extent.
9. **Mid plane** (planes `o₁, n₁` and `o₂, n₂`).
   - **Observed (S4, T1, T2).** Between two parallel faces or planes the plane is halfway
     between them. T1 says it follows the references when they move.
   - **Observed (T2).** The faces need not be parallel; the plane then lies between them.
     **Unknown.** Dassault does not describe the non-parallel case.
   - **Decision.** Let `n₂′ = n₂` if `n₁ · n₂ ≥ 0`, otherwise `−n₂`.
     - Parallel (`|n₁ × n₂′|` below tolerance): normal `n₁`, origin
       `o₁ + ½((o₂ − o₁) · n₁)·n₁`. The plane is the same whether the faces' normals face
       each other or not.
   - **Decision (non-parallel).** Of the two bisectors of two intersecting planes, use the
     one that is the limit of the parallel case as the planes become parallel, which is
     the one with normal `(n₁ + n₂′)` normalized, through the planes' line of intersection
     (origin: the point on that line closest to `(o₁ + o₂)/2`). It bisects the smaller
     angle between the planes once `n₂` is turned to agree with `n₁`, which makes it the
     symmetry plane of a tapered block. If `n₁ · n₂ = 0` (planes at 90°) the two bisectors
     are equally good: use `n₁ + n₂` with the normals as given, and let **Flip** choose
     the other.
10. **Tangent to a cylinder.**
    - **Observed (S4).** With a point on the surface, the plane is tangent to the surface
      at the point. **Observed (T2).** With a plane as the second reference, the tangent
      plane can be turned about the cylinder by an angle; with two cylinders, the plane
      is tangent to both; a flip chooses the side of the cylinder.
    - **Decision.** For a cylinder with axis `a, w` and radius `r`:
      - tangent at a point `P` on it: origin `P`, normal the outward surface normal at
        `P`;
      - tangent and Parallel to a plane with normal `n₁` (`n₁ · w = 0`): normal `n₁`,
        origin `a′ + r·n₁`, where `a′` is a point of the axis; the flip uses `−n₁` and
        the opposite side, giving the other tangent plane;
      - tangent and At angle `θ` to a plane with normal `n₁` (`n₁ · w = 0`): normal
        `m = R(w, θ)·n₁`, origin `a′ + r·m`;
      - two cylinders: deferred (four common tangent planes in general).
11. **Project** (a point and a non-planar surface).
    - **Observed (S4).** "Nearest location on surface" puts the plane at the surface point
      nearest the sketch point. "Along sketch normal" projects the point along its
      sketch's normal onto the surface; a flip may be needed for the preview to appear.
    - **Observed (S22, S26).** `SolutionIndex` chooses among several solutions; the
      legacy method had a choice of a plane tangent to, or normal to, the surface, and an
      angle for the normal case.
    - **Inferred.** The default Project plane is tangent to the surface at the projected
      point (its normal is the surface normal there).
    - **Unknown.** When the normal-to-surface variant is offered in the current UI.
12. **Parallel to screen.** **Observed (S4, S7).** A plane through a vertex, square to the
    current view direction, optionally offset by a distance; "Update Plane" re-aligns it
    to a new view. **Decision.** AnvilCAD stores the normal (the view direction toward
    the viewer) when the plane is made, so a rebuild never depends on the camera; an
    "Update" button re-captures it.
13. **Coincident with one plane** alone. **Inferred.** A copy of that plane (offset 0).

### Flip normal

- **Observed (S4).** **Flip Normal** reverses the normal vector of the plane.
- **Decision.** It changes `n` to `−n` and `x` to `−x`, keeping the origin and `y`, so a
  sketch on the flipped plane is mirrored left to right as seen from its new front. This is
  what today's `flip` does to the frame.

### Status and errors

- **Observed (S4).** The plane needs "Fully defined" status.
- **Decision.** Errors on the plane feature, all recoverable at rebuild:
  - references missing after an upstream change ("the referenced edge no longer
    exists");
  - degenerate references (collinear points, parallel planes for a line, a line not
    parallel to the plane it is angled from);
  - under or over definition;
  - a Project or Tangent with no solution.

## Axes

- **Observed (S10).** An axis is used for sketch geometry and circular patterns. Every
  cylindrical and conical face has a **temporary axis**, which can be shown or hidden and
  selected directly.
- **Observed (S11).** The Axis command has five types:

| Type | Option name | Selections | `swRefAxisType_e` (S28) |
|---|---|---|---|
| One line | "One Line/Edge/Axis" | a sketch line, an edge, or a temporary axis | `swAxisTypeOneLine` = 0 |
| Two planes | "Two Planes" | two planar faces or planes | `swAxisTypeTwoPlanes` = 1 |
| Two points | "Two Points/Vertices" | two vertices, points or midpoints | `swAxisTypeTwoPoints` = 2 |
| Cylinder or cone | "Cylindrical/Conical Face" | a cylindrical or conical face | `swAxisTypeCylOrConeFace` = 3 |
| Point and face | "Point and Face/Plane" | a surface or plane and a vertex, point or midpoint | `swAxisTypePtAndPlane` = 4 |

- **Observed (S11).** Point and face: the axis goes through the point, normal to the
  surface or plane. On a non-planar surface the point must lie on it.
- **Observed (S28).** An axis reports a start point and an end point
  (`GetRefAxisParams`: six numbers). **Inferred.** An axis is an infinite line; the two
  points carry its direction and its displayed extent (S9 sizes axes like planes).
- **Observed (S27).** The API example selects the Right Plane and the Top Plane, inserts
  an axis, and then selects it as "Axis1". **Inferred.** New axes are named Axis1, Axis2,
  …, and two planes give their line of intersection (here the Z axis).
- **Decision.** The geometry and direction of each type:
  - one line: the line, directed like the edge or sketch line;
  - two planes: the intersection line, directed `n₁ × n₂`, through the point of the line
    closest to the first plane's origin; error if the planes are parallel;
  - two points: through `P₁`, directed `P₂ − P₁`; error if they coincide;
  - cylinder or cone: the surface's axis, directed like the surface's own axis (the
    `frame.z` of `anvil-brep`'s `Surface::Cylinder` and `Surface::Cone`);
  - point and face: through `P`, directed along the face's outward normal at `P`; for a
    reference plane, its normal.

  A **Reverse direction** option negates the direction (no source mentions one for axes;
  patterns and revolves need a direction, so AnvilCAD offers it).
- **Unknown.** How SOLIDWORKS directs axes from two planes or a cylinder.

## Points

- **Observed (S12).** The Point command proposes a construction method from the
  selection (a face selects Center of Face) and the user can change it. The types:
  - **Arc Center**: the centre of the selected arc or circle.
  - **Center of Face**: the face's centre of mass, on planar or non-planar faces (S13's
    older wording: centre of gravity).
  - **Intersection**: where two selected edges, curves or sketch segments meet. Points
    can also be made where an axis meets a plane, an axis meets a surface (planar or not),
    and two axes meet.
  - **Projection**: a point, curve end or vertex projected onto a plane or face (planar
    or not), along the plane's or face's normal.
  - **On point**: on a sketch point or the end of a sketch segment (from 2014, S13).
  - **Along curve distance or multiple reference point**: points along an edge, curve or
    sketch segment.
- **Observed (S12).** Along a curve:
  - **Distance**: the given number of points, `d` apart; the first is `d` from the end,
    not on it.
  - **Percentage**: the given number of points, spaced by a percentage of the entity's
    length. On a 100 mm entity, 5 points at 10% are 10 mm apart.
  - **Evenly Distribute**: the given number of points spread evenly; changing the count
    re-spaces them from the starting end.
  - A value too large for the number of points draws a warning.
- **Observed (S29).** The API's `InsertReferencePoint` returns an array of features, one
  per point, and a count of 1 makes one point. **Inferred.** Each point is its own
  feature in the tree (Point1, Point2, …; the name pattern is not stated in a source).
- **Inferred.** Center of Face is the area centroid `∫ x dA / ∫ dA` over the face. On a
  curved face the centroid need not lie on the face. **Unknown.** Whether SOLIDWORKS moves
  it onto the face.
- **Inferred.** Percentage points sit at `k·p%` of the length (`k = 1 … N`), by analogy
  with Distance.
- **Decision.**
  - Evenly Distribute places `N` points at arc lengths `k·L/(N + 1)`, `k = 1 … N`, so
    neither end gets a point, matching the "not on the endpoint" rule of Distance.
    **Unknown.** Whether SOLIDWORKS includes the ends.
  - The starting end is the curve's start; a **Reverse** option measures from the other
    end. **Unknown.** Which end SOLIDWORKS starts from.
  - If `N·d` exceeds the length (Distance), or `N·p` exceeds 100% (Percentage), the
    feature is an error naming the largest value that fits.
  - Intersection of two lines that do not meet (closer than a tolerance) is an error;
    so is a line parallel to the plane it is intersected with.
  - Projection onto a curved face uses the nearest point of the face (which is along the
    face normal there).

## Coordinate systems

- **Observed (S14).** Coordinate systems are used for measuring, mass properties, export
  to neutral formats and mates.
- **Observed (S14).** The Coordinate System command takes:
  - **Origin**: a vertex, point, midpoint or the model origin; or, with **Define position
    with numeric values**, X, Y and Z values relative to the "local origin", which the
    page contrasts with the global (0, 0, 0);
  - for each of **X axis**, **Y axis** and **Z axis**, an optional direction reference:
    - a vertex, point or midpoint: the axis points toward it;
    - a linear edge or sketch line: the axis is parallel to it;
    - a non-linear edge or sketch entity: the axis points toward the selected location on
      it;
    - a planar face: the axis is along the face's normal;
  - **Reverse Axis Direction** for each axis;
  - **Define rotation with numeric values**: X, Y and Z rotation angles, at least one
    given. The axes are rotated about X, then Y, then Z (the order is shown by icons,
    whose rows in the page's HTML are the X, Y and Z rotation angles).
- **Observed (S16).** Numeric position and rotation are present from the 2022 help on and
  absent up to 2021. **Observed (S30).** The matching API method is available from 2022.
- **Observed (S15).** With too few entities where the system should be, the user defines
  it elsewhere and then picks a point or vertex as the origin to move it there.
- **Observed (S15).** Its XY, YZ and ZX planes, its axes and its origin can be used by
  later features, for example to sketch on its XY plane or revolve about its Z axis, or as
  a mirror plane. A reference plane can be made perpendicular to one of its axes through
  a point.
- **Observed (S15).** When saving to 3MF, ACIS, AMF, IFC, IGES, Parasolid, STEP, STL,
  VDAFS or VRML, an output coordinate system can be chosen; it becomes the origin of the
  exported file.
- **Observed (S30).** The API's `InsertCoordinateSystem` takes three flip flags and
  selections marked 1 (origin), 2 (X axis), 4 (Y axis) and 8 (Z axis). Not all three axes
  need to be selected. The example selects an origin point, an X line and a Y line and
  makes a system named "Coordinate System1" (S30's other example).
- **Observed (S30).** `CreateCoordinateSystemUsingNumericalValues` takes the position (in
  metres) and rotations (radians, 0 to 2π) relative to the global coordinate system.
  **Ambiguous.** S14 says the numeric position is relative to the local origin, not the
  global one; S30 says global. **Inferred.** The UI values are offsets from the selected
  origin entity (or the model origin when none is selected), which is the global origin in
  the API's case.
- **Inferred.** Coordinate systems are right-handed (`z = x × y`). No source states it.
- **Decision.** How AnvilCAD completes the axes:
  - each given axis reference yields a unit direction, then its Reverse flag applies;
  - two axes given: the third is their cross product in right-handed order
    (`z = x × y`, `x = y × z`, `y = z × x`); then the second-listed of the two given axes
    is re-squared to the first (the first given in the order X, Y, Z is kept exactly);
  - one axis given: it is kept; of the other two world axes, taken in X, Y, Z order, the
    first one not within 1° of parallel to it is squared to it and used as the next axis
    in the cycle X → Y → Z → X; the third follows by the cross product;
  - none given: the world axes;
  - two given directions within a tolerance of parallel are an error;
  - numeric rotation: start from the world axes and apply `R(X, α)`, then `R(Y, β)`, then
    `R(Z, γ)` about the fixed world axes, so the combined rotation is
    `R(Z, γ)·R(Y, β)·R(X, α)`.
- **Unknown.** Whether SOLIDWORKS rotates about fixed axes or about the moving axes
  (`R(X, α)·R(Y, β)·R(Z, γ)`). The two agree when only one angle is non-zero.

## Centre of mass

- **Observed (S17).** **Center of Mass** adds one point, the centre of mass of the whole
  model. It sits just below the origin in the tree, updates as features are added, moved
  or deleted, and can be measured to and given reference dimensions, but cannot be the
  target of a driving dimension. It can also be added from the mass properties dialog.
- **Observed (S17).** The mass properties dialog can override the centre of mass with
  typed X, Y, Z values, optionally in a chosen coordinate system; the point then moves
  there and shows a different icon.
- **Observed (S17).** A **Center of Mass Reference Point** (parts only) is made from the
  centre of mass's context menu. It is added as the next feature in the tree and stands at
  the centre of mass of the features above it. Later features do not move it; changes to
  the features above it do. It can be dragged to another place in the tree. Driving
  dimensions may use it.
- **Observed (S31).** After `InsertCenterOfMass` and `InsertCenterOfMassReferencePoint`
  the tree holds "Center of Mass" and "Center of Mass Reference Point1".
- **Inferred.** For a part of one material, the centre of mass is the volume centroid; the
  density cancels. AnvilCAD's kernel already computes the centroid
  (`Solid::mass_properties().centroid`).
- **Decision.** AnvilCAD shows the centre of mass as a non-feature item below the origin,
  computed from the final body. A centre of mass reference point is an ordinary feature
  whose position is the centroid of the body as rebuilt up to it.

## Testable statements

Unless stated, the part is a block 100 × 60 × 40 mm with one corner at the origin:
x ∈ [0, 100], y ∈ [0, 60], z ∈ [0, 40], made by extruding a rectangle on the Front Plane
by 40 mm. Its faces have outward normals. Positions are in mm. "Plane z = 50, normal +Z"
means origin on that plane and that normal. Tolerances 1e-9 mm and 1e-9 rad unless
stated.

### Default planes and origin

1. The Front Plane has origin (0, 0, 0) and normal (0, 0, 1); the Top Plane normal
   (0, 1, 0); the Right Plane normal (1, 0, 0).

### Planes

2. Offset 25 from the Front Plane: plane z = 25, normal +Z.
3. Offset 10 from the top face (z = 40, normal +Z): plane z = 50, normal +Z. With the side
   flipped: z = 30, normal +Z. With Flip Normal instead: z = 50, normal −Z, and its `x`
   axis is the reverse of the unflipped plane's.
4. Offset 10 from the top face with Number of planes 3: three planes z = 50, 60, 70, all
   normal +Z, named Plane1, Plane2, Plane3 in a new part.
5. Parallel to the Top Plane, coincident with vertex (100, 60, 40): plane y = 60, normal +Y.
6. Three points (0, 0, 0), (100, 0, 0), (0, 60, 0): plane z = 0, normal +Z, origin
   (0, 0, 0). In the order (100, 0, 0), (0, 0, 0), (0, 60, 0): normal −Z. Three points
   (0, 0, 0), (50, 0, 0), (100, 0, 0) are an error.
7. Line from (0, 0, 0) to (100, 0, 0) and point (0, 0, 40): plane y = 0, normal
   (0, −1, 0).
8. Top Plane at angle 30° about the edge (0, 0, 0)→(100, 0, 0): the plane contains the X
   axis and has normal (0, cos 30°, sin 30°) = (0, 0.8660254, 0.5). The point
   (0, −0.5, 0.8660254) lies on it.
9. As 8 with Number of planes 3: normals at 30°, 60° and 90°; the third is (0, 0, 1), the
   Front Plane's orientation, through the X axis.
10. Top Plane at angle about the edge (0, 60, 40)→(100, 60, 40) (parallel to the Top Plane,
    off it): at 0°, plane y = 60, normal +Y; at 90°, plane z = 40, normal +Z.
11. Top Plane at angle about the edge (0, 0, 0)→(0, 60, 40) (not parallel to the Top
    Plane) is an error.
12. Perpendicular to the edge (0, 0, 0)→(100, 0, 0), coincident with point (30, 20, 10):
    plane x = 30, normal +X, origin (30, 20, 10); with Set origin on curve, origin
    (30, 0, 0).
13. A circle of radius 50 about the origin in the Front Plane, directed counter-clockwise
    seen from +Z, and the point (30, 40, 0) on it: the perpendicular plane has normal
    (−0.8, 0.6, 0) and origin (30, 40, 0). With the point (60, 80, 5) instead (off the
    circle), the plane is the same, with origin (60, 80, 5), or (30, 40, 0) with Set origin
    on curve.
14. Mid plane of the faces x = 0 and x = 100: plane x = 50. Its normal is the first
    reference's: −X if the x = 0 face is first, +X if the x = 100 face is first.
15. Mid plane of the Front Plane and an offset plane at z = 30: plane z = 15, normal +Z.
16. Mid plane of the Top Plane (normal (0, 1, 0)) and the plane through the X axis with
    normal (0, 0.5, 0.8660254): the plane contains the X axis and has normal
    (0, 0.8660254, 0.5), at 30° to each.
17. A cylinder of radius 10 with axis through (50, 30, 0) along Z: tangent and parallel to
    the Right Plane gives plane x = 60, normal +X; flipped, x = 40, normal −X. Tangent at
    the point (50, 40, 20) gives plane y = 40, normal +Y.
18. Project the point (70, 30, 5) onto the cylinder of 17, nearest location: the plane is
    tangent at (60, 30, 5), normal +X.

### Axes

19. Two planes, Right Plane then Top Plane: the axis is the Z axis, through (0, 0, 0),
    direction (1, 0, 0) × (0, 1, 0) = (0, 0, 1). Named Axis1 in a new part.
20. Two planes that are parallel (the Front Plane and an offset plane) are an error.
21. Two points (0, 0, 0) and (100, 60, 40): direction (0.8111071, 0.4866643, 0.3244428)
    (to 1e-7).
22. A through hole of radius 5 along Z at (50, 30): the axis is the line x = 50, y = 30
    along ±Z; it does not depend on the hole's depth.
23. Point (20, 10, 40) and the top face: the axis through (20, 10, 40) along +Z.

### Points

24. Arc center of a circle of radius 10 centred at (50, 30, 40): (50, 30, 40).
25. Center of face of the top face: (50, 30, 40). With a through hole of radius 10 at
    (80, 30): (48.3424126, 30, 40) to 1e-6 (area 6000 − 100π = 5685.8407346).
26. Intersection of the X axis with the line through (20, −10, 0) and (20, 10, 0):
    (20, 0, 0). Intersection of the line through (20, 10, 0) along Z with the Front Plane:
    (20, 10, 0). Intersection of two skew lines is an error.
27. Projection of (20, 10, 70) onto the top face: (20, 10, 40). Projection of (70, 30, 5)
    onto the cylinder of 17: (60, 30, 5).
28. Along the edge (0, 0, 0)→(100, 0, 0): Distance 15 with 4 points gives
    x = 15, 30, 45, 60; Percentage 10% with 5 points gives x = 10, 20, 30, 40, 50;
    Distance 30 with 4 points is an error; Evenly Distribute with 4 points gives
    x = 20, 40, 60, 80. Each is a separate point feature.

### Coordinate systems

29. Origin (100, 0, 0), X axis along the edge (100, 0, 0)→(100, 60, 0), Y axis along the
    edge (100, 0, 0)→(100, 0, 40): axes X = (0, 1, 0), Y = (0, 0, 1), Z = (1, 0, 0). The
    vertex (100, 60, 40) has coordinates (60, 40, 0) in it.
30. As 29 with Reverse X: X = (0, −1, 0), Y = (0, 0, 1), Z = (−1, 0, 0).
31. Origin (0, 0, 0) and only the Z axis given as the top face's normal: X = (1, 0, 0),
    Y = (0, 1, 0), Z = (0, 0, 1).
32. Numeric position (200, 200, 200) and rotation Z = 90° only: origin (200, 200, 200),
    X = (0, 1, 0), Y = (−1, 0, 0), Z = (0, 0, 1).
33. X and Y given along the same edge is an error.

### Centre of mass

34. Centre of mass of the block: (50, 30, 20). With a through hole of radius 10 along Z at
    (80, 30): (48.3424126, 30, 20) to 1e-6 (volume 240000 − 4000π = 227433.6293856), when
    the hole is a true cylinder.
35. A centre of mass reference point added after the block and before the hole stays at
    (50, 30, 20) after the hole is added; making the block 120 long moves it to
    (60, 30, 20).

### Rebuild

36. A plane at angle 30° about a block edge follows the edge when the block is made
    longer or taller; deleting the edge's feature makes the plane an error, and features
    after it still rebuild.

## API names

For `anvil-migrate`. Names and values only.

- **Feature type names** (`IFeature::GetTypeName2`, S32): `RefPlane` (`IRefPlane` or
  `IRefPlaneFeatureData`), `RefAxis` (`IRefAxis` or `IRefAxisFeatureData`), `RefPoint`
  (`IRefPoint` or `IRefPointFeatureData`), `CoordSys` (`ICoordinateSystemFeatureData`),
  `CenterOfMass` (`ICenterOfMass`). Also in the table: `RefPlaneFtrFolder`,
  `RefAxisFtrFolder`. The centre of mass reference point's and the origin's type names
  are not in the table.
- **Planes.**
  - `IFeatureManager::InsertRefPlane(FirstConstraint, FirstConstraintAngleOrDistance,
    SecondConstraint, SecondConstraintAngleOrDistance, ThirdConstraint,
    ThirdConstraintAngleOrDistance)`, selections marked 0, 1, 2; returns the plane
    (S20). Available from 2010.
  - `swRefPlaneReferenceConstraints_e`, a bitmask (S21): `Parallel` = 0x1,
    `Perpendicular` = 0x2, `Coincident` = 0x4, `Distance` = 0x8, `Angle` = 0x10,
    `Tangent` = 0x20, `Project` = 0x40, `MidPlane` = 0x80, `OptionFlip` = 0x100,
    `OptionOriginOnCurve` = 0x200, `OptionProjectToNearestLocation` = 0x400,
    `OptionProjectAlongSketchNormal` = 0x800, `ParallelToScreen` = 0x1000,
    `OptionReferenceFlip` = 0x2000 (each prefixed `swRefPlaneReferenceConstraint_`).
    **Note.** The page gives the decimal values of the two projection options as 1028
    and 2056, which do not match their hex values (1024 and 2048). The hex values fit
    the bitmask; trust them.
  - `IRefPlaneFeatureData` (S22): `Constraint(Index)`, `Reference(Index)`,
    `AngleOrDistance(Index)`, `ReversedReferenceDirection(Index)` (constraint-based
    planes); `Type2` (returns `swRefPlaneConstraintBase` for 2010+ planes); `Type`
    (legacy type, or `swRefPlaneAngle` / `swRefPlaneDistance` for a constraint-based plane
    with an angle or distance reference); `Distance` (metres), `Angle` (radians),
    `OriginOnCurve`, `ProjectionType`, `SolutionIndex`, `UseNormalPlane`, `UpdatePlane`,
    `AutoSize`, `Selections`, `GetSelectionsCount`, `AccessSelections`,
    `ReleaseSelectionAccess`. The interface remarks list which members work with which
    kind of plane; `Distance`, `Angle`, `OriginOnCurve`, `ProjectionType`,
    `SolutionIndex`, `Type`, `UseNormalPlane` and `ReverseDirection` are for legacy
    planes, and `Constraint`, `Reference`, `AngleOrDistance` for constraint-based ones.
  - `swRefPlaneType_e` (S23): `swRefPlaneInvalid` 0, `swRefPlaneUndefined` 1,
    `swRefPlaneLinePoint` 2, `swRefPlaneThreePoint` 3, `swRefPlaneLineLine` 4,
    `swRefPlaneDistance` 5, `swRefPlaneParallel` 6, `swRefPlaneAngle` 7,
    `swRefPlaneNormal` 8, `swRefPlaneOnSurface` 9, `swRefPlaneSWStandard` 10,
    `swRefPlaneConstraintBase` 11.
  - `swOnSurfacePlaneProjectType_e` (S23): `swOnSurfacePlaneProjecttoNearestLocation` 0,
    `swOnSurfacePlaneProjectAlongSketchNormal` 1.
  - `IRefPlane` (S25): `Transform` (from the Front Plane's canonical position: image of
    (0, 0, 0) is the origin, of (0, 0, 1) the normal), `CornerPoints` (four points),
    `BoundingBox`.
  - Example (S24): `InsertRefPlane(16, 0.7853981633975, 4, 0, 0, 0)` with a face as
    reference 0 and an edge as reference 1 makes a 45° plane, "Plane1".
  - Obsolete (S26): `IModelDoc2::CreatePlaneAtOffset3`, `CreatePlaneAtAngle3`,
    `CreatePlaneThru3Points3`, `CreatePlanePerCurveAndPassPoint3`,
    `CreatePlaneAtSurface3`.
- **Axes.** `IModelDoc2::InsertAxis2(AutoSize)` from the current selection (S27);
  `IRefAxisFeatureData::Type`, `GetSelections`, `SetSelections` (S28); `swRefAxisType_e`
  as in the table above; `IRefAxis::GetRefAxisParams` (start xyz, end xyz, metres),
  `IsTempAxis`, `GetTempAxisReferenceFace` (S28). `anvil-migrate` already reads
  `GetRefAxisParams` (`crates/anvil-migrate/src/sw.rs`).
- **Points.** `IFeatureManager::InsertReferencePoint(NRefPointType,
  NRefPointAlongCurveType, DDistance_or_Percent, NumberOfRefPoints)` returns an array of
  features (S29). `swRefPointType_e`: `swRefPointInvalid` 0, `swRefPointUndefined` 1,
  `swRefPointAlongCurve` 2, `swRefPointCenterEdge` 3, `swRefPointFaceCenter` 4,
  `swRefPointFaceVertexProjection` 5, `swRefPointIntersection` 6,
  `swRefPointSketchPoint` 7. `swRefPointAlongCurveType_e`: `swRefPointAlongCurveDistance`
  0, `swRefPointAlongCurvePercentage` 1, `swRefPointAlongCurveEvenlyDistributed` 2.
  `IRefPointFeatureData`: `Type`, `Distance`, `AlongCurveOption`, `Selections`.
  `IRefPoint::GetRefPoint` returns the point (a math point, metres).
- **Coordinate systems.** `IFeatureManager::InsertCoordinateSystem(XFlippedIn,
  YFlippedIn, ZFlippedIn)` with selection marks 1 origin, 2 X, 4 Y, 8 Z;
  `IFeatureManager::CreateCoordinateSystemUsingNumericalValues(UseLocation, DeltaX,
  DeltaY, DeltaZ, UseRotation, AngleX, AngleY, AngleZ)` (2022+);
  `ICoordinateSystemFeatureData`: `OriginEntity`, `XAxisEntities`, `YAxisEntities`,
  `ZAxisEntities`, `XFlipped`, `YFlipped`, `ZFlipped`, `Transform` (S30).
- **Centre of mass.** `IFeatureManager::InsertCenterOfMass`,
  `IFeatureManager::InsertCenterOfMassReferencePoint`; `ICenterOfMass::GetCoordinates`
  (S31).
- **Decision for `anvil-migrate`.** Today it imports each `RefPlane` as a fixed placement
  and skips `RefAxis`, `RefPoint` and `CoordSys`. With this feature it should read the
  constraint-based definition (`Type2`, then `Constraint`, `Reference`, `AngleOrDistance`
  and `ReversedReferenceDirection` for indices 0–2) and map it to the AnvilCAD plane
  types above when the references can be mapped, falling back to a fixed placement
  (`IRefPlane::Transform`) otherwise. Axes and points map the same way, falling back to
  fixed geometry from `GetRefAxisParams` and `GetRefPoint`.

## Recommended scope

What exists: `PlaneFeature` (one base plane, an offset and a `flip` that both moves to the
other side and reverses the normal), `PlaneRef` (Front, Top, Right, a planar face, another
plane feature, or a fixed frame), `Axis` (X, Y, Z or a fixed line) used by circular and
linear patterns, faces named by `FaceId`, edges by `EdgeRef` (the two faces they separate),
exact surfaces in `anvil-brep` (planes, cylinders, cones), and `Solid::mass_properties()`
with a centroid. There are no vertex references, no reference axis or point features, and
no coordinate systems.

In order:

1. **Planes, part 1.** Generalise `PlaneFeature` to a list of up to three references with
   constraints. Keep today's serialized form loading (an `Offset` from `base` with `flip`
   meaning both "other side" and "flip normal"). Implement Offset distance with Number of
   planes, a side flip and Flip Normal separately; Parallel through a point; At angle about
   an edge or sketch line with Number of planes; Mid plane (parallel and non-parallel);
   three points; line and point; Perpendicular to an edge through a point, with Set origin
   on curve. Tests 2–16.
2. **Reference axes** as a feature, with the five types, and let patterns, revolves and
   the At angle plane use them through a new `Axis::Feature(id)`. Tests 19–23.
3. **Reference points** as features: arc centre, centre of face, intersection,
   projection, on point, along curve. Needed first by the plane and axis types that take a
   point when no vertex is picked. Add a vertex reference type for the body. Tests 24–28.
4. **Coordinate systems**, with the axis completion rules, and its XY/YZ/ZX planes and
   axes as references for sketches, mirrors and patterns. Then **an output coordinate
   system for STEP export** (S15). Tests 29–33.
5. **Centre of mass** display and the centre of mass reference point. Tests 34–35.
6. **Planes, part 2.** Tangent to a cylinder (point, parallel, angle), Project onto a
   curved face, perpendicular to a curved edge, Parallel to screen (frozen normal).
   Tests 17–18.

Leave out: two-cylinder tangent planes, the legacy "normal to surface" on-surface variant,
mass property overrides, multi-body and assembly centres of mass, grid systems and bounding
boxes (S1 lists them as reference geometry too), and automatic plane display sizing beyond
a simple rule.

## Unknown or not covered

1. SOLIDWORKS's normal and direction conventions for three-point planes, line-and-point
   planes, two-plane axes, cylinder axes and normal-to-curve planes. AnvilCAD's rules
   above are its own.
2. The sign of the At angle rotation and its default value (T2 says a new angle plane
   starts perpendicular).
3. Whether the offset side flip also reverses the normal, and which UI controls the
   `OptionFlip` and `OptionReferenceFlip` bits drive.
4. Where a mid plane between non-parallel planes goes (T2 says only "between"; the
   bisector choice is AnvilCAD's).
5. When Project offers a plane normal to the surface rather than tangent to it, and how
   `SolutionIndex` orders solutions.
6. Whether Evenly Distribute includes the curve's ends, and which end along-curve points
   start from.
7. Whether Center of Face is moved onto a curved face.
8. Whether numeric coordinate system rotations are about fixed or moving axes, and
   whether the numeric position is relative to the global origin (API) or the selected
   origin entity (help).
9. How a coordinate system completes axes when only one or two are given.
10. The tree type names of the origin and of the centre of mass reference point.
11. The decimal values of `OptionProjectToNearestLocation` and
    `OptionProjectAlongSketchNormal`, which the API page states inconsistently.
12. How reference geometry is stored in `.sldprt` files (not studied).
