# Sweep and loft features

What AnvilCAD's Sweep and Loft features should do. A **sweep** moves a closed profile along
a path and adds the swept solid to the part (boss) or removes it (cut). A **loft** builds a
solid that passes through two or more profiles in order, and adds it or removes it. A
short section covers the **boundary** feature, which is a loft-like feature that can span
curves in two directions. SOLIDWORKS users know these as Swept Boss/Base, Swept Cut,
Lofted Boss/Base, Lofted Cut and Boundary Boss/Base, so AnvilCAD follows their published
behaviour where that makes sense.

AnvilCAD has no sweep or loft today. Sketches are planar and hold lines, arcs and circles
only (`crates/anvil-sketch/src/sketch.rs`). The model has extrude, revolve, plane,
patterns, fillet, chamfer, shell and hole features (`crates/anvil-model/src/document.rs`),
and the exact kernel (`crates/anvil-brep`) has exact extrude, drafted extrude and revolve,
and B-spline patches. This specification is written for the implementer of the first
sweep and loft features; see "Recommended scope" at the end.

## How this was studied

- **Inputs.** Only published information was used:
  - Dassault's public online help for SOLIDWORKS 2025, with older pages (2010, 2014,
    2018, 2020, 2022) where the 2025 text has dropped an option that the API still has;
  - the public 2024 and 2025 API help, including its published example macros;
  - two public reseller articles on lofts, and public maths references (sources below).

  No SOLIDWORKS software, installer, library or file was downloaded, installed, run or
  analysed. No SOLIDWORKS licence was accepted on the machine used.
- **Retrieval.** The help site renders its articles with JavaScript and refuses a plain
  request. Each article was fetched over HTTPS with a browser user agent, and its text read
  from the page's embedded `__NEXT_DATA__` JSON (`props.pageProps.helpContentData.helpText`)
  with HTML stripped, by a script run with `python3 -I`. A second small script listed each
  article's links so related pages could be found. A missing page still returns HTTP 200
  with "page cannot be found" in its data, so each page's content was checked; pages that
  do not exist are not listed below. The two reseller articles and the Wikipedia pages were
  read through a web fetch. All text was paraphrased; none is copied into this repository.
- **REA.** REA (`rea-agents` 6.5.0, with the `third_party/rea` submodule at `3fc0075`) was
  set up and its research method followed:
  - each finding below is marked **Observed** (stated in a cited source), **Inferred**
    (our reasoning from the sources, often geometry), **Decision** (AnvilCAD's own choice
    where the sources are silent) or **Unknown**;
  - every observation cites its source.

  REA's analysis tools were **not** run. They inspect artifacts (binaries, packages,
  captures, files), and the only lawful inputs for this behaviour are prose documents.
  Pointing REA at SOLIDWORKS software is ruled out by
  [INTEROPERABILITY.md](../../INTEROPERABILITY.md). The `.sldprt` storage of sweep, loft
  and boundary features was not studied.
- **Geometry.** Volumes in "Testable statements" are worked out from first principles
  (Pappus's theorem and its generalisation, Cavalieri's principle, the prismatoid formula)
  and were checked numerically by a scratch script. They describe what the geometry must
  be if the behaviour is as stated, not values read from SOLIDWORKS.

## Sources

Base A is `https://help.solidworks.com/2025/English/SolidWorks/sldworks/`. Base A14 is the
same path for 2014, and Base L is the 2010 legacy help
`https://help.solidworks.com/2010/english/solidworks/sldworks/legacyhelp/sldworks/features/`.
Base B is `https://help.solidworks.com/2024/English/api/sldworksapi/`, and B + `X~Y` stands
for `SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.X~Y.html`. Base C is
`https://help.solidworks.com/2024/English/api/swconst/`, and C + `E` stands for
`SOLIDWORKS.Interop.swconst~SOLIDWORKS.Interop.swconst.E.html`. All were read on
2026-10-18.

