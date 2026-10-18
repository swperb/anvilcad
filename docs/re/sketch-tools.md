# Sketch tools and relations

What AnvilCAD's sketch editing tools and sketch relations should do. The tools are the
ones that change existing sketch geometry rather than draw new entities: fillet, chamfer,
offset, convert (project model edges), mirror, linear and circular pattern, move, copy,
rotate, scale, trim, extend, split and the construction toggle. The relations are the
geometric constraints a user adds between sketch entities. SOLIDWORKS users know all of
these well, so AnvilCAD follows their published behaviour where it makes sense. This
specification was written before the implementation, for `crates/anvil-sketch` (geometry,
constraints, solver) and `crates/anvil-app/src/sketcher.rs` (commands).

## How this was studied

- **Inputs.** Only published information was used:
  - Dassault's public online help for SOLIDWORKS 2025, with 2016, 2015 and 2012 pages
    where they word an option differently or record when it was added;
  - the public SOLIDWORKS 2024 API help;
  - one public post on Dassault's SOLIDWORKS blog (N1).

  No SOLIDWORKS software, installer, library or file was downloaded, installed, run or
  analysed. No SOLIDWORKS licence was accepted on the machine used.
- **Retrieval.** The help site renders its articles with JavaScript and refuses plain
  requests. Each article was fetched over HTTPS with a browser user agent, and its text
  read from the page's embedded `__NEXT_DATA__` JSON
  (`props.pageProps.helpContentData.helpText`), with HTML stripped by a script run with
  `python3 -I`. Link targets were read from the same JSON to find related pages. A
  missing page still returns HTTP 200, with "page cannot be found" in its data, so every
  page's content was checked. The text was paraphrased; none of it is copied into this
  repository. Many help pages explain an option only with a picture. Pictures were not
  read, and where a behaviour is shown only in a picture this specification says so.
- **REA.** REA (`rea-agents` 6.5.0, with the `third_party/rea` submodule at `3fc0075`) was
  set up and its research method followed:
  - each finding below is marked **Observed** (stated in a cited source), **Inferred**
    (our reasoning from the sources, or plane geometry), **Decision** (AnvilCAD's own
    choice where the sources are silent, with the reason) or **Unknown**;
  - every observation cites its source.

  REA's analysis tools were **not** run. They inspect artifacts (binaries, packages,
  captures, files), and the only lawful inputs for this behaviour are prose documents.
  Pointing REA at SOLIDWORKS software is ruled out by
  [INTEROPERABILITY.md](../../INTEROPERABILITY.md). No `.sldprt` file was studied; how
  SOLIDWORKS stores sketch relations on disk is out of scope here.
- **Not studied.** Splines, ellipses, parabolas, conics and slots; 3D sketches; sketches
  in drawings; blocks; assembly-context (in-context) references; SketchXpert; Stretch
  Entities beyond its options; the Segment tool.

## Sources

Base A is `https://help.solidworks.com/2025/English/SolidWorks/sldworks/`. Base B is
`https://help.solidworks.com/2024/English/api/sldworksapi/SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.`
Base C is
`https://help.solidworks.com/2024/English/api/swconst/SOLIDWORKS.Interop.swconst~SOLIDWORKS.Interop.swconst.`
All were read on 2026-10-18.

| # | Topic | URL | Version |
|---|---|---|---|
| S1 | Sketch fillets (overview) | A + `c_Sketch_Fillets.htm` | 2025 |
| S2 | Sketch Fillet PropertyManager | A + `HIDD_DVE_SKETCH_FILLET.htm` (same text at `https://help.solidworks.com/2012/English/SolidWorks/Sldworks/HIDD_DVE_SKETCH_FILLET.htm`) | 2025, 2012 |
| S3 | Sketching sketch fillets | A + `t_Sketching_Sketch_Fillets.htm` | 2025 |
| S4 | Sketch chamfers (overview) | A + `c_Sketch_Chamfers.htm` | 2025 |
| S5 | Sketch Chamfer PropertyManager | A + `HIDD_DVE_SKETCH_CHAMFER.htm` | 2025 |
| S6 | Sketching sketch chamfers | A + `t_Sketching_Sketch_Chamfers.htm` | 2025 |
| S7 | Virtual sharps | A + `t_Creating_Virtual_Sharps.htm` | 2025 |
| S8 | Offset entities (overview) | A + `c_Offset_Entities.htm` | 2025 |
| S9 | Offset Entities PropertyManager | A + `HIDD_OFFSET1.htm` (same at 2018) | 2025 |
| S10 | Offset Entities PropertyManager, older wording ("Make base construction") | `https://help.solidworks.com/2015/ENGLISH/SolidWorks/sldworks/HIDD_OFFSET1.htm` and `https://help.solidworks.com/2012/English/SolidWorks/Sldworks/HIDD_OFFSET1.htm` | 2015, 2012 |
| S11 | Using offset entities (offset relation) | A + `t_Using_Offset_Entities.htm` | 2025 |
| S12 | Cap ends for offset entities | A + `c_Capping_to_Single_Direction_Offset.htm` | 2025 |
| S13 | Changing the offset size | A + `t_Changing_the_Offset_Size.htm` | 2025 |
| S14 | Reversing sketch offsets | A + `t_Reversing_Sketch_Offsets.htm` | 2025 |
| S15 | What's New 2016: sketching; cap ends; inner loops for convert | `https://help.solidworks.com/2016/english/WhatsNew/c_Sketching.htm`, `.../c_Capping_to_Single_Direction_Offset.htm`, `.../c_Convert_Entities_with_Internal_Loops.htm` | 2016 |
| S16 | Convert entities (overview) | A + `c_Convert_Entities.htm` | 2025 |
| S17 | Convert Entities PropertyManager | A + `HIDD_DVE_SKETCH_CONVERT_ENTITIES.htm` | 2025 |
| S18 | Using convert entities (On Edge and internal Fixed relations) | A + `t_Using_Convert_Entities.htm` | 2025 |
| S19 | Selecting silhouette edges | A + `t_Selecting_Silhouette_Edges.htm` | 2025 |
| S20 | Mirror entities (overview, symmetric relations) | A + `c_Mirror_Entities.htm` | 2025 |
| S21 | Mirror PropertyManager | A + `HIDD_DVE_SKETCH_MIRRORING.htm` | 2025 |
| S22 | Mirroring existing sketch entities | A + `t_Mirroring_Existing_Sketch_Entities.htm` | 2025 |
| S23 | Dynamic mirror | A + `HIDD_DVE_SKETCH_DYNAMIC_MIRRORING.htm` | 2025 |
| S24 | Sketch patterns (overview) | A + `c_Sketch_Patterns.htm` | 2025 |
| S25 | Linear Sketch Pattern PropertyManager | A + `HIDD_DVE_SKETCH_PATTERN_LINEAR.htm` | 2025 |
| S26 | Circular Sketch Pattern PropertyManager | A + `HIDD_DVE_SKETCH_PATTERN_CIRCULAR.htm` | 2025 |
| S27 | Creating linear sketch patterns | A + `t_Creating_Linear_Sketch_Patterns.htm` | 2025 |
| S28 | Creating circular sketch patterns | A + `t_Creating_Circular_Sketch_Patterns.htm` | 2025 |
| S29 | Editing sketch patterns; skipping instances | A + `t_Editing_Sketch_Patterns.htm`, A + `t_Skipping_Pattern_Instances.htm` | 2025 |
| S30 | Move-Copy-Rotate-Scale-Stretch (overview and PropertyManager) | A + `c_Move_Copy_Rotate_Scale_or_Stretch_Entities.htm`, A + `HIDD_DVE_MOVE_COPY.htm` | 2025 |
| S31 | Move and Copy PropertyManagers | A + `r_Move_PropertyManager.htm`, A + `r_Copy_PropertyManager.htm` | 2025 |
| S32 | Moving or copying sketch entities | A + `t_Moving_or_Copying_Sketch_Entities.htm` | 2025 |
| S33 | Rotate PropertyManager; rotating entities | A + `r_Rotate_PropertyManager.htm`, A + `t_Rotating_Sketch_Entities.htm` | 2025 |
| S34 | Scale PropertyManager; scaling entities | A + `r_Scale_PropertyManager.htm`, A + `t_Scaling_Sketch_Entities.htm` | 2025 |
| S35 | Stretching entities | A + `t_Stretching_Sketch_Entities.htm` | 2025 |
| S36 | Trim entities (overview) | A + `c_Trim_Entities.htm` | 2025 |
| S37 | Trim PropertyManager | A + `HIDD_TRIM_CHOICES.htm` | 2025 |
| S38 | Trim tasks: power trim, extend with power trim, corner, trim away inside, trim away outside, trim to closest | A + `t_Trimming_with_Power_Trim.htm`, `t_Extending_with_Power_Trim.htm`, `t_Trimming_with_Corner.htm`, `t_Trimming_with_Trim_Away_Inside.htm`, `t_Trimming_with_Trim_Away_Outside.htm`, `t_Trimming_with_Trim_to_Closest.htm` | 2025 |
| S39 | Extend entities | A + `t_Extend_Entities.htm` | 2025 |
| S40 | Split entities | A + `c_split_entities.htm` | 2025 |
| S41 | Construction geometry | A + `c_Construction_Geometry.htm` | 2025 |
| S42 | Sketch tools index | A + `c_sketch_tools_top.htm` | 2025 |
| S43 | Sketch relations overview | A + `c_Sketch_Relations_Overview.htm` | 2025 |
| S44 | Description of sketch relations (the relation table) | A + `c_Description_of_Sketch_Relations.htm` | 2025 |
| S45 | Sketch relations icons | A + `c_Sketch_Relations_Icons.htm` | 2025 |
| S46 | Geometric relations (what may be related) | A + `c_Geometric_Relations.htm` | 2025 |
| S47 | Add Relations/Properties PropertyManager | A + `HIDD_DVE_SKETCH_DEFAULT.htm` | 2025 |
| S48 | Display/Delete Relations PropertyManager | A + `HIDD_DVE_SK_EDIT_RELATIONS.htm` | 2025 |
| S49 | FAQ: sketch relations | A + `c_FAQ_Sketch_Relations.htm` | 2025 |
| S50 | Inferencing; automatic relations; relations/snaps options | A + `c_Inferencing.htm`, A + `t_Using_Automatic_Relations.htm`, A + `r_relations_snaps_options.htm` | 2025 |
| S51 | Equal curve length; equidistant relations | A + `c_Equal_Curve_Length_Parametric_Relations.htm`, A + `c_equidistant_relations.htm` | 2025 |
| S52 | Relations index; areas relying on relations; callouts; external vs in-context; horizontal/vertical hints | A + `c_relations_top.htm`, `c_Areas_that_Rely_on_Sketch_Relations.htm`, `c_Sketch_Relations_Callouts.htm`, `c_External_vs_Defined_in_Context.htm`, `c_horizontal_vertical_sketch_relations.htm` | 2025 |
| B1 | `ISketchManager::CreateFillet` | B + `ISketchManager~CreateFillet.html` | 2024 |
| B2 | `IModelDoc2::SketchFillet2` | B + `IModelDoc2~SketchFillet2.html` | 2024 |
| B3 | `swConstrainedCornerAction_e` | C + `swConstrainedCornerAction_e.html` | 2024 |
| B4 | `ISketchManager::CreateChamfer` | B + `ISketchManager~CreateChamfer.html` | 2024 |
| B5 | `swSketchChamferType_e` | C + `swSketchChamferType_e.html` | 2024 |
| B6 | `ISketchManager::SketchOffset2` | B + `ISketchManager~SketchOffset2.html` | 2024 |
| B7 | `swSkOffsetCapEndType_e` | C + `swSkOffsetCapEndType_e.html` | 2024 |
| B8 | `swSkOffsetMakeConstructionType_e` | C + `swSkOffsetMakeConstructionType_e.html` | 2024 |
| B9 | `IModelDoc2::SketchOffsetEntities2` | B + `IModelDoc2~SketchOffsetEntities2.html` | 2024 |
| B10 | `ISketchManager::SketchUseEdge3` | B + `ISketchManager~SketchUseEdge3.html` | 2024 |
| B11 | `IModelDoc2::SketchMirror` | B + `IModelDoc2~SketchMirror.html` | 2024 |
| B12 | `ISketchManager` members (dynamic mirror, construction, split, patterns, `ConvertEntities` not implemented) | B + `ISketchManager_members.html` | 2024 |
| B13 | `ISketchManager::CreateLinearSketchStepAndRepeat` | B + `ISketchManager~CreateLinearSketchStepAndRepeat.html` | 2024 |
| B14 | `ISketchManager::CreateCircularSketchStepAndRepeat` | B + `ISketchManager~CreateCircularSketchStepAndRepeat.html` | 2024 |
| B15 | `IModelDocExtension::MoveOrCopy`, `RotateOrCopy`, `ScaleOrCopy` | B + `IModelDocExtension~MoveOrCopy.html` (and `~RotateOrCopy`, `~ScaleOrCopy`) | 2024 |
| B16 | `ISketchManager::SketchTrim` | B + `ISketchManager~SketchTrim.html` | 2024 |
| B17 | `swSketchTrimChoice_e` | C + `swSketchTrimChoice_e.html` | 2024 |
| B18 | `ISketchManager::SketchExtend` | B + `ISketchManager~SketchExtend.html` | 2024 |
| B19 | `ISketchManager::SplitOpenSegment`, `SplitClosedSegment` | B + `ISketchManager~SplitOpenSegment.html`, B + `ISketchManager~SplitClosedSegment.html` | 2024 |
| B20 | `ISketchSegment::ConstructionGeometry` | B + `ISketchSegment~ConstructionGeometry.html` | 2024 |
| B21 | `IModelDoc2::SketchAddConstraints` (relation name strings) | B + `IModelDoc2~SketchAddConstraints.html` | 2024 |
| B22 | `swConstraintType_e` | C + `swConstraintType_e.html` | 2024 |
| B23 | `ISketchRelationManager::AddRelation`; `ISketchRelation`; `ISketchRelation::GetRelationType` | B + `ISketchRelationManager~AddRelation.html`, B + `ISketchRelation.html`, B + `ISketchRelation~GetRelationType.html` | 2024 |
| B24 | `swSketchRelationFilterType_e` | C + `swSketchRelationFilterType_e.html` | 2024 |
| N1 | Virtual sharps in sketches (SOLIDWORKS blog, Dassault) | `https://blogs.solidworks.com/products/solidworks/solidworks-how-to-create-virtual-sharps-in-sketches` | 2019 post |

