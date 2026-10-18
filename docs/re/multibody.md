# Multibody parts and body features

What AnvilCAD should do when a part holds more than one solid body, and the features that
work on whole bodies: combining bodies, moving and copying them, deleting or keeping them,
splitting a body into several, intersecting bodies into regions, and scaling. It also covers
how a part comes to have several bodies, how bodies are listed and named, how a feature's
scope limits which bodies it touches, and how mass properties report several bodies.
SOLIDWORKS users know these as multibody parts and its Combine, Move/Copy Bodies,
Delete/Keep Body, Split, Intersect and Scale features. AnvilCAD follows their published
behaviour where that makes sense. Nothing here is implemented yet; the specification is
written for future work in `crates/anvil-model/src/rebuild.rs`, `crates/anvil-model/src/exact.rs`
and `crates/anvil-model/src/document.rs`.

## How this was studied

- **Inputs.** Only published information was used:
  - Dassault's public online help for SOLIDWORKS 2025, with 2012, 2014 and 2018 pages
    where the 2025 page was missing or worded differently;
  - the public SOLIDWORKS 2024 API help (interfaces, members, enumerations and the
    published VBA examples), with one 2022 example page.

  No SOLIDWORKS software, installer, library or file was downloaded, installed, run or
  analysed. No SOLIDWORKS licence was accepted on the machine used. The sample files that the
  API examples name (`multi_inter.sldprt`, `multi_local.sldprt`, `pot.SLDPRT`) were not
  fetched or opened; only the example text that names them was read.
- **Retrieval.** The help site renders its articles with JavaScript, and a plain request
  is refused. Each article was fetched over HTTPS with a browser user agent. Its text was read
  from the page's embedded `__NEXT_DATA__` JSON (`props.pageProps.helpContentData.helpText`),
  with HTML stripped by a script run with `python3 -I`. A missing page still returns HTTP 200,
  with "page cannot be found" in its data, so each page's content was checked. Page names were
  found with web searches restricted to `help.solidworks.com`. About 100 pages were read,
  counting older versions of the same page. Five 2025 tutorial pages (`swconntutorial`,
  multibody lessons) returned no article text by this method and were not used. The text was
  paraphrased; none of it is copied into this repository.
- **REA.** REA (`rea-agents` 6.5.0, with the `third_party/rea` submodule at `3fc0075`) was
  set up and its research method followed:
  - each finding below is marked **Observed** (stated in a cited source), **Inferred**
    (our reasoning from the sources), **Decision** (AnvilCAD's own choice where the
    sources are silent) or **Unknown**;
  - every observation cites its source.

  REA's analysis tools were **not** run. They inspect artifacts (binaries, packages,
  captures, files), and the only lawful inputs for this behaviour are prose documents.
  Pointing REA at SOLIDWORKS software is ruled out by
  [INTEROPERABILITY.md](../../INTEROPERABILITY.md). How bodies and body features are stored
  in `.sldprt` files was not studied.
- **What the sources do not cover.** The help describes options, not algorithms. It never
  states the order in which the X, Y and Z rotation angles are applied, where copies of a
  copied body go, how the pieces of a split or cut are numbered, or what error messages say.
  Those points are listed under Unknown and given an AnvilCAD decision.

## Sources

Base A is `https://help.solidworks.com/2025/English/SolidWorks/sldworks/`. Base B is
`https://help.solidworks.com/2024/English/api/sldworksapi/SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.`
(followed by `<member>.html`). Base C is
`https://help.solidworks.com/2024/English/api/swconst/SOLIDWORKS.Interop.swconst~SOLIDWORKS.Interop.swconst.`
(followed by `<enum>.html`). Base D is `https://help.solidworks.com/2024/English/api/sldworksapi/`.
All were read on 2026-10-18.