| # | Topic | URL | Version |
|---|---|---|---|
| S1 | Sweeps overview | A + `HIDD_DVE_FEAT_SWEEP.htm` | 2025 |
| S2 | Sweep rules | A + `c_sweep_rules.htm` | 2025 |
| S3 | Sweep profiles (sketch, circular, solid) | A + `c_sweep_profiles.htm` | 2025 |
| S4 | Swept Boss/Base PropertyManager | A + `r_swept_boss_base_property_manager.htm` (also compared with the 2018, 2020 and 2022 pages of the same name) | 2025 |
| S5 | Swept Cut PropertyManager | A + `r_swept_cut_property_manager.htm` | 2025 |
| S6 | Recommendations for sweeps with guide curves | A + `c_Recommendations_Sweeps_Guide_Curves.htm` | 2025 |
| S7 | Creating sweeps: basic, bidirectional, circular profile | A + `t_create_sweep_features.htm`, `t_Creating_a_Sweep.htm`, `t_create_bidirectional_sweep.htm`, `t_create_circular_profile_sweep.htm` | 2025 |
| S8 | Sweep PropertyManager, older layout (orientation/twist type, path alignment, align with end faces, thin feature) | A14 + `HIDD_DVE_FEAT_SWEEP.htm` | 2014 |
| S9 | Orientation/twist type: Follow Path (path alignment options) | A14 + `c_Orientation_Twist_Type_Follow_Path.htm` | 2014 |
| S10 | Surface Sweep PropertyManager (profile may be open) | A + `r_surface_sweep_property_manager.htm` | 2025 |
| S11 | Lofts overview | A + `c_Lofts_top.htm` | 2025 |
| S12 | Loft PropertyManager | A + `HIDD_DVE_FEAT_LOFT.htm` (also compared with the 2014 and 2018 pages, and L + `HIDD_DVE_FEAT_LOFT.htm`) | 2025 |
| S13 | Loft synchronization and connectors | A + `c_Loft_Synchronization_Overview.htm`, `t_Adding_Connectors_to_Lofts.htm`, `t_Controlling_Connectors_Loft.htm` | 2025 |
| S14 | Recommendations for lofts with guide curves | A + `c_Recommendations_Lofts_Guide_Curves.htm` | 2025 |
| S15 | Loft with centerline | A + `t_Loft_with_Centerline.htm` | 2025 |
| S16 | Loft with start and end constraints; examples of loft tangency | A + `t_Loft_with_Start_End_Constraints.htm`, `c_Examples_Loft_Tangency.htm` | 2025 |
| S17 | Working with lofts; guide curve with non-planar profile; adding and deleting loft sections; creating lofts | A + `c_Working_With_Lofts_Folder.htm`, `t_Loft_with_Guide_Curve_Non_Planar_Profile.htm`, `HIDD_DVE_ADD_LOFT_SECTIONS.htm`, `t_Deleting_Loft_Sections.htm`, `t_creating_lofts.htm` | 2025 |
| S18 | Loft overview and examples (close loft, default constraint, merge tangent faces, guide curve influence) | L + `Loft_Overview.htm`, `Example_of_Using_Close_Loft.htm`, `Example_of_Loft_Default_Constraint.htm`, `Example_of_Loft_Merge_Tangent_Faces.htm`, `Example_of_Loft_Guide_Curve_Influence.htm` | 2010 |
| S19 | Boundary overview, when to use, creating | A + `c_Boundary_Top_Folder.htm`, `c_boundary_features.htm`, `t_create_boundaries.htm` | 2025 |
| S20 | Boundary PropertyManager | A + `r_boundary_pm.htm` | 2025 |
| S21 | `ISweepFeatureData` members | B + `ISweepFeatureData_members.html` (namespace prefix as above) | 2024 |
| S22 | `ISweepFeatureData` details: `StartTangencyType`, `SetTwistAngle`, `SweepType`, `GetCutSweepOption`, `GetProfileType` | B + `ISweepFeatureData~<member>` | 2024 |
| S23 | `IFeatureManager::InsertProtrusionSwept4` | B + `IFeatureManager~InsertProtrusionSwept4` | 2024 |
| S24 | `IFeatureManager::InsertCutSwept5` (and `InsertCutSwept4`) | B + `IFeatureManager~InsertCutSwept5`, `IFeatureManager~InsertCutSwept4` | 2024 |
| S25 | API guide: sweep features and `SweepFeatureData` objects | `https://help.solidworks.com/2024/English/api/sldworksapiprogguide/OVERVIEW/Sweep_Features_and_SweepFeatureData_Objects.htm` | 2024 |
| S26 | Sweep and loft enumerations: `swTwistControlType_e`, `swTangencyType_e` (both also read in the 2025 API help, unchanged), `swThinWallType_e`, `swSweepDirection_e`, `swCutSweepOption_e`, `swGuideCurveInfluence_e`, `swFeatureNameID_e` | C + `<enum>` | 2024, 2025 |
| S27 | `ILoftFeatureData` members and properties (`StartTangencyType`, `StartTangentLength`, `StartConstraintDraftAngle`, `NumberOfSections`, `Close`, `PickPoints`, `GuideCurveInfluence`, `MaintainTangency`) | B + `ILoftFeatureData_members`, `ILoftFeatureData~<property>` | 2024 |
| S28 | `IFeatureManager::InsertProtrusionBlend2`, `IFeatureManager::InsertCutBlend` | B + `IFeatureManager~InsertProtrusionBlend2`, `IFeatureManager~InsertCutBlend` | 2024 |
| S29 | `IFeature::GetTypeName2` (feature type names) | B + `IFeature~GetTypeName2` | 2024 |
| S30 | `IBoundaryBossFeatureData` | B + `IBoundaryBossFeatureData`, `IBoundaryBossFeatureData_members` | 2024 |
| S31 | API examples: Insert Swept Cut Feature (creates "Cut-Sweep1"); Create Cut-sweep Feature Using Tool Body; Insert Protrusion Blend (creates "Loft1") | `https://help.solidworks.com/2024/English/api/sldworksapi/Insert_Swept_Cut_Feature_Example_VB.htm`, `.../Create_Cut-sweep_Feature_Using_Tool_Body_Example_VB.htm`, `https://help.solidworks.com/2011/English/api/sldworksapi/Insert_Protrusion_Blend_Example_VB.htm` | 2024, 2011 |
| R1 | Loft connectors and entity counts (reseller article) | `https://hawkridgesys.com/blog/3-steps-better-lofts-boundaries-solidworks` | not Dassault |
| R2 | Lofts FAQ (reseller article) | `https://www.goengineer.com/en-us/blog/solidworks-lofts-faq` | not Dassault |
| N1 | Pappus's centroid theorem, with the Goodman and Goodman generalisation to a figure moved along a curve | `https://en.wikipedia.org/wiki/Pappus%27s_centroid_theorem` | not Dassault |
| N2 | Prismatoid formula, exact when the section area is at most cubic in height | `https://en.wikipedia.org/wiki/Prismatoid` | not Dassault |

## Sweep

### Inputs and profile kinds

- **Observed (S1).** A sweep moves a profile (section) along a path to make a base, boss,
  cut or surface. Internally it makes copies of the profile at positions along the path
  and blends between them.
- **Observed (S3, S4, S5, S21, S24).** There are three kinds of profile:
  - **Sketch Profile**: a 2D sketch moved along a 2D or 3D path. Model faces, edges and
    curves can also be picked as the profile.
  - **Circular Profile**: no profile sketch; the user gives a **Diameter** and the sweep
    makes a solid rod (or with a cut, a round groove) along a sketch line, model edge or
    curve. It is offered for boss, cut and surface sweeps (S7).
  - **Solid Profile** (cuts only, called "Solid sweep" in 2014, S8): a tool body is moved
    along the path and the volume it passes through is removed. The API names the two cut
    kinds `swProfileSweep` (1) and `swSolidSweep` (2) (S26).
- **Observed (S2, S4).** For a boss or base the profile must be closed. It may be open
  only for a surface sweep (S10), which AnvilCAD does not have.
- **Observed (S2, S4).** The path may be open or closed. It may be a set of sketched curves
  in one sketch, a single curve, or a set of model edges.
- **Observed (S4, S5, S25).** The start point of the path must lie on the plane of the
  profile for a one-direction sweep. The rules page states the looser form that the path
  must cross the profile's plane (S2).
  - **Inferred.** The path need not start at a point inside the profile. The profile is
    carried at its offset from the path, and the path only fixes where on the profile's
    plane the sweep starts. The guide-curve page says a Pierce relation (path through a
    point of the profile sketch) is usual but not required (S6).
  - **Decision.** AnvilCAD requires the path to meet the profile's plane at the path's
    start point, or (for a bidirectional sweep) at an interior point, within tolerance.
    Otherwise the feature reports "the path must start on the plane of the profile".
- **Observed (S2, S4).** Neither the profile, the path nor the resulting solid may
  intersect itself. A sweep that would is an error.
- **Observed (S7).** The profile sketch itself must be non-intersecting and closed, and
  the path is usually built before the profile when guide curves are used.

### Path rules: tangent or not

- **Observed (S6).** With guide curves, the path must be a single entity or a chain of
  tangent segments: no angled corners.
- **Observed (S2, S5, S8).** For a solid (tool body) sweep, the path must be tangent
  within itself (no sharp corners) and must begin at a point on or inside the tool body's
  profile.
- **Inferred.** The help states the no-corners rule only for guide curves and tool-body
  sweeps. A plain sketch-profile sweep is therefore allowed a path with sharp corners.
- **Unknown.** How SOLIDWORKS forms the solid at a sharp corner of the path (mitre,
  round, or something else). The help does not say.