## General

- **Observed (S42).** All of these tools live under the sketch tools menu and work in an
  open sketch.
- **Inferred (S2, S5, S9, S21, S25, S26, S30, S37).** None of the PropertyManagers names a
  feature that the tool creates. The tools edit the sketch's own entities and relations;
  the only thing in the feature tree is the sketch itself ("Sketch1"). A sketch pattern
  is re-edited from one of its instances, not from the tree (S29).
- **Decision.** The same in AnvilCAD: every tool is an edit of one `SketchFeature`, undone
  as one step. No new `FeatureKind` is added.
- **Observed (S44).** A relation to a line acts on the infinite line through it, and a
  relation to an arc acts on its full circle. A relation to something not in the sketch
  plane acts on its projection onto the plane.
- **Inferred.** The tools that trim, extend or fillet therefore work on the curves an
  entity lies on (infinite line, full circle), not only on the drawn segment. AnvilCAD's
  existing `PointOnLine`, `PointOnCircle` and `Tangent` already act on the infinite line
  and the full circle, which matches.

## Sketch Fillet

### Inputs

- **Observed (S1).** The tool cuts away the corner where two sketch entities meet and puts
  in an arc tangent to both.
- **Observed (S3).** The user picks two entities or one corner (a shared end point). The
  entities do not need to touch: they are extended to their virtual intersection and that
  corner is filleted.
- **Observed (S3).** A preview shows on hover and can be dragged to change the size before
  accepting.
- **Observed (S2).** Options:

  | Option | Meaning |
  |---|---|
  | "Radius" | The fillet radius. |
  | "Keep constrained corners" | If the corner has dimensions or relations, keep a point at the virtual intersection so they survive. |
  | "Dimension each fillet" | Give every fillet its own radius dimension. When cleared, the fillets are tied together with Equal relations. |

- **Observed (S2).** Fillets made in one go with the same radius are not each dimensioned:
  later ones get an automatic Equal relation to the first fillet of the series.
- **Inferred (S2).** So, with "Dimension each fillet" cleared, the first fillet of the run
  gets one radius dimension and each later fillet gets Equal to it. With it selected,
  each fillet gets its own radius dimension of the same value.
- **Unknown.** The default radius and the default state of the two check boxes.

### The corner point and its dimensions

- **Observed (S2, B3).** When the corner carries dimensions or relations:
  - with "Keep constrained corners", a point is kept at the virtual intersection and the
    dimensions and relations stay on it;
  - without it, the user is asked whether to delete them.

  The API spells out four actions (B3): ask the user (0), keep by making a virtual
  intersection point (1), delete the relation or dimension and fillet (2), or do nothing
  (3). B1 and B2 say the choice is ignored when the corner is unconstrained.
- **Observed (S7, N1).** This kept point is a "virtual sharp": a sketch point at the place
  two entities would intersect. Dimensions and relations to it remain after a fillet or
  chamfer removes the real corner. A user can also make one by selecting two entities and
  clicking the Point tool.
- **Inferred.** The kept point must stay at the intersection of the two (infinite) curves
  as they move, so it is held by the equivalent of an Intersection relation: on both
  lines.
- **Inferred.** When the corner is unconstrained, nothing needs to survive and the corner
  point simply disappears with the trimmed-away ends.
