# Mirror and pattern features

What AnvilCAD's Mirror, Linear Pattern and Circular Pattern features should do, and what
its later pattern types (sketch-driven, table-driven, curve-driven, fill and variable)
would involve. A mirror or pattern copies one or more *seed* features to new places: by
reflecting them in a plane, by translating them along one or two directions, or by
rotating them about an axis. SOLIDWORKS users know these as its mirror and pattern
features, so AnvilCAD follows their published behaviour where that makes sense. This
specification was written for the existing `LinearPatternFeature` and
`CircularPatternFeature` in `crates/anvil-model/src/document.rs` and their rebuild in
`crates/anvil-model/src/rebuild.rs`, and for a Mirror feature that does not exist yet.

## How this was studied

- **Inputs.** Only published information was used:
  - Dassault's public online help for SOLIDWORKS 2025, with 2015, 2016, 2017, 2019 and
    2022 pages where they add detail or show how an option changed;
  - Dassault's public API help for SOLIDWORKS 2024 (one 2026 page), including its
    published example macros, read for the facts they state (feature names, index
    ranges), not copied.

  No SOLIDWORKS software, installer, library or file was downloaded, installed, run or
  analysed. No SOLIDWORKS licence was accepted on the machine used. No `.sldprt` file was
  opened; how these features are stored in a part file was not studied.
- **Retrieval.** The help site renders its articles with JavaScript, and a plain request
  is refused. Each article was fetched over HTTPS with a browser user agent, and its text
  read from the page's embedded `__NEXT_DATA__` JSON
  (`props.pageProps.helpContentData.helpText`), with HTML stripped by a script run with
  `python3 -I`. A missing page still returns HTTP 200, so each page was checked for the
  "page cannot be found" text. Link targets were listed from the same JSON to find
  related pages. The text was paraphrased; none of it is copied into this repository,
  apart from short option names such as "Pattern seed only".
- **REA.** REA (`rea-agents` 6.5.0, with the `third_party/rea` submodule at `3fc0075`) was
  set up and its research method followed:
  - each finding below is marked **Observed** (stated in a cited source), **Inferred**
    (our reasoning from the sources), **Decision** (AnvilCAD's own choice where the
    sources are silent) or **Unknown**;
  - every observation cites its source.

  REA's analysis tools were **not** run. They inspect artifacts (binaries, packages,
  captures, files), and the only lawful inputs for this behaviour are prose documents.
  Pointing REA at SOLIDWORKS software is ruled out by
  [INTEROPERABILITY.md](../../INTEROPERABILITY.md).
- **Geometry.** Reflection, translation and rotation formulas are standard linear algebra
  and are written out here in AnvilCAD's own terms. The worked volumes in "Testable
  statements" were computed by hand from those formulas; none comes from SOLIDWORKS.

## Sources

Base A is `https://help.solidworks.com/2025/English/SolidWorks/sldworks/`. Base B is
`https://help.solidworks.com/2024/English/api/sldworksapi/`, with API member pages named
`SolidWorks.Interop.sldworks~SolidWorks.Interop.sldworks.<Interface>~<Member>.html` (B +
`<Interface>~<Member>` below) and member lists `..._members.html`. Base C is
`https://help.solidworks.com/2024/English/api/swconst/SOLIDWORKS.Interop.swconst~SOLIDWORKS.Interop.swconst.<enum>.html`.
All were read on 2026-10-18.