- **Decision.** At a sharp corner AnvilCAD joins the two legs with a mitre: each leg is
  cut by the plane that bisects the angle between the two path tangents at the corner.
  The first version may instead refuse paths with sharp corners ("the path must be
  tangent-continuous"); see "Recommended scope".

### Direction: one way or both

- **Observed (S4, S5, S7, S26).** When the path runs through the profile's plane
  (the profile is mid-path), the user chooses **Direction 1**, **Direction 2** or
  **Bidirectional**. A bidirectional sweep goes both ways along the path from the profile.
  Guide curves and start/end tangency cannot be used with a bidirectional sweep, and it is
  not offered for tool-body cuts. Each direction has its own twist value (S21
  `GetD2TwistAngle`, `D2ReverseTwistDir`). The API's `swSweepDirection_e` is
  `swSweepDirection1` 0, `swSweepBidirectional` 1, `swSweepDirection2` 2.
- **Observed (S23, S24).** The direction setting applies only to sketch profiles that do
  not sit at an end of the path.

### Profile orientation and twist

The section's orientation along the path is set by one choice in the older help
("Orientation/twist type", S8) and by two lists in 2025 ("Profile Orientation" and
"Profile Twist", S4, S5). The API keeps the older single value, `swTwistControlType_e`
(S26), plus `PathAlignmentType` (`swTangencyType_e`) for the corrections under Follow
Path.

- **Follow Path** (`swTwistControlFollowPath` = 0).
  - **Observed (S4, S5, S8).** The section keeps the same angle to the path all along it.
  - **Observed (S9).** The profile always keeps a normal to the path. Small uneven changes
    in the path's curvature can make the sections wander about the path; the path
    alignment options steady them.
  - **Inferred.** On a planar path (every sketch path in AnvilCAD), the section is carried
    by the frame made of the path's unit tangent `T`, the path plane's fixed normal `B`,
    and `N = B × T`. That frame turns only about `B`, so on a planar path "Follow Path"
    with no correction is exact and has no twist. On a straight path the sweep is the
    extrude of the profile along the path. On a circular arc with the profile in a plane
    containing the arc's axis, the sweep is the revolve of the profile about that axis
    through the arc's angle.
  - **Inferred.** If the profile's plane is not square to the path at the start, the
    section keeps that tilt. On a straight path the result is an oblique prism.
- **Path alignment / Profile Twist under Follow Path** (`PathAlignmentType`).
  - **None**. **Observed (S4, S9, S26).** The default; no correction. The 2025 help calls
    it 2D-paths-only. The enum maps this choice to `swTangencyNormalToProfile` (1), while
    the `PathAlign` argument of `InsertProtrusionSwept4` uses 0 for it (S23). The sources
    disagree, so `anvil-migrate` should read 0 and 1 alike as no correction.
  - **Minimum Twist** (`swMinimumTwist` = 10). **Observed (S4, S5, S26).** 3D paths only;
    corrects the section so it twists as little as possible and does not self-intersect.
    **Inferred.** This corresponds to a rotation-minimising (parallel-transport) frame.
    **Unknown.** The exact frame SOLIDWORKS uses.
  - **Specify Direction Vector** (`swTangencyDirectionVector` = 2). **Observed (S9).**
    A plane, planar face or line gives a fixed direction (a plane's normal, a line's
    direction). Each section's plane contains that direction as its X axis; the section
    is turned about the path so that this holds. Not available with Keep Normal Constant
    (S4).
  - **Tangent to Adjacent Faces / All Faces** (`swTangencyAllFaces` = 3). **Observed (S8,
    S9, S26).** When the path runs along model faces, the section's Y axis follows the face
    normal and its Z axis is the path tangent. Only when attaching to existing geometry.
  - **Natural**. **Observed (S4).** 3D paths only; the section pivots to keep the same
    angle to the path's curvature. **Inferred.** A Frenet frame. **Unknown.** Its API
    value; it is not in the 2024 or 2025 `swTwistControlType_e` or `swTangencyType_e`.
- **Keep Normal Constant** (`swTwistControlKeepNormalConstant` = 1).
  - **Observed (S4, S5, S8).** The section stays parallel to the starting section
    whatever the path's tangent does. With several profiles they all stay parallel to the
    first.
  - **Inferred.** The profile is only translated: each section is the starting profile
    moved by (path point − path start point). Seen along the profile's normal `n`, every
    slice is a copy of the profile, so if `n · path(s)` only increases along the path, the
    volume is the profile area times the rise of the path along `n` (Cavalieri). On a
    straight path square to the profile it is the same as an extrude.
- **Follow Path and First Guide Curve** (`swTwistControlFollowPathFirstGuideCurve` = 2) and
  **Follow First and Second Guide Curves** (`swTwistControlFollowFirstSecondGuideCurves`
  = 3). **Observed (S4, S5, S8, S26).** Need one or two guide curves. The section's turn
  about the path is set so that the vector from the path to the first guide curve (or from
  the first guide curve to the second) keeps a constant angle to the section's horizontal.
- **Twist along path** (`swTwistControlConstantTwistAlongPath` = 8; 2025 "Specify Twist
  Angle" under Follow Path) and **twist with normal constant**
  (`swTwistControlNormalConstantTwistAlongPath` = 9).
  - **Observed (S4, S5, S8).** The section turns about the path by a total angle given in
    **Degrees**, **Radians** or **Revolutions** (2014: "Turns"). With normal constant, it
    turns while staying parallel to the first section.
  - **Observed (S22).** The API holds the angle in radians (one revolution is 2π) and
    `D1ReverseTwistDir` reverses its sense.
  - **Observed (S4).** On a closed path the twist must be a whole number of revolutions.
  - **Observed (S22).** Start and end tangency do not apply when this twist is used.
  - **Unknown.** How the angle is spread along the path (in proportion to arc length or
    to the path's parameter), and which sense is positive.
  - **Decision.** The turn at a point is the total angle times the arc length so far over
    the path length, counter-clockwise about the path tangent `T` (right-hand rule).
    Turning is about the path point, so a profile centred on the path keeps every section
    congruent.

### Guide curves

- **Observed (S4, S6).** Guide curves steer the section as it moves. Each must touch the
  profile or a point of the profile sketch, so the sweep can treat it as a pierce. They
  may be sketch curves, model edges or curves, and may meet at a point (an apex). Their
  order can be changed (**Move Up**, **Move Down**).
- **Observed (S6).** If guide curves are longer than the path, the sweep is as long as the
  path. If one is shorter, the sweep stops at the end of the shortest guide curve.
- **Observed (S6).** Sketch relations in the profile (horizontal, vertical, dimensions)
  limit how the intermediate sections can change shape, so removing them lets the
  sections stretch or twist.
- **Inferred.** Each intermediate section is the profile sketch re-solved with its pierce
  points moved to where the guide curves cross that section's plane. A circle whose
  circumference passes through a guide curve therefore changes its radius to the guide
  curve's distance from the path.
- **Observed (S4, S5, S21).** **Merge Smooth Faces** merges
  faces where the path or guide curves are smooth; clearing it splits the sweep wherever
  the guide curve or path is not curvature continuous. Its default is not stated. **Show Sections** displays numbered
  intermediate sections.

### Other options

- **Merge tangent faces** (`MaintainTangency`, API argument `KeepTangency`).
  **Observed (S4, S8, S23).** Where the profile has tangent segments, the matching swept
  faces are made tangent; faces that are planes, cylinders or cones stay so, others are
  merged and approximated, and arcs may become splines. It has no effect with guide
  curves. **Decision.** Off by default; AnvilCAD keeps one face per profile segment per
  path segment and never approximates when it is off.
- **Merge result** (`Merge`). **Observed (S7, S8).** On by default; merges the sweep with
  the existing body.
- **Align with end faces** (`AlignWithEndFaces`, API argument `Alignment`).
  **Observed (S8, S23, S24).** When the path runs from one face to another, the sweep is
  extended or trimmed to meet the faces at its ends (so a groove cut along a helix breaks
  cleanly through the end faces). With it off, the sweep starts and ends square to the
  path. It is on by default for a circular-profile cut (S7).
- **Start and end tangency** (`StartTangencyType`, `EndTangencyType`). **Observed (S4,
  S22).** **None** (default, `swTangencyNone` 0) or **Path Tangent**
  (`swTangencyNormalToProfile` 1), which makes the sweep square to the path at that end.
  Not valid for a bidirectional sweep or a twist along path.
  - **Unknown.** What changes in the solid when the profile is already square to the path;
    the help shows only pictures.
- **Tangent propagation** (`TangentPropagation`, API argument `Propagate`).
  **Observed (S21, S23).** When the path is a model edge, the path grows along edges
  tangent to it.
- **Thin feature** (`ThinFeature`, `ThinWallType`, `SetWallThickness`).
  - **Observed (S8, S21, S25, S26).** A thin sweep makes a wall from the profile:
    **One-Direction** (thickness one way, **Reverse Direction** flips it), **Mid-Plane**
    (half each way, total equal to the thickness) or **Two-Direction** (two thicknesses).
    API `swThinWallType_e`: `swThinWallOneDirection` 0, `swThinWallOppDirection` 1,
    `swThinWallMidPlane` 2, `swThinWallTwoDirection` 3. The thin choice cannot be added
    when editing an existing sweep through the API (S25).
  - **Observed (S4).** The 2025 PropertyManager text no longer mentions Thin Feature; the
    2014 page (S8) and the 2024 API (S21) do. A hollow tube from a circular profile (S4,
    S7) is this option.
  - **Unknown.** Which side One-Direction thickens a closed profile by default, and
    whether a thin sweep accepts an open profile.
  - **Decision.** One-Direction thickens outward (away from the region the closed profile
    bounds); Reverse thickens inward. Open profiles are not accepted at first.
- **Feature scope.** **Observed (S5, S21).** Cuts and bosses in multibody parts can be
  limited to chosen bodies. **Decision.** Out of scope; AnvilCAD has one body.

### Swept cut

- **Observed (S5, S24).** A swept cut has the same profile, path, orientation, guide curve
  and tangency options as a swept boss, and removes the swept volume from the body.
- **Observed (S5, S8).** The tool-body (solid) cut:
  - the tool body must be convex, not merged with the main body, and be either a revolve
    made only of analytic geometry (lines and arcs) or a cylindrical extrude;
  - the path must be tangent-continuous and start on or inside the tool body;
  - with Follow Path and no path alignment, the tool follows the path's tangent (along a
    helix it tilts with the helix); with a Direction Vector, it keeps a fixed attitude,
    for example square to the end face of a cylinder, as an end mill would.
- **Observed (S25).** Through the API, the tool body and the solid body to cut are both
  selected; the cut is meant for jobs such as machining simulation.
- **Inferred.** Along a straight path a convex tool body is only translated, so the removed
  volume is the Minkowski sum of the tool and the path segment: the tool's volume plus the
  path length times the area of the tool's shadow on a plane square to the path.

### Errors

A sweep that cannot be made is a rebuild error on the feature, not a panic. From the
sources and decisions above, the errors are:

- the profile is open, empty or self-intersecting (S2, S4);
- the path is empty, self-intersecting, or not in one sketch (S2);
- the path does not meet the profile's plane (S4);
- the swept solid would intersect itself (S2), for example an arc path whose radius is
  smaller than the profile's reach toward the arc's centre;
- a twist on a closed path that is not a whole number of revolutions (S4);
- a guide curve that does not touch the profile, or guide curves on a path with corners
  (S6);
- a tool-body cut whose path has corners or does not start on the tool (S5);
- a circular profile with a diameter that is not positive;
- a thin feature with a thickness that is not positive.

### In the tree

- **Observed (S7, S31).** A swept boss is named "Sweep" plus a number ("Sweep1") and a
  swept cut "Cut-Sweep1". The API type names are `Sweep` and `SweepCut` (S29), both read
  through `ISweepFeatureData`; `ISweepFeatureData::SweepType` returns `swFmSweep` or
  `swFmSweepCut` (S22, S26).
- **Decision.** AnvilCAD names them "Sweep1" and "Cut-Sweep1", matching its "Extrude" and
  "Cut-Extrude" names, and the sweep depends on both the profile sketch and the path
  sketch.

## Loft

### Profiles and order

- **Observed (S11, S12).** A loft makes transitions between two or more profiles, and can
  be a base, boss, cut or surface. Profiles are sketch profiles, faces or edges, and the
  loft passes through them in the order they are selected; **Move Up** and **Move Down**
  reorder them.
- **Observed (S11, S18).** Only the first profile, the last, or both may be a point (a
  loft to a point). All profiles and guide curves may sit in one 3D sketch.
- **Observed (S28).** For a solid loft the profiles must be closed. With only one profile,
  guide curves must be closed curves.
- **Observed (R2).** Profiles from a 3D sketch must be planar; a boundary feature accepts
  non-planar curves. A non-planar profile on a model face (from a split line) is allowed
  with guide curves (S17).

### Matching points between profiles: start points and connectors

- **Observed (S12).** For each profile the user picks the point the loft should start
  from. The API stores these as `PickPoints` (S27), and its example macro selects matching
  sketch points on each profile (S31).
- **Observed (S12, S14).** A poor preview (twisted loft) is fixed by re-picking or
  reordering, so that different points are joined.
- **Observed (S13).** A **connector** is shown when profiles are selected: a polyline
  through corresponding points of all profiles, with a handle on each profile. Dragging
  handles changes the synchronisation (which points correspond). On a profile with
  corners, the first connector jumps from vertex to vertex ("global twist control"); extra
  connectors added with **Add Connector** slide freely along the profile ("local twist
  control"). Connectors can be shown, hidden, deleted or reset.
- **Observed (S13).** With guide curves and closed profiles, the guide curve sets each
  profile's start point.
- **Observed (R1).** Connectors normally form at the junctions between sketch entities.
  When the profiles have the same number of entities the matching is automatic; when the
  counts differ, the article's advice is to split entities (or use a "Segment" tool, or
  fit a spline) until they match. Two squares turned 45° to each other twist unless each
  is split into eight matching points.
- **Observed (S14).** Making the same number of segments on all guide curves, or a guide
  curve through the profiles' corresponding vertices, controls which points correspond.
- **Unknown.** How SOLIDWORKS matches profiles whose vertex counts differ, such as a square
  and a circle, when the user has not split anything. Its loft accepts them; the rule is
  not published.
- **Decision.** AnvilCAD matches points as follows.
  1. Each closed profile is one loop of segments (lines and arcs; a circle is one closed
     segment). All loops are oriented the same way (counter-clockwise seen along the loft
     direction).
  2. Each loop's start point is the vertex nearest the user's pick point; with no pick,
     the vertex nearest the previous profile's start point projected onto this profile's
     plane, and for the first profile the vertex with the smallest angle from its plane's
     +X axis about its centroid. A circle's start point is where the pick (or the
     projected previous start) meets it.
  3. If all loops have the same number of segments, segment `i` of one loop matches
     segment `i` of the next, each parameterised by its own normalised arc length.
  4. Otherwise each loop is parameterised by normalised arc length from its start point,
     and every loop is split at the union of all loops' vertex parameters, so all loops
     have the same number of segments; then rule 3 applies.
  5. A point profile matches every point of the next loop.

  This gives exactly SOLIDWORKS's documented behaviour for equal counts (R1) and a
  deterministic answer for unequal counts. Users who want SOLIDWORKS's exact result
  should split their profiles so the counts match.

### Shape between profiles: start and end constraints

- **Observed (S12, S16, S18).** **Start constraint** and **End constraint** choices:
  - **Default**: only with three or more profiles; approximates a parabola through the
    first and last profiles and uses its tangent;
  - **None**: no tangency constraint, described as zero curvature;
  - **Direction Vector**: tangent to a picked linear edge or axis, or to a face's or
    plane's normal, or the line between two vertices; has a **Draft angle** and a tangent
    length;
  - **Normal to Profile**: tangent to the profile's normal; has a draft angle and a
    tangent length;
  - **Tangency to Face**: only when the end profile lies on existing geometry; the loft
    is tangent to the adjacent faces; **Next Face** chooses among them;
  - **Curvature to Face**: as Tangency to Face but curvature-continuous.
- **Observed (S12, S16).** **Start/End Tangent Length** sets how strongly the constraint
  pulls, and its effect reaches only to the next profile. **Reverse Tangent Direction**
  flips it. **Apply to all** gives one handle for the whole profile; clearing it gives a
  handle per segment. The draft angle is valid only with Apply to all on (S27).
- **Observed (S26, S27, S28).** The API stores the constraint in `StartTangencyType` and
  `EndTangencyType` as `swTangencyType_e`. `InsertProtrusionBlend2` takes integers: 0
  none, 1 normal to profile, 2 direction vector, 3 tangent to all adjacent faces, 4 some
  faces (not available).
- **Unknown.** The API value for Curvature to Face and for Default; the meaning and unit of
  the tangent length (the API calls it only "tangent length"); the exact "parabola" of
  Default.
- **Inferred.** With two profiles and None at both ends, each pair of corresponding points
  is joined by a straight line: the side faces are ruled. Every cross-section is then the
  blend `(1 − s)·P + s·Q` of the two loops, its area is quadratic in `s`, and the volume
  between parallel profiles a distance `h` apart is exactly
  `h/6 · (A0 + 4·Amid + A1)` (N2), where `Amid` is the area of the loop of midpoints.
  - Two similar, concentric, parallel squares with corners matched give four planar
    trapezoid faces: a frustum of a square pyramid, volume `h/3 · (A0 + A1 + √(A0·A1))`.
  - Two coaxial parallel circles give a cone frustum: volume
    `πh/3 · (R² + R·r + r²)`, and the side face is a cone.
  - A square to a point above its centre gives a pyramid: `A0·h/3`.
- **Decision.** For three or more profiles the corresponding points are joined by a cubic
  spline through them in order. **None** uses the natural end condition (zero second
  derivative, matching "zero curvature"), **Default** uses the parabola through the first
  three (or last three) points to set the end tangent, **Normal to Profile** and
  **Direction Vector** set the end tangent to that direction scaled by the tangent length
  times the distance to the next profile (tangent length 1 by default). With two profiles
  and both ends None the result is the ruled loft above.

### Guide curves

- **Observed (S12, S14).** Guide curves must cross every profile; any number may be used;
  they may meet at points; sketch curves, model edges and curves all qualify. A guide
  curve longer than the loft is allowed and the loft stops at the shortest guide curve.
- **Observed (S12, S26).** **Guide curves influence**: **To next guide**
  (`swGuideCurveInfluenceNextGuide` 0), **To next sharp** (1), **To next edge** (2),
  **Global** (3). A sharp is a corner between two sketch entities that are neither tangent
  nor equal in curvature.
- **Observed (S12).** **Guide tangency type**: None, Normal to Profile, Direction Vector
  (each with a draft angle), or Tangency to Face when the guide curve lies on a model
  edge. Profiles should be tangent to those faces within 2° for best results; beyond
  about 30° the loft fails.
- **Observed (S17).** Guide curves are tied to profiles with Pierce or Coincident relations
  at profile vertices or sketch points.

### Centerline loft

- **Observed (S12, S15).** A **centerline** guides the loft: every intermediate section
  lies in a plane square to the centerline. The centerline must pass through the inside of
  every closed profile, may be a sketch curve, model edge or curve, and can be combined
  with guide curves. **Number of Sections** sets how many intermediate sections are
  made, and it changes the shape. The API's `TessToleranceFactor` (default 1.0; larger
  gives more sections) and `NumberOfSections` hold it (S27, S28).
