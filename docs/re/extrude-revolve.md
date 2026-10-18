# Extrude and revolve options

What AnvilCAD's extrude and revolve features do, option by option. An extrude sweeps the
regions of a sketch along a straight direction; a revolve turns them about an axis. Each
adds material (boss) or removes it (cut). SOLIDWORKS users know these as Boss-Extrude,
Cut-Extrude, Revolve and Cut-Revolve, so AnvilCAD follows their published behaviour where
that makes sense. Today AnvilCAD has Blind, Mid Plane, Through All and Through All - Both
with Reverse for extrudes, and a one-direction angle for revolves
(`crates/anvil-model/src/document.rs`: `EndCondition`, `ExtrudeFeature`,
`RevolveFeature`; `crates/anvil-model/src/rebuild.rs`). This specification covers the rest
of both PropertyManagers so that they can be implemented, and so that `anvil-migrate` can
translate them.

## How this was studied

- **Inputs.** Only published information was used:
  - Dassault's public online help for SOLIDWORKS 2025, with older years (2010, 2012, 2013,
    2021, 2024) where the 2025 page is missing or says less;
  - the public SOLIDWORKS Connected tutorials (2024);
  - the 2024 and 2025 API help;
  - one third-party tutorial article and one public SOLIDWORKS user-forum post (N1, N2).

  No SOLIDWORKS software, installer, library or file was downloaded, installed, run or
  analysed. No SOLIDWORKS licence was accepted on the machine used. No `.sldprt` file was
  opened.
- **Retrieval.** The help site renders its articles with JavaScript and refuses a plain
  request. Each article was fetched over HTTPS with a browser user agent, and its text was
  read from the page's embedded `__NEXT_DATA__` JSON
  (`props.pageProps.helpContentData.helpText`), with HTML stripped by a script run with
  `python3 -I`. Link targets were read the same way to find related pages. A missing page
  still returns HTTP 200, so each page was checked for the "page cannot be found" marker.
  Page names were found by following links and by web search. The text was paraphrased;
  none of it is copied into this repository.
- **REA.** REA (`rea-agents` 6.5.0, with the `third_party/rea` submodule at `3fc0075`) was
  set up and its research method followed:
  - each finding below is marked **Observed** (stated in a cited source), **Inferred** (our
    reasoning from the sources, including geometry worked out from them), **Decision**
    (AnvilCAD's own choice where the sources are silent) or **Unknown**;
  - every observation cites its source.

  REA's analysis tools were **not** run. They inspect artifacts (binaries, packages,
  captures, files), and the only lawful inputs here are prose documents. Pointing REA at
  SOLIDWORKS software is ruled out by [INTEROPERABILITY.md](../../INTEROPERABILITY.md).
  How extrude and revolve features are stored in `.sldprt` files was not studied.
- **Pictures.** Several help pages explain an option only by a picture (S1 draft, S8
  translate surface, S10 thin features, S11 selected contours). The pictures were not
  read; only their captions were. Where an answer would have come from a picture, this
  specification says so.
- **Not readable.** A TriMech blog post on the revolve prompt for open sketches returned
  HTTP 403 and was not used.

## Sources

Base A is `https://help.solidworks.com/2025/English/SolidWorks/sldworks/`. Base B is
`https://help.solidworks.com/2024/English/api/sldworksapi/`. Base C is
`https://help.solidworks.com/2024/English/api/swconst/`. Base T is
`https://help.solidworks.com/2024/English/swconntutorial/`. All were read on 2026-10-18.

| # | Topic | URL | Version |
|---|---|---|---|
| S1 | Extrude PropertyManager | A + `r_extrude_propertymanager.htm` | 2025 |
| S2 | Extrude PropertyManager (older wording, adds cap thickness) | `https://help.solidworks.com/2010/English/SolidWorks/sldworks/LegacyHelp/Sldworks/Features/Extrude_PropertyManager.htm` | 2010 |
| S3 | End Condition Extrude | A + `c_end_condition_extrude.htm` | 2025 |
| S4 | End Condition Types | A + `c_End_Condition_Types.htm` | 2025 |
| S5 | End Condition Extrude (older) | `https://help.solidworks.com/2012/English/SolidWorks/Sldworks/End_Condition_Extrude.htm` | 2012 |
| S6 | Extrudes overview | A + `Hidd_dve_end_spec_dlg.htm` | 2025 |
| S7 | Creating an extrude feature | A + `t_creating_an_extrude_feature.htm` | 2025 |
| S8 | Example: Translate Surface (captions) | A + `c_Example_of_Translate_Surface.htm` | 2025 |
| S9 | Example: Specifying the direction of extrusion | A + `c_example_specifying_the_direction_of_extrustion.htm` | 2025 |
| S10 | Example: Extruding thin features (captions) | A + `c_example_extruding_thin_features.htm` | 2025 |
| S11 | Example: Selected contours in a cut extrude (no text) | A + `c_example_using_selected_contours_cut_extrude.htm` | 2025 |
| S12 | Feature scope in multibody parts | A + `r_feature_scope_multibody_parts.htm` | 2025 |
| S13 | Cuts; Bodies to Keep | A + `c_cuts.htm` | 2025 |
| S14 | Surface extrudes (cap end, extrude from face) | A + `c_Surface_Extrudes.htm` | 2025 |
| S15 | Check Sketch for Feature Usage: allowed contour types | A + `HIDD_SKETCH_DIAGNOSTICS.htm` | 2025 |
| S16 | Contour selection | `https://help.solidworks.com/2012/English/SolidWorks/sldworks/Contour_Selection.htm` | 2012 |
| S17 | Modify parameters for boss or cut extrudes (draft outward) | `https://help.solidworks.com/2021/english/SolidWorks/sldworks/r_utilities_modify_parameters_boss_cut_extrudes.htm` | 2021 |
| S18 | Revolve PropertyManager | A + `HIDD_DVE_FEAT_REV.htm` | 2025 |
| S19 | Revolve PropertyManager (older: default angle, 360° limit) | `https://help.solidworks.com/2010/english/solidworks/sldworks/legacyhelp/sldworks/Features/HIDD_DVE_FEAT_REV.htm` | 2010 |
| S20 | Revolves overview (centerline rules) | A + `c_Revolves_Folder.htm` | 2025 |
| S21 | Creating revolve features | A + `t_Creating_Revolve_Features.htm` | 2025 |
| S22 | Tutorial: Extruding a thin feature | T + `t_tut_pattern_extrudethin.htm` | 2024 |
| S23 | Tutorial: Completing the thin feature extrusion | T + `t_tut_pattern_completethin.htm` | 2024 |
| S24 | What's New: multiple contours for thin extrudes | `https://help.solidworks.com/2013/english/whatsnew/t_selecting_multiple_contours.htm` | 2013 |
| S25 | `IExtrudeFeatureData2` members | B + `SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.IExtrudeFeatureData2_members.html` | 2024 |
| S26 | `IExtrudeFeatureData2` member pages: `GetDepth`, `SetDepth`, `ReverseDirection`, `BothDirections`, `GetDraftOutward`, `GetDraftAngle`, `SetDraftWhileExtruding`, `GetWallThickness`, `ThinWallType`, `FromType`, `FromOffsetDistance`, `FromOffsetReverse`, `GetFromEntity`, `GetEndCondition`, `SetEndConditionReference`, `GetReverseOffset`, `GetTranslateSurface`, `GetDirectionReference`, `CapEnds`, `CapThickness`, `SetAutoFillet`, `GetAutoFilletRadius`, `LinkToThickness`, `Merge`, `FlipSideToCut` | B + `SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.IExtrudeFeatureData2~<member>.html` | 2024 |
| S27 | `IFeatureManager::FeatureExtrusion3` | B + `SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.IFeatureManager~FeatureExtrusion3.html` | 2024 |
| S28 | `IFeatureManager::FeatureCut4` | B + `…IFeatureManager~FeatureCut4.html` | 2024 |
| S29 | `IFeatureManager::FeatureExtrusionThin2` | B + `…IFeatureManager~FeatureExtrusionThin2.html` | 2024 |
| S30 | `IFeatureManager::FeatureCutThin2` | B + `…IFeatureManager~FeatureCutThin2.html` | 2024 |
| S31 | Obsolete `FeatureExtrusion2`, `FeatureCut3`, `FeatureRevolve` | B + `…IFeatureManager~<method>.html` | 2024 |
| S32 | `IFeatureManager::FeatureRevolve2` | B + `…IFeatureManager~FeatureRevolve2.html` | 2024 |
| S33 | `IRevolveFeatureData2` members | B + `SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.IRevolveFeatureData2_members.html` | 2024 |
| S34 | `IRevolveFeatureData2` member pages: `Type`, `Axis`, `GetAxisType`, `GetRevolutionAngle`, `ReverseDirection`, `ThinWallType`, `GetWallThickness` | B + `…IRevolveFeatureData2~<member>.html` | 2024 |
| S35 | `IFeature::GetTypeName2` feature type names | B + `SolidWorks.Interop.sldworks~SolidWorks.Interop.sldworks.IFeature~GetTypeName2.html` | 2024 |
| S36 | API example: Create Extruded Thin Feature | `https://help.solidworks.com/2025/English/api/sldworksapi/Create_Extruded_Thin_Feature_Example_VB.htm` | 2025 |
| S37 | `swEndConditions_e` | C + `SOLIDWORKS.Interop.swconst~SOLIDWORKS.Interop.swconst.swEndConditions_e.html` | 2024 |
| S38 | `swStartConditions_e` | C + `…swconst.swStartConditions_e.html` | 2024 |
| S39 | `swThinWallType_e` | C + `…swconst.swThinWallType_e.html` | 2024 |
| S40 | `swRevolveType_e` | C + `…swconst.swRevolveType_e.html` | 2024 |
| N1 | "SOLIDWORKS: Thin Features" (B. Leasure, 2019-09-01): open contours, "Extrude-Thin1" | `https://www.cati.com/?p=127618` | not Dassault |
| N2 | User forum: FeatureRevolve2 thin with an open sketch (2013-06-14): the auto-close prompt | `https://3dswym.3dexperience.3ds.com/question/solidworks-user-forum/featurerevolve2-thin-with-an-open-sketch_YTG81OcNRmOqkpGa_s0r_A` | user post, not documentation |