- **Decision.** AnvilCAD keeps the old corner point (same `PointId`, so every dimension and
  relation on it is untouched). It stops being an end point of the two entities, which
  get new end points at the tangent points, and gains two constraints that hold it on both
  curves (`PointOnLine` for a line, `PointOnCircle` for an arc). It is drawn as a virtual
  sharp. The user may delete it. If the corner is unconstrained, the point is removed.
  AnvilCAD does not prompt: keeping is always safe, and the point can be deleted.

### Geometry

- **Inferred (plane geometry).** For two lines meeting at interior angle θ (0 < θ < 180°):
  - the arc centre lies on the bisector, at r / sin(θ/2) from the corner;
  - each tangent point is set back t = r / tan(θ/2) from the corner along its line;
  - the arc sweeps 180° − θ;
  - the area cut away is r·t − (π − θ)·r²/2. At θ = 90° that is r² − πr²/4.
- **Inferred.** The fillet lies in the angle the picks select: of the four angles two
  crossing lines make, the one containing the picked parts of both entities, or for a
  corner pick, the angle between the two entities at that corner.
- **Inferred.** Line–arc and arc–arc fillets: the fillet centre is at r from the line (on
  the chosen side) and at R + r (outside the arc) or R − r (inside) from the arc's centre.
  The tangent point on an arc is where the line from the arc's centre to the fillet
  centre crosses it.
- **Inferred (S1).** The result is: each entity trimmed back to its tangent point, a new
  arc between the tangent points, the arc tangent to both, and its end points shared
  with the trimmed entities.
- **Decision.** AnvilCAD adds `Tangent` between the arc and each line (a new arc–arc
  tangent relation for arcs; see Relations), shares the end points (one `PointId`), and
  adds the radius dimension or the Equal relation as above.
- **Unknown.** What happens when the radius is too large for the entities (tangent points
  past the far ends), for parallel lines, and for two lines already tangent. **Decision.**
  Refuse with a message and change nothing. The set-back must not exceed the length of
  either entity's part on the corner side.

## Sketch Chamfer

- **Observed (S4, S6).** The tool chamfers two adjacent sketch entities. The user picks two
  entities or a corner (vertex). The chamfer is applied at once, and a sequence of
  chamfers can be undone in reverse order.
- **Observed (S5, B4, B5).** Three types:

  | Type (UI) | Parameters | API value |
  |---|---|---|
  | "Angle-distance" | "Distance 1", applied to the first entity picked; "Direction 1 Angle", measured from the first entity toward the second | `swSketchChamfer_DistanceAngle` = 0 |
  | "Distance-distance" with "Equal distance" cleared | "Distance 1" on the first entity, "Distance 2" on the second | `swSketchChamfer_DistanceDistance` = −1 |
  | "Distance-distance" with "Equal distance" selected | "Distance 1" on both entities | `swSketchChamfer_DistanceEqual` = 2 |

- **Inferred (S5).** A distance is measured along its entity from the (real or virtual)
  corner to the chamfer's end point. The chamfer is a straight line between those two
  points, and both entities are trimmed back to them.
- **Inferred (S5).** For angle-distance, the angle is the angle at the chamfer's end on the
  first entity, between the first entity (pointing back toward the corner) and the
  chamfer line. With corner angle θ, distance d and angle α, the distance along the
  second entity is d·sin α / sin(180° − α − θ). At θ = 90° that is d·tan α.
  **Unknown.** The help shows this only in a picture; whether the angle is taken at the
  first entity's end of the chamfer or at the corner is not stated in words.
- **Observed (S7).** A chamfer, like a fillet, can leave a virtual sharp; S7 says they appear
  automatically in 3D sketches.
- **Unknown.** Whether a 2D sketch chamfer keeps the corner point or offers "Keep
  constrained corners"; which dimensions it adds; the defaults; whether arcs can be
  chamfered (S4 says "sketch entities", not lines).
- **Decision.** AnvilCAD chamfers two lines only, at first. It treats the corner exactly as
  the fillet does (keeps a constrained corner as a virtual sharp, removes an unconstrained
  one). It adds driving dimensions for the chamfer's parameters: for distance-distance,
  the distance from the virtual sharp to each chamfer end (or one distance and an Equal
  between the two distances when equal); for angle-distance, the distance and the angle
  between the chamfer and the first line. When there is no virtual sharp to measure
  from, it adds the chamfer line with coincident ends and no dimensions.

## Offset Entities

### Inputs and options

- **Observed (S8, S9, S11).** Input is one or more sketch entities, a model face or a model
  edge. Finite lines, arcs and splines can be offset. Fit splines, splines that were
  themselves offset, and offsets that would intersect themselves cannot be made.
- **Observed (S9; older names from S10; enumerations from B6–B8).** Options:

  | Option | Meaning | API |
  |---|---|---|
  | "Offset Distance" | The distance. Dragging in the view previews it; the click that ends the drag finishes the offset, so options must be set first. | `Offset`; a negative value offsets the other way |
  | "Add dimensions" | Put the offset distance in the sketch as a dimension. Dimensions on the original are not touched. | `AddDimensions` |
  | "Reverse" | Flip the side of a one-directional offset. | sign of `Offset` |
  | "Select chain" | Offset every entity joined end to end with the selection. | `Chain` |
  | "Bi-directional" | Offset to both sides. | `BothDirections` |
  | "Construction geometry": "Base geometry", "Offset geometry" | Turn the originals, the new entities, or both into construction geometry. Before 2016 this was one box, "Make base construction". | `MakeConstruction`: `swSkOffsetDontMakeConstruction` 0, `swSkOffsetMakeOrigConstruction` 1, `swSkOffsetMakeOffsConstruction` 2, `swSkOffsetMakeBothConstruction` 3 |
  | "Cap ends": "Arcs", "Lines" | Close the ends of an offset of an open chain. | `CapEnds`: `swSkOffsetNoCaps` 0, `swSkOffsetArcCaps` 1, `swSkOffsetLineCaps` 2 |

- **Observed (S9, S12, S15).** Cap ends apply to open chains. With "Bi-directional" they
  join the two offsets' ends; one-directional caps (added in 2016) join the original's
  ends to the offset's ends. Arc or line caps can be chosen.
- **Unknown.** The defaults for every option, and which side a new one-directional offset
  goes to before the user drags. The help shows cap shapes only in pictures.

### Relation and dimension

- **Observed (S11).** An offset relation joins each new entity to the entity it came from.
  When the original changes, the offset follows. In an assembly context with external
  references turned off, the relation is not made.
- **Observed (S45, B22).** The relation is "Offset"; its icon example is a larger rectangle
  offset by 10 from a smaller one. The API has two relation types: `OFFSETEDGE` (16) and
  `SKETCHOFFSET` (47, "between entities of the same sketch").
- **Observed (S13).** The offset is resized by editing its dimension. A bi-directional
  offset has two dimensions, edited separately.
- **Observed (S14).** The side can be flipped later: the dimension's reverse-direction
  button, a minus sign typed before the value, or the dimension's right-click "Reverse
  Direction".
- **Inferred.** The dimension measures the perpendicular distance between the original
  and the offset: between two parallel lines, or the difference of two concentric radii.

### Geometry

- **Inferred (definition of an offset curve).** A line offsets to a parallel line at
  distance d, of the same length when it stands alone. An arc or circle of radius R offsets
  to a concentric arc of radius R + d or R − d with the same start and end angles. An
  inward offset of a circle with d ≥ R cannot be made (S8: self-intersecting or
  degenerate).
- **Inferred (S45).** The icon example offsets a rectangle into a larger rectangle, so at
  a corner between two lines the offset lines are extended or trimmed to meet: the corners
  stay sharp. No arc is added at a convex corner.
- **Inferred.** Where the chain is tangent (line into arc, arc into arc), the offsets meet
  at a shared tangent point and stay tangent, since offset curves of a tangent chain are
  tangent.
- **Unknown.** Line–arc and arc–arc corners that are not tangent, and how SOLIDWORKS joins
  them. **Decision.** Extend or trim the two offset curves (infinite line, full circle) to
  their intersection nearest the original corner. If they do not meet, refuse the offset.
- **Decision.** Offset entities in a chain share end points at each join, and each join
  point is held on both neighbouring offset curves.
- **Decision.** For a closed chain, a positive distance offsets outward, and "Reverse"
  offsets inward. For an open chain, a positive distance offsets to the left of the chain
  walked from the first selected entity's start point. This only decides the preview; the
  user can always flip it.
- **Decision.** Cap shapes (S12 shows them only in pictures):
  - bi-directional, line caps: a straight line between the two offsets' ends at each end of
    the chain;
  - bi-directional, arc caps: a half circle centred on the original's end point, of radius
    d, tangent to both offsets;
  - one-directional, line caps: a straight line from the original's end to the offset's
    end;
  - one-directional, arc caps: a half circle of radius d/2 centred midway between the
    original's end and the offset's end, bulging away from the chain.

### Errors

- **Observed (S8).** Self-intersecting results, fit splines and splines already offset are
  refused.
- **Decision.** AnvilCAD also refuses an inward offset that shrinks any entity to zero or
  negative length or radius, such as a 40 × 20 rectangle offset inward by 10 or more.

