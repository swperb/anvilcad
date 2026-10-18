# Rib, Draft, Dome and Wrap features

What AnvilCAD's Rib, Draft and Dome features should do, and a short note on Wrap. A rib
thickens an open or closed sketch contour into a wall and grows it until it meets the
part. A draft tilts chosen faces by an angle about a neutral plane so that a moulded part
releases from its mould. A dome raises (or sinks) a curved cap on a face. A wrap embosses,
debosses or scribes a sketch onto a curved face. SOLIDWORKS users know all four under the
same names, so AnvilCAD follows their published behaviour where that makes sense.

AnvilCAD has none of these features today. `crates/anvil-model/src/document.rs` has no
draft option on `ExtrudeFeature` and no rib, draft, dome or wrap variant in `FeatureKind`;
`crates/anvil-model/src/rebuild.rs` has nothing to rebuild them. This specification was
written before any implementation, to be implemented from.

## How this was studied

- **Inputs.** Only published information was used:
  - Dassault's public online help for SOLIDWORKS 2025, with the 2012 and 2015 versions of
    pages that the 2025 help words differently or no longer has;
  - the public SOLIDWORKS 2024 API help, including its published example macros;
  - standard geometry (frustum, spherical cap and ellipsoid volumes) for the numbers in
    the testable statements.

  No SOLIDWORKS software, installer, library or file was downloaded, installed, run or
  analysed. No SOLIDWORKS licence was accepted on the machine used. No `.sldprt` file was
  opened; the sample parts named in API example macros were not fetched.
- **Retrieval.** The help site renders its articles with JavaScript and refuses a plain
  request. Each article was fetched over HTTPS with a browser user agent, and its text was
  read from the page's embedded `__NEXT_DATA__` JSON
  (`props.pageProps.helpContentData.helpText`), with HTML stripped by a script run with
  `python3 -I`. A missing page still returns HTTP 200 with "page cannot be found" in its
  data, so each page's content was checked. Page names were found by web search and by
  following the links inside fetched articles. About 70 pages were read. The text was
  paraphrased; none of it is copied into this repository. The help's pictures (which carry
  much of the rib, dome and step-draft examples) were not seen, only their captions.
- **REA.** REA (`rea-agents` 6.5.0, with the `third_party/rea` submodule at `3fc0075`) was
  set up and its research method followed:
  - each finding below is marked **Observed** (stated in a cited source), **Inferred**
    (our reasoning from the sources), **Decision** (AnvilCAD's own choice where the
    sources are silent) or **Unknown**;
  - every observation cites its source.

  REA's analysis tools were **not** run. They inspect artifacts (binaries, packages,
  captures, files), and the only lawful inputs for this behaviour are prose documents.
  Pointing REA at SOLIDWORKS software is ruled out by
  [INTEROPERABILITY.md](../../INTEROPERABILITY.md). The `.sldprt` storage of these
  features was not studied.

## Sources

Base A is `https://help.solidworks.com/2025/English/SolidWorks/sldworks/`. Base A12 is
`https://help.solidworks.com/2012/English/SolidWorks/sldworks/`. Base B is
`https://help.solidworks.com/2024/English/api/sldworksapi/`, and B-I is B +
`SolidWorks.Interop.sldworks~SolidWorks.Interop.sldworks.`. Base C is
`https://help.solidworks.com/2024/English/api/swconst/SolidWorks.Interop.swconst~SolidWorks.Interop.swconst.`.
All were read on 2026-10-18.