## Conventions used below

- The sketch lies in a plane with unit normal **n**. Points in the sketch are (u, v);
  height above the sketch plane is z, measured along **n**.
- A *region* is an area bounded by closed sketch loops (an outer loop with any holes).
  A *profile* is the set of regions or open chains the feature uses.
- Volumes are in mm³ and lengths in mm. tan 5° = 0.0874887.

## Extrude: the feature

- **Observed (S1, S6).** One PropertyManager serves three tools: boss/base extrude, cut
  extrude and surface extrude. Each can be solid or thin.
- **Observed (S35).** The API names the feature types `Extrusion`, `Boss`, `BossThin`,
  `Cut`, `CutThin` and `BaseBody` (all read through `IExtrudeFeatureData2`).
- **Observed (N1).** An extrude made from an open contour appears in the tree as
  "Extrude-Thin1".
- **Decision.** Default names follow the existing `name_prefix`: "Boss-Extrude*n*",
  "Cut-Extrude*n*". A thin boss is "Extrude-Thin*n*" and a thin cut "Cut-Extrude-Thin*n*".
  The cut thin name is not sourced; it follows the same pattern.
- **Observed (S15).** Allowed contours:
  - boss extrude: multiple disjoint closed contours only;
  - thin boss extrude: a single open contour, or multiple disjoint closed contours;
  - cut extrude: a single open contour, or multiple disjoint closed contours;
  - thin cut extrude: the same as thin boss.
- **Observed (S1).** An open contour needs Thin Feature; a closed contour may use it.
  **Observed (S7).** A cut from an open profile is valid only with Blind or Through All.
- **Inferred (S15, S7).** So there are two cut cases with an open profile: a solid cut
  where the open chain splits the body and one side is removed, and a thin cut along the
  chain. SOLIDWORKS's Check Sketch dialog lists both.

## Extrude: From (start condition)

The start condition sets where the extrusion begins. Blind depths are measured from it.

- **Sketch Plane** (default). **Observed (S1).** Starts at the sketch plane.
- **Surface/Face/Plane.** **Observed (S1, S2).**
  - The extrude starts at the chosen surface, face or plane.
  - A planar entity need not be parallel to the sketch plane.
  - A nonplanar entity must contain the whole sketch within its boundary, and the
    extrude's start then follows that entity's shape.
  - The 2025 page also says, in another sentence, that the surface or face must be planar.
    The two statements conflict. **Unknown** which one is current.
  - **Inferred.** Each sketch point is carried along the extrude direction to where it
    meets the start entity. The start face of the result is the part of that entity
    inside the projected profile. With a Blind end, each point then travels the depth
    from its own start point, so the end face is a translated copy of the start face. S14
    says this outright for surface extrudes from a face (the cap is the face translated by
    the depth). For solid extrudes it is our inference.
- **Vertex.** **Observed (S1).** Starts from a chosen vertex. **Inferred (S4)**, by analogy
  with Up To Vertex: the start is the plane parallel to the sketch plane through the
  vertex.
- **Offset.** **Observed (S1).** Starts on a plane offset from the sketch plane by an
  entered distance. **Observed (S27).** A flag flips the offset to the other side.
  - **Inferred.** The offset is measured along **n**, positive in the default direction of
    the feature. **Unknown** whether that default is **n** for cuts too, or the cut's own
    default direction (−**n**).
- **Decision.** AnvilCAD adds `from: StartCondition` with `SketchPlane` (default) and
  `Offset { distance, flip }` first. Face/vertex starts come later.

## Extrude: Direction 1 end conditions

Each condition below says where the extrude ends. "Default direction" means the direction
before Reverse Direction is applied.

### Default direction and Reverse Direction