- **Inferred.** Between two equal circles centred on a circular-arc centerline and square
  to it, the result is the same as sweeping that circle along the arc.

### Other options

- **Merge tangent faces** (`MaintainTangency`, `KeepTangency`). **Observed (S12, S18,
  S28).** As for sweeps; planes, cylinders and cones are kept where the sections show
  them. **Decision.** Off by default.
- **Close loft** (`Close`). **Observed (S12, S18, S27).** Joins the last profile back to
  the first, making a closed ring along the loft direction. **Observed (S28).** With fewer
  than three profiles, guide curves must be closed curves. **Decision.** AnvilCAD
  requires at least three profiles to close a loft.
- **Merge result**. **Observed (S12).** Merges the loft with the body; on by default
  (**Inferred** from the sweep pages, S7).
- **Thin feature**. **Observed (S12).** One-Direction, Mid-Plane, Two-Direction, as for
  sweeps; API `ThinWallType` and `SetWallThickness` (S27), or `IsThinBody`, `Thickness1`,
  `Thickness2`, `ThinType` in `InsertProtrusionBlend2` (S28).
- **Micro tolerance**. **Observed (S12).** A tighter tolerance for tiny geometry.
- **Add Loft Section**. **Observed (S17).** After a loft is made, extra sections can be
  added on temporary planes along it and edited as sketches.