| # | Topic | URL | Version |
|---|---|---|---|
| S1 | Ribs (concept) | A + `c_Ribs_Folder.htm` | 2025 |
| S2 | Creating ribs | A + `t_Rib_Features_Overview.htm` | 2025 |
| S3 | Rib PropertyManager | A + `HIDD_FEAT_RIB.htm` | 2025 |
| S4 | Example: rib feature sketches | A + `c_Example_Rib_Feature_Sketches.htm` | 2025 |
| S5 | Example: rib with reference contour draft | A + `c_Example_Rib_Reference_Contour_Draft.htm` | 2025 |
| S6 | Rib PropertyManager and rib features (older wording) | A12 + `HIDD_FEAT_RIB.htm`, A12 + `Rib_Overview.htm` | 2012 |
| S7 | Drafts (overview) | A + `t_drafts_overview.htm` | 2025 |
| S8 | Draft PropertyManager | A + `HIDD_DVE_DRAFT.htm` | 2025 |
| S9 | Neutral plane draft | A + `t_neutral_plane_draft.htm` | 2025 |
| S10 | Example: face propagation with a neutral plane | A + `c_example_face_propagation_neutral_plane.htm` | 2025 |
| S11 | Parting line draft | A + `t_parting_line_draft.htm` | 2025 |
| S12 | Parting line with step draft | A + `t_parting_line_with_step_draft.htm` | 2025 |
| S13 | Example: tapered and perpendicular steps (captions only) | A + `c_example_tapered_and_perpendicular_draft_steps.htm` | 2025 |
| S14 | DraftXpert; DraftXpert PropertyManager, Add tab | A + `t_draftxpert.htm`, A + `HIDD_DVE_DRAFT_MGR_ADD.htm` | 2025 |
| S15 | Draft analysis; Draft Analysis PropertyManager | A + `c_Draft_Analysis_Overview.htm`, A + `HIDD_DVE_DRAFT_ANALYSIS_features.htm` | 2025 |
| S16 | Draft PropertyManager and neutral plane draft (older wording) | A12 + `HIDD_DVE_DRAFT.htm`, A12 + `Neutral_Plane_Draft.htm` | 2012 |
| S17 | Boss-Extrude PropertyManager (draft on extrudes) | A + `r_extrude_propertymanager.htm` | 2025 |
| S18 | Modify boss/cut extrude parameters (draft outward) | A + `r_utilities_modify_parameters_boss_cut_extrudes.htm` | 2025 |
| S19 | Dome (PropertyManager options) | A + `r_dome.htm` | 2025 |
| S20 | Dome examples (irregular faces, continuous, zero distance, elliptical; captions only) | A + `c_example_applying_a_dome_irregular_faces.htm`, `c_example_creating_a_continuous_dome.htm`, `c_example_dome_zero_distance.htm`, `c_example_elliptical_dome.htm` | 2025 |
| S21 | Dome (older wording) | A12 + `HIDD_DVE_FEAT_DOME.htm`; `https://help.solidworks.com/2015/English/SolidWorks/sldworks/HIDD_DVE_FEAT_DOME.htm` | 2012, 2015 |
| S22 | Wrap | A + `HIDD_DVE_SURF_WRAPPING_SKETCH.htm` | 2025 |
| S23 | `IFeatureManager::InsertRib` | B-I + `IFeatureManager~InsertRib.html` | 2024 |
| S24 | `IModelDoc2::InsertRib2` and `IModelDoc2::InsertRib` (obsolete) | B-I + `IModelDoc2~InsertRib2.html`, B-I + `IModelDoc2~InsertRib.html` | 2024 |
| S25 | `IRibFeatureData2` members, and properties `ExtrusionDirection`, `Type`, `Thickness`, `FlipSide`, `RefSketchIndex`, `DraftFromWall` | B-I + `IRibFeatureData2_members.html`, B-I + `IRibFeatureData2~<property>.html` | 2024 |
| S26 | `swRibType_e`, `swRibExtrusionDirection_e` | C + `swRibType_e.html`, C + `swRibExtrusionDirection_e.html` | 2024 |
| S27 | `IFeatureManager::InsertMultiFaceDraft` | B-I + `IFeatureManager~InsertMultiFaceDraft.html` | 2024 |
| S28 | `IDraftFeatureData2` members, and properties `Angle`, `Type`, `StepType`, `ReverseDirection`, `FacePropagation`, `NeutralPlane`, `DirectionPull` | B-I + `IDraftFeatureData2_members.html`, B-I + `IDraftFeatureData2~<property>.html` | 2024 |
| S29 | `swDraftType_e`, `swDraftFacePropagationType_e`, `swDraftStepType_e` | C + `<enum>.html` | 2024 |
| S30 | `IModelDoc2::InsertDome` | B-I + `IModelDoc2~InsertDome.html` | 2024 |
| S31 | `IDomeFeatureData2` members, and properties `Height`, `Elliptical`, `ReverseDir`, `Direction`, `ConstraintPointOrSketch` | B-I + `IDomeFeatureData2_members.html`, B-I + `IDomeFeatureData2~<property>.html` | 2024 |
| S32 | `IFeatureManager::InsertWrapFeature2`; `IWrapSketchFeatureData` members and `Type` | B-I + `IFeatureManager~InsertWrapFeature2.html`, B-I + `IWrapSketchFeatureData_members.html`, B-I + `IWrapSketchFeatureData~Type.html` | 2024 |
| S33 | `swWrapSketchType_e`, `swWrapMethods_e` | C + `<enum>.html` | 2024 |
| S34 | `IFeature::GetTypeName2` (feature type names and data objects) | B-I + `IFeature~GetTypeName2.html` | 2024 |
| S35 | API example macros: Get Rib Feature Data; Create and Modify Dome Feature; Change Height of Dome Feature; Get and Set Direction / Constraint for Dome Feature; Get Faces Affected by Draft Feature; Insert Wrap Feature; Create Wrap Feature on Multiple Faces; Change Wrap Feature Face | B + `Get_Rib_Feature_Data_Example_VB.htm`, `Create_and_Modify_Dome_Feature_Example_VB.htm`, `Change_Height_of_Dome_Feature_Example_VB.htm`, `Get_and_Set_Direction_for_Dome_Feature_Example_VB.htm`, `Get_and_Set_Constraint_for_Dome_Feature_Example_VB.htm`, `Get_Faces_Affected_by_Draft_Feature_Example_VB.htm`, `Insert_Wrap_Feature_Example_VB.htm`, `Create_Wrap_Feature_on_Multiple_Faces_Example_VB.htm`, `Change_Wrap_Feature_Face_Example_VBNET.htm` | 2024 |
| N1 | Frustum volume `h/3 (A1 + A2 + sqrt(A1 A2))`; spherical cap `R = (r² + h²)/(2h)`, `V = πh(3r² + h²)/6`; half-ellipsoid `V = 2/3 π a b c` | Standard geometry, derived here; not fetched | not Dassault |

## Rib

### What a rib is

- **Observed (S1, S6).** A rib is a special kind of extrusion made from open or closed
  sketch contours. It adds material of a given thickness, in a given direction, between
  the contour and the existing part. It can use one or several contours, can be drafted,
  and can take its draft from a chosen reference contour.
- **Observed (S2).** The contour is sketched on a plane that cuts through the part, or on
  a plane parallel to or at an angle to an existing plane.
- **Observed (S3).** On a multibody part the user picks the body the rib joins. The
  PropertyManager lists the contours used ("Selected Contours").
- **Inferred (S1, S3).** A rib is a thin-wall extrusion of the contour: the contour is
  thickened into a strip, and the strip is swept in the extrusion direction until it
  reaches the part. Every rib therefore needs a body to meet.