- **Observed (S27, S28, S30).** A boss goes along the sketch normal by default. A cut goes
  the opposite way. The sketch normal is the normal of the face or plane the sketch was
  placed on. Reverse Direction (`Dir`) flips Direction 1.
- **Observed (S27, S28).** In a two-ended extrude, Direction 2 is always opposite
  Direction 1.
- **Implemented.** AnvilCAD's `rebuild.rs` already does this: +**n** for a boss and −**n**
  for a cut, with `reverse` flipping it.

### Blind

- **Observed (S1, S3, S4).** The extrude ends a set depth from the sketch plane.
- **Inferred.** The depth is measured along **n** when the direction is the sketch normal.
  The end face is the profile translated by the depth.
- **Decision.** The depth must be positive. Zero or negative is a rebuild error.

### Through All

- **Observed (S1, S3, S4).** The extrude goes from the sketch plane through all existing
  geometry.
- **Inferred.** For a cut, the result must be the same as any depth that passes the far
  side of every body.
- **Unknown.** Where a Through All *boss* ends. The sources only say "through all existing
  geometry". The natural reading is that it ends where the last body ends: on the plane
  parallel to the start plane through the farthest point of the existing bodies, measured
  along the direction.
  - **Decision.** AnvilCAD ends a Through All boss on that plane, so a boss never sticks
    out past the existing geometry. Today's code goes about one body diagonal past it.
    That is right for cuts but adds material for bosses.
  - **Decision.** With no existing body, Through All is a rebuild error ("there is no
    geometry to extrude through"). Today's code uses a fixed 100 mm. The sources do not
    say what SOLIDWORKS does with no body.

### Through All - Both

- **Observed (S1, S3, S4).** Through all existing geometry in both directions from the
  sketch plane. **Observed (S37).** It has its own value, `swEndCondThroughAllBoth` (9).
- **Observed (S7).** The task page calls it "Through All - Both Directions".

### Up To Next

- **Observed (S1, S3, S4, S5).** The extrude goes to the next surface that cuts across
  the *whole* profile. That surface must be on the same part.
- **Inferred.** Every ray from the profile along the direction is traced, and the end is
  the nearest face of the existing body that every ray hits. A face that only some of the
  profile reaches does not stop it. The end face then follows that face's shape, as for
  Up To Surface.
- **Unknown.** What SOLIDWORKS does when no surface spans the whole profile (an error is
  likely). **Decision.** AnvilCAD reports a rebuild error ("no face covers the whole
  profile in this direction").
- **Observed (S37).** The current value is `swEndCondUpToNext` (11). The older
  `swEndCondThroughNext` (2) still exists. **Unknown** whether they differ.

### Up To Vertex

- **Observed (S3, S4, S5).** The extrude ends on the plane parallel to the sketch plane
  through the chosen vertex. Sketch points are valid vertices (S3, S5).
- **Inferred.** The depth is the signed distance from the sketch plane to the vertex,
  (P − O)·**n**. A vertex behind the sketch plane gives a negative depth.
  - **Decision.** A zero or negative distance in the chosen direction is a rebuild error.
- **Observed (S37).** The value `swEndCondUpToVertex` (3) is marked "do not use", replaced
  by `swEndCondUpToSelection` (10).

### Up To Surface

- **Observed (S1, S2).** The extrude ends at a chosen face or plane. Double-clicking a
  surface switches the end condition to Up To Surface with that surface. If the sketch
  reaches outside the chosen face or surface body, SOLIDWORKS can extend *one analytic
  face* to end the extrusion.
- **Inferred.** Each point of the profile travels along the direction until it meets the
  surface. The end face is the part of that surface (or its extension) inside the swept
  profile. So:
  - an **inclined plane** gives a slanted end face, and the side faces of the prism stop on
    it;
  - a **nonplanar face** gives an end face shaped like that face;
  - a profile point whose ray misses the face and its extension is an error.
- **Inferred.** "Analytic face" means plane, cylinder, cone, sphere or torus. Extension is
  then the same underlying surface, unbounded. A freeform (spline) face is not extended.
- **Decision.** AnvilCAD supports planes first (any orientation) and then cylinders. Every
  ray must meet the surface on the chosen side of the start; if one does not, the rebuild
  fails. Where a ray meets the surface more than once, it stops at the first hit.
- **Observed (S37).** The value `swEndCondUpToSurface` (4) is marked "do not use",
  replaced by `swEndCondUpToSelection` (10). **Observed (S27).** `FeatureExtrusion3`
  accepts faces, surfaces and planes for start and end references.

### Offset From Surface

- **Observed (S1, S3, S4).** The extrude ends a set distance from a chosen face or plane.
  **Observed (S26).** The offset distance is stored as the depth (`SetDepth` holds it when
  the end condition is `swEndCondOffsetFromSurface`).
- **Observed (S27, S28, S29).** The reverse flag (`OffsetReverse1/2`): true offsets the
  end *away* from the sketch (past the surface); false offsets it *toward* the sketch
  (short of the surface).
  - **Inferred.** With Reverse offset cleared in the UI, the end stops short of the
    surface, toward the sketch. The UI label and the API flag are not tied together in
    the sources.
- **Observed (S1, S27).** *Translate surface*: when on, the end is a copy of the reference
  surface moved along the extrude direction; when off, the end is a true offset of the
  surface. **Observed (S8).** The example (captions only) pairs a 7 mm offset without
  Translate surface with a 27 mm offset with it, and says to use Translate surface when a
  larger offset is needed.
- **Inferred.** The geometry:
  - **True offset.** The end surface is every point at distance d from the reference
    surface, measured along that surface's normal. For a plane, that is a parallel plane
    d away. If that plane is inclined at angle α to the sketch plane (α is the angle
    between the plane's normal and the extrude direction), the end moves d / cos α along
    the extrude direction. A cylinder of radius R gives a cylinder of radius R ± d; an
    offset toward the axis fails when d ≥ R. That is why a large offset needs Translate.
  - **Translate.** The reference surface is moved d along the extrude direction (toward
    or away from the sketch). For a plane square to the extrude direction, the two give
    the same result.
- **Decision.** AnvilCAD supports planes (true offset and translate) first.

### Up To Body

- **Observed (S1, S3, S4).** The extrude goes up to a chosen solid or surface body. It is
  meant for assemblies, mold parts and multibody parts, including bodies with uneven
  surfaces.
- **Inferred.** Like Up To Surface, with each ray stopping at its first hit on the chosen
  body. Unlike Up To Next, the rays need not all stop on one face.
- **Observed (S37).** `swEndCondUpToBody` (7).
- **Decision.** Left out until AnvilCAD has multibody parts.

### Mid Plane

- **Observed (S1, S3, S4).** The extrude goes the same distance both ways from the sketch
  plane. One depth is entered.
- **Inferred.** The entered depth is the *total*, split D/2 each side. The help does not
  say this in words. Two things support it: the revolve mid-plane puts the sketch plane
  at the middle of the one angle (S18), and thin mid-plane uses half the thickness each
  side (S29, S32). AnvilCAD already uses D/2 each side.
- **Observed (S32).** Reverse Direction has no effect with mid-plane (revolve API: it
  applies only when the type is not mid-plane). **Inferred:** the same for extrudes,
  because the result is symmetric.
- **Observed (S37).** `swEndCondMidPlane` (6).
- **Unknown.** What `IExtrudeFeatureData2::GetDepth(true)` returns for a mid-plane extrude
  (the total or the half). `anvil-migrate` currently reads it as the total.

### Direction of extrusion (non-normal direction)

- **Observed (S1, S2).** A direction vector can be chosen so that the extrude does not run
  square to the sketch. **Observed (S9).** Valid direction references: linear edges,
  linear sketch entities, reference axes, cylindrical and conical faces (their axis),
  planar faces and reference planes (their normal), and pairs of vertices, sketch points or
  reference points. It works for 2D and 3D sketches. **Observed (S26).**
  `GetDirectionReference` returns one reference (edge or axis) or two (two points).
- **Inferred.** The result is an oblique prism: the profile is swept along the unit
  vector **d**. Its volume is the profile area × the height along **n** = A · D · (**d**·**n**),
  *if* the depth D is measured along **d**.
- **Unknown.** Whether Blind depth is measured along **d** or square to the sketch plane.
  **Decision.** AnvilCAD measures it along **d**, and says so in the editor. A **d** that is
  parallel to the sketch plane (**d**·**n** = 0) is a rebuild error.
- **Inferred.** Reverse Direction flips **d**. Up To Vertex with a non-normal direction is
  not covered by the sources. **Decision.** With a non-normal direction, AnvilCAD offers
  only Blind, Through All and Up To Surface.

## Extrude: Direction 2

- **Observed (S1, S2).** Direction 2 has the same options as Direction 1 and extrudes the
  other way from the sketch plane. **Observed (S27).** It is always opposite Direction 1.
- **Observed (S17).** Depth and draft can be set separately for each direction.
- **Inferred.** Direction 2 is not offered with Mid Plane (already two-sided) or with
  Through All - Both. Total length is D1 + D2.
- **Observed (S25, S27).** `BothDirections` (true for two-ended); `Sd` is the opposite
  (true for single-ended).
- **Decision.** AnvilCAD stores Direction 2 as an optional second end (`end2:
  Option<EndSpec>`), each `EndSpec` with its own condition, depth and draft.

## Extrude: Draft

- **Observed (S1, S2).** Draft On/Off adds draft to the extruded faces at a set angle. A
  *Draft outward* check box changes the side. The pictures show no draft, 10° inward and
  10° outward.
- **Observed (S17).** Draft outward changes the slope of the draft "from decreasing to
  increasing": inward draft makes the cross-section shrink with distance from the sketch;
  outward draft makes it grow.
- **Observed (S27, S28, S29).** In the API the draft flag is the other way round:
  `Ddir1/2` true means *inward*, false means outward. The angle is in radians (S26).
- **Inferred.** The geometry. The draft angle θ is measured between each side face and
  the extrude direction. At height z (0 ≤ z ≤ D) above the start, each profile edge moves
  z · tan θ along its in-plane normal: toward the inside of the region for inward, away
  from it for outward.
  - Lines become planar side faces tilted by θ. Neighbouring faces meet in sharp edges, so
    a polygon stays a polygon with the same number of sides (mitred offset).
  - A circle becomes a cone.
  - For a hole loop inside a region, "inside the region" is *away* from the hole. Inward
    draft makes the hole grow with z, so the solid between them shrinks. This is our
    reading of "the cross-section shrinks".
- **Inferred.** For a cut the same rule applies to the cutting tool. Inward draft makes the
  pocket narrow as it goes deeper, which is the usual draft for a moulded pocket.
- **Inferred.** Where an inward draft shrinks a loop to nothing before the end (for a
  rectangle w × h, when D · tan θ ≥ min(w, h) / 2), the feature fails.
  **Decision.** AnvilCAD reports a rebuild error.
- **Unknown.** Draft with Mid Plane. The API has one draft per direction, and mid-plane
  has one depth. **Decision.** AnvilCAD tapers both halves from the sketch plane with the
  same angle and side. The sketch section is the largest section for inward draft and the
  smallest for outward.
- **Unknown.** Whether draft is offered with Through All, Up To Surface or a non-normal
  direction. **Decision.** AnvilCAD allows draft only with Blind, Mid Plane and Up To
  Vertex at first, where the end is a plane parallel to the sketch.
- **Unknown.** Whether draft applies to thin features, and how.

## Extrude: Thin Feature

- **Observed (S1, S2).** Thin Feature sets a wall thickness (separate from the depth). A
  thin base can become the start of a sheet metal part.
- **Observed (S1, S2).** The types:
  - **One-Direction:** the wall goes one way from the sketch, "outward".
  - **Mid-Plane:** the wall goes equally both ways from the sketch.
  - **Two-Direction:** separate Direction 1 and Direction 2 thicknesses.
- **Observed (S29, S30).** In `FeatureExtrusionThin2` and `FeatureCutThin2`, Mid-Plane uses
  Thk1/2 on each side, so the entered thickness is the total. `RevThinDir`: 0 one
  direction, 1 one direction reversed, 2 mid-plane, 3 two direction. Thk2 is used only
  for two direction. The same numbers are `swThinWallType_e` (S39) and
  `IExtrudeFeatureData2::ThinWallType` (S26).
- **Observed (S22, S23).** In the tutorial, a closed circle (offset 2 mm inside a round
  face's edge) is extruded thin, One-Direction, 3 mm thick. Reverse Direction in Thin
  Feature is clicked "to extrude the wall to the inside".
  - **Inferred.** For a *closed* contour, One-Direction puts the wall outside the region
    by default, and reverse puts it inside. Mid-Plane centres the wall on the sketch line.
    Two-Direction puts Direction 1 thickness outside and Direction 2 thickness inside. The
    two-direction side assignment comes from the revolve page (S18), which says Direction 1
    adds the wall outward and Direction 2 inward; S1 does not say it for extrudes.
- **Unknown.** Which side is "outward" for an *open* contour. An open chain has no inside.
  **Decision.** AnvilCAD's Direction 1 side for an open chain is the left side when walking
  the chain from its first point to its last, in the sketch's own (u, v) axes. The preview
  shows the side, and Reverse flips it.
- **Inferred.** The wall at each point lies along the in-plane normal of the sketch curve.
  At a corner without auto-fillet, the wall's two faces are mitred offsets of the chain:
  each face is an offset of the sketch by the wall thickness on its side, with lines
  extended to meet. The open ends are cut square to the sketch curve.
  **Decision.** AnvilCAD uses mitred (sharp) corners on both faces of the wall.
- **Auto-fillet corners.** **Observed (S1, S2).** Open sketches only. It rounds every
  place where lines meet at an angle. *Fillet Radius* is the *inside* radius of the round.
  **Observed (S26).** `SetAutoFillet(AutoFillet, Radius)`; `GetAutoFilletRadius` returns 0
  when off.
  - **Inferred.** The rounds on the two faces of the wall are concentric. The inside
    (concave) face has radius r and the outside face has radius r + t, so the wall keeps
    thickness t round the bend. The sketch line itself is then rounded at the corner too,
    with radius r + t if it is the outside face, r if it is the inside face, or r + t/2 for
    mid-plane.
- **Cap ends.** **Observed (S1, S2).** Caps the end of a thin extrude to make a hollow
  part, with a set *Cap Thickness*. It is offered only for the first extruded body in a
  model. **Observed (S2).** The cap thickness runs from the end of the extrude back toward
  the sketch plane. **Observed (S29).** `CapEnds` applies to base features only.
  - **Inferred.** Caps close both ends (the hollow body is closed), each cap lies inside
    the extrude's length, and caps apply only to closed contours. The help says "end" in
    one place and "ends" in another. **Unknown** whether one or both ends are capped.
    **Decision.** AnvilCAD caps both ends.
- **Observed (S24).** A thin extrude can use several selected open sketch lines as
  separate contours (each line gives a separate thin wall, "fins").

## Extrude: Flip side to cut

- **Observed (S1, S2).** For cuts only, Flip side to cut removes the material *outside*
  the profile instead of inside.
- **Inferred.** With a closed profile, the flipped cut is the body minus the complement of
  the prism: it keeps only the material inside the swept profile, within the cut's depth
  range. Outside that depth range the body is untouched.
- **Observed (S26).** For an open sketch, the API says the side cut is tied to the first
  edge returned by `ISketch::GetContourEdges`. **Unknown** what that means geometrically.
  **Decision.** For an open-profile cut, the chain splits the body. AnvilCAD removes the
  Direction 1 side as defined under Thin Feature (left of the chain), and Flip side swaps
  it.

## Extrude: Merge result, feature scope, selected contours, link to thickness

- **Merge result.** **Observed (S1, S2).** Boss only. It merges the new body into an
  existing body where possible; when off, it makes a separate solid body.
  **Observed (S26).** `Merge` is true by default.
  - **Decision.** AnvilCAD has one body list. Merge off is deferred until it has
    multibody parts.
- **Feature scope.** **Observed (S1, S12).** It chooses which bodies the feature affects:
  *All bodies*, or *Selected bodies* with *Auto-select* (the bodies the feature first
  touches) or a manual list. **Observed (S13).** A cut that splits bodies asks which
  bodies to keep (*Bodies to Keep*).
  - **Decision.** Deferred with multibody parts.
- **Selected contours.** **Observed (S1, S16).** Only part of the sketch is used: the
  chosen contours or regions, which may include model edges. Contours must be on the
  sketch's own face (S16).
  - **Decision.** AnvilCAD stores `contours: Option<Vec<RegionId>>`, where `None` means
    every region. A region is named by a point inside it, so the choice survives sketch
    edits.
- **Link to thickness.** **Observed (S1, S2, S26).** Sheet metal only. It ties the boss
  depth to the base sheet's thickness. **Decision.** Out of scope.
- **Normal cut.** **Observed (S1).** Sheet metal only. **Decision.** Out of scope.

## Revolve: the feature

- **Observed (S18, S20, S21).** Revolved boss/base, revolved cut and revolved surface share
  one PropertyManager. Profiles are turned about an axis. The feature can be solid, thin
  or a surface.
- **Observed (S35).** Type names: `Revolution`, `RevolutionThin`, `RevCut` (all read
  through `IRevolveFeatureData2`).
- **Decision.** Default names follow `name_prefix`: "Revolve*n*" and "Cut-Revolve*n*".
  A thin revolve is "Revolve-Thin*n*" (by analogy with N1; not sourced).
- **Observed (S15).** Allowed contours:
  - boss and cut revolve: multiple disjoint closed contours;
  - thin boss and thin cut revolve: a single open contour, or multiple disjoint closed
    contours.
- **Observed (S20).**
  - A solid revolve's sketch may hold several *intersecting* profiles; Selected Contours
    picks the regions.
  - A thin or surface revolve's sketch may hold several open or closed intersecting
    profiles.
  - The profile sketch must be 2D. The axis may be in a 3D sketch.

## Revolve: axis

- **Observed (S18, S19).** The axis can be a centerline, a line or an edge, depending on
  the kind of revolve. **Observed (S34).** The API's axis types are reference axes
  (`swSelDATUMAXES`), edges (`swSelEDGES`) and sketch segments (`swSelSKETCHSEGS`).
- **Observed (S20).** If the sketch has more than one centerline, the user chooses one.
  **Inferred.** With exactly one centerline, it is used without asking.
- **Observed (S20).** Profiles must not cross the centerline. For revolved surfaces and
  thin revolves, the sketch also must not *lie on* the centerline.
- **Inferred.** A solid profile may *touch* the axis along an edge: an edge on the axis
  gives a solid with no hole, such as a cylinder. A profile on both sides of the axis is
  an error, even if it only crosses at one point.
- **Inferred (S20).** A dimension from the centerline to a point is a radius on the near
  side and a diameter when placed across the centerline. That is a sketch matter, not a
  revolve one; see `docs/re/sketch-dimension.md`.
- **Decision.** AnvilCAD accepts as an axis any sketch line, including construction lines
  (centerlines). Next come straight model edges and reference axes. A curved edge is an
  error. The axis line is infinite: a profile beyond the end of the sketch line is still
  turned about the full line.

## Revolve: open profiles

- **Observed (N2).** For a non-thin revolve of an open sketch, SOLIDWORKS asks whether to
  close the sketch automatically, because a non-thin revolve needs a closed sketch.
  Answering no keeps the open sketch for a thin revolve.
- **Inferred.** The automatic close adds the segment that joins the open chain's two ends,
  which works when both ends lie on the axis.
- **Implemented.** AnvilCAD's `close_along_axis` already closes an open chain whose two
  loose ends lie on the axis. **Decision.** Keep doing that silently, without asking. An
  open chain whose ends are not both on the axis is a rebuild error for a solid revolve
  and valid for a thin revolve.

## Revolve: Direction 1 types

- **Observed (S18, S32).** The types are Blind, Up To Vertex, Up To Surface, Offset From
  Surface and Mid-Plane. They use the `swEndConditions_e` values: Blind 0, Up To Vertex 3,
  Up To Surface 4, Offset From Surface 5, Mid-Plane 6.
- **Blind.** **Observed (S18).** The revolve turns one way from the sketch through the
  Direction 1 angle. **Observed (S19).** The default angle is 360°.
  **Observed (S32).** Angles are in radians in the API.
- **Up To Vertex.** **Observed (S18).** The revolve goes from the sketch plane to a chosen
  vertex. **Inferred.** It stops at the half-plane through the axis that contains the
  vertex. The angle is measured about the axis in the revolve direction, from the sketch
  plane to that half-plane, between 0° and 360°.
- **Up To Surface.** **Observed (S18).** The revolve goes from the sketch plane to a
  chosen surface. **Inferred.** Each profile point moves on its circle about the axis and
  stops where it meets the surface. With a plane that contains the axis, this is the same
  as Up To Vertex.
- **Offset From Surface.** **Observed (S18).** The same, but stopped a set distance from
  the surface, with Reverse offset to go the other way. **Unknown.** Whether the distance
  is measured along the surface normal (a true offset) or as an angle. The API has a
  distance (`OffsetDistance1/2`), not an angle (S32). **Inferred.** The end is the true
  offset of the surface.
- **Mid-Plane.** **Observed (S18, S19).** The revolve goes both ways from the sketch
  plane, with the sketch plane in the middle of the one angle: ±A/2.
- **Reverse Direction.** **Observed (S18, S32).** It reverses the revolve direction. It
  has no effect with Mid-Plane (S32).
- **Observed (S19).** The angle is measured clockwise from the sketch. **Unknown** which
  end of the axis that is viewed from. **Decision.** AnvilCAD turns positive angles by the
  right-hand rule about the axis direction (from the sketch line's first point to its
  second). Reverse flips the sign. This is what `revolve` does today with signed angles.

## Revolve: Direction 2

- **Observed (S18).** Optional; once Direction 1 is set, Direction 2 defines the revolve
  the other way from the sketch plane, with the same options.
- **Observed (S19).** The 2010 help (when it still called it "Two-Direction") says the two
  angles together must not exceed 360°.
  - **Decision.** AnvilCAD treats a total over 360° as a rebuild error. A total of exactly
    360° gives a full revolve.
- **Observed (S32).** If both directions go up to the same entity, it is selected once and
  `BothDirectionUpToSameEntity` is set.
- **Observed (S40).** `IRevolveFeatureData2::Type` uses `swRevolveType_e`: one direction
  0, mid-plane 1, two direction 2, and the "360 degrees" forms 3, 4 and 5.

## Revolve: Thin Feature

- **Observed (S18, S19).**
  - **One-Direction:** the wall goes one way from the sketch; Reverse Direction (in the
    thin section) flips it.
  - **Mid-Plane:** the wall is centred on the sketch, half each side.
  - **Two-Direction:** Direction 1 thickness goes outward from the sketch and Direction 2
    thickness inward.
- **Observed (S32).** With `swThinWallMidPlane`, ThinThickness1/2 is used on each side.
  ThinThickness2 is used only for two-direction.
- **Unknown.** What "outward" means for an open revolve profile: away from the axis, or a
  side of the chain. **Decision.** The same rule as thin extrudes (the left of the chain
  in sketch (u, v) axes is Direction 1), so one rule serves both.
- **Inferred.** A thin revolve is the thin wall section (as for a thin extrude) turned
  about the axis. If the wall section crosses the axis, the rebuild fails, as for any
  profile.

## Revolve: Flip side to cut, merge, selected contours, feature scope

- **Observed (S18).** Flip side to cut is offered for revolved cuts, as for extruded cuts.
  **Inferred.** It keeps the material inside the swept profile and removes the rest
  within the swept angle.
- **Observed (S18).** Merge result (boss only), Selected Contours (pick regions; any mix
  gives single or multibody results) and Feature Scope behave as for extrudes.
  **Decision.** The same deferrals as for extrudes.

## Errors

A feature that cannot be made is a rebuild error on that feature, not a panic. Later
features still rebuild. AnvilCAD should report:

- a sketch with no closed region, for a solid extrude or revolve (with an open chain:
  "open profile: use a thin feature");
- a depth, thickness or cap thickness that is not positive; an angle outside (0°, 360°];
  a two-direction revolve over 360°;
- Through All with no existing body (Decision above);
- Up To Next with no face covering the whole profile; Up To Surface where some of the
  profile misses the surface; Up To Vertex with the vertex on or behind the start;
- a true offset that collapses the surface (for example a cylinder offset by its radius
  or more);
- an inward draft that shrinks a loop to nothing within the depth;
- a direction of extrusion parallel to the sketch plane;
- a revolve profile on both sides of the axis; a thin revolve sketch lying on the axis;
  an axis that is not straight;
- cap ends on an open profile or on a feature that is not the first body;
- auto-fillet with a radius that does not fit a segment (the round's tangent points
  overlap).

## Testable statements

Unless stated, the sketch is on the XY plane (normal +Z). The rectangle is 20 × 10,
corners (0, 0) and (20, 10), area 200. "Block" is a box 40 × 40 × 20, x and y from −10 to
30, z from 0 to 20.

1. A Blind boss of the rectangle, 10 deep, is the box z ∈ [0, 10]: volume 2000.
2. The same with Reverse is z ∈ [−10, 0]: volume 2000.
3. A Blind *cut* of the rectangle from a sketch on the block's top face (normal +Z, at
   z = 20), 5 deep, with no reverse, removes z ∈ [15, 20]: the block keeps
   32000 − 1000 = 31000. (A cut's default direction is opposite the sketch normal.)
4. Mid Plane 10 gives z ∈ [−5, 5]: volume 2000.
5. Direction 1 Blind 10 with Direction 2 Blind 5 gives z ∈ [−5, 10]: volume 3000.
6. Through All cut of the rectangle sketched on the block's top face removes the whole
   prism: the block keeps 32000 − 4000 = 28000. Through All - Both from a sketch at
   z = 10 gives the same.
7. The same Through All cut with Flip side to cut keeps only the prism: volume 4000.
8. A Through All *boss* of the rectangle sketched on a plane at z = −5 (normal +Z), which
   runs up through the block, ends at z = 20 (the block's top), not past it. The union has volume
   32000 + 200 × 5 = 33000. (Decision; see Through All.)
9. Draft, inward 5°, Blind 10: the section at height z is (20 − 2z tan 5°) ×
   (10 − 2z tan 5°). Volume = 200D − 30 tan 5° · D² + (4/3) tan² 5° · D³ = **1747.740**
   (D = 10).
10. The same with Draft outward: 200D + 30 tan 5° · D² + (4/3) tan² 5° · D³ =
    **2272.672**.
11. A circle of radius 10, Blind 10, 5° inward: a cone frustum with end radius
    10 − 10 tan 5° = 9.12511. Volume = (π·10/3)(10² + 10·9.12511 + 9.12511²) =
    **2874.754**. Outward: end radius 10.87489, volume **3424.462**.
12. Mid Plane 10 with 5° inward draft (Decision: both halves taper from the sketch):
    2 × (1000 − 30 tan 5° · 25 + (4/3) tan² 5° · 125) = **1871.318**. Outward:
    **2133.784**.
13. Inward draft on a 20 × 10 rectangle fails when D · tan θ ≥ 5. At 5° that is
    D ≥ 57.15 (10 / (2 tan 5°)); D = 57 rebuilds and D = 58 does not.
14. Start offset 5 then Blind 10 gives z ∈ [5, 15]: volume 2000, with no material at
    z < 5.
15. Up To Vertex with the vertex at (35, 35, 7) gives z ∈ [0, 7]: volume 1400. A vertex
    at z = −3 (behind the sketch, no reverse) is an error.
16. Up To Surface to the plane z = 10 + 0.5x gives a prism whose height at x is
    10 + 0.5x. Volume = 10 · ∫₀²⁰ (10 + 0.5x) dx = **3000**. The end face is a 22.361 × 10
    rectangle (20 / cos α, with cos α = 1 / √1.25).
17. Offset From Surface 2 to that same plane, toward the sketch (Reverse offset cleared),
    as a true offset: the end moves 2 / cos α = 2√1.25 = 2.23607 along Z. Volume =
    200 × (15 − 2.23607) = **2552.786**. With Translate surface the end moves exactly 2:
    volume 200 × 13 = **2600**.
18. Offset From Surface 5 to a plane parallel to the sketch at z = 30: Reverse offset
    cleared ends at z = 25 (volume 5000); Reverse offset set ends at z = 35 (volume 7000).
    Translate surface makes no difference here.
19. Up To Next from a sketch at z = 0, extruding +Z toward a block 40 × 40 × 5 at
    z ∈ [20, 25] that covers the whole profile, stops at z = 20: added volume 4000. If the
    block covers only half the profile (x from 10 to 30), the extrude does not stop there;
    with nothing else in the way, the rebuild fails.
20. Direction of extrusion along **d** = (sin 30°, 0, cos 30°), Blind 10 (Decision: depth
    along **d**): the volume is 200 × 10 × cos 30° = **1732.051**, and the top face is the
    rectangle moved by (5, 0, 8.66025).
21. Thin, closed rectangle, One-Direction (outward) 1 mm, Blind 10: wall area
    22 × 12 − 200 = 64, volume **640**. With Reverse (inward): 200 − 18 × 8 = 56, volume
    **560**. Mid-Plane 1 mm (half each side): 21 × 11 − 19 × 9 = 60, volume **600**
    (corrected during implementation: the figures are for 1 mm in all, not 2 mm). Two-Direction, 1 out
    and 2 in: 22 × 12 − 16 × 6 = 168, volume **1680**.
22. Thin, closed rectangle, inward 1 mm, Blind 10, Cap ends 1 mm: the outer box 2000
    minus the cavity 18 × 8 × 8 = 1152 gives **848**. The body is closed (no open faces).
23. Thin, open L chain (0, 10) → (0, 0) → (20, 0), Mid-Plane 2 mm, Blind 10, sharp
    corners: wall area 2 × (20 + 10) = 60, volume **600**.
24. The same L, One-Direction 2 mm with the wall on the concave side (toward +x, +y),
    sharp corners: area 40 + 20 − 4 = 56, volume **560**. On the convex side: area
    40 + 20 + 4 = 64, volume **640**.
25. The same L, wall 2 mm on the concave side, Auto-fillet radius 1: inside radius 1,
    outside radius 3. Area = 2 × (30 − 2 × 3) + (π/4)(3² − 1²) = 48 + 2π, volume
    **542.832**. On the convex side: 2 × (30 − 2 × 1) + 2π, volume **622.832**.
26. Revolve 360° of the rectangle x ∈ [10, 20], y ∈ [0, 20] about the Y axis (a
    centerline along x = 0): a tube with volume π(20² − 10²) · 20 = 6000π = **18849.556**.
27. The same at 90°: 1500π = **4712.389**. Mid-Plane 90°: the same volume, from −45° to
    +45° about the sketch plane. Direction 1 at 90° plus Direction 2 at 45° gives 135°:
    **7068.583**. Direction 1 at 270° plus Direction 2 at 120° (390° total) is an error.
28. Revolve of x ∈ [0, 10], y ∈ [0, 20] (one edge on the axis) gives a solid cylinder:
    2000π = **6283.185**, with no hole along the axis.
29. Revolve of a rectangle x ∈ [−5, 10] (crossing the axis) is an error.
30. An open chain (0, 0) → (10, 0) → (10, 20) → (0, 20), revolved solid about the Y axis,
    is closed along the axis: a cylinder of volume 2000π.
31. Thin revolve of the line (10, 0) → (10, 20), Mid-Plane 2 mm, 360°: a tube between
    radii 9 and 11, volume π(11² − 9²) · 20 = 800π = **2513.274**.
32. Revolve Up To Vertex, with the vertex 60° round the axis from the sketch plane (in the
    revolve direction), of the rectangle in statement 26: 1000π = **3141.593**.
33. A thin feature with an open contour is required: a solid boss of an open chain fails
    with the "open profile" error. A solid *cut* of an open chain right across the block
    (Blind or Through All) splits it, and one side is removed.
34. The default tree names, in order, in a new part: "Boss-Extrude1", "Cut-Extrude1",
    "Revolve1", "Cut-Revolve1"; a thin boss is "Extrude-Thin1".

## API names

For `anvil-migrate`. Names and values only. Lengths are in metres and angles in radians
in the API.

- **Feature type names** (`IFeature::GetTypeName2`, S35): `Extrusion`, `Boss`, `BossThin`,
  `Cut`, `CutThin`, `BaseBody` → `IExtrudeFeatureData2`; `Revolution`,
  `RevolutionThin`, `RevCut` → `IRevolveFeatureData2`. Surface forms: `ExtruRefSurface`
  (`ISurfExtrudeFeatureData`), `RevolvRefSurf` (`ISurfRevolveFeatureData`).
- **`swEndConditions_e`** (S37): `swEndCondBlind` 0, `swEndCondThroughAll` 1,
  `swEndCondThroughNext` 2, `swEndCondUpToVertex` 3 (superseded),
  `swEndCondUpToSurface` 4 (superseded), `swEndCondOffsetFromSurface` 5,
  `swEndCondMidPlane` 6, `swEndCondUpToBody` 7, `swEndCondThroughAllBoth` 9,
  `swEndCondUpToSelection` 10, `swEndCondUpToNext` 11.
- **`swStartConditions_e`** (S38): `swStartSketchPlane` 0, `swStartSurface` 1,
  `swStartVertex` 2, `swStartOffset` 3.
- **`swThinWallType_e`** (S39): `swThinWallOneDirection` 0, `swThinWallOppDirection` 1,
  `swThinWallMidPlane` 2, `swThinWallTwoDirection` 3.
- **`swRevolveType_e`** (S40): `swRevolveTypeOneDirection` 0, `swRevolveTypeMidPlane` 1,
  `swRevolveTypeTwoDirection` 2, `swRevolveTypeOneDirection360Degrees` 3,
  `swRevolveTypeMidPlane360Degrees` 4, `swRevolveTypeTwoDirection360Degrees` 5.
- **`IExtrudeFeatureData2`** (S25, S26).
  - Properties: `BothDirections`, `ReverseDirection`, `FlipSideToCut`, `Merge` (default
    true), `FromType`, `FromOffsetDistance`, `FromOffsetReverse`, `ThinWallType` (0–3 as
    above), `CapEnds`, `CapThickness`, `LinkToThickness`, `NormalCut`,
    `OptimizeGeometry`, `Contours`, `FeatureScope`, `FeatureScopeBodies`, `AutoSelect`,
    `AssemblyFeatureScope`, `AutoSelectComponents`, `PropagateFeatureToParts`.
  - Per-direction methods (argument `Forward`: true for Direction 1, false for
    Direction 2): `GetEndCondition`/`SetEndCondition`, `GetDepth`/`SetDepth` (also the
    Offset From Surface distance), `GetEndConditionReference`/`SetEndConditionReference`,
    `GetDraftWhileExtruding`/`SetDraftWhileExtruding`, `GetDraftAngle`/`SetDraftAngle`,
    `GetDraftOutward`/`SetDraftOutward` (true = outward), `GetReverseOffset`/
    `SetReverseOffset`, `GetTranslateSurface`/`SetTranslateSurface`,
    `GetWallThickness`/`SetWallThickness`.
  - Other methods: `GetFromEntity`/`SetFromEntity`, `GetDirectionReference`/
    `SetDirectionReference` (one or two reference entities), `GetAutoFilletCorners`,
    `GetAutoFilletRadius`, `SetAutoFillet`, `IsThinFeature`, `IsBossFeature`,
    `GetContoursCount`, `AccessSelections`/`ReleaseSelectionAccess`. Obsolete: `GetFace`,
    `GetVertex`, `IsBaseExtrude`.
  - The `FromType` enumeration is named `swExtrudeFromType_e` on S26 and the offset value
    `swExtrudeFrom_Offset`; that enumeration page was not found. **Inferred.** It follows
    `swStartConditions_e` (offset = 3).
- **`IFeatureManager::FeatureExtrusion3`** (S27), in order: `Sd`, `Flip`, `Dir`, `T1`,
  `T2`, `D1`, `D2`, `Dchk1`, `Dchk2`, `Ddir1`, `Ddir2` (true = **inward**), `Dang1`,
  `Dang2`, `OffsetReverse1`, `OffsetReverse2` (true = away from the sketch),
  `TranslateSurface1`, `TranslateSurface2`, `Merge`, `UseFeatScope`, `UseAutoSelect`,
  `T0`, `StartOffset`, `FlipStartOffset`. Selection marks: direction edge 16, start
  reference 32, end reference 1.
- **`IFeatureManager::FeatureCut4`** (S28): as `FeatureExtrusion3`, but `NormalCut` in
  place of `Merge`, then `AssemblyFeatureScope`, `AutoSelectComponents`,
  `PropagateFeatureToParts` after `UseAutoSelect`, and `OptimizeGeometry` last. `Flip` is
  flip side to cut.
- **`IFeatureManager::FeatureExtrusionThin2`** (S29) and **`FeatureCutThin2`** (S30): the
  extrusion arguments, then `Thk1`, `Thk2`, `EndThk` (cap), `RevThinDir` (0–3),
  `CapEnds` (0/1), `AddBends`, `BendRad` (auto-fillet), `UseFeatScope`,
  `UseAutoSelect`, `T0`, `StartOffset`, `FlipStartOffset`.
- **`IRevolveFeatureData2`** (S33, S34): `Type` (`swRevolveType_e`), `Axis`,
  `GetAxisType`, `ReverseDirection`, `GetRevolutionAngle(Forward)`/`SetRevolutionAngle`,
  `ThinWallType`, `GetWallThickness(Forward)`/`SetWallThickness`, `IsThinFeature`,
  `IsBossFeature`, `Merge`, `Contours`, `FeatureScope`, `FeatureScopeBodies`,
  `AutoSelect`. The member list has no getter for revolve end-condition references or
  offsets. **Unknown** how an Up To / Offset revolve is read back (probably through
  `AccessSelections`).
- **`IFeatureManager::FeatureRevolve2`** (S32): `SingleDir`, `IsSolid`, `IsThin`, `IsCut`,
  `ReverseDir`, `BothDirectionUpToSameEntity`, `Dir1Type`, `Dir2Type`
  (`swEndConditions_e`), `Dir1Angle`, `Dir2Angle`, `OffsetReverse1`, `OffsetReverse2`,
  `OffsetDistance1`, `OffsetDistance2`, `ThinType` (`swThinWallType_e`),
  `ThinThickness1`, `ThinThickness2`, `Merge`, `UseFeatScope`, `UseAutoSelect`. Selection
  marks: sketch 0, axis 4 or 16, up-to entity 32.

## Recommended scope

Given the current code (`EndCondition` with four values, one `depth`, `reverse`; a
revolve with one signed `angle`; `close_along_axis`), implement in this order:

1. **Fix Through All for bosses** (end at the far extent of existing geometry; error with
   no body). Small, and it removes a visible difference from SOLIDWORKS (statement 8).
2. **Direction 2 for extrudes**: `end2: Option<EndSpec>` with its own condition and depth.
   Map `BothDirections` with differing depths in `anvil-migrate`, which today only
   accepts equal depths (as Mid Plane).
3. **Draft** on Blind / Mid Plane / Direction 2 (angle + outward flag per direction).
   Polygons and circles first; exact kernel faces are planes and cones.
4. **Up To Vertex, Up To Surface (planes), Offset From Surface (planes, true offset and
   translate)**. These need face and vertex references that survive rebuilds, as the
   Hole and Fillet features already use.
5. **Revolve directions**: Mid-Plane and Direction 2 (Blind), then Up To Vertex. Map
   `swRevolveType_e` 1, 2, 4, 5 in `anvil-migrate`, which today rejects them.
6. **Thin features** for extrude and revolve (one/mid/two direction, reverse), sharp
   corners first, then auto-fillet, then cap ends.
7. **Flip side to cut** and **start offset** (`From: Offset`).
8. **Up To Next**, Up To Surface on cylinders, **direction of extrusion**.

Leave out for now: Up To Body, Merge result off, Feature Scope, Bodies to Keep (all need
multibody parts); Link to Thickness and Normal Cut (sheet metal); From: Surface/Face/Plane
on nonplanar faces; surface extrudes and revolves; Instant3D handles.

## Unknown or not covered

1. Whether a nonplanar start face is still allowed (S1 contradicts itself).
2. Where a Through All boss ends, and what Through All does with no body.
3. What Up To Next does when no face covers the whole profile; whether
   `swEndCondThroughNext` (2) and `swEndCondUpToNext` (11) differ.
4. Whether depth along a non-normal direction of extrusion is measured along the vector
   or square to the sketch.
5. Draft with Mid Plane, Through All, Up To Surface, thin features, and on hole loops (only
   pictures show draft).
6. Which side is Direction 1 for a thin feature on an open contour, and what "outward"
   means for a thin revolve.
7. Whether cap ends caps one or both ends.
8. The exact shape of auto-fillet corners (where the round sits relative to the sketch
   line).
9. Which side an open-profile cut removes by default.
10. How Offset From Surface is measured for revolves (distance or angle), and how revolve
    end references are read through the API.
11. Which way "clockwise" turns a revolve (viewed from which end of the axis).
12. What `GetDepth` returns for a mid-plane extrude (total or half).
13. How the Up To Surface face extension works beyond "one analytic face".
14. The default names of thin cut extrudes and thin revolves.