## Convert Entities

- **Observed (S16, S17, S18).** Convert Entities makes sketch curves by projecting onto the
  sketch plane: a model edge, a loop, a face, a curve, an external sketch contour, a set of
  edges or a set of sketch curves. The selection can be made before or after starting the
  tool.
- **Observed (S18).** Selecting a face converts its edges. Selecting a face and then, with
  Ctrl, one edge of a loop converts only that loop. Selecting several adjacent faces gives
  the outline of the set, one segment per face edge.
- **Observed (S17, S18, S15, B10).** Options: "Select chain" (convert every contiguous
  entity), "For construction" (make the result construction geometry), and "Select all
  inner loops" (convert a face's inner loops too, added in 2016; API `InnerLoops`).
- **Observed (S18).** Two relations are made:
  - **On Edge** between each new curve and its source, so the curve follows the source
    when it changes;
  - **Fixed** on the new curve's end points, kept internal and not listed in
    Display/Delete Relations, so the sketch stays fully defined. Dragging an end point
    removes it.

  In an assembly context with external references turned off, neither is made.
- **Observed (S19).** The silhouette edge of a curved face, such as a cylinder seen side on,
  can be picked for Convert Entities and Offset Entities.
- **Observed (S45, B21, B22).** "On Edge" is the relation's name; the API type is `USEEDGE`
  (32), string `sgUSEEDGE`.
- **Inferred.** The projection is orthogonal onto the sketch plane. A straight edge
  projects to a line (or a point, if square to the plane). A circular edge in a plane
  parallel to the sketch plane projects to a circle or arc of the same radius; a circular
  edge in a tilted plane projects to an ellipse, which AnvilCAD's sketch cannot hold.