- **Lofted cut**. **Observed (S11, S12, S28).** Same inputs; removes the loft volume.
  `InsertCutBlend` takes the same arguments as `InsertProtrusionBlend2` without tangent
  lengths, directions, merge and guide influence.

### Errors

- fewer than two profiles, or a point profile in the middle (S11);
- an open profile for a solid loft (S28);
- profiles that cannot be matched (for AnvilCAD, a profile with more than one loop);
- a guide curve that does not cross every profile (S14);
- a centerline that misses the inside of a profile (S15);
- Default constraint with only two profiles (S12): **Decision**, treated as None;
- close loft with fewer than three profiles (Decision above);
- a loft that would intersect itself.

### In the tree

- **Observed (S31).** A lofted boss is named "Loft1". The API type names are `Blend` and
  `BlendCut` (S29), both read through `ILoftFeatureData`; feature IDs `swFmBlend` and
  `swFmBlendCut` (S26).
- **Unknown.** The default name of a lofted cut. **Decision.** "Cut-Loft1", by analogy
  with "Cut-Sweep1" and "Cut-Extrude1".

## Boundary (brief)

- **Observed (S19, S20).** A boundary spans two or more curves in one direction or in two
  (Direction 1 and Direction 2), as a base, boss, cut or surface. The two directions are
  interchangeable. It suits shapes where loft quality is not enough, and its surfaces can
  be tangent or curvature-continuous on all sides.