| # | Topic | URL | Version |
|---|---|---|---|
| S1 | Mirror feature overview (seed edits update the copy) | A + `c_Mirror_Feature_Overview.htm` | 2025 |
| S2 | Mirror Feature PropertyManager | A + `HIDD_DVE_FEAT_MIRROR.htm` | 2025 |
| S3 | Mirroring features (task; merge solids, knit surfaces) | A + `t_mirroring_features_general.htm` | 2025 |
| S4 | Mirror and opposite-hand disambiguation | A + `c_mirror_opposite_hand_disambiguation.htm` | 2025 |
| S5 | Opposite-hand versions of parts (Mirror Part) | A + `t_Mirror_Part.htm` | 2025 |
| S6 | Feature scope in multibody parts | A + `r_feature_scope_multibody_parts.htm` | 2025 |
| S7 | What's New: mirroring about two planes | `https://help.solidworks.com/2022/English/WhatsNew/c_wn2022_parts_mirroring_about_two_planes.htm` | 2022 |
| S8 | Linear patterns and the Linear Pattern PropertyManager | A + `t_Linear_Patterns_Overview.htm` | 2025 |
| S9 | Linear Pattern PropertyManager (older page) | `https://help.solidworks.com/2017/English/SolidWorks/sldworks/HIDD_LPATTERN.htm` | 2017 |
| S10 | What's New: controlling linear patterns with reference geometry | `https://help.solidworks.com/2015/English/WhatsNew/t_controlling_linear_patterns_with_reference_geometry.htm` | 2015 |
| S11 | What's New: more inputs for linear pattern directions | `https://help.solidworks.com/2016/English/WhatsNew/c_allow_more_inputs_for_linear_patterns.htm` | 2016 |
| S12 | Circular Pattern PropertyManager | A + `HIDD_CPATTERN.htm` (2020, 2022 and 2024 pages read too; same text) | 2025 |
| S13 | Circular Pattern PropertyManager (older page, no Direction 2) | `https://help.solidworks.com/2017/English/SolidWorks/sldworks/HIDD_CPATTERN.htm` | 2017 |
| S14 | Circular patterns overview | A + `t_Circular_Patterns_Overview.htm` | 2025 |
| S15 | Bidirectional circular patterns (task with numbers) | A + `t_bidirectional_circular_pat.htm` | 2025 |
| S16 | What's New: circular pattern second direction | `https://help.solidworks.com/2019/English/WhatsNew/r_circular_pattern_enh.htm` | 2019 |
| S17 | Circular component pattern (equal spacing over a changed total angle) | `https://help.solidworks.com/2016/English/SolidWorks/sldworks/HIDD_DVE_COMPPAT_CIRCULAR.htm` | 2016 |
| S18 | Circular Component Pattern PropertyManager | A + `r_circular_component_pattern_pm.htm` | 2025 |
| S19 | Control and modify patterns (contents) | A + `c_Control_Modify_Patterns.htm` | 2025 |
| S20 | Geometry pattern | A + `c_Geometry_Pattern_Concept.htm` | 2025 |
| S21 | Seed feature | A + `t_Seed_Feature_Task.htm` | 2025 |
| S22 | Selecting instances to skip | A + `t_selecting_instances_to_skip_patterns.htm` | 2025 |
| S23 | Pattern deletion and restoring instances | A + `c_Pattern_Deletion_Overview.htm` | 2025 |
| S24 | Varying dimension patterns (instances to vary) | A + `c_vary_dim_patterns2.htm` | 2025 |
| S25 | Vary sketch | A + `t_Vary_Sketch.htm` | 2025 |
| S26 | No limit on number of instances | A + `c_remove_instance_limit.htm` | 2025 |
| S27 | System options for patterns | A + `c_system_options_for_patterns.htm` | 2025 |
| S28 | Patterns of patterns | A + `t_Patterns_of_Patterns.htm` | 2025 |
| S29 | Centroid of a seed feature | A + `c_Centroid_Overview.htm` | 2025 |
| S30 | Sketch driven patterns | A + `HIDD_PATTERN_BY_SKETCH.htm` | 2025 |
| S31 | Table driven patterns | A + `HIDD_PATTERN_TABLE2.htm` | 2025 |
| S32 | Saving and loading table driven patterns | A + `c_Saving_and_Loading_Table_Driven_Patterns.htm` | 2025 |
| S33 | Curve driven patterns and PropertyManager | A + `t_Curve_Driven_Pattern_Overview.htm` | 2025 |
| S34 | Fill Pattern PropertyManager | A + `HIDD_DVE_AREA_FILL_PATTERN.htm` | 2025 |
| S35 | Fill patterns overview | A + `c_Fill_Patterns_Overview.htm` | 2025 |
| S36 | Variable patterns | A + `c_variable_patterns.htm` | 2025 |
| S37 | Variable Pattern PropertyManager; creating a variable pattern; pattern tables | A + `r_variable_pattern_property_manager.htm`, `t_creating_variable_pattern.htm`, `c_pattern_tables.htm` | 2025 |
| S38 | Types of patterns | A + `c_Types_of_Patterns_Folder.htm` | 2025 |
| S39 | Circular sketch pattern (sketch tool, for comparison) | A + `HIDD_DVE_SKETCH_PATTERN_CIRCULAR.htm`, `t_Creating_Circular_Sketch_Patterns.htm` | 2025 |
| S40 | `IMirrorPatternFeatureData` members | B + `IMirrorPatternFeatureData_members` (2025 and 2026 lists read too; same members) | 2024 |
| S41 | `IMirrorPatternFeatureData` `GetMirrorPlaneType`, `Plane`, `PatternFeatureArray` | B + `IMirrorPatternFeatureData~<member>` | 2024 |
| S42 | `IMirrorSolidFeatureData` members, `Merge`, `Face` | B + `IMirrorSolidFeatureData_members`, `~Merge`, `~Face` | 2024 |
| S43 | `IFeatureManager::InsertMirrorFeature2` | B + `IFeatureManager~InsertMirrorFeature2` | 2024 |
| S44 | `ILinearPatternFeatureData` members | B + `ILinearPatternFeatureData_members` | 2024 |
| S45 | `ILinearPatternFeatureData` `D1Axis`, `GetD1AxisType`, `D1Spacing`, `D1TotalInstances`, `D1EndCondition`, `D1EndReference`, `D1EndRefOffset`, `D1EndUseSeedReference`, `D1EndUseSpacing`, `D1ReverseDirection`, `D2TotalInstances`, `D2PatternSeedOnly`, `IsDirection2Specified`, `PatternElement`, `GetTransform` | B + `ILinearPatternFeatureData~<member>` | 2024 |
| S46 | `ILinearPatternFeatureData::SkippedItemArray` | B + `ILinearPatternFeatureData~SkippedItemArray` | 2024 |
| S47 | `ICircularPatternFeatureData` members | B + `ICircularPatternFeatureData_members` | 2024 |
| S48 | `ICircularPatternFeatureData` `Spacing`, `EqualSpacing`, `Spacing2`, `Symmetric`, `TotalInstances`, `TotalInstances2`, `Direction2`, `ReverseDirection`, `Axis`, `GetAxisType`, `GetTransform` | B + `ICircularPatternFeatureData~<member>` | 2024 |
| S49 | `ICircularPatternFeatureData::SkippedItemArray` | B + `ICircularPatternFeatureData~SkippedItemArray` | 2024 |
| S50 | `IFeatureManager::FeatureLinearPattern5` | B + `IFeatureManager~FeatureLinearPattern5` | 2024 |
| S51 | `IFeatureManager::FeatureCircularPattern5` (equal spacing means total angle) | B + `IFeatureManager~FeatureCircularPattern5` | 2024 |
| S52 | `ISketchPatternFeatureData` members, `UseCentroid`, `ReferencePoint`, `GetReferencePointType`, `Sketch`, `GetTransform` | B + `ISketchPatternFeatureData_members` and `~<member>` | 2024 |
| S53 | `ITablePatternFeatureData`, `ICurveDrivenPatternFeatureData`, `IFillPatternFeatureData`, `IDimPatternFeatureData` members | B + `<Interface>_members` | 2024 |
| S54 | `IFeature::GetTypeName2` (type names and data objects) | B + `IFeature~GetTypeName2` | 2024 |
| S55 | `IFeatureManager::CreateDefinition` (pattern IDs) | B + `IFeatureManager~CreateDefinition` | 2024 |
| S56 | `swFeatureNameID_e`, `swPatternEndCondition_e`, `swPatternElementSelection_e`, `swFeatureScope_e` | C + `<enum>` | 2024 |
| S57 | API examples: Get Transform for Each Circular Pattern Instance; Create Bidirectional Circular Pattern Feature; Get Linear Pattern Feature Data; Get Mirror Pattern Feature Data; Get Mirror Solid Feature Data (VB.NET) | B + `Get_Transform_for_Each_Circular_Pattern_Instance_Example_VBNET.htm`, `Create_Bidirectional_Circular_Pattern_Feature_Example_VBNET.htm`, `Get_Linear_Pattern_Feature_Data_Example_VBNET.htm`, `Get_Mirror_Pattern_Feature_Data_Example_VBNET.htm`, `Get_Mirror_Solid_Feature_Data_Example_VBNET.htm` | 2024 |
| S58 | `ILocalCircularPatternFeatureData::EqualSpacing` (component pattern) | `https://help.solidworks.com/2026/English/api/sldworksapi/SolidWorks.Interop.sldworks~SolidWorks.Interop.sldworks.ILocalCircularPatternFeatureData~EqualSpacing.html` | 2026 |

## Ideas common to every mirror and pattern