| # | Topic | URL | Version |
|---|---|---|---|
| S1 | Multibody overview: Solid Bodies folder, features that make several bodies | A + `c_Multibody_Overview.htm` | 2025 |
| S2 | Multibody overview (older wording) | `https://help.solidworks.com/2012/English/SolidWorks/sldworks/Multibody_Overview.htm` | 2012 |
| S3 | Organizing solid bodies: count, `[n]` instance numbers, folders, feature history | A + `c_Organizing_Solid_Bodies.htm` | 2025 |
| S4 | Multibody parts index; hiding, showing and isolating bodies; mates in multibody parts | A + `c_Multibody_Parts_folder.htm`, `c_Hiding_and_Showing_Bodies.htm`, `c_Mates_in_Multibody_Parts.htm` | 2025 |
| S5 | Feature scope in multibody parts | A + `r_feature_scope_multibody_parts.htm` | 2025 |
| S6 | Bodies to Keep dialog for cuts | `https://help.solidworks.com/2018/English/SolidWorks/sldworks/HIDD_Select_Body.htm` | 2018 |
| S7 | Cuts (disjoint bodies, Bodies to Keep) | A + `c_cuts.htm` | 2025 |
| S8 | Surface cut feature scope | A + `hidd_surf_cut.htm` | 2025 |
| S9 | Extrude PropertyManager: Merge result, Feature Scope | A + `r_extrude_propertymanager.htm` | 2025 |
| S10 | Combine PropertyManager | A + `HIDD_DVE_FEAT_COMBINE_BODIES.htm` | 2025 |
| S11 | Combining bodies: Add, Subtract, Common | A + `t_combining_bodies_add.htm`, `t_combining_bodies_subtract.htm`, `t_combining_bodies_common.htm` | 2025 |
| S12 | Combine bodies (older page: "Combine1", weldment warning) | `https://help.solidworks.com/2012/English/solidworks/Sldworks/HIDD_DVE_FEAT_COMBINE_BODIES.htm` | 2012 |
| S13 | Modeling techniques: body intersection, bridging, local operations, symmetry, tool bodies | A + `c_Modeling_Techniques_folder.htm`, `c_Body_Intersection.htm`, `c_Bridging_Multibody.htm`, `t_Local_Operations.htm`, `c_Symmetry_Modeling.htm`, `c_Tool_Body_Modeling.htm` | 2025 |
| S14 | Move/Copy Body PropertyManager | A + `HIDD_DVE_FEAT_MOVE_SURF.htm` | 2025 |
| S15 | Move/Copy bodies task; manipulation of features index | A + `t_move_copy_bodies.htm`, `c_Manipulation_of_Features_Folder.htm` | 2025 |
| S16 | Move/Copy bodies (older wording: centre of mass, negative distance) | `https://help.solidworks.com/2012/English/SolidWorks/sldworks/HIDD_DVE_FEAT_MOVE_SURF.htm`, `https://help.solidworks.com/2014/English/SolidWorks/sldworks/t_Move_Copy_Body_PM_features.htm` | 2012, 2014 |
| S17 | Delete/Keep Body | A + `HIDD_DVE_FEAT_DELETE_BODY.htm` | 2025 |
| S18 | Split and Save Bodies PropertyManagers | A + `HIDD_DVE_FEAT_SPLIT.htm` | 2025 |
| S19 | Changing trim tools when editing a split | A + `t_Changing_Trim_Tools.htm` | 2025 |
| S20 | Intersect PropertyManager | A + `HIDD_DVE_FEAT_SCULPT.htm` | 2025 |
| S21 | Measuring internal volume with Intersect | A + `t_measure_internal_volume.htm` | 2025 |
| S22 | Insert Part PropertyManager | A + `HIDD_DVE_IMPORT_FEATURES.htm` | 2025 |
| S23 | Inserting a part into another part | A + `t_Inserting_a_Part_into_Another_Part.htm` | 2025 |
| S24 | Scale PropertyManager | A + `HIDD_DVE_INSERT_SCALE.htm` | 2025 |
| S25 | Scales overview, scaling a part, examples of scaling | A + `c_Scale_Feature_Overview.htm`, `t_Scaling_a_Part.htm`, `c_example_scaling.htm` | 2025 |
| S26 | Mass Properties dialog box | A + `HIDD_MASSPROPERTY_TEXT_DLG.htm`; `https://help.solidworks.com/2012/english/solidworks/sldworks/hidd_massproperty_text_dlg.htm` | 2025, 2012 |
| S27 | Materials and multibody parts | A + `c_multibody_materials.htm` | 2025 |
| S28 | Entity properties: renaming a body | A + `HIDD_ENT_PROPERTY.htm` | 2025 |
| S29 | `IFeatureManager::InsertCombineFeature`, `ICombineBodiesFeatureData` members and `OperationType` | B + `IFeatureManager~InsertCombineFeature`, `ICombineBodiesFeatureData_members`, `ICombineBodiesFeatureData~OperationType` | 2024 |
| S30 | `swBodyOperationType_e`, `swCombineBodiesOperationType_e` | C + `swBodyOperationType_e`, `swCombineBodiesOperationType_e` | 2024 |
| S31 | Combine Bodies example (VBA) | D + `Combine_Bodies_Example_VB.htm` | 2024 |
| S32 | `IFeatureManager::InsertMoveCopyBody2`; `IMoveCopyBodyFeatureData` members, `TransformType`, `TransformX`, `Copy`, `NumberOfCopies`; `swMoveCopyBodyFeatureTransformType_e` | B + `IFeatureManager~InsertMoveCopyBody2`, `IMoveCopyBodyFeatureData_members`, `IMoveCopyBodyFeatureData~<property>`; C + `swMoveCopyBodyFeatureTransformType_e` | 2024 |
| S33 | Move/copy examples: Move Bodies, Copy and Rotate Body Using Edge, Move and Copy Body by Setting Transforms | D + `Move_Bodies_Example_VB.htm`, `Copy_and_Rotate_Body_Using_Edge_Example_VB.htm`; `https://help.solidworks.com/2022/english/api/sldworksapi/move_and_copy_body_by_setting_transforms_example_vb.htm` | 2024, 2022 |
| S34 | `IFeatureManager::InsertDeleteBody2` and `InsertDeleteBody`; `IDeleteBodyFeatureData` members; Insert Body-Delete/Keep example | B + `IFeatureManager~InsertDeleteBody2`, `IFeatureManager~InsertDeleteBody`, `IDeleteBodyFeatureData_members`; D + `Insert_Delete_Body_Feature_Example_VB.htm` | 2024 |
| S35 | `IFeatureManager::InsertScale`; `IScaleFeatureData` members, `Type`, `Bodies`; `swScaleType_e`; scale examples | B + `IFeatureManager~InsertScale`, `IScaleFeatureData_members`, `IScaleFeatureData~Type`, `IScaleFeatureData~Bodies`; C + `swScaleType_e`; D + `Insert_Scale_Feature_Example_VB.htm`, `Get_Faces_Affected_By_Scale_Feature_Example_VB.htm` | 2024 |
| S36 | `IFeatureManager::PreSplitBody`, `PreSplitBody2`, `PostSplitBody`, `PostSplitBody2`; `ISplitBodyFeatureData` members | B + `IFeatureManager~<method>`, `ISplitBodyFeatureData_members` | 2024 |
| S37 | `IFeatureManager::PreIntersect`, `PreIntersect2`, `PostIntersect`; `IIntersectFeatureData` members; Create Intersect example | B + `IFeatureManager~<method>`, `IIntersectFeatureData_members`; D + `Create_Intersect_Feature_Example_VB.htm` | 2024 |
| S38 | `IPartDoc::InsertPart3`, `swInsertPartOptions_e`, `IDerivedPartFeatureData` members | B + `IPartDoc~InsertPart3`, `IDerivedPartFeatureData_members`; C + `swInsertPartOptions_e` | 2024 |
| S39 | `IFeatureManager::CreateSaveBodyFeature`, `ISaveBodyFeatureData` members | B + `IFeatureManager~CreateSaveBodyFeature`, `ISaveBodyFeatureData_members` | 2024 |
| S40 | `IFeature::GetTypeName2` (feature type names) | B + `IFeature~GetTypeName2` | 2024 |
| S41 | Bodies: `IPartDoc::GetBodies2`, `swBodyType_e`, `IBodyFolder::GetBodies`, `IBody2::Name`, `IBody2::GetFeatures`, `IFeature::GetBody`, `IFeature::GetFaces` | B + `IPartDoc~GetBodies2`, `IBodyFolder~GetBodies`, `IBody2~Name`, `IBody2~GetFeatures`, `IFeature~GetBody`, `IFeature~GetFaces`; C + `swBodyType_e` | 2024 |
| S42 | Mass properties API: `IMassProperty::AddBodies`, `IMassProperty::Volume`, `IMassProperty2` members, `IModelDocExtension::CreateMassProperty` and `CreateMassProperty2`, `IBody2::GetMassProperties` | B + `IMassProperty~AddBodies`, `IMassProperty~Volume`, `IMassProperty2_members`, `IModelDocExtension~CreateMassProperty`, `IModelDocExtension~CreateMassProperty2`, `IBody2~GetMassProperties` | 2024 |
| S43 | Extrude merge and scope: `IExtrudeFeatureData2::Merge`, `FeatureScope`, `AutoSelect`, `FeatureScopeBodies`; `IFeatureManager::FeatureExtrusion3` | B + `IExtrudeFeatureData2~<property>`, `IFeatureManager~FeatureExtrusion3` | 2024 |
| N1 | A reseller's article saying unrenamed bodies take the name of the last feature applied to them | `https://www.javelin-tech.com/blog/2011/11/name-bodies/` (seen only as a web-search summary, not fetched) | not Dassault |