- **Observed (S20).** Per direction: curves in order with a pick point each (**Flip
  Connectors** fixes bad matching); curves influence **Global**, **To Next Curve**, **To
  Next Sharp**, **To Next Edge** or **Linear** (spreads the influence evenly, like a ruled
  surface); tangent type **Default** (three or more curves), **None**, **Normal to
  Profile**, **Direction Vector**, **Tangency to Face**, **Curvature to Face**, with draft
  angle, **Tangent influence (%)** and tangent length. Options: merge tangent faces,
  **Close Surface**, **Trim by direction 1 and direction 2**.
- **Observed (S29, S30).** API type name `NetBlend`, data interface
  `IBoundaryBossFeatureData`, created with `IFeatureManager::InsertNetBlend` (with
  `SetNetBlendCurveData` and `SetNetBlendDirectionData`).
- **Unknown.** Default feature names; the exact surface construction.
- **Decision.** Not in AnvilCAD's first scope. A one-direction boundary with None
  tangency can be read by `anvil-migrate` as a loft; two-direction boundaries need a
  Coons- or Gordon-style surface and come later.

## Testable statements

Lengths in mm, volumes in mm³. "Follow Path" with no twist and no correction, and a
profile in a plane square to the path at its start, unless stated. Where a statement rests
on a **Decision**, it says so.

### Sweep

1. A 10 mm diameter circle swept along a 50 mm straight line square to it is a cylinder:
   volume `π·5²·50 = 1250π ≈ 3926.991`, three faces (two planes, one cylinder).
2. A 4 mm diameter circle centred on a 90° arc path of radius 20 gives a quarter torus:
   by Pappus (N1) volume `π·2² · (π/2·20) = 40π² ≈ 394.784`. Its side face is a torus
   with major radius 20 and minor radius 2, and its end faces are planar discs in planes
   that meet at the arc's axis at 90°.
3. The same circle along a tangent path of a 30 mm line, that 90° arc of radius 20, and a
   30 mm line has volume `4π·(30 + 10π + 30) = 240π + 40π² ≈ 1148.766`. The section stays
   square to the path all along, so the two end faces are perpendicular to each other and
   the solid has five faces: two discs, two cylinders and one torus, each joined
   tangentially.
4. The same circle along a closed circular path of radius 20 (profile at the path's start
   point, square to it) gives a full torus with no end faces: `2π²·20·2² = 160π² ≈
   1579.137`.