- **Observed (S21).** The original feature of any pattern or mirror is its *seed*. The seed
  stays editable; the instances are not edited on their own (apart from "instances to
  vary", below).
- **Observed (S1).** When the seed changes, the mirrored copy changes with it. **Inferred.**
  The same holds for every pattern, because a pattern stores the seed's identity and its
  placement rules, not copies of its geometry. In AnvilCAD terms: the mirror or pattern
  rebuilds from the seed's tool body on every rebuild, so a changed seed dimension, sketch
  or end condition reaches every instance.
- **Observed (S2, S8, S12).** What can be copied: features ("Features to Pattern" /
  "Features to Mirror"), the faces that make up a feature ("Faces to Pattern", meant for
  imported models without features), whole solid or surface bodies ("Bodies to Pattern"),
  and for linear and circular patterns one reference plane or axis.
- **Observed (S8, S12, S30).** Faces to Pattern only works if the copies stay within the
  same face; a cut or step that splits the face stops it.
- **Observed (S28, S57).** A pattern or mirror can itself be a seed. S57's mirror example
  mirrors an extrude and a linear pattern together to make "Mirror1".
- **Observed (S20).** *Geometry pattern* copies the seed's faces and edges exactly
  without solving the copies again. Without it, each instance is solved again in its new
  place, so an end condition such as an offset from a surface gives each copy its own
  depth. Geometry pattern is not possible when the seed's faces merge with the rest of the
  part. It exists for every pattern type except the variable pattern.
  - **Inferred.** AnvilCAD's patterns today move the seed's *tool body* and combine it
    again. That is geometry-pattern behaviour for features whose tool does not depend on
    where it lies (blind, mid-plane, through-all on a convex body). Instances that should
    be solved again (up to next, up to surface) are not supported.
  - **Unknown.** Whether geometry pattern is on or off by default in the SOLIDWORKS
    dialog.
- **Observed (S2, S8, S12).** *Propagate visual properties* copies colours, textures and
  cosmetic thread data from the seed to the copies. **Decision.** AnvilCAD records the
  flag for `anvil-migrate` and copies the seed's appearance when appearances exist.
- **Observed (S12, S8).** *Full preview* shows each instance solved; *partial preview*
  shows the seed's geometry at each place. **Decision.** These are display choices only;
  AnvilCAD does not store them.
- **Observed (S6, S8, S12).** *Feature scope* (all bodies, selected bodies, auto-select)
  decides which bodies of a multibody part the copies cut or join. **Decision.** Not
  implemented while AnvilCAD has one body per part.
- **Observed (S26).** Linear, circular, curve-driven and fill patterns have no upper
  limit on instance count; the program asks for confirmation when the count is large.
  **Decision.** AnvilCAD rejects counts above 10 000 with an error, as a guard against a
  typing slip that would hang the rebuild.

## Mirror

### Inputs

- **Observed (S2, S3).** *Mirror Face/Plane*: one plane or planar face to reflect in.
  **Observed (S41).** The API reports whether it is a face (0) or a reference plane (1).
- **Observed (S2, S7).** *Secondary Mirror Face/Plane* (parts only, added in 2022): a
  second plane or planar face. The items are mirrored about both planes in one feature.
- **Observed (S2).** *Mirror seed only*: offered only when the secondary plane is
  perpendicular to the first.
- **Observed (S2, S3).** What to mirror: *Features to Mirror*, *Faces to Mirror* (parts
  only), or *Solid/Surface Bodies to Mirror* (parts only).
- **Observed (S2, S3).** Options: *Geometry Pattern* (features and faces only; required
  when mirroring features of one body onto another), *Merge solids* (bodies only),
  *Knit surfaces* (surface bodies only), *Propagate visual properties*, *Full preview* /
  *Partial preview*, and *Feature Scope*.

### Geometry

- **Inferred (S1, S2).** A mirror reflects in a plane through point `o` with unit normal
  `n`. A point `p` maps to

  `p' = p - 2 ((p - o) · n) n`.

  A direction `d` maps to `d - 2 (d · n) n`. The map is its own inverse and has
  determinant -1: it reverses handedness, so the copy of a right-handed shape is
  left-handed. Translations and rotations (patterns) keep handedness.
- **Inferred.** Reflection turns a solid's faces inside out. A mirrored tool body must have
  its face orientations (and loop directions) reversed to remain a valid solid. AnvilCAD's
  exact `Solid::transformed` (in `crates/anvil-brep/src/topo.rs`) accepts only rigid
  motions today, so it needs a reflecting counterpart.
- **Inferred (S1, S3).** A mirrored boss is joined to the body and a mirrored cut is
  removed from it, as the seed was. Where the copy overlaps the seed (a seed that crosses
  the mirror plane), the overlap is joined or removed once.
- **Observed (S2).** Two planes, *Mirror seed only* selected or cleared, changes the
  result (S2 illustrates both cases but describes neither in words).
  - **Inferred.** With seed `S`, reflections `M1` and `M2`:
    - *Mirror seed only* cleared: `S`, `M1(S)`, `M2(S)` and `M2(M1(S))`, four copies in
      all. For perpendicular planes, `M2∘M1` is a half turn about the planes' line of
      intersection.
    - *Mirror seed only* selected: `S`, `M1(S)` and `M2(S)`, three in all; the first
      mirror's copy is not mirrored again.

    This matches how *Pattern seed only* behaves in linear patterns (S8). It is our
    reading of S2's illustrations and option name, not a stated rule.
  - **Unknown.** What happens with two planes that are not perpendicular, and why
    *Mirror seed only* needs perpendicular planes. Reflections in two planes at angle `θ`
    compose to a rotation by `2θ`, so with non-perpendicular planes `M2(M1(S))` and
    `M1(M2(S))` are different places.
- **Observed (S3).** Bodies with *Merge solids* selected become one body with the
  original; cleared, the mirrored body is a separate body even when it touches the
  original (for example, mirrored about one of the original's own faces). **Observed
  (S42).** The API's `Merge` is true by default.
- **Observed (S3).** *Knit surfaces* joins a mirrored surface to the original when they
  meet edge to edge without gaps.
- **Observed (S4, S5).** Mirroring the whole part into a new, linked opposite-hand part
  ("Mirror Part") is a different command. It is out of scope here.

### Tree and naming

- **Observed (S57).** The first mirror is named "Mirror1". **Observed (S54, S43).** A
  mirror of features or faces has type name `MirrorPattern` (data `IMirrorPatternFeatureData`);
  a mirror of bodies has type name `MirrorSolid` (data `IMirrorSolidFeatureData`).
  **Unknown.** Whether a body mirror is also named "MirrorN" in the tree.
- **Decision.** AnvilCAD names its mirror "Mirror1", "Mirror2", and so on, and lists the
  seed features as its dependencies, like `LinearPatternFeature` does.

### Errors

- **Decision.** These are rebuild errors on the feature, not panics: no seed; a seed that
  failed or was deleted; a mirror face that is not planar; a mirror plane that no longer
  exists; *Mirror seed only* with planes that are not perpendicular (within 1e-9 rad);
  two parallel or coincident mirror planes.
- **Unknown.** What SOLIDWORKS does if the copy does not touch the body (a boss reflected
  into empty space). **Decision.** AnvilCAD keeps it as a separate lump of the same body
  and reports no error, matching how its patterns behave today.

## Linear pattern

### Direction 1

- **Observed (S8, S9, S11, S45).** *Pattern Direction* may be a linear edge, sketch line,
  axis, linear dimension, planar face or surface, conical face or surface, circular edge,
  or reference plane. Before 2016 only an edge, line, axis or dimension could be used
  (S11). **Observed (S45).** `GetD1AxisType` reports axis (0), edge (1), dimension (2) or
  sketch segment (3).
- **Inferred.** The direction vector each input gives:
  - linear edge, sketch line or axis: along the line;
  - linear dimension: along the line the dimension measures (S25 uses an 8 mm dimension
    as the direction for a pattern of cuts);
  - planar face, planar surface or reference plane: the plane's normal;
  - conical face or surface, circular edge: the cone's or circle's axis.
- **Observed (S8, S45, S50).** *Reverse Direction* flips the direction
  (`D1ReverseDirection`, `FlipDir1`).
- **Unknown.** Which sense SOLIDWORKS picks before *Reverse Direction* is applied, for each
  kind of input (edge orientation, dimension, face normal). S8 and S25 only say to reverse
  it "if necessary", after looking at the preview.
- **Decision.** AnvilCAD resolves the input to a unit vector and stores it with a `reverse`
  flag, so the meaning stays fixed when the file is reopened. For a dimension, the
  default sense is the one in which the dimension's value grows. For a face, it is the
  face's outward normal.

### Spacing and instances

- **Observed (S8, S45).** *Spacing and instances* sets the spacing `s` and the number of
  instances `n` separately. **Observed (S8, S50).** `n` *includes the seed*.
- **Inferred.** Instance `i` (1-based, `i = 1` is the seed) is the seed translated by
  `(i - 1) s d̂`, where `d̂` is the unit direction. With `n = 4` and `s = 20` the copies
  are at 0, 20, 40 and 60 mm.
- **Observed (S45).** Spacing is a distance (metres in the API).
- **Decision.** Spacing must be greater than zero and `n` at least 1; `n = 1` leaves only
  the seed. A negative typed spacing is rejected, not read as a reversed direction.

### Up to reference

- **Observed (S8, S10).** *Up to reference* works out the pattern from a reference
  geometry (vertex, edge, face or plane, S45) instead. *Offset distance* is the distance
  of the *last* instance from the reference; *Reverse offset direction* moves that offset
  to the reference's other side.
- **Observed (S8, S45).** The offset is measured from the seed's *Centroid*, or from a
  *Selected reference* on the seed (a *Seed Reference*).
- **Observed (S45).** Either the spacing or the count is given (`D1EndUseSpacing`), and
  the other follows. Added to the API in 2019 (`D1EndCondition`); in the dialog since
  2015 (S10).
- **Inferred.** Let `L` be the distance along `d̂` from the seed's reference point to the
  reference geometry, less the offset. Given the spacing `s`, the count is
  `n = floor(L / s) + 1`. Given the count `n ≥ 2`, the spacing is `s = L / (n - 1)`, and
  the last instance sits exactly at the offset.
- **Unknown.** Whether SOLIDWORKS rounds the count down or to nearest; what happens when
  `L` is negative or smaller than one spacing; and which centroid is meant for a seed
  with several sketch contours (S29 defines centroids for sketch- and table-driven
  patterns only).

### Direction 2

- **Observed (S8, S9).** *Direction 2* adds a second direction with its own direction,
  reverse, spacing and count (or up to reference). Its count also includes the seed.
- **Observed (S8, S9, S45).** *Pattern seed only*: when cleared, the whole Direction 1 row
  is copied along Direction 2; when selected, only the seed is copied along Direction 2.
- **Inferred.** With `n1` and `n2` instances and unit directions `d̂1` and `d̂2`, instance
  `(i, j)` (1-based, `(1, 1)` the seed) is the seed translated by
  `(i - 1) s1 d̂1 + (j - 1) s2 d̂2`.
  - *Pattern seed only* cleared: every `(i, j)`, a grid of `n1 × n2` places.
  - *Pattern seed only* selected: the row `(i, 1)` and the column `(1, j)`, which is
    `n1 + n2 - 1` places.
- **Inferred.** `d̂2` need not be perpendicular to `d̂1`; a slanted `d̂2` gives a
  parallelogram grid.
- **Decision.** AnvilCAD rejects a Direction 2 parallel to Direction 1 (within 1e-9 rad),
  which would stack instances on one line. **Unknown.** What SOLIDWORKS does in that case.
- **Observed (S8).** The 2025 dialog also offers *Symmetric* under Direction 2, described
  as a symmetrical pattern in Direction 2 that uses the seed and the Direction 1 settings.
  The 2017 dialog (S9) has no such option, and the 2024 API (S44) has no member for it.
  **Unknown.** Its exact geometry. **Inferred.** Probably a copy of the Direction 1
  pattern running the opposite way from the seed.

### Instances to skip

- **Observed (S8, S22).** Instances are skipped by picking their markers in the graphics
  area; the dialog shows each picked instance's coordinates. Picking a skipped one again
  restores it. Box and lasso selection also toggle instances (S22).
- **Observed (S23).** Deleting an instance's face and choosing *Delete Pattern Instances*
  adds it to the skip list; deleting it from that list restores it.
- **Observed (S8, S24).** Instances are named by their column and row, such as `(5,1)`.
- **Observed (S46).** The API stores skipped instances as integers, 0-based:
  - one direction: `I = i - 1`;
  - two directions: `I = n2 (i - 1) + (j - 1)`,

  where `i` is the Direction 1 index, `j` the Direction 2 index and `n2` the Direction 2
  count. The Direction 2 index therefore runs fastest. The seed, `(1, 1)`, would be 0.
- **Observed (S46).** The same page's example says that skipping the third and fifth items
  gives the values 3 and 5, which disagrees with `I = i - 1` (that would give 2 and 4).
  **Unknown.** Which is right. `anvil-migrate` should check a sample part: read
  `SkippedItemArray` and compare it with what the user sees.
- **Unknown.** How instances are numbered when *Pattern seed only* is selected (the grid
  formula then names places that do not exist), and whether the seed can be skipped.
- **Decision.** AnvilCAD stores skipped instances as 1-based `(i, j)` pairs (`j = 1` for a
  one-direction pattern), the form the user sees. It converts to and from the API integer
  only in `anvil-migrate`. The seed `(1, 1)` cannot be skipped; a skip entry outside the
  pattern is ignored and reported as a warning.

### Instances to vary and vary sketch

- **Observed (S8, S24).** *Instances to Vary* adds a spacing increment per direction: the
  gap grows by the increment at each step. With spacing 50 mm and increment 20 mm, the
  second instance is 70 mm from the first and the third 90 mm from the second. A
  dimension increment does the same to a seed dimension (8 mm, increment 10: 18, 28, 38,
  48). Single instances can also be overridden.
  - **Inferred.** With increment `δ`, instance `i` sits at
    `(i - 1) s + δ (i - 1) i / 2`. For 50 and 20: 0, 70, 160, 270 mm.
- **Observed (S25).** *Vary sketch* lets each instance's sketch solve against the model
  again, so undimensioned sizes follow the model.
- **Decision.** Neither is in the first version.

## Circular pattern

### Axis

- **Observed (S12, S13).** *Pattern Axis* may be an axis, a circular edge, a linear edge or
  sketch line, a cylindrical face or surface, a revolved face or surface, or an angular
  dimension. **Observed (S48).** `GetAxisType` reports reference axis (0), edge (1),
  dimension (2) or sketch line (3).
- **Inferred.** The axis each input gives: a circular edge, its circle's axis through its
  centre; a cylindrical or revolved face, its axis of revolution; a line or axis, that
  line.
- **Unknown.** Which axis an angular dimension gives. **Inferred.** The line through the
  angle's vertex, normal to the plane of its two lines.
- **Observed (S12, S48).** *Reverse Direction* reverses the sense of rotation.
- **Unknown.** The sense before reversing. **Decision.** AnvilCAD rotates by the right-hand
  rule about its stored axis direction (as `rotation_frame` in `rebuild.rs` does today),
  and *Reverse* rotates the other way.

### Angle, equal spacing and count

- **Observed (S12, S13, S18).** The number of instances includes the seed (S51 says so
  for the API). *Equal spacing* sets the angle to 360°.
- **Observed (S12).** From about 2020, Direction 1 offers *Instance spacing* or *Equal
  spacing*. With instance spacing, the angle is the angle *between neighbouring
  instances*.
- **Observed (S51).** In the API, the angle (`Spacing`, in radians) is the step between
  instances, *unless equal spacing is on, when it is the total angle*.
- **Observed (S17, S58).** For component patterns: choosing equal spacing sets the angle
  to 360°, the user may type another angle, and the instances are then spread evenly over
  that total.
- **Inferred.** Two modes, for `n` instances (seed included) and angle `A`:
  - instance spacing: instance `k` (1-based) is the seed turned by `(k - 1) A`. The pattern
    spans `(n - 1) A`, which may exceed 360° and overlap itself;
  - equal spacing, `A = 360°`: the step is `360° / n`, so the last instance is one step
    short of the seed (6 instances: 0°, 60°, ..., 300°).
- **Unknown.** With equal spacing and a total `A` below 360°, whether the step is `A / n`
  (last instance one step short of `A`) or `A / (n - 1)` (last instance exactly at `A`).
  No source gives a worked example. The circular *sketch* pattern (S39) also treats its
  angle as the total degrees, without settling this.
- **Decision.** AnvilCAD uses `A / (n - 1)` for a partial total, so the last instance sits
  at `A`, and `360° / n` for a full turn. This is what `CircularPatternFeature` already
  does, so existing files keep their meaning. `anvil-migrate` must check this against a
  sample part with a partial equal-spaced pattern before trusting it.
- **Observed (S12).** In the 2025 dialog, the help text for *Instance spacing* speaks of
  the "distance between the centers of instances". **Inferred.** It means the angle between
  them; the field is an angle.

### Direction 2 (bidirectional)

- **Observed (S12, S15, S16, S48).** *Direction 2* patterns the seed about the same axis in
  the other sense of rotation. It has its own angle, equal spacing and count, or
  *Symmetric*, which uses the Direction 1 values. Added for features in 2017 (API) and
  for component patterns in 2019 (S16).
- **Observed (S15).** Worked example: Direction 1 at 25° and 4 instances; selecting
  Direction 2 with *Symmetric* adds three instances the other way. Clearing *Symmetric* and
  setting 30° and 6 changes Direction 2 only.
- **Inferred.** Direction 2 counts the seed too, so the pattern has `n1 + n2 - 1`
  instances: seed, `n1 - 1` one way and `n2 - 1` the other. In S15, 4 + 4 - 1 = 7
  symmetric, and 4 + 6 - 1 = 9 after the change. Unlike a linear pattern, it is not a
  grid.
- **Observed (S48).** `Spacing2`, `TotalInstances2` and `EqualSpacing2` are only available
  when *Symmetric* is off.
- **Observed (S15).** In the example, instances 2 and 3 of Direction 2 are skipped.
  **Unknown.** How those map to `SkippedItemArray` integers: S49 gives the same grid
  formula as for linear patterns, which does not fit a non-grid pattern.

### Instances to skip and to vary

- **Observed (S12, S49).** Skipping works as for linear patterns; one direction uses
  `I = i - 1`, 0-based, with the same discrepancy in the example as S46.
- **Inferred.** Instance `k` is numbered in the order of rotation from the seed, `k = 1`
  the seed.
- **Observed (S12).** *Instances to Vary* exists with a spacing increment (1.5 mm and 0.3:
  1.8, 2.1, 2.4 between successive instances) and dimension increments. **Decision.** Not in
  the first version.
- **Observed (S48).** `GetTransform` documents its index as running from 1 to
  `TotalInstances`. **Observed (S57).** The published example calls it with 0 to
  `TotalInstances - 1`. **Unknown.** Which is right.

## Sketch driven pattern

- **Observed (S30).** The seed is copied to each point of a sketch. The *reference point*
  decides which point of the seed lands on each sketch point: the seed's *Centroid*, or a
  *Selected point* (a vertex or sketch point; S30 also mentions the sketch origin).
- **Observed (S29).** The centroid depends on the seed:
  - a cylindrical, conical or revolved feature: where its axis meets the pattern's XY
    plane;
  - a sketch of lines and arcs on a plane parallel to the XY plane: the sketch's centroid;
  - otherwise: the centroid of the seed's face;
  - with several seeds, the first seed's; with several contours, the largest closed
    contour's.
- **Observed (S52).** The API's reference point type is centroid (-1), closed curve (0),
  sketch point (1) or vertex (2).
- **Inferred.** Each instance is the seed translated by `q - r`, where `q` is a sketch
  point and `r` the reference point. The copies are translated only, never rotated.
  Whether the seed's own point counts is not stated.
  - **Decision.** A sketch point at the reference point makes no extra copy.
- **Observed (S22, S23).** Instances cannot be skipped; to remove one, delete its sketch
  point.
- **Decision.** AnvilCAD would use a sketch's free points, as the Hole feature does
  (see [hole-wizard.md](hole-wizard.md)). Recommended after the three main features.

## Table driven pattern

- **Observed (S31, S32).** Instances are placed at XY coordinates in a coordinate system's
  XY plane, measured from its origin. The table can be typed in, or read from a `.sldptab`
  file or a two-column text file (X and Y, separated by spaces, commas or tabs). The
  reference point is the seed's centroid or a selected point, shown as "Point 0".
  Coordinates may be negative.
- **Inferred.** The same translation as the sketch-driven pattern, with the coordinates
  taken from the table.
- **Decision.** Later work. AnvilCAD would accept the plain two-column text file only.

## Curve driven pattern

- **Observed (S33).** Instances follow a planar or 3D curve, open or closed, in one or two
  directions. Spacing is given or equal along the curve. *Transform curve* keeps the
  seed's X and Y offsets from the curve's start; *Offset curve* keeps its normal distance.
  *Tangent to curve* turns each instance with the curve; *Align to seed* keeps the seed's
  orientation. *Pattern seed only* applies to Direction 2.
- **Unknown.** The exact frames that *Transform curve*, *Offset curve* and *Tangent to
  curve* use, and how equal spacing measures along the curve (arc length is likely).
- **Decision.** Out of scope.

## Fill pattern

- **Observed (S34, S35).** A planar region (faces or a sketch) is filled with copies of a
  feature or with a built-in cut (circle, square, diamond, polygon). Layouts:
  *Perforation* (instance spacing and stagger angle), *Circular*, *Square* and *Polygon*
  (concentric loops with loop spacing and per-loop spacing or count), with margins and a
  pattern direction. The count is a driven value; *Validate count* drops instances that
  miss the model.
- **Decision.** Out of scope.

## Variable pattern

- **Observed (S36, S37).** Each instance has its own values for chosen seed dimensions,
  held in a table; instances appear as children of a "VarPattern" feature. Supported
  seeds include extrudes, revolves, sweeps, lofts, fillets, chamfers, domes and drafts.
  Geometry pattern does not apply (S20).
- **Decision.** Out of scope. It amounts to "a list of copies of a feature with different
  dimensions", which AnvilCAD users can model directly.

## Testable statements

All solids below are in millimetres; a *plate* is an axis-aligned block. The seed in
1 to 15 is a *square cut*: a 10 × 10 mm rectangle cut through all, centred at the stated
XY point on the plate's top face.

**Linear pattern.** Plate 100 × 60 × 10, `x ∈ [-50, 50]`, `y ∈ [-30, 30]`, `z ∈ [0, 10]`
(60 000 mm³). Seed centred at `(-35, -20)`.

1. Direction 1 = +X, spacing 20, 4 instances: cuts centred at x = -35, -15, 5, 25.
   Volume 60 000 - 4 × 1 000 = 56 000 mm³.
2. Add Direction 2 = +Y, spacing 20, 3 instances, *Pattern seed only* cleared: a 4 × 3
   grid, 12 cuts, volume 48 000 mm³.
3. The same with *Pattern seed only* selected: 4 + 3 - 1 = 6 cuts (x = -35 … 25 at
   y = -20, and y = 0, 20 at x = -35), volume 54 000 mm³.
4. The grid of statement 2 with instances `(2, 1)` and `(4, 3)` skipped: 10 cuts, volume
   50 000 mm³. In the API's integers (S46 formula, `n2 = 3`), these are 3 and 11.
5. Direction 1 reversed, 2 instances: the copy is centred at x = -55, so the cut spans
   x ∈ [-60, -50] and removes nothing. Volume 59 000 mm³.
6. One instance: the pattern adds nothing; volume 59 000 mm³ (seed only).
7. Changing the seed to 6 × 6 rebuilds every instance at 6 × 6: case 1 becomes
   60 000 - 4 × 360 = 58 560 mm³.
8. Spacing increment 20 on spacing 50 (Instances to Vary, when implemented): instances at
   0, 70, 160, 270 mm from the seed.
9. Up to reference (when implemented): reference the face x = 50, offset 0, measured from
   the centroid, spacing 20: `L = 85`, `n = floor(85 / 20) + 1 = 5`, cuts at
   x = -35 … 45. With 5 instances given instead, the spacing is 85 / 4 = 21.25 mm.

**Circular pattern.** Plate 100 × 100 × 10 centred on the origin (100 000 mm³), axis Z
through the origin.

10. Seed centred at `(30, 0)`, equal spacing, 360°, 6 instances: centres at 0°, 60°, …,
    300° on radius 30; volume 100 000 - 6 000 = 94 000 mm³.
11. Seed as 10, equal spacing, 360°, 4 instances: centres at (30, 0), (0, 30), (-30, 0),
    (0, -30); volume 96 000 mm³.
12. Seed as 10, instance spacing 30°, 4 instances: centres at 0°, 30°, 60°, 90°; volume
    96 000 mm³. The instance at 90° is centred at (0, 30).
13. Seed as 10, equal spacing, total 90°, 4 instances (AnvilCAD's decision, `A/(n-1)`):
    centres at 0°, 30°, 60°, 90°, the same as 12.
14. Seed centred at `(40, 0)`, Direction 1 at 25° with 4 instances, Direction 2
    *Symmetric*: 7 cuts at -75°, -50°, -25°, 0°, 25°, 50°, 75°; volume 93 000 mm³.
    Direction 2 changed to 30° and 6 instances: 9 cuts (D2 at -30° … -150°), volume
    91 000 mm³. Then skipping Direction 2 instances 2 and 3 (-30° and -60°): 7 cuts,
    93 000 mm³.
15. *Reverse direction* on 12 puts the instance at 90° at (0, -30) instead.

**Mirror.** Plate 100 × 60 × 10 as for the linear pattern (60 000 mm³), sketched on the
Front Plane (XY). The plane x = 0 is AnvilCAD's Right Plane and y = 0 its Top Plane.

16. Seed square cut at `(-35, -20)` mirrored in the plane x = 0: a second cut centred at
    `(35, -20)`; volume 58 000 mm³.
17. Add the secondary plane y = 0, *Mirror seed only* cleared: cuts at `(±35, ±20)`,
    four in all, volume 56 000 mm³. *Mirror seed only* selected: `(-35, -20)`,
    `(35, -20)`, `(-35, 20)`, three, volume 57 000 mm³ (Inferred reading of S2).
18. Seed square cut centred at `(3, 0)` (spanning x ∈ [-2, 8]) mirrored in x = 0: the
    copy spans x ∈ [-8, 2]; together they remove x ∈ [-8, 8] × y ∈ [-5, 5], 1 600 mm³;
    volume 58 400 mm³.
19. Changing the seed of 16 to 6 × 6: both cuts become 6 × 6; volume 59 280 mm³.
20. The linear pattern of 1 (seed included) mirrored in x = 0: 8 cuts (x = ±5, ±15, ±25,
    ±35 at y = -20), volume 52 000 mm³. The cuts at x = ±5 touch at x = 0 but do not
    overlap.
21. A point `(a, b, c)` of a seed maps to `(-a, b, c)` in the plane x = 0, and to
    `(a - 2 (a - 10), b, c) = (20 - a, b, c)` in the plane x = 10.
22. Handedness: take an L-shaped seed outline with corners (10, 0), (30, 0), (30, 5),
    (15, 5), (15, 15), (10, 15), signed area +150 mm² in that order. Its mirror image in
    x = 0, with the corners kept in the same order, has signed area -150 mm²; a circular
    or linear instance keeps +150 mm². The mirrored boss itself is still a valid solid of
    the same volume.
23. Mirror body, merge on: block x ∈ [0, 20], y ∈ [0, 10], z ∈ [0, 5] (1 000 mm³)
    mirrored about its own face x = 0 gives one body of 2 000 mm³, x ∈ [-20, 20]. With
    merge off it gives two bodies of 1 000 mm³ each.

**Errors and naming.**

24. A mirror whose plane is a cylindrical face fails with an error on the feature; later
    features still rebuild.
25. *Mirror seed only* with planes at 60° fails with an error.
26. A linear pattern with spacing 0 or -5, or with 0 instances, fails with an error; one with
    Direction 2 parallel to Direction 1 fails with an error.
27. The first mirror in a new part is named "Mirror1", the first linear pattern
    "LPattern1" and the first circular pattern "CirPattern1".

## API names

For `anvil-migrate`, which reads a user's own licensed SOLIDWORKS through its published
API. Names and values only.

**Type names (`IFeature::GetTypeName2`) and data objects (S54).**

| Feature | Type name | Feature data |
|---|---|---|
| Mirror of features or faces | `MirrorPattern` | `IMirrorPatternFeatureData` |
| Mirror of bodies | `MirrorSolid` | `IMirrorSolidFeatureData` |
| Mirror Part (opposite hand) | `MirrorStock` | `IMirrorPartFeatureData` |
| Linear pattern | `LPattern` | `ILinearPatternFeatureData` |
| Circular pattern | `CirPattern` | `ICircularPatternFeatureData` |
| Sketch driven pattern | `SketchPattern` | `ISketchPatternFeatureData` |
| Table driven pattern | `TablePattern` | `ITablePatternFeatureData` |
| Curve driven pattern | `CurvePattern` | `ICurveDrivenPatternFeatureData` |
| Fill pattern | `APattern` | `IFillPatternFeatureData` |
| Variable pattern | `DimPattern` | `IDimPatternFeatureData` |
| Derived patterns | `DerivedLPattern`, `DerivedCirPattern`, `DerivedHolePattern` | `IDerivedPatternFeatureData` |
| Component patterns (assemblies) | `LocalLPattern`, `LocalCirPattern`, `LocalSketchPattern`, `LocalCurvePattern`, `LocalChainPattern`, `MirrorCompFeat` | `ILocal…PatternFeatureData`, `IChainPatternFeatureData`, `IMirrorComponentFeatureData` |

**`swFeatureNameID_e` (S56, S55)**, for `IFeatureManager::CreateDefinition`:
`swFmMirrorPattern`, `swFmMirrorSolid`, `swFmLPattern`, `swFmCirPattern`,
`swFmSketchPattern`, `swFmTablePattern`, `swFmCurvePattern`, `swFmFillPattern`,
`swFmDimPattern`, `swFmDerivedLPattern`, and the `swFmLocal…Pattern` and
`swFmMirrorComponent` members for assemblies.

**Mirror (S40 to S43).**
- `IMirrorPatternFeatureData`: `Plane`, `GetMirrorPlaneType` (0 face, 1 plane),
  `PatternFeatureArray`, `MirrorFaceArray`, `GetMirrorFaceCount`, `GetPatternFeatureCount`,
  `GeometryPattern`, `PropagateVisualProperty`, `FeatureScope`, `FeatureScopeBodies`.
- `IMirrorSolidFeatureData`: `Face` (the mirror face), `PatternBodyArray`, `Merge` (default
  true), `KnitSurface`, `PropagateVisualProperty`, `GetTransform`.
- `IFeatureManager::InsertMirrorFeature2(BMirrorBody, BGeometryPattern, BMerge, BKnit,
  ScopeOptions)`; selection marks: features 1, faces 128, bodies 256, mirror plane 2.
- **Unknown.** No API member for the *secondary* mirror plane or *Mirror seed only*
  appears in the 2024, 2025 or 2026 member lists. `anvil-migrate` cannot read them from
  the published API; a mirror about two planes may need to come across as geometry or be
  checked by hand.

**Linear pattern (S44 to S46, S50).**
- `ILinearPatternFeatureData`: `D1Axis`, `GetD1AxisType` (0 axis, 1 edge, 2 dimension,
  3 sketch segment), `D1ReverseDirection`, `D1Spacing` (metres), `D1TotalInstances`
  (seed included), `D1EndCondition`, `D1EndReference`, `D1EndRefOffset`,
  `D1EndRefReverseOffset`, `D1EndUseSeedReference`, `D1EndSeedReference`,
  `D1EndUseSpacing`; the same `D2…` members plus `D2PatternSeedOnly`;
  `IsDirection2Specified`; `SkippedItemArray`, `GetSkippedItemCount`;
  `PatternFeatureArray`, `PatternFaceArray`, `PatternBodyArray`, `PatternElement`,
  `BodyPattern`; `GeometryPattern`, `VarySketch`, `InstancesToVary`,
  `GetInstanceToVaryOptions`, `PropagateVisualProperty`; `FeatureScope`, `AutoSelect`;
  `GetTransform` (index documented as 1 to `D1TotalInstances + D2TotalInstances`, which
  does not match a grid of `n1 × n2`; **Unknown** which instances it covers).
- `swPatternEndCondition_e`: `swPatternEndCondition_SpacingAndInstances` = 0,
  `swPatternEndCondition_UpToReference` = 1.
- `swPatternElementSelection_e`: `swErrorInPatternElementSelection` = 0,
  `swFeatureFaces` = 1, `swBodiesToPattern` = 2.
- `IFeatureManager::FeatureLinearPattern5`: `Num1`, `Spacing1`, `Num2`, `Spacing2`,
  `FlipDir1`, `FlipDir2`, `DName1`, `DName2`, `GeometryPattern`, `VaryInstance`,
  `HasOffset1/2`, `CtrlByNum1/2`, `FromCentroid1/2`, `RevOffset1/2`, `Offset1/2`,
  `D2PatternSeedOnly`, `SyncSubAssemblies`.

**Circular pattern (S47 to S49, S51).**
- `ICircularPatternFeatureData`: `Axis`, `GetAxisType` (0 reference axis, 1 edge,
  2 dimension, 3 sketch line), `ReverseDirection`, `Spacing` (radians), `EqualSpacing`,
  `TotalInstances` (seed included), `Direction2`, `Symmetric`, `Spacing2`,
  `EqualSpacing2`, `TotalInstances2`, `SkippedItemArray`, `GetSkippedItemCount`,
  `PatternFeatureArray`, `PatternFaceArray`, `PatternBodyArray`, `PatternElement`,
  `BodyPattern`, `GeometryPattern`, `VarySketch`, `InstancesToVary`,
  `PropagateVisualProperty`, `FeatureScope`, `AutoSelect`, `GetTransform`.
- `IFeatureManager::FeatureCircularPattern5`: `Number`, `Spacing` (the step, or the total
  angle when `EqualSpacing` is true), `FlipDirection`, `DName`, `GeometryPattern`,
  `EqualSpacing`, `VaryInstance`, `SyncSubAssemblies`, `BDir2`, `BSymmetric`, `Number2`,
  `Spacing2`, `DName2`, `EqualSpacing2`.
- Mapping to AnvilCAD: `EqualSpacing = true` → equal spacing with total angle `Spacing`;
  `EqualSpacing = false` → instance spacing with step `Spacing`.

**Sketch and table driven (S52, S53).**
- `ISketchPatternFeatureData`: `Sketch`, `UseCentroid`, `ReferencePoint`,
  `GetReferencePointType` (-1 centroid, 0 closed curve, 1 sketch point, 2 vertex),
  `GetBasePoint`, `PatternFeatureArray`, `PatternFaceArray`, `PatternBodyArray`,
  `GeometryPattern`, `PropagateVisualProperty`, `GetTransform`.
- `ITablePatternFeatureData`: `CoordinateSystem`, `PointArray`, `ReferencePoint`,
  `UseCentroid`, `SkippedItemArray`, `PatternFeatureArray`, `GeometryPattern`.

**Others (S53).** `ICurveDrivenPatternFeatureData` (`D1Direction`, `D1InstanceCount`,
`D1IsEqualSpaced`, `D1Spacing`, `D1CurveMethod`, `D1AlignmentMethod`, `D1FaceNormal`,
`D2…`, `D2PatternSeedOnly`, `SkippedItemArray`); `IFillPatternFeatureData`
(`PatternLayoutType`, `InstanceSpacing`, `LoopSpacing`, `StaggerAngle`, `Margins`,
`PatternDirection`, `CreateSeedCutType`, `SkippedItemArray`, …); `IDimPatternFeatureData`
(`PropagateVisualProperties` only).

**Feature scope (S56).** `swFeatureScope_e`: `swFeatureScope_AllBodies` = 0,
`swFeatureScope_SelectedBodiesWithAutoSelect` = 1,
`swFeatureScope_SelectedBodiesWithOutAutoSelect` = 2.

## Recommended scope

What exists today: `LinearPatternFeature { features, direction: Axis, spacing, count }`
and `CircularPatternFeature { features, axis: Axis, total_angle, count }`, where `Axis`
is X, Y, Z or a line, and a total angle of exactly 360 means equal spacing. Both move the
seeds' exact tool bodies (or faceted tools) and combine them again. There is no mirror.
The exact kernel's `Solid::transformed` handles rigid motions only.

In order:

1. **Mirror of features about one plane.** A `MirrorFeature { features, plane: PlaneRef }`
   named "Mirror". The plane is Front, Top, Right, a reference plane or a planar face. It
   needs a reflecting transform in both kernels that reverses face orientation. Tests 16,
   18, 19, 20, 21, 22, 24 and 27.
2. **Linear pattern, second direction and skipping.** Add an optional Direction 2
   (direction, spacing, count), `pattern_seed_only`, a `reverse` flag per direction and
   `skip: Vec<(u32, u32)>`. Let directions also come from an edge or a face normal,
   resolved to a stored vector. Keep old files reading as one direction with nothing
   skipped (`#[serde(default)]`). Tests 1 to 7 and 26.
3. **Circular pattern, explicit spacing mode, second direction and skipping.** Add
   `equal_spacing: bool` (old files: true, keeping today's meaning of `total_angle`), let
   the angle be the step when it is false, and add `reverse`, an optional Direction 2
   (symmetric, or its own angle, mode and count) and a skip list. Let the axis come from
   a circular edge or a cylindrical face. Tests 10 to 15.
4. **Mirror about two planes and mirror of the body.** Add the secondary plane and
   `mirror_seed_only` (tests 17, 25), then a body mirror with merge (test 23, once
   multibody parts exist; with merge on, it works on today's single body).
5. **Sketch driven pattern**, reusing the Hole feature's free-point handling, with centroid
   or a selected point as the reference.
6. **Up to reference** for linear patterns (test 9), once the count rule is checked.
7. **Table driven pattern** from typed coordinates or a two-column text file.

Leave out for now: faces to pattern, geometry pattern versus solved instances (AnvilCAD
copies tool bodies; record the flag for `anvil-migrate`), instances to vary, vary sketch,
the linear Direction 2 *Symmetric* option, feature scope, propagate visual properties
(record only), curve driven, fill and variable patterns, and Mirror Part.

## Unknown or not covered

1. With equal spacing and a total angle below 360°, whether SOLIDWORKS divides by `n` or by
   `n - 1`. AnvilCAD's decision is `n - 1`; check it on a sample part.
2. `SkippedItemArray`: the formula is 0-based but the same page's example is not. Also how
   instances are numbered with *Pattern seed only*, in bidirectional circular patterns, and
   whether the seed can be skipped.
3. `GetTransform` index ranges: 1-based in the remarks, 0-based in the example; for linear
   patterns, `D1 + D2` rather than `D1 × D2`.
4. The geometry of *Mirror seed only*, and of two non-perpendicular mirror planes, is read
   from illustrations and the option name, not stated. No API member exposes the
   secondary plane.
5. The default sense of a linear direction taken from an edge, dimension or face, and of a
   circular pattern's rotation, before *Reverse Direction*.
6. The axis an angular dimension gives a circular pattern.
7. The *Symmetric* option under a linear pattern's Direction 2 (2025 dialog only, no API
   member).
8. The rounding rule for *Up to reference* with a given spacing, and its behaviour when the
   reference is behind the seed or closer than one spacing.
9. The default of *Geometry pattern* in the dialog, and SOLIDWORKS's handling of copies that
   do not touch the body.
10. Default tree names for sketch, table, curve and fill patterns, and for a body mirror.
11. The frames used by curve driven patterns (*Transform curve*, *Offset curve*, *Tangent to
    curve*).
12. How any of these features is stored in a `.sldprt` file; not studied.