- **Decision.** AnvilCAD converts lines and circles/arcs that stay lines and circles/arcs
  when projected, and refuses others with a message. A converted entity stores a
  reference to its model edge (in the same way as a sketch's `PlaneRef` refers to a face)
  and is re-projected on rebuild. Its points are fixed by that reference rather than by
  `Fix` constraints, so the sketch solves as fully defined; deleting the reference (the
  On Edge relation) leaves ordinary geometry.

## Mirror Entities

- **Observed (S20, S21, S22).** The user picks entities to mirror and a line to mirror
  about. Options:
  - "Entities to mirror": some or all entities;
  - "Copy": selected keeps the originals and adds mirrored copies; cleared, the mirrored
    entities replace the originals;
  - "Mirror about": any line (not only a centerline), a linear model edge, a reference
    plane, a planar model face, or a linear drawing edge.
- **Observed (S20).** Every corresponding pair of sketch points gets a Symmetric relation
  (line end points, arc centres and so on), so a change to one side is reflected on the
  other.
- **Observed (S23, B12).** "Dynamic Mirror Entities" is a mode: the user first picks the
  line, then everything newly sketched is mirrored as it is drawn. Existing entities are
  not mirrored. Clicking the tool again turns the mode off. The API toggles it with
  `ISketchManager::SetDynamicMirror` and reads it with `GetDynamicMirror`.
- **Observed (B11).** `IModelDoc2::SketchMirror` mirrors the selection; entities are selected
  with mark 1 and the centerline with mark 2.
- **Inferred (plane geometry).** A point p mirrors about the line through a with unit
  direction u to 2(a + ((p − a)·u)u) − p. A circle keeps its radius. An arc's direction
  reverses, so in AnvilCAD (where an arc runs counter-clockwise from `a` to `b`) the
  mirrored arc runs from the image of `b` to the image of `a`.
- **Inferred (S44).** Symmetric relations about a non-construction line must be allowed,
  since S20 lets any line be the mirror line, though S44's table says "a centerline".
- **Unknown.** With "Copy" cleared, whether any relation ties the mirrored entities to
  anything; what happens to points that lie on the mirror line; whether Equal relations
  are added between circles. **Decision.**
  - With "Copy" cleared, no relation is added; the originals are deleted (with their
    relations) and the mirrored entities get images of the relations that were among the
    originals.
  - A point on the mirror line (within tolerance) is shared, not copied, so a profile
    mirrored about one of its own edges' ends stays closed.
  - For circles and arcs, the centres are made Symmetric and the two radii Equal. Lines
    get Symmetric on both end-point pairs; that fixes their length.

## Linear and Circular Sketch Patterns

### Linear

- **Observed (S25, S27).** Options, per direction:

  | Option | Meaning | API (B13) |
  |---|---|---|
  | "Pattern Direction" | Direction 1 defaults to the X axis; a line of the same sketch, a linear entity or a model edge can be picked. | — |
  | "Reverse direction" | Pattern to the other side of the seed. | sign of spacing |
  | "Spacing" | Distance between instances. | `SpacingX`, `SpacingY` |
  | "Dimension X spacing" / "Dimension Y spacing" | Add the spacing as a dimension. | `XSpacingDim`, `YSpacingDim` |
  | "Number of Instances" | Count, including the seed. | `NumX`, `NumY` |
  | "Display instance count" | Show the count in the view, where it can be edited. | `CreateNumOfInstancesDimInXDir`, `...InYDir` |
  | "Angle" | Direction 1: angle from the X axis. Direction 2: angle stated from the Y axis. | `AngleX`, `AngleY` |
  | "Fix X-axis direction" | Constrain the direction so instances cannot rotate. | — |
  | "Dimension angle between axes" | Add the angle between the two directions as a dimension. | `AngleDim` |
  | "Entities to Pattern" | The seed. | selection |
  | "Instances to Skip" | Instances picked in the view are left out. | `DeleteInstances`, a string "(a) (b) (c)" |

- **Observed (S25).** A fully defined seed gives a fully defined pattern when "Dimension X
  spacing", "Fix X-axis direction", "Dimension Y spacing" and "Dimension angle between
  axes" are all selected.
- **Observed (S25).** The line used as a direction cannot be part of the pattern, and a
  line inside a block cannot be a direction.
- **Observed (S27).** Picking a model edge for Direction 1 turns Direction 2 on; otherwise
  Direction 2 is turned on by hand. S25 says setting Direction 2's instance count turns it
  on.
- **Observed (B13).** Counts include the seed.
- **Inferred.** Instance (i, j), for 0 ≤ i < n1 and 0 ≤ j < n2, is the seed moved by
  i·s1·(cos a1, sin a1) + j·s2·(cos a2, sin a2). (0, 0) is the seed itself.
- **Unknown (S25, B13).** How the Direction 2 angle is measured. S25 says "from the vertical
  (Y axis)", its picture example says "100° off the Y axis", and B13 says "relative to the
  y axis". **Decision.** AnvilCAD stores both angles counter-clockwise from +X, with
  Direction 2 defaulting to 90°, and shows Direction 2's angle as its angle from +Y in the
  editor so the numbers match what a SOLIDWORKS user types.
- **Unknown.** How instances are numbered for "Instances to Skip" and in `DeleteInstances`.
  **Decision.** AnvilCAD stores skipped instances as (i, j) pairs.

### Circular

- **Observed (S26, S28).** Options:

  | Option | Meaning | API (B14) |
  |---|---|---|
  | Centre: sketch origin (default), or "Center X", "Center Y", or drag | Pattern centre. | via `ArcRadius`, `ArcAngle` |
  | "Reverse direction" | Pattern the other way round. | — |
  | "Spacing" | Total angle the pattern covers. | `PatternSpacing` |
  | "Equal spacing" | Space instances evenly. | — |
  | "Dimension radius" | Add the pattern radius as a dimension. | `RadiusDim` |
  | "Dimension angular spacing" | Add the angle between instances as a dimension. | `AngleDim` |
  | "Number of Instances" | Count, including the seed (B14). | `PatternNum` |
  | "Display instance count" | Show the count. | `CreateNumOfInstancesDim` |
  | "Radius" | Distance from the pattern centre to the seed's centre. | `ArcRadius` |
  | "Arc Angle" | Angle of the line from the seed's centre to the pattern centre. | `ArcAngle` |
  | "Instances to Skip" | As for linear. | `DeleteInstances` |
  | — | Whether instances rotate with the pattern. | `PatternRotate` |

- **Inferred (S26).** Radius and arc angle are another way of giving the centre: centre =
  seed centre + R·(cos A, sin A). Changing Center X/Y changes R and A, and the other way
  round.
- **Inferred.** Instance k (0 ≤ k < n) is the seed rotated about the centre by k·step, with
  step = total / n when the total is 360° and total / (n − 1) otherwise, when "Equal
  spacing" is on. **Unknown.** The help does not say how a total below 360° is divided;
  the 360° case is the only one where n and n − 1 give the same picture of evenly spaced
  instances, so this is the usual CAD convention, not a quoted rule.
- **Unknown.** The default turning direction, and how "PatternRotate" appears in the UI.
  **Decision.** Counter-clockwise by default; "Reverse direction" makes it clockwise.
  Instances rotate with the pattern (a seed circle's centre moves on the pattern circle;
  a seed line turns), which is what `PatternRotate = true` names.

### Relations

- **Observed (B22).** The API has relation types for patterns: `PATTERNED` (57),
  `LINEARPATTCNT` (76) and `CIRCULARPATTCNT` (77).
- **Observed (S29).** A pattern is edited by right-clicking an instance; its instance
  count can be edited in the view if shown.
- **Decision.** AnvilCAD records a sketch pattern as an object in the sketch (seed
  entities, parameters, skipped instances, list of instance entities) and constrains each
  instance to its seed by the transform: point-for-point equal offsets (linear) or equal
  rotation about the centre (circular). Spacing, angle and radius become driving
  dimensions when their "Dimension ..." boxes are on.

## Move, Copy, Rotate, Scale and Stretch

- **Observed (S30, S31, S32).** Move and Copy take sketch entities (and annotations), a
  "Keep relations" box, and either "From/To" (a base point and a destination) or "X/Y"
  ("Delta X", "Delta Y"). "Repeat" applies the same displacement again.
- **Observed (S32).** Dimensions go with moved or copied entities. Relations go with them
  only when "Keep relations" is selected. When it is cleared, relations between selected
  and unselected entities are broken, and relations among the selected entities are kept.
- **Observed (S31).** The Copy PropertyManager page says that dimensions and relations are
  copied. **Unknown.** This conflicts with S32 for relations; S32 is the more specific.
- **Observed (S33).** Rotate takes the entities, "Keep relations", a centre of rotation
  (base point) and an "Angle".
- **Observed (S34).** Scale takes the entities, a point to scale about, a "Scale Factor" (2
  doubles, 0.5 halves), "Copy" to keep the originals, and "Number of Copies".
- **Observed (B15).** The API calls are `MoveOrCopy(Copy, NumCopies, KeepRelations, BaseX,
  BaseY, BaseZ, DestX, DestY, DestZ)`, `RotateOrCopy(Copy, NumCopies, KeepRelations, BaseX,
  BaseY, BaseZ, DestX, DestY, DestZ, Angle)` (Dest is the rotation axis; usually 0, 0, 1)
  and `ScaleOrCopy(Copy, NumCopies, BaseX, BaseY, BaseZ, Scale)`. Scaling may break
  relations, including those made by Offset and Convert.
- **Observed (S35).** Stretch takes entities and a base point or a Delta X/Y. **Not
  specified further here.**
- **Unknown.** The sign convention of the rotation angle; what happens to dimensions and
  relations that would conflict after a move (for example a moved line with a fixed
  dimension to the origin); how scaled copies are spaced when "Number of Copies" > 1.
  **Decision.**
  - A positive angle turns counter-clockwise.
  - Move, rotate and scale first apply the transform to the points, then re-solve. Any
    dimension or relation from a moved entity to an unmoved one that now conflicts is
    removed when "Keep relations" is cleared, and kept (the sketch may over-define and say
    so) when it is selected.
  - Scaling multiplies every length and radius dimension among the selected entities by
    the factor; angles are unchanged.
  - Copy n times with scale f gives copies scaled by f, f², ..., fⁿ about the same point.

## Trim Entities

- **Observed (S36, S37, B16, B17).** The options:

  | Option (UI) | What it does | API `swSketchTrimChoice_e` |
  |---|---|---|
  | "Power trim" | Drag across entities; each one crossed is trimmed back to its nearest intersections on either side of the crossing. Dragging along an entity instead extends it. | `swSketchTrimEntities` 4 (pick points), `swSketchTrimEntityPoint` 3 (trim at a point), `swSketchTrimTwoEntities` 2 (trim the first pick to the second entity) |
  | "Corner" | Extend or trim two entities until they meet at their virtual corner. | `swSketchTrimCorner` 1 |
  | "Trim away inside" | Two boundary entities, then entities to trim: the parts between the boundaries are removed. | `swSketchTrimInside` 6 |
  | "Trim away outside" | As above, keeping only the parts between the boundaries. | `swSketchTrimOutside` 5 |
  | "Trim to closest" | Click an entity: the clicked part is removed up to its closest intersections. Dragging extends it instead, in the drag direction. | `swSketchTrimClosest` 0 |

- **Observed (S37).** Corner:
  - the two entities can be of different types;
  - one can be lengthened and the other shortened, or both lengthened;
  - which end of each entity is picked matters; the order of the picks does not;
  - a message appears when the corner cannot be made (S38).
- **Observed (S37, S38).** Trim away inside:
  - the two boundaries can be of different types;
  - each entity to trim must cross each boundary exactly once, or neither boundary at all;
  - only open entities (not circles) are trimmed.
- **Observed (S37).** Trim away outside follows the same rules. The boundaries count as
  unlimited (not cut off at their end points). An entity that crosses only one boundary
  loses its part outside that boundary, and its part inside is extended to the next
  entity.
- **Observed (S39, S38).** Power trim can also extend an entity along its natural path; an
  arc can be extended only so far, after which the extension moves to its other end.
- **Observed (S37).** Two more options:
  - "Keep trimmed entities as construction geometry": the trimmed-off parts stay, as
    construction geometry;
  - "Ignore trimming of construction geometry": construction entities are never trimmed.

  These keep dimensions to the trimmed entities and may keep geometric relations,
  depending on the case. Selecting both turns trimmed solid parts into construction and
  leaves existing construction alone.
- **Observed (B16).** In the API, Corner needs exactly two entities, Trim to closest exactly
  one, and the inside/outside choices at least three (two boundaries and one or more to
  trim).

### What happens to relations

- **Unknown.** The help does not say, beyond the two options above, what happens to
  relations and dimensions on a trimmed entity.
- **Decision.** AnvilCAD:
  - keeps the trimmed entity's identity (same `EntityId`) for the piece containing its
    first end point when that piece survives, so relations on the entity itself (Horizontal,
    Parallel, Equal, Tangent and so on) stay;
  - moves the cut end to the intersection point and adds a relation holding it on the
    cutting entity (`PointOnLine` or `PointOnCircle`), or makes it Coincident with an
    existing end point of the cutting entity when they are the same point;
  - removes a dimension or relation that referred to an end point that no longer exists,
    unless "Keep trimmed entities as construction geometry" is on, in which case the
    removed piece survives as construction geometry and keeps them;
  - when trimming a middle piece splits an entity in two, the second piece is a new
    entity that copies the relations that still make sense for it (Horizontal, Vertical,
    Parallel, Perpendicular for lines; Concentric and Equal radius for arcs);
  - trimming a circle at two intersections turns it into an arc; trimming an entity with
    no intersections deletes it.

## Extend Entities

- **Observed (S39, B18).** Lines, centerlines and arcs can be extended to meet the nearest
  entity. The preview shows which end is extended; it is the end nearer the pointer (move
  to the other half to extend the other end). Clicking again extends past the next entity.
- **Decision.** The extended end point gains the same on-entity relation as a trim cut
  (see above). An entity with nothing in the way is not changed.

## Split Entities

- **Observed (S40, B19).** Clicking an entity splits it in two at the clicked point, with a
  split point shared by the two pieces. A circle, full ellipse or closed spline needs two
  split points. Dimensions can be attached to a split point. Deleting the split point joins
  the two pieces back into one.
- **Observed (B19).** The API calls are `SplitOpenSegment(X, Y, Z)` and
  `SplitClosedSegment(X1, Y1, Z1, X2, Y2, Z2)`; they ask that snapping be turned off first.
- **Inferred.** The two pieces of a split line stay collinear, and the two arcs of a split
  circle stay coradial, or the "rejoin" on deleting the split point would not make sense.
- **Decision.** AnvilCAD splits a line into two lines sharing the split point, with a
  Collinear relation between them; an arc into two arcs sharing the split point, with a
  Coradial relation; a circle (two clicks) into two arcs, Coradial. Relations on the
  original entity are copied to both pieces where they still apply. Deleting a split point
  that joins exactly two pieces related in that way merges them back.

## Construction Geometry

- **Observed (S41).** Any sketch entity can be made construction geometry. Construction
  geometry helps build the sketch and is ignored when a feature uses the sketch; it is drawn
  like a centerline. Points and centerlines are always construction.
- **Observed (S41, S47, B20, B12).** It is toggled with the "For construction" box in an
  entity's PropertyManager, the Construction Geometry button or menu item. API:
  `ISketchSegment::ConstructionGeometry` (get/set) and
  `ISketchManager::CreateConstructionGeometry`.
- **Inferred.** Construction geometry takes part in relations and dimensions like any other
  entity.
- **Note.** AnvilCAD already has `Entity::construction`, draws it dashed, and leaves it out
  of profiles (`regions.rs`). What is missing is a toggle command for selected entities.

## Relations

### Rules that apply to every relation

- **Observed (S46).** At least one item in a relation is a sketch entity. The others can be
  sketch entities, or a model edge, face, vertex, the origin, a plane, an axis, or a curve
  of another sketch that projects to a line or arc on the sketch plane.
- **Observed (S44).** Relations to lines act on the infinite line; relations to arcs act on
  the full circle; relations to things off the plane act on their projections.
- **Observed (S50).** While sketching, relations are inferred and added automatically
  (horizontal, vertical, coincident, midpoint, perpendicular, tangent, among others) when
  "Automatic relations" is on. Holding Ctrl suppresses them for the moment.
- **Observed (S48, B24).** Display/Delete Relations filters relations as all, dangling,
  over defining / not solved, external, defined in context, locked, broken, or the
  selected entities' relations. A relation can be suppressed per configuration. Dangling
  relations can be repaired by replacing the missing entity.
- **Observed (S49).** A redundant relation that agrees with the others leaves a sketch fully
  defined; a dimension that duplicates relations over-defines it.

### Every 2D relation

From S44's table, with icons and type from S45 and API names from B21 and B22. "AnvilCAD"
compares with the `Constraint` enum in `crates/anvil-sketch/src/sketch.rs` and the
relations offered by `SketchSession::available_relations` in
`crates/anvil-app/src/sketcher.rs`.

| Relation | Selections | Meaning | API `swConstraintType_e` / `sgXXX` | AnvilCAD today |
|---|---|---|---|---|
| Horizontal | One or more lines, or two or more points | Lines run along the sketch X axis; points share a Y | `HORIZONTAL` 4, `HORIZPOINTS` 25 / `sgHORIZONTAL2D`, `sgHORIZONTALPOINTS2D` | `Horizontal { line }`, `HorizontalPoints { p, q }`; one line or two points only |
| Vertical | One or more lines, or two or more points | Along the sketch Y axis; points share an X | `VERTICAL` 5, `VERTPOINTS` 26 / `sgVERTICAL2D`, `sgVERTICALPOINTS2D` | `Vertical`, `VerticalPoints`; one line or two points only |
| Collinear | Two or more lines | On one infinite line | `COLINEAR` 27 / `sgCOLINEAR` | **Missing** |
| Coradial | Two or more arcs | Same centre and same radius | `CORADIAL` 28 / `sgCORADIAL` | **Missing** (equals `Concentric` + `Equal`) |
| Perpendicular | Two lines | At right angles | `PERPENDICULAR` 8 | `Perpendicular { a, b }` |
| Parallel | Two or more lines (in 3D, also a line and a plane) | Parallel | `PARALLEL` 7 | `Parallel { a, b }`; two lines only |
| Tangent | An arc, ellipse or spline, and a line or arc | Tangent | `TANGENT` 6 | `Tangent { line, entity }`: line and arc/circle only. **Arc–arc missing** |
| Concentric | Two or more arcs, or a point and an arc | Same centre | `CONCENTRIC` 10 | `Concentric { a, b }` for two rounds. **Point–arc missing** (same as `Coincident` with the centre) |
| Midpoint | A point and a line, or two lines | The point stays at the line's midpoint | `ATMIDDLE` 12 / `sgATMIDDLE` | `Midpoint { p, line }`. **Two-line form missing** |
| Intersection | Two lines and a point | The point stays where the lines cross | `ATINTERSECT` 13 / `sgATINTERSECT` (`INTERSECTION` 56 also exists) | **Missing** (equals two `PointOnLine`) |
| Coincident | A point and a line, arc or ellipse (and two points) | The point lies on the curve | `COINCIDENT` 9 | `Coincident { p, q }`, `PointOnLine`, `PointOnCircle` |
| Equal | Two or more lines, or two or more arcs | Equal lengths, or equal radii | `SAMELENGTH` 14 / `sgSAMELENGTH` | `Equal { a, b }`; two at a time. The UI offers it for two lines or two rounds |
| Equal curve length | Any two of line, arc, circle, spline (S51) | Equal lengths along the curves | `SAMECURVELENGTH` 85 | **Missing** (`ArcLength` dimension exists) |
| Symmetric | A centerline (any line, see Mirror) and two points, lines, arcs or ellipses | Mirror images across the line | `SYMMETRIC` 11 | **Missing** |
| Fix | Any entity | Size and place fixed; a fixed line's ends can still slide along its infinite line, a fixed arc's ends along its full circle | `FIXED` 17 / `sgFIXED` | `Fix { p, x, y }`: points only. **Entity form missing** |
| Merge points | Two points or end points | They become one point | `MERGEPOINTS` 42 / `sgMERGEPOINTS` | **Missing** as a relation; AnvilCAD shares one `PointId` where SOLIDWORKS merges |
| Pierce | A sketch point and an axis, edge, line or spline outside the sketch | The point sits where the curve passes through the sketch plane (used for sweep guide curves) | `ATPIERCE` 40 / `sgATPIERCE` | **Missing** (needs references to model curves) |
| On Edge | A sketch entity and a model edge (made by Convert Entities) | The entity is the edge's projection | `USEEDGE` 32 / `sgUSEEDGE` | **Missing** |
| Offset | A sketch entity and its offset source (made by Offset Entities) | Keeps the offset | `OFFSETEDGE` 16, `SKETCHOFFSET` 47 / `sgOFFSETEDGE` | **Missing** |
| Doubled distance | A centerline and a sketch entity | The entity is dimensioned at twice its distance from the centerline | `DOUBLEDISTANCE` 41 | `AxisDiameter { p, axis, value }` (as a dimension) |
| Equal slots, Fix slot | Slots | Equal slot length and radius; fixed slot | `SAMESLOTS` 75, `FIXEDSLOT` 74 | Not applicable (no slot entity) |
| Equal curvature, Torsion continuity | Splines with other curves | Curvature continuity | `EQUALCURVATURE` 61 | Not applicable (no splines) |
| Patterned | Sketch pattern instances | Instance follows its seed | `PATTERNED` 57, `LINEARPATTCNT` 76, `CIRCULARPATTCNT` 77 | **Missing** |
| On Plane, On Surface, Tangent to Face, Along X/Y/Z, Normal, Parallel YZ/ZX | 3D sketches | — | `ALONGX` 48 and others, `NORMAL` 45, `PARALLELYZ` 54, `PARALLELZX` 55, `TANGENTFACE` 63 | Not applicable (2D only) |
| Traction | Layout sketches with belts | — | `TRACTION` 68, `BELTTRACTION` 69 | Not applicable |

- **Observed (S51).** Equidistant relations are made by the Segment tool on points spread
  along a curve. Not studied further.
- **Inferred (S44).** Selections of "two or more" make one relation over all of them in
  SOLIDWORKS. AnvilCAD's constraints are pairwise.
- **Decision.** For n items, AnvilCAD adds n − 1 pairwise constraints, each between the
  first item and one other, and deletes them as a group when the user deletes the
  relation. Equal, Parallel, Horizontal, Vertical, Collinear and Coradial work this way.
- **Unknown.** What the two-line form of Midpoint means (S44 lists "two lines" but describes
  only a point on a line). **Decision.** Not offered until a source explains it.
- **Unknown.** For arc–arc Tangent, whether the arcs touch on the outside or inside is not
  stated. **Decision.** The constraint stores which one, taken from the geometry when the
  relation is added (outside when the centres are farther apart than the larger radius).
- **Decision.** A Fix on a line holds its infinite line (two residuals: the line's angle and
  its distance from the origin); a Fix on a circle or arc holds the centre and radius. The
  end points stay free along the curve, as S44 says.

## Testable statements

Units are millimetres and degrees. "Rectangle 40 × 20" means corners (0, 0), (40, 0),
(40, 20), (0, 20) unless stated. Areas are of the closed profile from `Sketch::profiles`.

### Fillet

1. A 5 mm fillet on the (0, 0) corner of the 40 × 20 rectangle leaves area
   800 − (25 − 25π/4) = 794.6350 (the corner loses 25 − 25π/4 = 5.3650).
2. Its tangent points are (5, 0) and (0, 5); its centre is (5, 5); the bottom line is then
   35 long and the left line 15; the arc is π·5/2 = 7.8540 long; the perimeter is
   117.8540.
3. 5 mm fillets on all four corners (in one run) give area 800 − 4(25 − 25π/4) = 778.5398 and
   perimeter 80 + 10π = 111.4159. The first fillet has a radius dimension and the other
   three have Equal relations to it; with "Dimension each fillet" on, four radius
   dimensions of 5.
4. Before filleting, add an HDistance of 40 between (0, 0) and (40, 0) and Fix (0, 0). After
   a 5 mm fillet at (0, 0), the point at (0, 0) still exists, is no longer an end point of
   either line, lies on both lines, and the 40 dimension still solves. Changing that
   dimension to 50 moves the right side to x = 50.
5. Filleting an unconstrained corner removes the corner point and adds three: the arc's
   centre and the two tangent points. The sketch's point count goes up by two.
6. After a fillet of an unconstrained corner the sketch has the two shortened lines and one
   new arc, and the arc's end points are the same `PointId`s as the lines' new ends.
7. Two lines meeting at 60° (from (0, 0) along (1, 0) and along (cos 60°, sin 60°), each
   30 long) take a 5 mm fillet with set-back 5/tan 30° = 8.6603, centre at distance
   5/sin 30° = 10 from the corner on the bisector, so at (8.6603, 5); arc sweep 120°, arc
   length 10.4720; area removed 17.1213.
8. Line from (−20, 0) to (0, 0) and arc centred (20, 0) of radius 20 from (20, 20) to
   (0, 0) (counter-clockwise from (20, 20)), corner at (0, 0). A 5 mm fillet has centre
   (20 − 10√6, 5) = (−4.4949, 5), touches the line at (−4.4949, 0) and the arc at
   (20 − 8√6, 4) = (0.4041, 4), and the fillet is tangent to both.
9. Two lines that do not touch, (0, 0)–(30, 0) and (40, 5)–(40, 20), filleted at 5:
   both are extended to the virtual corner (40, 0) first; the result has tangent points
   (35, 0) and (40, 5) and centre (35, 5).
10. A 25 mm fillet on the (0, 0) corner of the 40 × 20 rectangle is refused (set-back 25 is
    longer than the 20 side) and the sketch is unchanged.

### Chamfer

11. Distance-distance 5 (first: bottom line) and 10 (second: left line) on the (0, 0)
    corner of the 40 × 20 rectangle: chamfer from (5, 0) to (0, 10), length √125 = 11.1803,
    area 800 − 25 = 775.
12. Equal distance 5 on the same corner: chamfer (5, 0)–(0, 5), length 7.0711, area 787.5.
13. Angle-distance, distance 5 on the bottom line and angle 30°: chamfer (5, 0)–(0, 2.8868),
    length 5.7735, area 800 − 7.2169 = 792.7831.

### Offset

14. The 40 × 20 rectangle offset outward by 5 (chain) is a rectangle (−5, −5)–(45, 25), 50
    × 30, area 1500, with sharp corners and four lines.
15. Offset inward by 5: (5, 5)–(35, 15), 30 × 10, area 300.
16. Offset inward by 10 is refused (zero height).
17. A circle of radius 10 offset outward by 5 has radius 15 and the same centre; inward by 5,
    radius 5; inward by 10, refused.
18. Changing the offset dimension of test 14 from 5 to 8 makes the outer rectangle 56 × 36.
    Changing the original rectangle to 60 × 20 makes the outer one 70 × 30.
19. A line (0, 0)–(40, 0) offset bi-directionally by 5 with arc caps gives a closed slot:
    lines y = ±5 from x = 0 to 40 and two half circles of radius 5 centred (0, 0) and
    (40, 0). Area 400 + 25π = 478.5398. With line caps: a 40 × 10 rectangle, area 400.
20. Bi-directional offset gives two independent dimensions: setting one to 3 and the other
    to 7 gives lines at y = 3 and y = −7 (or the reverse, by side).
21. Offset with "Base geometry" construction: the original entities have
    `construction = true` and `profiles` returns only the offset loop.
22. A chain line (0, 0)–(20, 0), then a tangent arc centred (20, 10) of radius 10 from
    (20, 0) to (30, 10), offset by 2 toward the arc's centre: the line moves to y = 2,
    the arc keeps centre (20, 10) with radius 8, and they share the point (20, 2).

### Convert

23. On a 40 × 20 × 10 box with a sketch on its top face, converting the top face gives four
    lines forming a 40 × 20 rectangle that matches the face, and the sketch is fully
    defined with no user constraints. Changing the box to 50 × 20 and rebuilding makes the
    converted rectangle 50 × 20.
24. Converting the circular edge of a 10 mm diameter hole through that face gives a circle
    of radius 5 at the hole's centre.

### Mirror

25. A circle of radius 3 at (10, 5) mirrored about the line x = 0 (from (0, −10) to
    (0, 10)) with "Copy" on gives a second circle of radius 3 at (−10, 5), with the
    centres Symmetric about the line and the radii Equal. Moving the first centre to
    (12, 6) moves the second to (−12, 6).
26. A line (5, 0)–(5, 10) mirrored about the line through (0, 0) and (10, 10) gives
    (0, 5)–(10, 5).
27. An arc centred (10, 0) from (15, 0) to (10, 5) (counter-clockwise) mirrored about x = 0
    is the arc centred (−10, 0) from (−10, 5) to (−15, 0), counter-clockwise, with the same
    sweep of 90°.
28. With "Copy" off, the original entities are gone and the entity count is unchanged.
29. Mirroring the open chain (0, 0)–(10, 0)–(10, 10) about the line x = 0 shares the point
    (0, 0): the entities use 5 distinct points, not 6.

### Patterns

30. A circle of radius 2 at (0, 0), linear pattern 3 × 2 with spacing 10 (X) and 15 (Y),
    angles 0° and 90°: 6 circles, centres (0, 0), (10, 0), (20, 0), (0, 15), (10, 15),
    (20, 15).
31. The same with instance (1, 1) skipped: 5 circles, no circle at (10, 15).
32. Linear pattern, 2 instances, spacing 10, angle 10°: the second centre is
    (9.8481, 1.7365).
33. Changing the X spacing dimension of test 30 to 12 moves the centres to x = 0, 12, 24.
34. A circle of radius 2 at (20, 0), circular pattern about the origin, 360° equal
    spacing, 4 instances: centres (20, 0), (0, 20), (−20, 0), (0, −20).
35. The same with 6 instances: the second centre is (10, 17.3205).
36. With "Reverse direction", 4 instances: the second centre is (0, −20).
37. Radius 20 and arc angle 180° for a seed at (20, 0) put the pattern centre at the
    origin.

### Move, copy, rotate, scale

38. Move the 40 × 20 rectangle by Delta X = 10, Delta Y = −5: corners (10, −5)–(50, 15);
    its 40 and 20 dimensions are unchanged.
39. Copy it the same way: two rectangles, both 40 × 20, total entity count 8.
40. Rotate the point (10, 0) by 30° about the origin: (8.6603, 5). Rotate the rectangle by
    90° about the origin: corners (0, 0), (0, 40), (−20, 40), (−20, 0).
41. Scale the rectangle by 0.5 about the origin: (0, 0)–(20, 10), area 200; its length
    dimensions become 20 and 10.

### Trim, extend, split

42. Lines (−10, 0)–(10, 0) and (0, −10)–(0, 10). Power trim of the horizontal line at
    (5, 0) leaves (−10, 0)–(0, 0), length 10, with its new end held on the vertical line.
43. Power trim of a line that crosses nothing deletes it.
44. A circle of radius 10 at the origin crossed by the line x = 5 (from (5, −20) to
    (5, 20)): trimming the circle at (10, 0) leaves an arc from (5, 8.6603) round through
    (−10, 0) to (5, −8.6603), sweep 240° (2 × 120°), centred at the origin, radius 10.
45. Trimming the middle of (0, 3)–(30, 3) between vertical lines x = 10 and x = 20 (pick at
    (15, 3)) leaves two lines, (0, 3)–(10, 3) and (20, 3)–(30, 3), both still horizontal.
46. Corner trim of (0, 0)–(10, 0) and (15, 5)–(15, 20), picked near (0, 0)'s far end and
    (15, 20)'s far end: (0, 0)–(15, 0) and (15, 0)–(15, 20), lengths 15 and 20, sharing
    (15, 0).
47. Trim away inside with boundaries x = 0 and x = 10 (vertical lines from y = −5 to
    y = 5) on (−5, 3)–(15, 3): two pieces (−5, 3)–(0, 3) and (10, 3)–(15, 3). Trim away
    outside on the same: one piece (0, 3)–(10, 3).
48. With "Keep trimmed entities as construction geometry", test 42 leaves the piece
    (0, 0)–(10, 0) as construction, and `profiles` ignores it.
49. Extend (0, 0)–(5, 0) toward the vertical line x = 12 (from y = −5 to y = 5): the line
    becomes (0, 0)–(12, 0).
50. Split (0, 0)–(40, 0) at (10, 0): two lines of 10 and 30 sharing the point (10, 0),
    collinear. Deleting the split point gives back one line of 40.

### Relations

51. Collinear on (0, 0)–(10, 0) and (20, 1)–(30, 3): after solving, both lie on one line
    (all four points satisfy one line equation within 1e-9).
52. Coradial on an arc of radius 5 at (0, 0) and an arc of radius 7 at (1, 1): after
    solving, same centre and same radius.
53. Symmetric about the line x = 0 for points (3, 2) and (−5, 4): after solving they are
    mirror images, (a, b) and (−a, b).
54. Intersection of a point with lines y = 0 and x = 7: the point solves to (7, 0).
55. A fixed line keeps its infinite line: dragging an end point of a fixed line from
    (0, 0)–(10, 0) moves it only along y = 0.
56. Arc–arc Tangent between circles of radius 5 at (0, 0) and radius 3 at (10, 0) with
    the second free to move along y = 0: externally tangent, the centre goes to (8, 0).
57. Each new relation type removes exactly the degrees of freedom expected: Collinear 2,
    Coradial 3, Symmetric (two points) 2, Intersection 2, Merge 2, Fix on a line 2, Fix on
    a circle 3.

## API names

For `anvil-migrate` (names and values only; sources B1–B24).

- **Fillet.** `ISketchManager::CreateFillet(Radius, ConstrainedCorners)`;
  `IModelDoc2::SketchFillet2(Rad, ConstrainedCorners)`; `swConstrainedCornerAction_e`:
  `swConstrainedCornerInteract` 0, `swConstrainedCornerKeepGeometry` 1,
  `swConstrainedCornerDeleteGeometry` 2, `swConstrainedCornerStopProcessing` 3.
- **Chamfer.** `ISketchManager::CreateChamfer(Type, Distance, AngleORdist)`;
  `swSketchChamferType_e`: `swSketchChamfer_DistanceAngle` 0,
  `swSketchChamfer_DistanceDistance` −1, `swSketchChamfer_DistanceEqual` 2.
- **Offset.** `ISketchManager::SketchOffset2(Offset, BothDirections, Chain, CapEnds,
  MakeConstruction, AddDimensions)` (2016; `SketchOffset` is obsolete);
  `IModelDoc2::SketchOffsetEntities2(Offset, BothDirections, Chain)` for model edges and
  external curves; `swSkOffsetCapEndType_e`; `swSkOffsetMakeConstructionType_e` (values
  in the Offset section).
- **Convert.** `ISketchManager::SketchUseEdge3(Chain, InnerLoops)` (2016;
  `SketchUseEdge`, `SketchUseEdge2` obsolete; `ConvertEntities` not implemented).
- **Mirror.** `IModelDoc2::SketchMirror()` (entities mark 1, centerline mark 2);
  `ISketchManager::SetDynamicMirror`, `GetDynamicMirror`.
- **Patterns.** `ISketchManager::CreateLinearSketchStepAndRepeat(NumX, NumY, SpacingX,
  SpacingY, AngleX, AngleY, DeleteInstances, XSpacingDim, YSpacingDim, AngleDim,
  CreateNumOfInstancesDimInXDir, CreateNumOfInstancesDimInYDir)`,
  `EditLinearSketchStepAndRepeat`;
  `ISketchManager::CreateCircularSketchStepAndRepeat(ArcRadius, ArcAngle, PatternNum,
  PatternSpacing, PatternRotate, DeleteInstances, RadiusDim, AngleDim,
  CreateNumOfInstancesDim)`, `EditCircularSketchStepAndRepeat`.
- **Transforms.** `IModelDocExtension::MoveOrCopy`, `RotateOrCopy`, `ScaleOrCopy` (signatures
  in that section).
- **Trim, extend, split.** `ISketchManager::SketchTrim(Option, X, Y, Z)`;
  `swSketchTrimChoice_e`: `swSketchTrimClosest` 0, `swSketchTrimCorner` 1,
  `swSketchTrimTwoEntities` 2, `swSketchTrimEntityPoint` 3, `swSketchTrimEntities` 4,
  `swSketchTrimOutside` 5, `swSketchTrimInside` 6; `ISketchManager::SketchExtend(X, Y, Z)`;
  `ISketchManager::SplitOpenSegment(X, Y, Z)`, `SplitClosedSegment(X1, Y1, Z1, X2, Y2, Z2)`.
- **Construction.** `ISketchSegment::ConstructionGeometry`;
  `ISketchManager::CreateConstructionGeometry`.
- **Relations.** `ISketchRelationManager::AddRelation(Entities, RelationType)`;
  `ISketchRelation::GetRelationType` returns `swConstraintType_e`; relations are reached
  from `ISketchSegment::GetRelations`, `ISketchPoint::GetRelations` and
  `ISketchRelationManager::GetRelations`. `IModelDoc2::SketchAddConstraints(IdStr)` takes
  `sgXXX` strings (2006 and later: `sgHORIZONTAL2D`, `sgVERTICAL2D`,
  `sgHORIZONTALPOINTS2D`, `sgVERTICALPOINTS2D`, `sgCOLINEAR`, `sgCORADIAL`,
  `sgPERPENDICULAR`, `sgPARALLEL`, `sgTANGENT`, `sgCONCENTRIC`, `sgCOINCIDENT`,
  `sgSYMMETRIC`, `sgATMIDDLE`, `sgATINTERSECT`, `sgATPIERCE`, `sgFIXED`, `sgSAMELENGTH`,
  `sgOFFSETEDGE`, `sgUSEEDGE`, `sgMERGEPOINTS`, plus dimension and snap strings).
  `swSketchRelationFilterType_e`: `swAll` 0, `swDangling` 1, `swOverDefining` 2,
  `swExternal` 3, `swDefinedInContext` 4, `swLocked` 5, `swBroken` 6,
  `swSelectedEntities` 7.
- **`swConstraintType_e` values used in 2D sketches** (B22): `DISTANCE` 1, `ANGLE` 2,
  `RADIUS` 3, `HORIZONTAL` 4, `VERTICAL` 5, `TANGENT` 6, `PARALLEL` 7, `PERPENDICULAR` 8,
  `COINCIDENT` 9, `CONCENTRIC` 10, `SYMMETRIC` 11, `ATMIDDLE` 12, `ATINTERSECT` 13,
  `SAMELENGTH` 14, `DIAMETER` 15, `OFFSETEDGE` 16, `FIXED` 17, `HORIZPOINTS` 25,
  `VERTPOINTS` 26, `COLINEAR` 27, `CORADIAL` 28, `USEEDGE` 32, `ATPIERCE` 40,
  `DOUBLEDISTANCE` 41, `MERGEPOINTS` 42, `ARCLENGTH` 44, `SKETCHOFFSET` 47,
  `INTERSECTION` 56, `PATTERNED` 57, `LINEARPATTCNT` 76, `CIRCULARPATTCNT` 77,
  `SAMECURVELENGTH` 85. All with the `swConstraintType_` prefix.

## Recommended scope

What exists now: `Sketch` holds points, lines, circles and counter-clockwise arcs, with a
`construction` flag; the `Constraint` enum covers the relations and dimensions in the
table above; the solver is a dense least-squares Newton solver over point coordinates and
circle radii; the sketcher's tools are Select, Line, Centerline, Rectangle, Circle, Arc,
Point and Dimension. At the time of writing an uncommitted
`crates/anvil-sketch/src/curve.rs` in this worktree adds curve intersection, projection,
`corner_fillet`, `rotate_about` and `reflect` helpers for these tools.

In order:

1. **Construction toggle** for selected entities (the flag exists; only the command is
   missing).
2. **Missing 2D relations**: Collinear, Coradial, Intersection, Symmetric, arc–arc Tangent,
   point–arc Concentric, Fix on a line or arc, Merge points, and "two or more" selections
   as pairwise groups. Mirror, split and offset depend on them.
3. **Sketch Fillet** (line–line, then line–arc and arc–arc), with the virtual sharp,
   the radius dimension and Equal for later fillets.
4. **Trim** (power trim by pick, trim to closest, corner, inside/outside), **Extend** and
   **Split**, with the relation rules above.
5. **Sketch Chamfer** between two lines (three types).
6. **Mirror Entities** about a sketch line, with Copy and Symmetric relations.
7. **Move, Copy, Rotate, Scale** with "Keep relations".
8. **Offset Entities** of sketch chains of lines and arcs, with a new offset relation
   (one constraint per original/offset pair sharing one driving distance), bi-directional
   and caps.
9. **Linear and circular sketch patterns** with their dimensions and skipped instances.
10. **Convert Entities** and the On Edge relation, which need sketch entities to refer to
    model edges and to survive rebuilds (the same naming problem as faces in `PlaneRef`).
    Offsetting model edges comes with it.

Leave out for now: dynamic mirror, Stretch, splines, ellipses and slots and every relation
that needs them, Pierce, 3D-sketch relations, Equal curve length, Equidistant (Segment
tool), Traction, configurations of relations, and repairing dangling relations.

## Unknown or not covered

1. Default values and default check-box states of every PropertyManager here.
2. What a too-large fillet radius, parallel lines or already-tangent entities do in
   Sketch Fillet.
3. Whether 2D Sketch Chamfer keeps a constrained corner, which dimensions it adds, whether
   it works on arcs, and exactly where the angle of angle-distance is measured.
4. How Offset Entities joins non-tangent line–arc and arc–arc corners, which side a new
   one-directional offset takes, and the exact cap shapes (pictures only).
5. How the Direction 2 angle of a linear pattern is measured (S25 and B13 disagree with
   its own example), how skipped instances are numbered, and how a circular pattern with
   "Equal spacing" divides a total below 360°.
6. The sign of the rotation angle in Rotate Entities, and how scaled copies are spaced.
7. Whether Copy Entities copies relations without "Keep relations" (S31 and S32 differ).
8. What trimming does to relations and dimensions beyond the two construction options.
9. The two-line form of the Midpoint relation, and whether arc–arc Tangent is inside or
   outside.
10. How SOLIDWORKS stores any of these relations in a `.sldprt` file (not studied).