## Terms

- A **body** is one connected solid: a closed, bounded region of space with its boundary
  faces. A part is a list of bodies. Two bodies may touch or even overlap; they stay separate
  until a feature combines them.
- A **tool** is the solid a feature makes (the prism of an extrude, the solid of a revolve)
  before it is applied to the bodies.
- Volumes are in mm³, lengths in mm, angles in degrees, unless stated.

## How a part comes to have several bodies

- **Observed (S1, S2).** A part can contain several solid bodies, and they can be worked on
  like a single body: features can be added to them, and each can be renamed and coloured.
- **Observed (S1).** One feature can make several bodies. The help lists extruded, revolved
  and swept bosses and cuts (thin ones included), surface cuts, thicken (boss and cut) and
  cavity.
- **Observed (S9, S43).** Boss/base extrudes have a **Merge result** option. When it is set,
  the new material is merged into an existing body where that is possible. When it is clear,
  the feature makes a body of its own. The API property `Merge` defaults to true.
  - **Inferred (S9).** "Where possible" means: the tool merges with the bodies it overlaps or
    touches. A tool that touches no body becomes a body of its own even with Merge result
    set. The same option exists on revolves, sweeps and lofts; the help was read only for
    extrudes.
  - **Inferred (S13).** Merging joins *every* body the tool reaches, not just one. The
    bridging technique (S13) makes a boss that connects separate bodies into one, and the
    local-operations example models a connecting bar and then "merges" the separately
    shelled cups through it.
  - **Inferred.** With Merge result clear, the new body may overlap existing bodies. Nothing
    is subtracted or joined; the overlap belongs to both bodies.
  - **Unknown.** Whether two bodies that share only a face (touching, with no overlap of
    volume) are merged. **Decision.** A shared face of positive area merges them; contact
    along an edge or at a point does not (the result would not be a manifold solid).
- **Observed (S7).** A cut can leave a body in several disjoint pieces; each piece becomes a
  body.
- **Observed (S18, S20, S23, S14).** Split, Intersect, Insert Part and Move/Copy Bodies with
  *Copy* set also add bodies.
- **Observed (S13).** Patterning or mirroring bodies, then combining them, is a published
  technique (symmetry modelling). Body patterns and mirrors are outside this specification.
- **Decision.** In AnvilCAD the same rules hold: a boss with `merge` set joins every body it
  overlaps or shares a face with into one body; a boss with `merge` clear always adds a new
  body; a cut that severs a body leaves one body per piece.

## The Solid Bodies folder and body names

- **Observed (S1, S2, S3).** When a part holds solid bodies, the feature tree shows a folder
  named **Solid Bodies** with the number of bodies in parentheses, for example
  "Solid Bodies(2)". Bodies can be grouped into sub-folders, and a command chosen on a
  folder (hide, delete, appearance) applies to every body in it. *Show Feature History*
  lists, under each body, the features that built it.
- **Observed (S3).** When one feature makes several bodies, each is listed with an instance
  number in square brackets after it: an extruded cut that leaves two bodies lists
  "Cut-Extrude1[1]" and "Cut-Extrude1[2]" (the number follows the feature name).
- **Observed (S21).** An Intersect that makes a new internal region lists two bodies,
  "Intersect[1]" (the new region) and "Intersect[2]" (the original model).
- **Inferred (S31, S33, S34, S37, N1).** A body that has not been renamed is named after the
  last feature that changed it. The published API examples select bodies named
  "Boss-Extrude1" and "Extrude-Thin1" (S31), "Fillet5" (S34) and "Shell1" (S37), in
  sample parts whose last feature on each body has that name. N1 says the same in so many
  words. The help pages themselves do not state the rule.
- **Observed (S28, S41).** A body can be renamed (Body Properties). The name must be unique
  in the part and must not contain "@". The new name shows in the Solid Bodies folder.
- **Observed (S41).** `GetBodies2` may return bodies in any order; `IBodyFolder::GetBodies`
  returns them in tree order.
- **Observed (S4).** Bodies can be hidden and shown, and isolated (others hidden or made
  transparent) for display. These are display states, not features.
- **Decision.** AnvilCAD names a body after the feature that last changed it, with `[n]`
  appended when that feature leaves more than one body, and lets the user rename it. A
  user's name sticks to the body until a feature consumes it (a Combine, a Delete) or
  splits it.
- **Unknown.** How SOLIDWORKS numbers the `[n]` pieces. **Decision.** AnvilCAD numbers the
  pieces of one feature in a fixed order so the numbers survive a rebuild: by the lowest
  corner of each piece's bounding box, comparing x, then y, then z (rounded to 1e-6 mm).
  Features that pick bodies (Combine, Delete/Keep, Move/Copy, Scale, feature scope) store
  the pair (feature, n) and resolve it on each rebuild, failing with a clear error if that
  body no longer exists.

## Feature scope

- **Observed (S5).** Some features have a **Feature Scope** with these options:
  - **All bodies.** The feature applies to every body it reaches, on every rebuild. Bodies
    added earlier in the tree later on are picked up automatically.
  - **Selected bodies.** The feature applies only to listed bodies. A body added later is
    left alone until the user edits the feature and adds it.
  - **Auto-select** (with Selected bodies). When the feature is first made, the list is
    filled with every body the feature reaches. After that it behaves like Selected bodies.
    Clearing it lets the user pick bodies (**Solid Bodies to Affect**).
- **Observed (S6, S7).** A cut that affects several bodies offers the same choices as
  **Bodies to Keep**, under the heading Feature Scope. Auto-select is on by default.
- **Observed (S8).** A surface cut has the same Feature Scope.
- **Observed (S43).** In the API, `UseFeatScope` false (property `FeatureScope` false) means
  all bodies; true means only the bodies in `FeatureScopeBodies`. `UseAutoSelect` true
  means the list is filled automatically. The tool is extended along its sketch plane and
  affects the bodies it reaches.
- **Observed (S9).** The 2025 extrude page lists Feature Scope for extrudes, but its wording
  ties it to an option ("Geometry pattern") that does not obviously belong there. **Unknown**
  whether the help is in error.
- **Inferred (S5, S43).** For a cut, scope limits which bodies lose material. For a boss with
  Merge result set, scope limits which bodies the new material can merge with.
- **Decision.** AnvilCAD's bosses and cuts get a scope: `All` (the default, and today's
  behaviour) or `Selected(list)`. Auto-select is a user-interface helper: when the user
  chooses Selected, the list is pre-filled with the bodies the tool reaches. Unlisted bodies
  are left unchanged even where the tool passes through them.