### Thickness and sides

- **Observed (S3).** Thickness goes on "First Side", "Both Sides" or "Second Side" of the
  sketch. Both Sides splits it equally.
- **Observed (S25).** `Thickness` is the rib's overall thickness, so Both Sides puts half
  on each side.
- **Observed (S23).** `Is2Sided` selects both sides; `ReverseThicknessDir` (single-sided
  only) thickens the side opposite the sketch normal.
- **Inferred (S23).** For a rib parallel to the sketch, First Side is the side the sketch
  plane's normal points to, Second Side the other.
- **Unknown.** Which side is "first" for a rib normal to the sketch, where the thickness
  lies in the sketch plane across the contour.
  - **Decision.** AnvilCAD's first side is to the left of the reference segment's
    direction, looking down the sketch normal (the side of `normal × tangent`).
- **Unknown.** The help's default (S3 lists First Side first; the API example in S35 uses
  two sides).
  - **Decision.** A new rib is Both Sides.

### Extrusion direction and material side

- **Observed (S3, S26).** The extrusion is "Parallel to Sketch" or "Normal to Sketch".
  "Flip material side" reverses the extrusion.
- **Observed (S4).** In the examples, a contour sketched on the part's face, or on a
  plane cutting the part, is extruded parallel to the sketch. A contour sketched on a
  plane parallel to the base, level with the top of the part's walls, is extruded normal
  to the sketch, down into the part.
- **Observed (S23).** `ReferenceEdgeIndex` names the sketch segment that sets the
  material direction and is the draft reference.
- **Inferred (S3, S4, S23).**
  - *Parallel to Sketch.* The strip lies across the sketch plane (its thickness is
    measured along the sketch normal). It is swept within the sketch plane, square to the
    reference segment, towards the part.
  - *Normal to Sketch.* The strip lies in the sketch plane (its thickness is measured in
    the plane, square to the contour). It is swept along the sketch normal, towards the
    part.
- **Unknown.** How SOLIDWORKS picks the default direction (towards the part, it
  appears from S4).
  - **Decision.** AnvilCAD tries both senses and uses the one in which the rib meets the
    part; if both do, it uses the one closer to the reference segment's midpoint, and
    Flip reverses it. If neither meets the part, the rib fails with an error.

### How the rib reaches the part (type)

- **Observed (S3, S26).** Two types:
  - **Linear** (`swRibLinear`, 0): the contour's open ends are carried on in straight
    lines until they meet a boundary.
  - **Natural** (`swRibNatural`, 1): the open ends are carried on along the same curve.
    An arc is continued round its circle, a line along its line, until each meets a
    boundary. The help's example closes an arc into a full circle.
- **Unknown: the sources disagree (S3).** The help says Linear extends the contours "normal to the direction
  of the sketch". Its own example of a line end in a Natural rib is "a straight line
  equation".
  - **Inferred.** Linear carries each open end on along its tangent at that end.
    - For a line, Linear and Natural agree.
    - For an arc, Linear leaves along the tangent, while Natural follows the circle.
- **Observed (S1).** Material is added between the contour and the part.
  - **Decision.** AnvilCAD builds a rib like this:
    1. extend the open ends by the type's rule, far past the part's bounding box;
    2. thicken the chain into a strip;
    3. sweep the strip far past the bounding box in the extrusion direction;
    4. subtract the body from the swept slab;
    5. keep only the pieces that touch the original (unextended) contour;
    6. union those pieces with the body.

    A kept piece that reaches the bounding box has not met the part on that side, and
    the rib fails with "the rib does not meet the part". This makes the rib stop at the
    first wall it meets in every direction (like "Up To Next"). It never runs through a
    floor into open space beyond.
- **Inferred (S1, S3).** A closed contour has no open ends to extend. Its strip is the
  ring between its two offsets, and the inside of the loop is not filled.

### Draft

- **Observed (S3, S23, S25).** "Draft On/Off" with a draft angle tapers the rib. "Draft
  Outward" makes the draft outward; cleared, it is inward. With draft on, the thickness
  holds either "At sketch plane" or "At wall interface".
- **Observed (S23, S24).** When the rib is drafted, the midpoint of the reference segment
  keeps the given thickness, and the thickness elsewhere varies.