5. A profile whose centroid is off the path: a 4 × 2 rectangle whose centroid lies 3 mm
   outside a 90° arc path of radius 20 (in the radial direction) has volume
   `8 · (π/2 · 23) = 92π ≈ 289.027` (Pappus with the centroid's radius 23).
6. Any profile centred on its centroid on a planar path (lines and tangent arcs), with no
   self-intersection, has volume = profile area × path length (Goodman and Goodman, N1).
7. Twist: a 10 × 10 square centred on a 50 mm straight path, twisted 90° (or one full
   revolution), has volume 5000, the same as untwisted, since every section square to the
   path is the same square turned. The end face is the square turned by the twist angle
   about the path (counter-clockwise about the path's tangent, **Decision**).
8. A twist of 90° on a closed path is an error; 360° is accepted.
9. Keep Normal Constant: a 4 mm diameter circle in the XY plane at the origin, swept along
   a quarter arc of radius 20 that starts at the origin heading +Z and ends heading +X
   (centre at (20, 0, 0)), stays parallel to XY. Its volume is the circle's area times the
   path's rise along Z: `4π · 20 = 80π ≈ 251.327`. The end face is a disc in the plane
   Z = 20, centred at (20, 0, 20).
10. Oblique profile (**Inferred**): a 10 mm diameter circle in a plane tilted 30° from
    square to a 50 mm straight path, Follow Path, both tangencies None, gives an oblique
    cylinder of volume `25π · 50 · cos 30° ≈ 3400.873`. Its two end faces are the 10 mm
    circle itself, in parallel planes tilted 30° to the path.
11. Circular profile: Diameter 6 along a 40 mm straight sketch line gives volume
    `π·3²·40 = 360π ≈ 1130.973`, the same as statement 1's construction with a 6 mm
    circle.
12. Thin sweep, Mid-Plane, thickness 2, of a 10 mm diameter circle along a 50 mm line:
    a tube with radii 4 and 6, volume `π·(6² − 4²)·50 = 1000π ≈ 3141.593`. One-Direction,
    thickness 1 (**Decision**: outward): radii 5 and 6, volume `550π ≈ 1727.876`;
    reversed: radii 4 and 5, volume `450π ≈ 1413.717`.
13. Guide curve (**Inferred**): a circle of radius 5 at the origin in the XY plane, a
    40 mm straight path along +Z from its centre, and a straight guide line from (5, 0, 0)
    to (10, 0, 40) with no other relations on the circle give a cone frustum of radii 5
    and 10: volume `π·40/3·(25 + 50 + 100) = 7000π/3 ≈ 7330.383`.
14. A guide curve shorter than the path: the same set-up with the guide line ending at
    (7.5, 0, 20) gives a sweep 20 long, volume `π·20/3·(25 + 37.5 + 56.25) = 2375π/3 ≈
    2487.094`.
15. Sharp-cornered path (**Decision**: mitre): a 10 × 10 square centred on an L-shaped
    path of a 50 mm line and a 30 mm line meeting at 90° has volume `100 · (50 + 30) =
    8000`; the joint face is a 10 × 10√2 rectangle in the bisecting plane.
16. Swept cut: in a 100 × 100 × 20 block, a 4 mm diameter circle swept along a 100 mm
    straight line through the block's middle, parallel to an edge, removes `400π`; the
    block's volume becomes `200000 − 400π ≈ 198743.363`.
17. Tool-body cut (**Inferred**): a cylinder of radius 3 and height 10 with its axis along
    Z, moved 40 mm along +X from its axis (path in a plane square to the axis), sweeps
    `π·3²·10 + 40·(2·3)·10 = 90π + 2400 ≈ 2682.743`; cut from a block that contains it,
    the block loses exactly that.
18. Bidirectional: a 10 mm circle at the middle of a 60 mm straight path, Bidirectional,
    gives volume `25π·60 = 1500π`; Direction 1 alone gives `750π`.
19. Errors: an open profile for a boss; a path that does not meet the profile's plane; a
    2 mm radius profile on an arc path of radius 1.5 (would self-intersect) all fail with
    a rebuild error and leave the body as it was.

### Loft

20. Two parallel squares, 20 × 20 at z = 0 and 10 × 10 at z = 30, concentric, corners
    matched, both ends None: a frustum with six planar faces, volume
    `30/3·(400 + 100 + 200) = 7000`.
21. Two coaxial circles, radius 10 at z = 0 and radius 5 at z = 30, None: a cone frustum,
    volume `π·30/3·(100 + 50 + 25) = 1750π ≈ 5497.787`, side face a cone of half-angle
    `atan(5/30) ≈ 9.462°`.
22. A 20 × 20 square at z = 0 lofted to a point at (0, 0, 30) above its centre: a pyramid,
    volume `400·30/3 = 4000`, four planar triangular sides.
23. Square to circle (**Decision** for the matching): a 20 × 20 square at z = 0 with its
    corners at ±45°, and a circle of radius 10 at z = 30 coaxial with it, None at both
    ends. Under the matching rule above the circle is split at ±45° and ±135°, each square
    side is joined to one quarter arc by straight rulings, and the volume is
    `h/3 · (a² + πr² + 4√2·a·r/π)` with `a = 20`, `r = 10`, `h = 30`:
    `4000 + 1000π + 8000√2/π ≈ 10742.858`. The mid-height section (z = 15) has area
    `(400 + 100π + 2·4√2·200/π)/4 ≈ 358.603`.
24. Twisted loft: two 10 × 10 squares 30 apart, the upper turned 45°, matched vertex to
    vertex: the rulings make four doubly-ruled (non-planar) sides, the mid-height section
    is a square of area `25·(2 + √2) ≈ 85.355`, and the volume is
    `30/6·(100 + 4·85.355 + 100) = 2000 + 500√2 ≈ 2707.107`.
25. Three equal coaxial circles of radius 10 at z = 0, 20 and 40, with None (or Default)
    constraints: a cylinder of volume `π·100·40 = 4000π ≈ 12566.371`; the interpolation
    must not bulge when the profiles are equal.
26. Two equal 10 × 10 squares 25 apart with Normal to Profile at both ends and tangent
    length 1: still a square prism of volume 2500.
27. Centerline loft (**Inferred**): two circles of radius 5, each centred on and square to
    the ends of a 90° arc centerline of radius 20, give the quarter torus of volume
    `25π · 10π = 250π² ≈ 2467.401`.
28. Lofted cut: statement 20's loft cut from a 40 × 40 × 40 block that contains it leaves
    `64000 − 7000 = 57000`.
29. Thin loft, Mid-Plane, thickness 2, between two coaxial circles of radius 10 at z = 0
    and z = 30: a tube of radii 9 and 11, volume `π·(121 − 81)·30 = 1200π ≈ 3769.911`.
30. Errors: a loft with one profile, a middle profile that is a point, an open profile, or
    Close loft with two profiles each fail with a rebuild error.

## API names

For `anvil-migrate`, which reads a user's own licensed SOLIDWORKS through its published
API. Names and values only.

- **Feature type names** (`IFeature::GetTypeName2`, S29): `Sweep`, `SweepCut`
  (`ISweepFeatureData`); `Blend`, `BlendCut` (`ILoftFeatureData`); `NetBlend`
  (`IBoundaryBossFeatureData`). Surface variants (not used): `SweepRefSurface`,
  `BlendRefSurface`.
- **`swFeatureNameID_e`** (S26): `swFmSweep`, `swFmSweepCut`, `swFmBlend`, `swFmBlendCut`,
  `swFmRefSurface`. Create a definition with `IFeatureManager::CreateDefinition(swFmSweep
  or swFmSweepCut)` and build it with `IFeatureManager::CreateFeature` (S25).
- **`ISweepFeatureData`** (S21, S22): `Profile`, `Path`, `GuideCurves`,
  `GetGuideCurvesCount`, `CircularProfile`, `CircularProfileDiameter`, `GetProfileType`,
  `GetPathType`, `GetCutSweepOption` (`swCutSweepOption_e`: `swProfileSweep` 1,
  `swSolidSweep` 2), `SweepType`, `Direction` (`swSweepDirection_e`: `swSweepDirection1`
  0, `swSweepBidirectional` 1, `swSweepDirection2` 2), `TwistControlType`,
  `PathAlignmentType`, `GetPathAlignmentDirectionVector`, `GetTwistAngle` /
  `SetTwistAngle` (radians), `GetD2TwistAngle`, `D1ReverseTwistDir`, `D2ReverseTwistDir`,
  `StartTangencyType`, `EndTangencyType`, `MaintainTangency` (merge tangent faces),
  `MergeSmoothFaces`, `AdvancedSmoothing`, `AlignWithEndFaces`, `TangentPropagation`,
  `Merge`, `ThinFeature`, `IsThinFeature`, `ThinWallType`, `GetWallThickness` /
  `SetWallThickness`, `IsBossFeature`, `FeatureScope`, `FeatureScopeBodies`, `AutoSelect`,
  `AccessSelections`, `ReleaseSelectionAccess`.
- **`IFeatureManager::InsertProtrusionSwept4`** (S23, obsolete since the 2018 sweep
  rework): `(Propagate, Alignment, TwistCtrlOption, KeepTangency, BAdvancedSmoothing,
  StartMatchingType, EndMatchingType, IsThinBody, Thickness1, Thickness2, ThinType,
  PathAlign, Merge, UseFeatScope, UseAutoSelect, TwistAngle, BMergeSmoothFaces,
  CircularProfile, CircularProfileDiameter, Direction)`. Selection marks: 1 profile, 2
  guide curve, 4 path. `PathAlign`: 0 none, 2 direction vector, 3 all faces.
- **`IFeatureManager::InsertCutSwept4` / `InsertCutSwept5`** (S24): the same, without
  `Merge`, and (5) with `AssemblyFeatureScope`, `AutoSelectComponents`,
  `PropagateFeatureToParts` before `CircularProfile`. Mark 1 is the profile or tool body.
  With `CreateDefinition`, a tool-body cut selects the tool with mark 1 and the body to
  cut with mark 2048; a circular profile's path uses mark 4; a direction vector mark 128
  (S25).
- **`swTwistControlType_e`** (S26): `swTwistControlFollowPath` 0,
  `swTwistControlKeepNormalConstant` 1, `swTwistControlFollowPathFirstGuideCurve` 2,
  `swTwistControlFollowFirstSecondGuideCurves` 3, `swTwistControlConstantTwistAlongPath`
  8, `swTwistControlNormalConstantTwistAlongPath` 9.
- **`swTangencyType_e`** (S26): `swTangencyNone` 0, `swTangencyNormalToProfile` 1,
  `swTangencyDirectionVector` 2, `swTangencyAllFaces` 3, `swMinimumTwist` 10. Used for
  sweep path alignment and start/end tangency, and loft start/end constraints.
- **`swThinWallType_e`** (S26): `swThinWallOneDirection` 0, `swThinWallOppDirection` 1,
  `swThinWallMidPlane` 2, `swThinWallTwoDirection` 3.
- **`ILoftFeatureData`** (S27): `Profiles`, `GetProfileCount`, `PickPoints`,
  `GuideCurves`, `GetGuideCurvesCount`, `GuideCurveInfluence`, `GetGuideTangencyType` /
  `SetGuideTangencyType`, `Centerline`, `NumberOfSections`, `Close`, `MaintainTangency`,
  `AdvancedSmoothing`, `Merge`, `StartTangencyType`, `EndTangencyType`,
  `StartTangentLength`, `EndTangentLength`, `ReverseStartTangentDirection`,
  `ReverseEndTangentDirection`, `StartDirectionVector`, `EndDirectionVector`,
  `StartConstraintDraftAngle`, `EndConstraintDraftAngle`,
  `StartConstraintDraftAngleDirection`, `EndConstraintDraftAngleDirection`,
  `StartConstraintApplyToAll`, `EndConstraintApplyToAll`, `IsThinFeature`,
  `ThinWallType`, `GetWallThickness` / `SetWallThickness`, `IsBossFeature`,
  `FeatureScope`, `AccessSelections`, `ReleaseSelectionAccess`.
- **`IFeatureManager::InsertProtrusionBlend2`** (S28): `(Closed, KeepTangency,
  ForceNonRational, TessToleranceFactor, StartMatchingType, EndMatchingType,
  StartTangentLength, EndTangentLength, StartTangentDir, EndTangentDir, IsThinBody,
  Thickness1, Thickness2, ThinType, Merge, UseFeatScope, UseAutoSelect,
  GuideCurveInfluence)`. Matching types 0 none, 1 normal to profile, 2 direction vector,
  3 all adjacent faces. Selection marks: 1 profiles, 2 guide curves, 4 centerline, 8
  start vector, 32 end vector.
- **`IFeatureManager::InsertCutBlend`** (S28): `(Closed, KeepTangency, ForceNonRational,
  TessToleranceFactor, StartMatchingType, EndMatchingType, IsThinBody, Thickness1,
  Thickness2, ThinType, UseFeatScope, UseAutoSelect)`.
- **`swGuideCurveInfluence_e`** (S26): `swGuideCurveInfluenceNextGuide` 0,
  `swGuideCurveInfluenceNextSharp` 1, `swGuideCurveInfluenceNextEdge` 2,
  `swGuideCurveInfluenceNextGlobal` 3.
- **Boundary** (S30): `IBoundaryBossFeatureData`, `IFeatureManager::InsertNetBlend`,
  `SetNetBlendCurveData`, `SetNetBlendDirectionData`.

## Recommended scope

What exists to build on: planar sketches of lines, arcs and circles with closed-loop
profile extraction (`Sketch::profiles` in `crates/anvil-sketch/src/regions.rs`); features
resolved in order in `crates/anvil-model/src/rebuild.rs`, each producing a tool solid that
is joined or cut (`BodyOp::Boss` / `BodyOp::Cut`); exact `extrude`, `extrude_drafted` and
`revolve` in `crates/anvil-brep`, with B-spline patches (`patch.rs`) and booleans that
accept them. There are no 3D sketches, no open-chain path extraction, no thin features
and no multibody parts.

In order:

1. **Sweep and swept cut along a planar line-and-arc path, Follow Path, no twist.** The
   profile is a closed sketch region (one or more loops, like extrude). The path is the
   one open or closed chain of non-construction lines and arcs in a second sketch (or a
   circle as a closed path), tangent at every joint. The profile's plane must be square to
   the path at the path's start point (oblique profiles later). Because the path is planar
   and the frame does not twist, each line segment of the path sweeps an exact extrude of
   the section and each arc segment an exact revolve about the arc's axis; consecutive
   pieces share their end section exactly, so the pieces can be built with the existing
   exact extrude and revolve and joined, or assembled directly into one solid. This gives
   exact planes, cylinders, cones and tori, matching statements 1 to 6 and 16. Check the
   self-intersection error on each arc (statement 19). Names "Sweep1" / "Cut-Sweep1".
   Dependencies: profile sketch and path sketch.
2. **Circular Profile** option (statement 11), and **Bidirectional** (statement 18) when
   the profile's plane meets the path inside it.
3. **Loft and lofted cut between two planar closed profiles, None/None** (ruled), with the
   matching rules in the Decision above and a pick point per profile. Recognise the exact
   cases: parallel matched straight segments give planes (statement 20), coaxial circles
   give a cone (statement 21), a square shrinking about its centre is also a drafted
   extrude. Otherwise each side face is a degree-1 B-spline patch between the two matched
   segments. Then a point as the first or last profile (statement 22).
4. **Loft through three or more profiles** with the cubic interpolation and None,
   Default, Normal to Profile and Direction Vector constraints (statements 25, 26), and
   Close loft. Faces are B-spline patches.
5. **Twist along path** and **Keep Normal Constant** for sweeps (statements 7 to 9), and
   sharp-cornered paths with mitres (statement 15). These need swept B-spline faces.
6. **Thin** sweeps and lofts (statements 12, 29), which also need an offset of the
   profile; and **guide curves** for sweeps and lofts (statements 13, 14), which need the
   sketch solver to re-solve the profile at each section.
7. **Centerline lofts** (statement 27) and **tool-body cuts** (statement 17).

Leave out for now: 3D paths and their corrections (Minimum Twist, Natural, Direction
Vector, All Faces), because AnvilCAD has no 3D sketches; model edges as paths, profiles
or guide curves; Tangency to Face and Curvature to Face; Align with end faces; merge
tangent and merge smooth faces (AnvilCAD keeps its faces separate); connectors as
draggable handles beyond the pick point; Add Loft Section; feature scope; surface sweeps
and lofts; and the boundary feature.

## Unknown or not covered

1. How SOLIDWORKS forms a sweep at a sharp corner of the path (AnvilCAD decides a mitre).
2. How SOLIDWORKS matches loft profiles whose vertex counts differ, such as a square and a
   circle (AnvilCAD decides normalised arc length from the start point). Results for
   such lofts will differ from SOLIDWORKS unless the user splits the profiles.
3. How twist is distributed along the path and which sense is positive.
4. The meaning and unit of loft tangent length, the parabola behind the Default
   constraint, and the API values for Curvature to Face and Default.
5. The exact frames behind Minimum Twist and Natural on 3D paths, and Natural's API value.
6. Which side a One-Direction thin sweep or loft grows by default, and whether thin sweeps
   take open profiles. The 2025 Swept Boss/Base page no longer describes Thin Feature,
   though the API keeps it.
7. What Path Tangent start/end tangency changes when the profile is not square to the path.
8. How intermediate sections are re-solved with guide curves beyond the pierce points.
9. The surface types SOLIDWORKS uses for loft faces that happen to be planes or cones
   (whether it keeps them analytic without Merge tangent faces).
10. Default names of lofted cuts and boundary features, and the defaults of Merge tangent
    faces, Merge smooth faces and guide curve influence.
11. Boundary feature surface construction.