- **Not covered.** Bodies to Keep's "Selected bodies" meaning "discard the other pieces"
  (S6's example keeps some of the bodies a cut produces) is not specified further here.

## Combine

- **Observed (S10, S11, S12).** Combine works on bodies in one part; to combine separate
  parts, insert one into the other first (Insert Part). It has three operation types:
  - **Add**: all selected bodies become a single body.
  - **Subtract**: material that overlaps the selected bodies is removed from one **Main Body**,
    the body to keep. The bodies whose material is removed are listed under
    **Bodies to Subtract**.
  - **Common** (the 2025 page labels the button "Combine"): all material is removed except
    where the selected bodies overlap.
- **Observed (S10).** Add and Common take one list, **Bodies to Combine**. Subtract takes
  **Main Body** (one body) and **Bodies to Subtract** (one or more). *Make main body
  transparent* is a display aid. *Show Preview* previews.
- **Observed (S12).** The command's first feature is "Combine1". Combining weldment bodies is
  advised against because cut-list properties may not be computed correctly.
- **Observed (S13).** "Body intersection" is Common applied to two or three extrusions, each
  drawn from one orthographic view of the part.
- Geometry, with bodies `B1 … Bn`, main body `M` and tools `T1 … Tk`:
  - **Inferred (S10).** Add: the result is the union `B1 ∪ … ∪ Bn`, one body, and the inputs
    are removed from the body list.
  - **Inferred (S10).** Subtract: the result is `M − (T1 ∪ … ∪ Tk)`, and the tools are
    removed from the body list (only the main body is "kept").
  - **Inferred (S10, S13).** Common: the result is `B1 ∩ … ∩ Bn`, and the inputs are removed.
  - **Inferred.** Bodies not named in the feature are not changed.
- **Unknown.** What happens when Add's union is not connected, when Common's intersection
  is empty, or when Subtract leaves `M` in several pieces or empty. Forum reports (not
  used as sources) describe failures with messages, but the help says nothing.
  **Decision.**
  - Add fails with an error if the union is not one connected body ("the bodies do not
    touch").
  - Common fails with an error if the intersection is empty.
  - Subtract keeps every piece that is left, each as a body (`Combine1[1]`, `Combine1[2]`);
    it fails with an error if nothing of `M` is left.
  - Each input list must name at least two bodies for Add and Common, and at least one tool
    for Subtract. A body may not be both main body and tool.
- **Decision.** The result body is named after the Combine feature ("Combine1"). Its faces
  keep the names they had in the input bodies (AnvilCAD's face ids carry the feature that
  made each face), so fillets and other references on them survive.
- **Observed (S29, S30).** The API uses `swBodyOperationType_e` for
  `InsertCombineFeature` (`SWBODYADD` 15903, `SWBODYCUT` 15902, `SWBODYINTERSECT` 15901)
  and `swCombineBodiesOperationType_e` for `ICombineBodiesFeatureData::OperationType`
  (Add 0, Subtract 1, Common 2). For Subtract the main body is passed; for Add and Common it
  is null.

## Move/Copy Bodies

- **Observed (S14, S15).** Move/Copy Bodies moves, rotates or copies solid and surface
  bodies in a multibody part, or places them with mates. The selected bodies move together
  as one rigid group; the bodies not selected stay where they are. The PropertyManager has
  two pages: **Translate/Rotate** and **Constraints** (mates).
- **Observed (S14).** **Copy** makes copies instead of moving, with a **Number of Copies**.
  Clearing Copy moves the bodies. Copy applies to translate and rotate only.
- **Observed (S32).** In the API, translation and rotation cannot both be applied by one
  move/copy feature; if both are given, only the rotation is applied.
  `swMoveCopyBodyFeatureTransformType_e` has `swTransformType_None` 0,
  `swTransformType_Translation` 1 and `swTransformType_Rotation` 2. `TransformX/Y/Z` are in
  metres for a move and radians for a rotation.
- **Observed (S23, S33).** Default feature name: "Body-Move/Copy1".

### Translate

- **Observed (S14).** With no reference, **Delta X**, **Delta Y** and **Delta Z** give the
  move: every point `p` goes to `p + (Δx, Δy, Δz)`, in the part's coordinates. Each delta
  has a reverse-direction button (unavailable when the value is an equation).
- **Observed (S14, S16).** **Translation Reference** can be an edge or a vertex:
  - an edge gives a direction and a **Distance**: `p + d·u`, where `u` is the unit
    direction of the (straight) edge; a negative distance or the flip button reverses it;
  - a vertex `v1` with **To Vertex** `v2`: the bodies move by `v2 − v1`.
- **Observed (S14, S15).** The deltas, distance and angles accept equations and global
  variables.
- **Unknown.** Which way an edge's direction points (from which end). **Decision.** From the
  edge's start vertex to its end vertex as stored; the user flips it if needed.

### Rotate

- **Observed (S14).** **X/Y/Z Rotation Origin** set the point `c` the bodies turn about, and
  **X/Y/Z Rotation Angle** the angles about the X, Y and Z axes through it. Alternatively a
  **Rotation Reference** edge gives the axis and an **Angle**.
- **Observed (S14, S16).** The default rotation origin is the centre of the selected bodies'
  bounding box (2025). Older help (2012, 2014) says the centre of mass. **Decision.**
  AnvilCAD uses the bounding-box centre (the axis-aligned box of the selected bodies).
- **Observed (S14).** An **Euler's angular rotation** option replaces the three angles with
  **Yaw**, **Pitch** and **Roll**, which the help says give the same result whatever order
  they are entered in.
- **Unknown.** The order in which the X, Y and Z angles are applied, and the axis sequence of
  yaw, pitch and roll. **Decision.** AnvilCAD applies X first, then Y, then Z, each about
  the fixed part axes through `c`: `p' = c + Rz(γ)·Ry(β)·Rx(α)·(p − c)`, angles positive
  anticlockwise looking down the positive axis (right-hand rule). Yaw/pitch/roll are not
  implemented.
- **Decision.** About an edge: `p' = a + R(u, θ)·(p − a)`, where `a` is a point of the
  straight edge, `u` its unit direction as above, and `R` the right-hand rotation by `θ`.

### Copies

- **Unknown.** Where the copies go: whether copy `k` gets the transform once or `k` times.
  **Decision.** Copy `k` (for `k = 1 … N`) gets the transform applied `k` times
  (translation `k·Δ`, rotation `k·θ` about the same axis), so the copies form a row or ring
  like a pattern. The originals stay where they are.
- **Decision.** Copies are new bodies and are never merged with anything, even where they
  touch or overlap. They are named after the feature, with `[n]`.

### Constraints

- **Observed (S14, S4).** The Constraints page places the selected bodies with mates:
  coincident, parallel, perpendicular, tangent, concentric, distance and angle, each aligned
  or anti-aligned. Several mate sets may be applied to the same body and may conflict.
- **Not specified.** AnvilCAD leaves mate placement out (see Recommended scope).

## Delete/Keep Body

- **Observed (S17).** Delete/Keep Body takes a mode, **Delete Bodies** or **Keep Bodies**,
  and a list of bodies. Delete removes the listed bodies; Keep removes every body not
  listed.
- **Observed (S34).** `InsertDeleteBody2(KeepBodies)`: true keeps, false deletes.
  `IDeleteBodyFeatureData` has `Bodies` and `KeepBodies`.
- **Ambiguous (S17, S34).** The help says a "Keep Body" or "Delete Body" feature appears in
  the tree; the API example names the result "Body-Delete/Keep1". **Decision.** AnvilCAD
  names it "Delete Body1" or "Keep Body1" by mode.
- **Inferred.** The feature works at its place in the tree, so bodies made by later
  features are not affected. Keep mode with every body listed changes nothing.
- **Decision.** Deleting every body is allowed (the part is then empty, and later features
  that need a body fail as they would at the start of the tree). A listed body that no
  longer exists is a rebuild error on the feature.

## Split

- **Observed (S18).** Split divides bodies with **Trim Tools** (**Trimming Surfaces**):
  - reference planes and planar faces, extended without limit in all directions;
  - sketches, extruded through all in both directions (square to the sketch plane);
  - reference surfaces and non-planar faces, *not* extended; holes inside them are closed
    for the split.
- **Observed (S18).** In a single-body part, **Cut Part** splits the part. In a multibody
  part, **Target Bodies** is **All bodies** (every body the extended tools cut) or
  **Selected bodies** (**Solids or Surfaces to Split**), and **Cut Bodies** splits them.
- **Observed (S18).** **Resulting Bodies** lists the pieces. Each piece's check box controls
  only whether it is made into a separate body. Pieces given a **Title** are saved as
  separate part files (*Auto-assign Titles* names them `Body<n>.SLDPRT`); untitled pieces
  stay in the part. **Consume cut bodies** removes the saved pieces from the part.
  *Origin location*, property copying and template settings concern the saved files.
- **Observed (S19).** When trim tools are changed, SOLIDWORKS offers to reassign the new
  pieces to the previously saved ones and warns that the result may be wrong.
- **Inferred (S18).** A piece is a connected region of a target body after removing the
  tools' surfaces. A plane through a convex body gives two pieces; `n` parallel planes
  through a box give `n + 1`.
- **Ambiguous.** What an unchecked piece becomes: whether it stays joined to the other
  unchecked pieces as one body or stays as it was. **Decision.** AnvilCAD makes every
  piece a body (as if all were checked) and leaves saving to files out.
- **Decision.** Pieces are named "Split1[1]", "Split1[2]", numbered by the rule above. The
  new faces along the cut are named after the Split feature and the tool.
- **Observed (S36, S40).** `ISplitBodyFeatureData` (type name `Split`) has `Consume`,
  `TrimTools`, `GetSplitBodies`. A body saved to its own part reports type `SplitBody`.

## Intersect

- **Observed (S20).** Intersect takes solids, surfaces and planes. With **Intersect**, it
  makes a distinct region from each closed volume bounded by the selections. Choices:
  - **Create intersecting regions**: regions where the selections overlap one another;
  - **Create internal regions**: regions enclosed by the selections but empty (a hollow);
  - **Create both**.
- **Observed (S20).** *Cap planar openings on surfaces* closes flat openings of surfaces.
  **Regions to Exclude** lists the regions; at least one must be included. **Merge result**
  joins the included regions into one body where they touch, when possible; clear, each
  included region is a body. **Consume surfaces** removes the input surfaces.
- **Observed (S21).** Selecting an open-topped shell and a plane across its opening, with
  *Create internal regions*, gives the cavity as a new body ("Intersect[1]") and keeps the
  original as "Intersect[2]"; the cavity's volume can then be measured.
- **Observed (S37).** `PreIntersect2(CapPlanar, RegionType)` with region type 0
  (intersecting), 1 (internal) or 2 (both) returns the candidate bodies;
  `PostIntersect(IntersectionsToExclude, Merge, Consume)` makes the feature. The type
  name is `Sculpt`, data `IIntersectFeatureData`. Default feature name "Intersect1".
- **Inferred (S20).** For two solids `A` and `B`, the intersecting regions are the cells of
  their arrangement that lie inside at least one of them: `A − B`, `A ∩ B` and `B − A`
  (each split further into connected pieces). Including all of them with Merge result gives
  `A ∪ B`; including only `A ∩ B` gives Common. A plane among the selections divides the
  cells it crosses.
- **Inferred (S20, S21).** The input bodies are replaced by the included regions (the
  example keeps the original model as a region of the result).

## Scale

- **Observed (S24, S25).** Scale changes the size of the part's geometry, or of selected
  bodies in a multibody part (**Solid and Surface or Graphics Bodies to Scale**). It does
  not scale dimensions, sketches or reference geometry, and it does not change the
  definitions of earlier features; rolling back or suppressing it restores the original
  size.
- **Observed (S24).** **Scale About**: **Centroid** (computed by the system), **Origin**, or a
  user **Coordinate System**. **Uniform Scaling** applies one **Scale Factor**; cleared, X,
  Y and Z factors are given.
- **Observed (S24, S25).** The help's examples: scaling a part by 3 along X about the origin
  stretches it only to the right if all its material lies at positive X, and both ways if
  it straddles the origin.
- **Inferred (S24, S25).** Scale is the affine map `p' = c + A·S·Aᵀ·(p − c)`, where `c` is
  the scale point (centroid, part origin, or the coordinate system's origin), `A` the
  rotation whose columns are the reference axes (identity for Centroid and Origin, the
  coordinate system's axes otherwise) and `S = diag(sx, sy, sz)`. The help's one-sided
  example is this map, not a special rule. Volumes scale by `sx·sy·sz`; with a uniform
  factor `k`, areas by `k²` and lengths by `k`.
- **Inferred (S25).** Features after the Scale are built at the scaled size from their own
  (unscaled) sketches. A sketch drawn on a face of the scaled body follows the face, but
  its dimensions are not scaled.
- **Unknown.** Which centroid is used when only some bodies are scaled; the defaults for
  Scale About and the factor; whether a factor of zero or below is refused. **Decision.**
  The centroid is the volume centroid of the selected bodies taken together (all bodies when
  none are selected); the defaults are Centroid, uniform, factor 1; factors must be greater
  than zero.
- **Decision.** With no bodies selected, every body is scaled.
- **Inferred.** A non-uniform scale turns circles into ellipses, cylinders into elliptic
  cylinders and spheres into ellipsoids. A uniform scale keeps planes, cylinders, cones,
  spheres and tori of the same kind.
- **Observed (S35).** `InsertScale(Type, Uniform, Xscale, YScale, ZScale)`; `swScaleType_e`:
  `swScaleAboutCentroid` 0, `swScaleAboutOrigin` 1, `swScaleAboutCoordinateSystem` 2.
  `IScaleFeatureData` has `Type`, `IsUniform`, `ScaleUniform`, `ScaleX/Y/Z`,
  `CoordinateSystem`, `Bodies`. The example's feature is named by `swScaleFeat.Name`; a
  default name is not shown. **Decision.** "Scale1".

## Insert Part (derived part), briefly

- **Observed (S22, S23).** Insert Part brings another part into this one. The target becomes
  a multibody part and the inserted part a solid body. **Transfer** chooses what comes
  across: solid bodies, surface bodies, axes, planes, cosmetic threads, sketches, custom and
  cut-list properties, coordinate systems, model dimensions, hole data, sheet-metal data,
  body material and part material.
- **Observed (S22, S23, S38).** The part is inserted at the origin. *Locate part with
  Move/Copy feature* places it with a Move/Copy Body feature (with mates from its mate
  references). The link to the source part can be broken, and cannot be restored after.
- **Observed (S27).** Inserting a multibody part keeps each body's material assignment;
  bodies without one take the source part's material, but the source's part material is not
  copied.
- **Observed (S38, S40).** `IPartDoc::InsertPart3(FileName, Options, ConfigurationName)`
  with `swInsertPartOptions_e` bits (for example `swInsertPartImportSolids` 1,
  `swInsertPartImportSurfaces` 2, `swInsertPartBreakLink` 512). The feature's data is
  `IDerivedPartFeatureData` (type name `Stock`), whose `GetMoveFeature` returns its
  Move/Copy feature.
- **Inferred.** AnvilCAD's `ImportedBody` feature with `adds = true` already behaves like an
  unlinked Insert Part of exact geometry.

## Save Bodies, briefly

- **Observed (S18, S39).** Save Bodies writes chosen bodies to separate part files, optionally
  removes them from the part (*Consume*), and can build an assembly of them; identical bodies
  can be saved once and instanced. Type name `CreateAssemFeat`, data `ISaveBodyFeatureData`,
  created with `CreateSaveBodyFeature(Bodies, FilePaths, AssemName, ConsumeBody,
  CopyCustomProperty)`.
- **Decision.** Out of scope for now.

## Mass properties of a multibody part

- **Observed (S26).** With nothing selected, the Mass Properties dialog reports the whole
  multibody part; with bodies selected, only those (2012 wording). **Include hidden
  bodies/components** decides whether hidden bodies count. Results: density, mass, volume,
  surface area, centre of mass, principal axes and moments, moments and products of inertia.
- **Observed (S42).** In the API, without `AddBodies` all solid bodies of a part are used;
  with it, only the given bodies. Sheet bodies are ignored. `IMassProperty2` has
  `SelectedItems` and `IncludeHiddenBodiesOrComponents`. Moments are taken about the part
  origin unless a coordinate system is set. `IBody2::GetMassProperties(Density)` gives one
  body's centre of mass, volume, area, mass and moments.
- **Observed (S27).** A material can be set on the part, which every body takes, or on single
  bodies, which overrides the part's.
- **Inferred (S26, S27, S42).** The part's results are sums over the counted bodies:
  `V = Σ Vi`, `A = Σ Ai`, `m = Σ ρi·Vi` (each body with its own density), centre of mass
  `Σ mi·ci / m`, and the inertia tensor the sum of each body's tensor moved to the common
  centre of mass (parallel-axis theorem).
- **Inferred.** Since each body is measured on its own, two bodies that overlap count the
  overlap twice. The help does not say this directly.
- **Unknown.** What "density" shows when bodies have different materials. **Decision.**
  AnvilCAD shows `m / V`.

## Testable statements

Cubes below are 10 mm on a side unless stated. `A = [0,10]³`. `B = [5,15]×[0,10]×[0,10]`
(A moved 5 mm along x). `D = [20,30]×[0,10]×[0,10]` (clear of A). "Body" means a separate
entry in the body list. Statements marked (D) rest on an AnvilCAD decision above, and (I) on
an inference, rather than on a published statement.

1. A boss B with Merge result clear, on a part holding A, leaves 2 bodies; the part's
   reported volume is 2000 (the 500 mm³ overlap counted twice). (I)
2. The same boss with Merge result set leaves 1 body of volume 1500 and surface area 800.
3. A boss D (Merge result set) on a part holding A leaves 2 bodies, volume 2000, area 1200,
   centre of mass (15, 5, 5).
4. Bridging: with A and D as separate bodies, a boss `[5,25]×[2,8]×[2,8]` with Merge result
   set leaves 1 body of volume 1000 + 1000 + 720 − 2·180 = 2360.
5. Combine Common of A and B gives 1 body `[5,10]×[0,10]×[0,10]`, volume 500, area 400;
   the body count goes from 2 to 1.
6. Combine Add of A and B gives 1 body of volume 1500; count 2 → 1.
7. Combine Subtract with main body A and tool B gives `[0,5]×[0,10]×[0,10]`, volume 500;
   B is gone; count 2 → 1. With main body B and tool A: `[10,15]×[0,10]×[0,10]`, volume 500.
8. Combine Common of A, B and `C = [0,10]×[5,15]×[0,10]` gives `[5,10]×[5,10]×[0,10]`,
   volume 250.
9. Combine Common of A and D fails with an error; the part keeps A and D unchanged. (D)
10. Combine Add of A and D fails with an error (the union is not connected). (D)
11. Combine Subtract with main body `[0,30]×[0,10]×[0,10]` and tool
    `[14,16]×[−1,11]×[−1,11]` leaves 2 bodies of 1400 each, named "Combine1[1]"
    (x from 0 to 14) and "Combine1[2]". (D)
12. An extruded cut, slot `x ∈ [14,16]` through all, across the bar `[0,30]×[0,10]×[0,10]`
    leaves 2 bodies of volume 1400, listed "Cut-Extrude1[1]" (x from 0 to 14) and
    "Cut-Extrude1[2]" (numbering (D)); the folder reads "Solid Bodies(2)".
13. Feature scope: with A and D as bodies, a through-all cut of the rectangle
    `x ∈ [4,26], y ∈ [4,6]` (along z) leaves A and D at 880 each with scope All bodies,
    and A at 880 and D at 1000 with scope Selected bodies = {A}.
14. Move/Copy, translate Δ = (20, 0, 0), on A: A becomes `[20,30]×[0,10]×[0,10]`, volume
    1000, centre of mass (25, 5, 5); body count unchanged.
15. Move/Copy, translate by an edge along +x with distance 15: A's centre of mass goes to
    (20, 5, 5). Distance −15: (−10, 5, 5).
16. Move/Copy, vertex (0,0,0) To Vertex (10,10,0): A becomes `[10,20]×[10,20]×[0,10]`.
17. Move/Copy with Copy set, 2 copies, Δ = (20, 0, 0), on A: 3 bodies, at x ∈ [0,10],
    [20,30] and [40,50] (D); total volume 3000; centre of mass (25, 5, 5).
18. Move/Copy rotate, Z angle 90°, default origin, on the box `[0,10]×[0,20]×[0,30]`: the
    default origin is (5, 10, 15) and the box becomes `[−5,15]×[5,15]×[0,30]`.
19. The same rotation with origin (0, 0, 0): the box becomes `[−20,0]×[0,10]×[0,30]`.
20. Rotation order: on the box `[0,10]×[0,20]×[0,30]` about origin (0,0,0), X 90° then Z 90°
    (AnvilCAD's order) takes the point (10, 20, 30) to (30, 10, 20). (D)
21. Delete Body D from a part holding A and D leaves 1 body of volume 1000; Keep Body {D}
    leaves only D.
22. Split A with the plane x = 3 gives 2 bodies of volumes 300 and 700. With planes x = 3 and
    x = 7: 3 bodies of 300, 400 and 300.
23. Split the block `[−10,10]×[−10,10]×[0,10]` with a sketch circle of radius 5 at the origin
    on its top face: a cylinder of volume 250π = 785.398 and the rest, 4000 − 250π =
    3214.602. The total stays 4000.
24. Intersect A and B, intersecting regions: 3 regions of 500 each. All included, Merge
    result set: 1 body of 1500. All included, Merge result clear: 3 bodies, total 1500.
    Region `A ∩ B` excluded, Merge result set: 2 bodies of 500 (they do not touch).
25. Intersect with internal regions: a 10 mm cube shelled 1 mm with its top face removed,
    and a plane at its top face, gives a cavity body of 8 × 8 × 9 = 576; the shell body is
    1000 − 576 = 424.
26. Scale A about the origin, uniform 3: `[0,30]³`, volume 27000, area 5400.
27. Scale A about the origin, X factor 3 only: `[0,30]×[0,10]×[0,10]`, volume 3000. The cube
    `[−5,5]×[0,10]×[0,10]` scaled the same way becomes `[−15,15]×[0,10]×[0,10]`.
28. Scale A about its centroid, uniform 2: `[−5,15]³`, volume 8000, centre of mass still
    (5, 5, 5).
29. Scale A about a coordinate system at (10, 0, 0) with the part's axes, uniform 2:
    `[−10,10]×[0,20]×[0,20]`.
30. Scale a cylinder of radius 5 and height 10, standing on the origin with its axis along z,
    about the origin, by X factor 2 (Y and Z factors 1): an elliptic cylinder of volume π·10·5·10 = 500π = 1570.796.
31. Scale leaves sketch dimensions alone: after scaling A by 2, the dimension "10" of A's
    sketch still reads 10.
32. Mass: bodies A (body material 7.85 g/cm³) and D (part material 2.70 g/cm³): mass
    10.55 g, volume 2000, centre of mass x = (7.85·5 + 2.70·25) / 10.55 = 10.1185 mm,
    y = z = 5. Mass of D alone: 2.70 g, centre of mass (25, 5, 5).
33. A part made of A and D, no selection: volume 2000. Selecting only A: volume 1000.
34. Feature names: the first Combine is "Combine1" and the first Move/Copy
    "Body-Move/Copy1"; the first Intersect is "Intersect1".

## API names

For `anvil-migrate`, which reads a user's own licensed SOLIDWORKS through its published API.
Names and values only.

- **Bodies.** `IPartDoc::GetBodies2(BodyType, BVisibleOnly)` with `swBodyType_e`
  (`swSolidBody` 0, `swSheetBody` 1, `swAllBodies` −1, others not used); order is not
  guaranteed. `IBodyFolder::GetBodies` (tree order). `IBody2::Name`,
  `IBody2::GetMassProperties`, `IBody2::GetFeatures` (sheet metal only).
  `IFeature::GetBody` is obsolete; use `IFeature::GetFaces` and `IFace2::GetBody` to find
  the bodies a feature touched.
- **Folders.** Type names `SolidBodyFolder`, `SurfaceBodyFolder`, `FeatSolidBodyFolder`,
  `FeatSurfaceBodyFolder` (data `IBodyFolder`).
- **Merge and scope.** `IFeatureManager::FeatureExtrusion3` arguments `Merge`,
  `UseFeatScope`, `UseAutoSelect`; `IExtrudeFeatureData2::Merge` (default true),
  `FeatureScope`, `AutoSelect`, `FeatureScopeBodies`.
- **Combine.** `IFeatureManager::InsertCombineFeature(OperationType, MainBody, ToolVar)`;
  `swBodyOperationType_e`: `SWBODYADD` 15903, `SWBODYCUT` 15902, `SWBODYINTERSECT` 15901.
  `ICombineBodiesFeatureData`: `OperationType` (`swCombineBodiesOperationType_e`:
  `swCombineBodiesOperationAdd` 0, `swCombineBodiesOperationSubtract` 1,
  `swCombineBodiesOperationCommon` 2), `MainBody`, `BodiesToCombine`. Type name
  `CombineBodies`.
- **Move/Copy.** `IFeatureManager::InsertMoveCopyBody2(TransX, TransY, TransZ, TransDist,
  RotPointX, RotPointY, RotPointZ, RotAngleX, RotAngleY, RotAngleZ, BCopy, NumCopies)`
  (metres, radians; selection marks 1 body, 2 rotation reference, 4 translation reference,
  8 translation vertex). `IMoveCopyBodyFeatureData`: `Bodies`, `Copy`, `NumberOfCopies`,
  `TransformType` (`swMoveCopyBodyFeatureTransformType_e`: `swTransformType_None` 0,
  `swTransformType_Translation` 1, `swTransformType_Rotation` 2), `TransformX/Y/Z`,
  `TransformValue`, `TransformReferenceEntity`, `TranslateToVertex`,
  `RotationOriginX/Y/Z`, `AddMate`. Type name `MoveCopyBody`.
- **Delete/Keep.** `IFeatureManager::InsertDeleteBody2(KeepBodies)` (older
  `InsertDeleteBody`). `IDeleteBodyFeatureData`: `Bodies`, `KeepBodies`. Type name
  `DeleteBody`.
- **Split.** `IFeatureManager::PreSplitBody2`, `PostSplitBody2(BodiesToMark, ConsumeCut,
  Origins, SavePaths, OverrideTemplateName)` (older `PreSplitBody`, `PostSplitBody`).
  `ISplitBodyFeatureData`: `Consume`, `TrimTools`, `GetSplitBodies`, `SetSplitBodies2`.
  Type names `Split` (the feature) and `SplitBody` (a body saved to its own part).
- **Intersect.** `IFeatureManager::PreIntersect2(CapPlanar, RegionType)` (0 intersecting,
  1 internal, 2 both), `PostIntersect(IntersectionsToExclude, Merge, Consume)`.
  `IIntersectFeatureData`: `CapPlanar`, `Consume`, `Merge`, `GetIntersections`,
  `GetIntersectionsTools`, `SetIntersections`. Type name `Sculpt`.
- **Scale.** `IFeatureManager::InsertScale(Type, Uniform, Xscale, YScale, ZScale)`;
  `swScaleType_e`: `swScaleAboutCentroid` 0, `swScaleAboutOrigin` 1,
  `swScaleAboutCoordinateSystem` 2. `IScaleFeatureData`: `Type`, `IsUniform`,
  `ScaleUniform`, `ScaleX/Y/Z`, `CoordinateSystem`, `Bodies`, `GetScale`, `SetScale`.
  Type name `Scale`.
- **Insert Part.** `IPartDoc::InsertPart3(FileName, Options, ConfigurationName)` with
  `swInsertPartOptions_e`; `IDerivedPartFeatureData` (`ImportSolidBodies`, `ImportSurf`,
  `PartConfiguration`, `PathName`, `GetMoveFeature`). Type name `Stock`. The table also
  lists `XformStock` with `DerivedPartFeatureData`, without saying what it is.
- **Save Bodies.** `IFeatureManager::CreateSaveBodyFeature`; `ISaveBodyFeatureData`. Type
  name `CreateAssemFeat`.
- **Mass properties.** `IModelDocExtension::CreateMassProperty` / `CreateMassProperty2`;
  `IMassProperty::AddBodies`, `Volume`; `IMassProperty2`: `SelectedItems`,
  `IncludeHiddenBodiesOrComponents`, `Mass`, `Volume`, `SurfaceArea`, `CenterOfMass`,
  `Density`, `PrincipalMomentsOfInertia`.
- **Note.** `anvil-migrate` (`crates/anvil-migrate/src/export.rs`) today captures bodies with
  `GetBodies2` for solid bodies, visible only, so hidden bodies are not carried across.

## AnvilCAD today

- The exact kernel's build state keeps a list of bodies (`BuildState::exact:
  Option<Vec<anvil_brep::Solid>>`). `exact::combine` applies a boss by uniting it with every
  body whose bounding box it touches and that the union actually joins; a cut is subtracted
  from every body it touches, and a body a cut severs becomes several. That is the
  "Merge result set, scope All bodies" behaviour, with no way to turn either off.
- The faceted kernel holds one polygon soup (`BuildState::body: Solid`). A boss is united
  into it and a cut subtracted from all of it, so separate bodies are not tracked; once a
  part falls back to the faceted kernel, the body count is lost. Imported bodies with
  `adds` are appended to the soup.
- Mass properties (`BuildState::mass_report`) come from the faceted soup with one part
  material. Summing signed volumes over a soup of closed shells already gives `Σ Vi` (an
  overlap counted twice), so statements 1 and 3 hold today for volume; per-body materials
  and per-body reports need a body list.
- Bodies have no names or identities. Faces are named by the feature that made them.
- `anvil_brep::boolean` has `Union`, `Subtract` and `Intersect`; the faceted kernel has
  `union`, `subtract` and `intersect`. Both kernels transform solids only rigidly
  (`transformed(&Frame)`). The exact surface types (plane, cylinder, cone, sphere, torus,
  B-spline) are closed under rigid motion and uniform scale, but not under non-uniform
  scale except through B-splines.

## Recommended scope

In order:

1. **A body list in both kernels.** Make the faceted build state a list of solids too (the
   single soup becomes a view built from it for display), and give each body an identity
   (feature, `[n]`) and a display name as decided above, with the fixed piece numbering.
   Show "Solid Bodies(n)" in the tree. Every later item depends on this.
2. **Merge result and scope on bosses and cuts.** Add `merge: bool` (serde default true)
   to extrudes and revolves, and a scope (`All` default, or a list of bodies) to bosses and
   cuts. Defaults keep every existing document building exactly as now.
3. **Mass properties per body.** Total and per-body volume, area, mass and centre of mass;
   a body material that overrides the part material; a selection of bodies to report.
4. **Combine** (Add, Subtract, Common) with the errors above, on the exact kernel first,
   faceted as the fallback.
5. **Delete/Keep Body.**
6. **Move/Copy Bodies**: translate by deltas, along an edge, vertex to vertex; rotate by
   X/Y/Z angles about a point, or about an edge; copies. No mates.
7. **Split** by planes, planar faces and sketches (closed profiles as prisms, open curves as
   through-all sheets); every piece kept as a body.
8. **Scale**: uniform first (exact), then non-uniform (B-spline surfaces or the faceted
   kernel).
9. **Intersect** of solids and planes, with region exclusion and Merge result.

Leave out for now: the Constraints (mates) page of Move/Copy; Euler yaw/pitch/roll;
configuration-specific values; Save Bodies and saving split pieces to files, assemblies
from bodies, and the related templates and property copying; linked Insert Part (use the
existing import with `adds`); body sub-folders, feature history in the folder, hide, show
and isolate (user interface, later); surface and graphics bodies; Bodies to Keep beyond
"all pieces"; non-planar split tools; weldment cut lists.

## Unknown or not covered

1. The order of the X, Y and Z rotation angles in Move/Copy, and the sequence behind
   yaw/pitch/roll.
2. Where the copies of a Move/Copy go when there are several (once or `k` times the
   transform).
3. How SOLIDWORKS numbers the `[n]` pieces of one feature, and whether the numbers are
   stable across rebuilds.
4. What Combine does with a disconnected Add, an empty Common, or a Subtract that splits or
   empties the main body, and the wording of its errors.
5. Whether face-to-face contact merges bodies.
6. What an unchecked piece of a Split becomes.
7. Which centroid Scale uses for a selection of bodies; the defaults of Scale; whether
   factors of zero or below are refused.
8. Whether the default name of a Delete/Keep feature is "Body-Delete/Keep1" or
   "Delete Body1" / "Keep Body1" (the help and the API example disagree); the default names
   of Split and Scale features.
9. The rule that an unrenamed body takes the last feature's name rests on API examples and
   one non-Dassault article, not on a help page.
10. The meaning of the `XformStock` feature type.
11. How bodies and body features are stored in `.sldprt` files (not studied).