- **Observed (S3, S5).**
  - "Next Reference" cycles through the contours to choose the one the draft is based on.
    It appears only for ribs parallel to the sketch with draft on.
  - Only one contour can be the reference.
  - The faces made by thickening the reference contour keep the rib thickness. Other
    contours' faces take varying thickness.
  - Draft is outward on one side of the reference line (the arrow's side) and inward on
    the other.
- **Inferred (S17, S18).** The extrude command words its draft the same way, and there
  "Draft outward" turns the slope from narrowing to widening. Read the same way, a rib
  with inward draft (the default) narrows as it runs from the sketch into the part, and
  outward draft widens. An inward rib is also the mouldable shape: thicker at the open
  side of the part and thinner at the wall.
- **Inferred.** Each thickness face of the rib tilts by the draft angle, about its line in
  the plane where the thickness is held (the sketch plane, or the wall interface).
  - With a draft angle α, a two-sided rib's thickness changes by `2·d·tan α` over a
    distance `d` along the extrusion direction.
  - A one-sided rib's thickness changes by `d·tan α`, with only the free face tilting.
- **Unknown.** Whether the sketch-side face of a one-sided drafted rib also tilts.
- **Unknown.** Where "wall interface" is measured when the wall is not flat or not square
  to the extrusion direction.
  - **Decision.** AnvilCAD measures it where a line from the reference segment's
    midpoint, in the extrusion direction, first meets the part.
- **Unknown: the sources disagree (S23, S25).** `InsertRib` describes `IsDraftedFromWall` as true when the
  draft is from the wall. `IRibFeatureData2::DraftFromWall` describes true as drafting
  from the sketch plane. `anvil-migrate` must check one case against geometry, not trust
  either description.
- **Decision.** A draft that would make the rib's thickness zero or negative anywhere in
  its extent is an error.

### Rib in the tree

- **Observed (S35).** The API example names its rib "Rib1".
- **Observed (S34).** `GetTypeName2` returns "Rib" with data object `IRibFeatureData2`.
- **Decision.** AnvilCAD's prefix is "Rib". The rib depends on its sketch and, through
  it, on the body.

## Draft

### Types

- **Observed (S7, S8, S29).** Three types: neutral plane (`swNeutralPlaneDraft`, 0),
  parting line (`swPartingLineDraft`, 1) and step draft (`swStepDraft`, 3). The value 2
  is unused in the published enumeration.
- **Observed (S7, S9).** Draft can also be part of an extruded base, boss or cut. It
  works on solid and surface bodies.
- **Observed (S8, S14).** The command has two modes. *Manual* edits one draft feature.
  *DraftXpert* (neutral plane only) manages the drafts itself, which can include
  reordering drafts before neighbouring fillets. Editing an existing draft always opens
  the manual form.

### Neutral plane draft

- **Observed (S8, S9, S16).** The inputs are:
  - a neutral plane, which is a planar face or a plane feature;
  - the faces to draft;
  - a draft angle;
  - a face propagation mode;
  - "Reverse direction", which slants the draft the other way.
- **Observed (S8, S9).** The draft angle is measured perpendicular to the neutral plane.
- **Observed (S10).**
  - The +Z direction of the neutral plane is the direction in which the part ejects from
    the mould (the direction of pull).
  - Reverse direction changes which way the faces slant, not the direction of pull.
  - The examples use the part's top face as the neutral plane.
- **Observed (S27, S28).** The API stores the angle in radians and the neutral plane as a
  face (`NeutralPlane`). `FlipDir` / `ReverseDirection` flips the draft.
- **Observed (S35).** A draft changes existing faces and makes no new ones.
  `IFeature::GetFaces` on a draft returns nothing; `IDraftFeatureData2::GetDraftedEntities`
  lists the faces it changed.
- **Inferred (S8, S9, N1).** The geometry, for a face drafted by angle α:
  - The face turns about its *hinge*, the line where the face meets the neutral plane
    (extended if need be). Points on the hinge do not move, which is why the plane is
    called neutral.
  - After drafting, the angle between the face and the direction of pull is α.
  - A plane square to the neutral plane becomes a plane at α to the pull.
  - A cylinder whose axis is along the pull becomes a cone with half-angle α. Its
    radius is unchanged where it meets the neutral plane.
  - Neighbouring faces that are not drafted are trimmed or extended to meet the moved
    faces. The topology stays the same.
- **Unknown.** Which way SOLIDWORKS slants faces when Reverse is off. The help gives the
  pull direction and says that Reverse flips the slant, but no rule for the default, and
  the pictures were not available.
  - **Decision.**
    - The pull is the neutral plane's normal: a plane feature's own normal, or a planar
      face's outward normal.
    - Without Reverse, the drafted faces close in on the body as they go in the direction
      of pull, so the body gets narrower in that direction. Reverse makes it wider.
    - So a box standing on a plane whose normal points up into it narrows towards its top.
      With the box's own top face as the neutral plane (normal up, away from the body),
      the box widens towards its bottom.
    - `anvil-migrate` should not rely on matching the flag alone. It should compare a
      drafted face's normal after the rebuild with the face normal the API reports, and
      flip Reverse if they disagree.
- **Inferred.** A face that crosses the neutral plane tilts as a whole. On one side of the
  plane the body narrows, and on the other it widens. The draft never splits a face (S35:
  it makes no new faces).
- **Decision.** These are errors on the feature, not panics:
  - an angle not strictly between 0° and 90°;
  - a face to draft that is parallel to the neutral plane (it has no hinge);
  - a face whose drafted surface AnvilCAD cannot represent exactly: a plane, or a
    cylinder or cone whose axis is along the pull, are supported first;
  - a draft that would turn a face inside out or make neighbouring faces stop meeting.

### Face propagation

- **Observed (S8, S16, S27, S29).** The values are:

  | Help name | API value | What it drafts besides the selection |
  |---|---|---|
  | None | `swFacePropNone` 0 | nothing |
  | Along Tangent | `swFacePropTangent` 1 | faces tangent to a selected face, in a chain (faces joined by fillets). S27 adds: that share the base neutral plane |
  | All Faces | `swFacePropAllLoops` 2 | every face next to the neutral plane (and extruded from it) |
  | Inner Faces | `swFacePropInnerLoops` 3 | faces next to the neutral plane along its inner loops: the walls of a pocket or boss |
  | Outer Faces | `swFacePropOuterLoops` 4 | faces next to the neutral plane along its outer loop: the sides of a box drafted about its bottom face |

  Parting line and step drafts offer only None and Along Tangent.
- **Inferred (S27).** All, Inner and Outer are defined by the neutral plane's loops, so
  they need the neutral plane to be a face of the body. With a plane feature they have
  nothing to follow.
  - **Decision.** AnvilCAD refuses them with a plane feature as the neutral plane.
- **Inferred (S27).** Under All, Inner and Outer, the selected faces seem to be only a
  starting point: the drafted faces are the neighbours across the neutral face's loops.
  - **Decision.** AnvilCAD drafts the union of the selected faces and the propagated
    ones.

### Parting line and step drafts

- **Observed (S8, S11).**
  - A parting line draft drafts faces about a parting line (model edges or a split line)
    that need not be planar. The inputs are a direction of pull (an edge or face, with
    Reverse) and the parting lines.
  - Direction 1 takes an angle, and an optional Direction 2 takes another angle the
    opposite way; "Symmetrical Draft" uses the same angle for both.
  - "Other Face" flips which side of a parting line segment is drafted. It is not
    available when Direction 2 has an angle.
- **Observed (S8, S11).** "Allow reduced angle" (parting line only) lets the draft go ahead
  when the steepest angle between the pull and a side face's normal, plus the draft angle,
  is 90° or more. Parts of the face then get less than the stated angle.
- **Observed (S12, S13, S29).**
  - A step draft is a parting line draft whose face turns about the plane that gives the
    pull direction. This leaves small step faces.
  - The steps are "Tapered" (`swDraftTaperedStep`, 3), built like the tapered faces, or
    "Perpendicular" (`swDraftPerpendicular`, 6), square to the original face.
  - The parting line must touch the parting plane on each drafted face, with its other
    segments along the pull and none square to the plane.
- **Unknown.** The exact construction of step faces. The help explains it only with
  pictures.

### Draft on extrusions

- **Observed (S17).** An extrude's "Draft On/Off" adds draft at an angle, with "Draft
  outward" as an option. The help pictures 10° inward and outward drafts.
- **Observed (S18).** Draft outward changes the draft's slope from narrowing to widening.
- **Inferred.** An extrusion with draft α is the same solid as a straight extrusion
  drafted about the sketch plane, with pull along the extrusion direction, on all its side
  faces. Inward is this specification's default (narrowing in the pull direction), and
  outward is Reverse.

### Draft analysis and DraftXpert

- **Observed (S15).** Draft analysis colours faces by their angle to a pull direction,
  against a reference angle. The categories are:
  - positive draft: the face's angle to the pull is greater than the reference;
  - requires draft: between minus and plus the reference;
  - negative draft: less than minus the reference;
  - straddle faces, which carry both signs;
  - steep faces, for curved faces of which only parts meet the angle.

  Reversing the pull swaps positive and negative.
- **Decision.** Neither tool is in AnvilCAD's first scope. Draft analysis is a display
  mode, and the DraftXpert automates choices around the manual feature.

### Draft in the tree

- **Observed (S34).** `GetTypeName2` returns "Draft" with data object
  `IDraftFeatureData2`.
- **Unknown.** The default feature name. No fetched source shows one.
  - **Inferred.** It follows the pattern of Rib1, Dome1 and Wrap1 (S35).
  - **Decision.** AnvilCAD's prefix is "Draft". A draft depends on the features that own
    its neutral face and drafted faces. Drafted faces keep their `FaceId`s, because a
    draft makes no new faces.

## Dome

### Inputs and options

- **Observed (S19, S21).** The inputs and options are:
  - **Faces to Dome:** one or more faces, planar or not. A face whose centroid lies
    outside the face is allowed, so irregular outlines work. One feature can dome several
    faces.
  - **Distance:** how far the dome rises.
  - **Reverse Direction:** a concave dome. The default is convex.
  - **Constraint Point or Sketch:** a sketch of points that shapes the dome. It disables
    Distance.
  - **Direction:** a vector (a linear edge, or two sketch points) to raise the dome along
    instead of the face normal.
  - **Elliptical dome:** for cylindrical or conical models. The dome is half an
    ellipsoid, and its height is one of the ellipsoid's radii.
  - **Continuous dome:** for polygonal faces. The dome slopes up evenly on every side.
    Cleared, it rises square to the polygon's edges. It is not available for four-sided
    polygons, or with a constraint or a direction.
  - **Distance 0:** allowed on cylindrical and conical models. The height is then worked
    out from the arc's radius, so that the dome is tangent to the cylinder or cone below.
- **Observed (S30, S31).** `InsertDome(Height, ReverseDir, DoEllipticSurface)`, with the
  height in metres. `ReverseDir` is true for concave. `Elliptical` is true for the half
  ellipsoid.

### Geometry

- **Inferred (S19, S30, N1).** Distance is the dome's height: the greatest distance from
  the face's plane to the dome's surface, measured along the face normal (or the
  Direction).
- **Inferred (N1).** A plain (not elliptical) dome on a circular face of radius `r`, with
  height `h`:
  - It is a spherical cap that meets the face's circular edge.
  - Its sphere radius is `R = (r² + h²) / (2h)`, centred on the face's axis at `h − R`
    above the face.
  - Its volume is `πh(3r² + h²)/6`.
  - It meets a cylinder below at an angle unless `h = r`, which gives a hemisphere
    tangent to the cylinder.
- **Inferred (S19, S31, N1).** An elliptical dome on a circular face of radius `r`, with
  height `h`:
  - It is half a spheroid with semi-axes `r`, `r` and `h`, of volume `2/3·π r² h`.
  - It is always tangent to a cylinder below, whatever `h` is.
  - On an elliptical face with semi-axes `a` and `b`, it is half an ellipsoid of volume
    `2/3·π a b h`. The API example domes an ellipse-shaped boss face this way (S35).
- **Inferred (S19).** Distance 0 on a cylinder of radius `r` gives the tangent dome, a
  hemisphere of radius `r`. For an elliptical dome that is the case `h = r`.
- **Observed (S19, S30).** Reverse makes the same shape concave: the cap is cut into the
  face instead of added.
- **Unknown.** The surface of a dome on a non-circular planar face (a rectangle or
  polygon), with or without Continuous dome. Also unknown:
  - what "rises normal to the edges" means exactly;
  - why four-sided polygons are excluded;
  - how a dome on a non-planar face is built;
  - how several constraint points combine.

  The help explains these only with pictures. AnvilCAD should not guess them.
- **Unknown.** Whether a plain dome with `h > r` (more than a hemisphere, overhanging the
  face) is allowed.
  - **Decision.** AnvilCAD allows it, since the result is a valid union. It refuses
    `h ≤ 0`, except for exactly 0 on a circular face, which means the tangent dome.

### Dome in the tree

- **Observed (S35).** The API example names its dome "Dome1".
- **Observed (S34).** `GetTypeName2` returns "Dome" with data object
  `IDomeFeatureData2`.
- **Decision.** AnvilCAD's prefix is "Dome". A dome depends on the feature that owns its
  face.

## Wrap (brief)

- **Observed (S22, S33).** A wrap lays a sketch onto a face as one of three types:
  - **Emboss** (`swWrapSketchType_Emboss`, 0): raised;
  - **Deboss**, called `swWrapSketchType_Engrave` (1) in the API: sunk in;
  - **Scribe** (`swWrapSketchType_Scribe`, 2): the contours only imprinted, splitting
    the face.
- **Observed (S22, S32, S33).** There are two methods:
  - **Analytical** (`swWrapMethods_Analytical`, 0) wraps onto cylindrical, conical or
    extruded faces. The sketch plane must be tangent to the face, so that the face normal
    and sketch normal are parallel where they are closest.
  - **Spline Surface** (`swWrapMethods_SplineSurface`, 1) wraps onto any faces, including
    several at once, but cannot go all the way round a model. `InsertWrapFeature2` takes
    a mesh accuracy of 1 to 10 for it.
- **Observed (S22, S32).**
  - The sketch must contain only closed contours (a 2D sketch, not 3D), except that
    Scribe accepts single-line ("stick") fonts.
  - Emboss and deboss take a thickness and Reverse direction. They can also take a pull
    direction: along a line or edge, or normal to a plane. With none, the wrap goes
    normal to the sketch plane.
- **Inferred.** Analytical wrapping onto a cylinder of radius `R` keeps lengths on the
  cylinder: a sketch distance `x` along the wrap becomes the angle `x / R`.
  - An emboss of thickness `t` grows the wrapped region outwards by `t` along the
    normal. Its volume is `A·(t + t²/(2R))` for a sketch region of area `A`.
  - A deboss removes `A·(t − t²/(2R))`.
  - A scribe adds no volume.
- **Observed (S35).** An API example names its wrap "Wrap1".
- **Observed (S34).** The `GetTypeName2` table lists `IWrapSketchFeatureData` under the
  type name "Emboss" and has no "Wrap" entry.
- **Unknown.** Whether deboss and scribe wraps report another type name.
- **Decision.** Wrap is not in AnvilCAD's first scope. When it comes, its prefix is
  "Wrap".

## Testable statements

The rib fixture is an open box: outer `x ∈ [0, 100]`, `y ∈ [0, 60]`, `z ∈ [0, 30]`, with
5 mm walls and a 5 mm floor. Its cavity is `x ∈ [5, 95]`, `y ∈ [5, 55]`, `z ∈ [5, 30]`,
and its volume is 67 500 mm³. Volumes are in mm³ and tolerances are 1e-6 relative unless
stated.

1. A rib parallel to the sketch, on the plane `y = 30` (normal +y), from a line
   `(5, 30) → (95, 30)` in (x, z). Both sides, thickness 4, material towards −z, no
   draft. It fills `x ∈ [5, 95]`, `y ∈ [28, 32]`, `z ∈ [5, 30]`, adding 9000; the part is
   76 500.
2. The same rib from a shorter line `(20, 30) → (80, 30)`, Linear. Both ends extend to the
   walls, and it adds the same 9000.
3. Statement 1 with First Side puts the rib at `y ∈ [30, 34]` (rib centroid `y = 32`);
   Second Side puts it at `y ∈ [26, 30]`. Each adds 9000.
4. A rib normal to the sketch, on the plane `z = 30` (normal +z), from the line `y = 30`,
   `x ∈ [20, 80]`. Both sides, thickness 4. The direction is chosen automatically, so it
   runs down (−z) to the floor. It adds 9000. Flip material side sends it up (+z), where
   it meets nothing, which is an error.
5. A closed circle of radius 10 centred `(50, 30)` on the plane `z = 30`, normal to the
   sketch, both sides, thickness 2. This gives a ring between radii 9 and 11, 25 deep,
   adding `1000π` = 3141.593. The disc inside the ring stays empty.
6. A quarter arc of that circle, Natural type, also closes into the full ring and adds
   `1000π`.
7. Statement 1 with 2° inward draft (Draft Outward cleared), thickness held at the
   sketch plane. The thickness is 4 at `z = 30` and `4 − 50·tan 2°` = 2.253962 at the
   floor. The rib adds `9000 − 56250·tan 2°` = 7035.707.
8. Statement 1 with 2° outward draft, at the sketch plane, adds `9000 + 56250·tan 2°` =
   10 964.293. With 2° inward draft held at the wall interface, the thickness is 4 at the
   floor and 5.746038 at the sketch plane, and the rib adds the same 10 964.293.
9. Statement 7 with 5° (more than `atan(0.08)` = 4.5739°) would make the rib's thickness
   negative before it reaches the floor, which is an error.
10. Draft fixture: a box `[0, 20] × [0, 20] × [0, 10]`, neutral plane `z = 0` (a plane
    feature, normal +z), its four side faces drafted 5°, Reverse off.
    - The top face becomes a square of side `20 − 20·tan 5°` = 18.250227 (area
      333.070776).
    - The bottom stays 20 × 20.
    - The volume is `10/3·(400 + a² + 20a)` = 3660.251034.
    - The body still has 6 faces, and every face keeps its `FaceId`.
11. Statement 10 with Reverse: the top side is 21.749773 and the volume 4360.160342.
12. Statement 10's box with its top face as the neutral plane (outward normal +z), Reverse
    off: the top stays 20 × 20, the bottom grows to 21.749773, and the volume is
    4360.160342.
13. Statement 10 with only the face `x = 20` drafted: that face meets the base at `x = 20`
    and the top at `x = 20 − 10·tan 5°` = 19.125113. The volume is `4000 − 1000·tan 5°` =
    3912.511336.
14. Statement 10 with the neutral plane at `z = 5` (normal +z): the top side is 19.125113,
    the bottom 20.874887, and the volume `4000 + (1000/3)·tan² 5°` = 4002.551422.
15. A cylinder of radius 10 and height 20 on `z = 0`, its side drafted 5° about `z = 0`,
    becomes a cone with half-angle 5°. Its radius is 10 at `z = 0` and 8.250227 at
    `z = 20`, and the volume `π·20/3·(10² + 10·8.250227 + 8.250227²)` = 5247.894568 (from
    6283.185307).
16. Propagation fixture: a base `[0, 40]² × [0, 10]` with a boss `[15, 25]² × [10, 15]`,
    neutral face the base's top face (`z = 10`, outward normal +z), 5°, no faces selected
    beyond what propagation finds.
    - Inner Faces drafts only the boss's 4 sides. The boss top becomes a square of side
      9.125113, and the total volume is 16 457.531379.
    - Outer Faces drafts only the base's 4 sides. They hinge at `z = 10`, the bottom grows
      to side 41.749773, and the total is 17 210.114997.
    - All Faces drafts all 8, and the total is 17 167.646376.
17. All, Inner or Outer propagation with a plane feature (not a face) as the neutral plane
    is an error. A draft angle of 0° or 90°, or a face to draft parallel to the neutral
    plane, is an error.
18. An extrusion of a 20 × 20 square, 10 deep, with 5° draft (outward off) has the
    geometry of statement 10 (3660.251034). With outward on, it has the geometry of
    statement 11 (4360.160342).
19. A dome of height 5 on the top face of a cylinder of radius 10 and height 20:
    - The dome is a sphere of radius 12.5 centred 7.5 below the top face.
    - It adds `π·5·(300 + 25)/6` = 850.848010, for a total of 7134.033318.
    - The dome face's area is `2π·12.5·5` = 392.699082.
20. Statement 19 with Reverse cuts the same cap, for a total of 5432.337297.
21. Statement 19 with Distance 0 makes a hemisphere of radius 10, tangent to the
    cylinder: it adds 2094.395102, for a total of 8377.580410.
22. Statement 19 with Elliptical makes a half spheroid with semi-axes 10, 10 and 5,
    tangent to the cylinder: it adds `2/3·π·100·5` = 1047.197551, for a total of
    7330.382858. With height 15, it adds 3141.592654.
23. A plain dome of height 15 on that radius-10 face (more than a hemisphere) is a cap of
    sphere radius 10.833333, adding 4123.340358.
24. (Wrap, when built.) A 20 × 10 rectangle wrapped analytically onto a cylinder of radius
    25 with thickness 1 adds 204 as an emboss and removes 196 as a deboss. A scribe leaves
    the volume unchanged and adds one face.

## API names

For `anvil-migrate`, names and values only.

- **Rib.**
  - `IFeatureManager::InsertRib(Is2Sided, ReverseThicknessDir, Thickness,
    ReferenceEdgeIndex, ReverseMaterialDir, IsDrafted, DraftOutward, DraftAngle,
    IsNormToSketch, IsDraftedFromWall)`.
  - Obsolete: `IModelDoc2::InsertRib2` (the same without `IsDraftedFromWall`) and
    `IModelDoc2::InsertRib`.
  - `IRibFeatureData2`: `Body`, `DraftAngle`, `DraftFromWall`, `DraftOutward`,
    `EnableDraft`, `ExtrusionDirection`, `FlipSide`, `IsTwoSided`, `RefSketchIndex`,
    `ReverseThicknessDir`, `Thickness` (overall), `Type`, `NextReference()`,
    `AccessSelections`, `ReleaseSelectionAccess`.
  - `swRibType_e`: `swRibLinear` 0, `swRibNatural` 1.
  - `swRibExtrusionDirection_e`: `swRibNormalToSketch` 0, `swRibParallelToSketch` 1.
  - Type name "Rib".
- **Draft.**
  - `IFeatureManager::InsertMultiFaceDraft(Angle, FlipDir, EdgeDraft, PropType,
    IsStepDraft, IsBodyDraft)`. The angle is in radians. Selection marks: 1 for the
    direction (neutral plane), 2 for the faces to draft, 4 for draft edges.
  - `IDraftFeatureData2`: `AllowReducedAngle`, `Angle`, `DirectionPull`,
    `FacePropagation`, `FacesToDraft`, `NeutralPlane`, `PartingLines`,
    `ReverseDirection`, `StepType`, `Type` (read only), `GetDraftedEntities`,
    `GetDraftedEntityCount`, `GetFacesToDraftCount`, `GetPartingLinesCount`,
    `Get/SetOtherFacesFlagAtIndex`.
  - `swDraftType_e`: `swNeutralPlaneDraft` 0, `swPartingLineDraft` 1, `swStepDraft` 3.
  - `swDraftFacePropagationType_e`: `swFacePropNone` 0, `swFacePropTangent` 1,
    `swFacePropAllLoops` 2, `swFacePropInnerLoops` 3, `swFacePropOuterLoops` 4.
  - `swDraftStepType_e`: `swDraftTaperedStep` 3, `swDraftPerpendicular` 6.
  - Type name "Draft".
- **Dome.**
  - `IModelDoc2::InsertDome(Height, ReverseDir, DoEllipticSurface)`. The height is in
    metres, and the face takes selection mark 1.
  - `IDomeFeatureData2`: `ConstraintPointOrSketch`, `Direction`, `Elliptical`, `Faces`,
    `Height`, `ReverseDir` (true = concave), `GetFaceCount`, `IGetFaces`, `ISetFaces`.
    `Face` and `IFace` are obsolete.
  - The API help shows no property for Continuous dome.
  - Type name "Dome".
- **Wrap.**
  - `IFeatureManager::InsertWrapFeature2(Type, Thickness, ReverseDir, Method,
    MeshFactor)`. The thickness ranges from 0.00001 to 10000. Selection marks: 1 for the
    faces, 2 for the pull direction, 4 for the sketch.
  - `IFeatureManager::InsertWrapFeature` is obsolete and takes one face.
  - `IWrapSketchFeatureData`: `Face`, `PullDirection`, `ReverseDirection`,
    `SourceSketch`, `Thickness`, `Type`.
  - `swWrapSketchType_e`: `swWrapSketchType_Emboss` 0, `swWrapSketchType_Engrave` 1
    (Deboss), `swWrapSketchType_Scribe` 2.
  - `swWrapMethods_e`: `swWrapMethods_Analytical` 0, `swWrapMethods_SplineSurface` 1.
  - Type name listed as "Emboss" (see Unknown above).

## Recommended scope

In order:

1. **Neutral plane draft on planar faces, and on cylinders and cones whose axis lies
   along the pull.**
   - Data: a new `FeatureKind::Draft` with:
     - the neutral plane (a `PlaneRef`, or a `FaceId` for face propagation);
     - `faces: Vec<FaceId>`;
     - `angle: Dim`;
     - `reverse: bool`;
     - propagation (None, Outer, Inner, All first; Along Tangent once fillets are
       common).
   - Geometry: it replaces surfaces and keeps topology, as `anvil_brep::offset` does
     (`offset_faces` moves each face onto a new exact surface and re-solves edges and
     vertices with the same indices). A drafted plane becomes a tilted `Surface::Plane`,
     and a drafted cylinder becomes a `Surface::Cone` with `half_angle = α`. Because a
     draft makes no new faces, every `FaceId` survives, and later fillets and shells
     keep their references.
   - Tests: statements 10 to 17.
2. **Draft on extrudes.** Add `draft: Option<(Dim, bool /*outward*/)>` to
   `ExtrudeFeature` and build it through the same drafting code (statement 18). Users
   reach for this more often than the Draft feature.
3. **Dome on circular (and elliptical) planar faces:** plain, elliptical, distance 0 and
   reverse.
   - A plain dome is a `Surface::Sphere` cap. Union it with the body, or subtract it for
     reverse.
   - An elliptical dome needs a spheroid. The kernel has no ellipsoid surface, so make
     it a `Surface::BSpline` (an exact rational patch), or start with plain domes only.
   - Tests: statements 19 to 23.
4. **Rib from one sketch with open lines and arcs:** parallel and normal to the sketch,
   the three thickness sides, flip, Linear extension, and closed contours.
   - Build it with the slab-and-keep-the-touching-pieces rule above.
   - The sketch side needs a new way to get *open* chains:
     `anvil_sketch::regions::profiles` rejects open contours today.
   - Then add draft (sketch plane / wall interface, inward / outward), and then the
     Natural type.
   - Tests: statements 1 to 9.
5. Later, once the sources or sample files settle them:
   - parting line and step drafts;
   - Along Tangent propagation over B-spline faces;
   - domes on polygons and non-planar faces, with a constraint point or a direction;
   - Wrap.

Leave out: DraftXpert and draft analysis (tools around the feature, not geometry), "Allow
reduced angle", and the Spline Surface wrap method.

## Unknown or not covered

1. Which way SOLIDWORKS slants a neutral plane draft by default. AnvilCAD's rule (narrow
   in the pull direction) is a Decision, and `anvil-migrate` should check it against
   geometry.
2. Whether a rib's First Side is the sketch normal's side in every case, and which side it
   is for ribs normal to the sketch.
3. The opposite descriptions of the rib's draft-from-wall flag in `InsertRib` and in
   `IRibFeatureData2::DraftFromWall`.
4. The exact meaning of "Linear" extension, worded as "normal to the direction of the
   sketch".
5. Where the wall interface is measured on uneven walls. How the reference contour draft
   splits outward and inward across several contours (S5 is mostly pictures).
6. The surfaces of domes on non-circular planar faces and on non-planar faces, Continuous
   dome, and constraint sketches with several points.
7. The construction of step draft faces.
8. The default feature name of a draft ("Draft1" is assumed, not seen). The type name of
   deboss and scribe wraps.
9. The defaults SOLIDWORKS uses for thickness, angles and heights. The PropertyManager
   remembers its last used state (S8), so it may have no fixed defaults.
