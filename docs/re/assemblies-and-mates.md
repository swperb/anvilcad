# Assemblies and mates

What AnvilCAD's assemblies and mates should do. An assembly places parts (and other
assemblies) as components, each a rigid instance of its file. Mates are geometric
constraints between entities of two components, solved together to position them.
SOLIDWORKS users know the same ideas as its assemblies, mates and Mate PropertyManager, so
AnvilCAD follows their published behaviour where that makes sense. This specification was
written for the mate solver in `crates/anvil-model/src/mates.rs` (ROADMAP item 3).

## How this was studied

- **Inputs.** Only published information was used: Dassault's public online help for
  SOLIDWORKS 2025 and its 2024 API help. No SOLIDWORKS software, installer, library or
  file was downloaded, installed, run or analysed. No SOLIDWORKS licence was accepted on
  the machine used.
- **Retrieval.** As for [hole-wizard.md](hole-wizard.md): each article was fetched over
  HTTPS with a browser user agent, its text read from the page's embedded `__NEXT_DATA__`
  JSON (`props.pageProps.helpContentData.helpText`) and stripped of HTML by a script run
  with `python3 -I`. A missing page still returns HTTP 200, so each page's content was
  checked for "cannot be found". Articles were found by following links from the
  *Assemblies* entry of the help's table of contents (also in `__NEXT_DATA__`). The text
  was paraphrased; none of it is copied into this repository.
- **Matrix pages.** The per-mate pages (S12–S18) show which entity pairs are valid as a
  grid of tick images, which carry no text. The pairs below are therefore taken from the
  text table in S11, which covers the same ground. The per-mate pages were used only for
  their notes (for example "Cylinder refers to the cylinder's axis").
- **REA.** REA (`rea-agents` 6.5.0, with the `third_party/rea` submodule at `3fc0075`) was
  set up and its research method followed:
  - each finding below is marked **Observed** (stated in a cited source), **Inferred**
    (our reasoning from the sources, including standard rigid-body kinematics),
    **Decision** (AnvilCAD's own choice where the sources are silent) or **Unknown**;
  - every observation cites its source.

  REA's analysis tools were **not** run. They inspect artifacts (binaries, packages,
  captures, files), and the only lawful inputs for this behaviour are prose documents.
  How `.sldasm` files store components and mates was not studied; reading them is future
  work that needs sample files and its own specification.
- **Degrees of freedom.** The help says what each mate does geometrically but never how
  many degrees of freedom it removes. Every DOF count below is therefore **Inferred** from
  the geometry, not observed.

## Sources

Base A is `https://help.solidworks.com/2025/English/SolidWorks/sldworks/`. Base B is
`https://help.solidworks.com/2024/English/api/sldworksapi/`. Base C is
`https://help.solidworks.com/2024/English/api/swconst/`. Interface pages in B are named
`SolidWorks.Interop.sldworks~SolidWorks.Interop.sldworks.<Interface>[~<Member>].html`, and
enumeration pages in C `SOLIDWORKS.Interop.swconst~SOLIDWORKS.Interop.swconst.<enum>.html`.
All were read on 2026-10-19.

| # | Topic | URL | Version |
|---|---|---|---|
| S1 | Assemblies (section root) | A + `c_Assemblies_first_map_topic.htm` | 2025 |
| S2 | The design tree in an assembly (instance suffix, status prefixes) | A + `c_featuremanager_design_tree_assembly.htm` | 2025 |
| S3 | Adding components; inserting from the PropertyManager | A + `t_adding_components_assembly.htm`, `t_inserting_components_propertymanager.htm` | 2025 |
| S4 | Insert Components / Begin Assembly PropertyManager (fix / float on insert) | A + `HIDD_DVE_INSERT_COMPONENT.htm` | 2025 |
| S5 | Creating an assembly from a part (first component fixed) | A + `t_creating_assembly_from_part.htm` | 2025 |
| S6 | Fixing the position of a component | A + `t_Fixing_the_Position_of_a_Component.htm` | 2025 |
| S7 | Inferencing to the assembly origin | A + `t_Inferencing_to_the_Assembly_Origin_SWassy.htm` | 2025 |
| S8 | Adding instances by dragging (instance number increments) | A + `c_Adding_Components_by_Dragging_and_Dropping.htm` | 2025 |
| S9 | Mates overview | A + `c_Mates_Overview_Assemblies.htm` | 2025 |
| S10 | Mate PropertyManager, Standard tab | A + `HIDD_NEW_ADD_MATE.htm` | 2025 |
| S11 | Standard mates by entity (the valid-pairs table) | A + `r_Standard_Mates_by_Entity.htm` | 2025 |
| S12 | Standard mates (index) | A + `c_Standard_Mates_SWassy.htm` | 2025 |
| S13 | Angle mate; reference entity for angle mates | A + `r_Angle_Mate_SWassy.htm`, `c_reference_entity_for_angle_mates.htm` | 2025 |
| S14 | Coincident mate | A + `r_Coincident_Mate_SWassy.htm` | 2025 |
| S15 | Concentric mate; preventing rotation | A + `r_Concentric_Mate_SWassy.htm`, `t_assemblies_mates_concentric_lockingrotation.htm` | 2025 |
| S16 | Distance mate; distance mates to cylinders | A + `r_Distance_Mate_SWassy.htm`, `c_assy_distance_mates_between_cylinders.htm` | 2025 |
| S17 | Lock mate | A + `r_Lock_Mate_SWassy.htm` | 2025 |
| S18 | Parallel and perpendicular mates; tangent mate | A + `r_Parallel_and_Perpendicular_Mates_SWassy.htm`, `r_Tangent_Mate_SWassy.htm` | 2025 |
| S19 | Mate alignment | A + `c_Mate_Alignment_SWassy.htm` | 2025 |
| S20 | Basic mate operations; default mate types; driven mate dimensions; mating to origins | A + `c_Basic_Mate_Operations.htm`, `t_assemblies_default_mate_types.htm`, `t_assy_driven_mate_dimensions.htm`, `c_Mating_to_Origins_and_Coordinate_Systems.htm` | 2025 |
| S21 | Adding mates (forcing a conflicting mate) | A + `t_Adding_Mates_SWassy.htm` | 2025 |
| S22 | Mates FAQ (red and yellow errors, (+) and (?)) | A + `c_FAQ_Mates.htm` | 2025 |
| S23 | Best practices for mates | A + `c_Best_Practices_for_Mates_SWassy.htm` | 2025 |
| S24 | Mate errors | A + `r_Mate_Errors_SWassy.htm` | 2025 |
| S25 | Identifying mate status; mate diagnostics overview | A + `t_Identifying_Mate_Status_SWassy.htm`, `c_MateXpert_Overview_SWassy.htm` | 2025 |
| S26 | Example: conflicting mates; example: redundant distance mates | A + `c_Conflicting_Mates_SWassy.htm`, `c_example_fixing_redundant_mates.htm` | 2025 |
| S27 | Techniques for fixing mate problems | A + `t_techniques_fixing_mate_problems.htm` | 2025 |
| S28 | Moving components (dragging within DOF) | A + `t_Moving_a_Component_SWassy.htm` | 2025 |
| S29 | Advanced mates; limit mates; width mates | A + `c_Advanced_Mates.htm`, `t_Limit_Mates_SWassy.htm`, `t_Width_Mates_SWassy.htm` | 2025 |
| S30 | `IMate2` members, `Alignment`, `Flipped`, `CanBeFlipped`, `MateEntity` | B + `IMate2_members`, `IMate2~Alignment`, `IMate2~Flipped`, `IMate2~CanBeFlipped`, `IMate2~MateEntity` | 2024 |
| S31 | `IMateEntity2` members, `EntityParams`, `ReferenceType2` | B + `IMateEntity2_members`, `IMateEntity2~EntityParams`, `IMateEntity2~ReferenceType2` | 2024 |
| S32 | `IComponent2::Transform2`, `IsFixed`, `Name2`, `GetConstrainedStatus` | B + `IComponent2~<member>` | 2024 |
| S33 | `IMathTransform::ArrayData` | B + `IMathTransform~ArrayData` | 2024 |
| S34 | `IAssemblyDoc::AddMate5` (obsolete; flip and limit remarks) and `CreateMate` | B + `IAssemblyDoc~AddMate5`, `IAssemblyDoc~CreateMate` | 2024 |
| S35 | `swMateType_e`, `swMateAlign_e`, `swMateEntity2ReferenceType_e`, `swConstrainedStatus_e`, `swAddMateError_e` | C + `<enum>` | 2024 |

## Assembly structure

- **Observed (S1, S3).** An assembly's components are parts or other assemblies
  (subassemblies). Adding one links the component's file to the assembly: the geometry
  stays in the component's own file and changes to that file show in the assembly.
- **Observed (S2, S8).** One file can be used many times. Each use is an instance, shown
  in the tree with a suffix `<n>` that increases with each new instance (`Bracket<1>`,
  `Bracket<2>`).
- **Observed (S32).** Through the API, the same instance is named with a hyphen
  (`Part1-1`), and a component inside a subassembly gets a path (`subAssem1-2/Part1-1`).
- **Observed (S4).** When inserting, the user picks the file and, for a file with several,
  its configuration.
- **Inferred.** A component is therefore a reference to (file, configuration) plus a
  placement. See [configurations.md](configurations.md).
- **Observed (S4, S7).** A component is placed where the user clicks. If the user drops it
  on the assembly origin, the component's origin coincides with the assembly origin and
  its planes line up with the assembly's planes (identity placement).
- **Observed (S32, S33).** A component's placement is a transform relative to the root
  assembly. It is held as 16 numbers: a 3×3 rotation, a translation, a scale and three
  unused values.
- **Observed (S32).** Setting a transform directly may break mates; the next rebuild moves
  the component to the nearest position that satisfies them.

### Fixed and floating

- **Observed (S5, S6).** The first component in an assembly is fixed by default, and the
  user can float it later. The insert options are "fix only the first", "fix" and
  "float" (S4).
- **Observed (S6).** A fixed component cannot move relative to the assembly origin. The
  help advises that at least one component be fixed or mated to the assembly's origin or
  planes, as a frame for the other mates.
- **Observed (S6).** Fixed or floating can vary by configuration.
- **Observed (S32).** The API reports the fixed flag for top-level components only. Seen
  from the root, all of a subassembly's components may move; their fixed or floating
  state is read from the subassembly document itself.
- **Inferred.** A floating component with no mates has six degrees of freedom: three
  translations and three rotations. A fixed component has none. The help never states
  the number six.

### Status of a component

- **Observed (S2, S6).** The tree prefixes a component name with its state:

  | Prefix | Meaning |
  |---|---|
  | `(f)` | fixed |
  | `(-)` | under defined (can still move) |
  | none | fully defined by mates |
  | `(+)` | over defined |
  | `(?)` | not solved |

- **Observed (S35, `swConstrainedStatus_e`).** The API's statuses are unknown (1), under
  constrained (2), fully constrained (3), over constrained (4), no solution (5), invalid
  solution (6) and autosolve off (7).

## Mates in general

- **Observed (S9, S22).** Mates are solved together as one system. The order in which they
  are added does not matter.
- **Observed (S9, S20).** Mates can be suppressed (not solved) like features. Deleting a
  mate deletes it in all configurations (S20).
- **Observed (S10).** Only mate types valid for the current selections are offered. For
  example, a concentric mate to a planar face is not available (S22).
- **Observed (S10).** "Use for positioning only" moves the components as the mate would,
  but adds no mate.
- **Observed (S20).** Default types: a cylinder or circular edge with an axis defaults to
  concentric, and two parallel planar faces default to coincident. If coincident would
  over define the assembly, parallel is chosen instead.
- **Observed (S22).** A mate to an analytic surface may be satisfied on the surface's
  infinite extension (a line touching a half-cylinder may end up beyond its edge). The
  advice is to move components near the intended position first.
- **Inferred.** So mates constrain the underlying *infinite* geometry (plane, line, axis,
  cylinder, sphere), not the bounded face or edge.
- **Observed (S31).** The API reduces each mate entity to a small record in assembly
  coordinates: a point; a line (point and direction); a plane (point and normal); a
  cylinder (point on axis, axis direction, radius); a cone (point on axis, axis, two
  radii). Entity kinds (`swMateEntity2ReferenceType_e`, S35) are point 0, line 1, circle
  2, plane 3, cylinder 4, sphere 5, set 6, cone 7, swept surface 8, multiple surface 9,
  general surface 10, ellipse 11, general curve 12, unknown 13.
- **Inferred.** That record is all a solver needs for the standard mates. A face, edge,
  vertex, reference plane, reference axis or origin each reduces to one of these kinds.
  The notes in S13 and S18 ("Cylinder refers to the cylinder's axis") agree.

### Alignment and flip

- **Observed (S10, S19).** Mates on faces have an alignment: *aligned* means the two
  normals point the same way; *anti-aligned* means they point opposite ways. For
  cylinders the axis direction is used, though the user cannot see which way it points.
- **Observed (S35, `swMateAlign_e`).** Aligned 0, anti-aligned 1, closest 2. Older
  members (`swAlignSAME`, `swAlignAGAINST`, `swAlignNONE`) are obsolete.
- **Inferred.** *Closest* means "whichever of the two is nearer to the current positions
  when the mate is created". The help does not define it.
- **Observed (S30, S34).** Distance and angle mates also have a *flip*. A distance or angle
  value has two solutions (either side); the mate takes the closer one, and flip moves to
  the other. `CanBeFlipped` reports whether flipping applies.
- **Observed (S23).** Alignment conflicts and dimension direction conflicts are a common
  cause of mate errors.
- **Inferred.** Alignment fixes the sign of the normals' dot product; flip fixes the sign
  of the signed offset or angle. For planes they overlap: a distance with the normals
  anti-aligned and positive offset is the usual "face to face with a gap".

## Standard mates

The valid pairs come from S11. The DOF column is **Inferred** for the generic case in which
the mate is independent of the others; see "Counting degrees of freedom". "Line" includes
straight edges and axes (S16 note). "Point" includes vertices and origins. "Plane"
includes planar faces and reference planes. "Cylinder" and "cone" are faces; for
parallel, perpendicular and angle mates they stand for their axis (S13, S18).

### Coincident

- **Observed (S10).** Puts faces, edges and planes on the same infinite plane (or a vertex
  on them), and makes two vertices touch.
- **Observed (S20).** Coincident between two origins or coordinate systems, with *align
  axes*, fully fixes one to the other.
- **Observed (S14).** Two cones must have the same half-angle.

| Pair (S11) | Constraint (Inferred) | DOF removed (Inferred) |
|---|---|---|
| plane–plane | coplanar, normals per alignment | 3 |
| plane–line | line lies in the plane | 2 |
| plane–point | point on the plane | 1 |
| plane–circular edge | edge's plane coincides with the plane | 3 |
| line–line | collinear | 4 |
| line–point | point on the line | 2 |
| line–cylinder | line lies on the cylinder (parallel to axis, at radius) | 3 |
| point–point | same point | 3 |
| point–cylinder / cone / sphere | point on the surface | 1 |
| circular edge–cylinder / cone | edge lies on the surface | Unknown |
| origin–origin, with *align axes* | frames coincide | 6 |

Pairs with curves, extrusions and non-analytic surfaces are valid in S11 but out of scope.

### Parallel

- **Observed (S10).** Keeps the items a constant distance apart. Non-analytic surfaces
  cannot be parallel (S18).

| Pair (S11) | Constraint (Inferred) | DOF removed (Inferred) |
|---|---|---|
| plane–plane | normals parallel | 2 |
| line–line (and cylinder/cone axes) | directions parallel | 2 |
| line–plane | direction perpendicular to the normal | 1 |

### Perpendicular

- **Observed (S10).** Puts the items at 90°.

| Pair (S11) | Constraint (Inferred) | DOF removed (Inferred) |
|---|---|---|
| plane–plane | normals perpendicular | 1 |
| line–line (and axes) | directions perpendicular | 1 |
| line–plane | direction parallel to the normal | 2 |

### Tangent

- **Observed (S10).** At least one selection must be a cylinder, cone or sphere.

| Pair (S11) | Constraint (Inferred) | DOF removed (Inferred) |
|---|---|---|
| plane–sphere | centre at radius from the plane | 1 |
| plane–cylinder | axis parallel to the plane, at radius | 2 |
| sphere–sphere | centres at r1 ± r2 | 1 |
| cylinder–sphere, line–sphere | centre at the right distance from the axis or line | 1 |
| cylinder–cylinder, line–cylinder | axes parallel, at r1 ± r2 (or r) | 3 |
| plane–cone, cone–sphere | contact along a line or point | Unknown |

- **Unknown.** Whether two cylinders, or a line and a cylinder, must have parallel axes
  for a tangent mate, or may touch at a point with skew axes. **Decision.** AnvilCAD
  treats them as line contact (parallel axes).
- **Inferred.** Whether contact is outside (r1 + r2) or inside (|r1 − r2|) follows from the
  alignment.

### Concentric

- **Observed (S10).** The selections share a centreline. *Lock rotation* also stops
  rotation about it, and is not allowed when a referenced component is over defined (S15).

| Pair (S11) | Constraint (Inferred) | DOF removed (Inferred) |
|---|---|---|
| cylinder–cylinder, cylinder–line, line–cone, cone–cone, cylinder–cone | axes collinear | 4 (5 with lock rotation) |
| circular edge–circular edge / line / cylinder / cone | axes collinear | 4 |
| point–cylinder / cone | point on the axis | 2 |
| sphere–sphere, point–sphere | centres coincide | 3 |
| sphere–cylinder, sphere–line | centre on the axis | 2 |

- **Unknown.** Whether concentric circular edges are also made coplanar. The help says
  only "share the same centreline", so AnvilCAD treats it as axes only.
- **Observed (S10).** Two concentric mates on parts whose holes are not the same distance
  apart can be made a *misaligned* pair that shares the error. Out of scope for now.

### Distance

- **Observed (S16).** The value defaults to the current distance. It can differ between
  configurations.
- **Observed (S16).** With a cylinder in the pair, distance is measured centre to centre by
  default, or as minimum, maximum or a per-cylinder mix of centre / min / max.
- **Observed (S14, S16).** Two cones must have the same half-angle.

| Pair (S11) | Constraint (Inferred) | DOF removed (Inferred) |
|---|---|---|
| plane–plane | parallel, offset d along the normal | 3 |
| plane–line | line parallel to the plane, at d | 2 |
| plane–point, plane–sphere | point (centre) at d from the plane | 1 |
| plane–cylinder | axis parallel to the plane, at d | 2 |
| line–line, line–cylinder, cylinder–cylinder | Unknown whether parallel is implied; see below | 1 or 3 |
| point–line, point–cylinder | point at d from the line / axis | 1 |
| point–point, point–sphere, sphere–sphere, line–sphere | points (centres) at d | 1 |

- **Unknown.** Whether a distance between two lines or axes forces them parallel (3 DOF)
  or constrains the minimum distance between skew lines (1 DOF). **Decision.** Parallel
  at distance d, the same as the plane–plane case, since S16's cylinder options describe
  parallel cylinders.
- **Observed (S22, S23, S26).** Distance and angle mates are the two types that may never
  be redundant (see below).

### Angle

- **Observed (S13).** The value defaults to the current angle and can differ between
  configurations. Valid pairs are plane–plane, line–line, line–plane and the axes of
  cylinders, cones and extrusions (S11, S13).
- **Observed (S13).** Without a *reference entity* an angle has two solutions and may flip
  unexpectedly. The reference entity gives the rotation axis: it must be held
  perpendicular to both mated entities. It can be found automatically once other mates
  fix that axis.

| Pair (S11) | Constraint (Inferred) | DOF removed (Inferred) |
|---|---|---|
| plane–plane, line–line, line–plane | signed angle between normals / directions | 1 |

- **Inferred.** The sign of the angle needs an axis; the reference entity supplies it.
  Without one, only the unsigned angle is defined and *flip* picks the side.

### Lock

- **Observed (S17).** Keeps the position and orientation between two components fixed; it
  acts as if the two were a rigid subassembly. It takes components, not entities.
- **Inferred.** Removes 6 DOF.

### Advanced mates (later)

- **Observed (S29).** Limit mates are distance or angle mates with a starting value, a
  minimum and a maximum; flip applies as for distance. The API takes the limits as
  absolute upper and lower values and treats equal limits as no limit (S34).
- **Observed (S29).** A width mate centres a tab (two planar faces, or a cylinder or axis)
  between two other faces. The options are centred, free, dimension and percent. Centred
  leaves the tab free to slide along the centre plane and turn about its normal.
- **Observed (S20).** Distance, angle, limit, width (and others) can be made *driven*:
  they report the value rather than set it.
- **Decision.** Limit mates, width mates, driven mate dimensions and the other advanced
  and mechanical mates (S29, `swMateType_e`) are later work.

## Solving and status

- **Observed (S25).** Each mate is *satisfied*, *not satisfied*, or *satisfied but over
  defining*.
- **Observed (S22, S24).** A red error means the mate is violated (the parts are not where
  it says) or dangling. A yellow warning means the mate is satisfied, but another mate is
  trying to violate it, or it is a redundant distance or angle mate.
- **Observed (S24).** Causes of a red error: another mate violates it, it is redundant
  with an existing distance or angle mate, or it tries to move a fixed component. The
  message can give the residual, for example how far apart two cylinders' centres are.
- **Observed (S24).** A mate whose entity no longer exists (feature edited, suppressed or
  deleted, part replaced) is *dangling* and reported as an error. Suppressing a whole
  part only makes its mates inactive.
- **Observed (S26, S23).** Redundant mates are allowed for all types except distance and
  angle. A coincident mate that duplicates a chain of other mates is tolerated, but a
  duplicate distance or angle is over defining even when it agrees with the rest.
- **Inferred.** That is because a driving value could later be edited into conflict; the
  same rule applies to sketch dimensions (S22).
- **Observed (S21).** When a new mate conflicts, the user chooses: solve the new mate and
  break the old one, or keep the old one and leave the new mate unsolved. Either way the
  broken mate shows an error.
- **Observed (S26).** In the conflict example, the diagnosis names all the mates involved
  in the conflict and the residual (how far apart the faces are).
- **Observed (S23, S27).** Solving depends on the starting position: components dragged
  near the answer solve more reliably, and dragging can clear an error.
- **Observed (S28).** Dragging moves a component only within its remaining degrees of
  freedom. A fixed or fully defined component cannot be dragged. Moves can also be along
  an entity (1 DOF on a line, 2 on a plane), by a delta or to a coordinate.
- **Observed (S35, `swAddMateError_e`).** Creating a mate can fail with: unknown, wrong
  type, wrong alignment, wrong selections, gear ratios, or over-defining the assembly.

### Counting degrees of freedom

- **Inferred.** The DOF a mate removes are its independent equations, not a fixed number.
  A second mate that repeats what the first already does removes nothing (redundant).
  The robust count is the rank of the mate equations' Jacobian with respect to the
  components' rigid motions, at the solved position. A component's remaining DOF is
  6 times the number of floating components minus that rank; under defined means rank
  below that.

## The API view (data model only)

These help define what an assembly file must hold. Names and meanings only.

- **Observed (S30).** A mate (`IMate2`) has a type, an alignment, a flip flag (distance and
  angle), a display dimension (its value), minimum and maximum variation (limits, in
  metres or radians), cylinder distance conditions, and an ordered list of mate entities
  (`MateEntity(index)`, `GetMateEntityCount`).
- **Observed (S31).** A mate entity (`IMateEntity2`) has a reference component, the
  reference (the face, edge, plane and so on), its kind (`ReferenceType2`) and the
  parameter record above, in assembly coordinates.
- **Observed (S35, `swMateType_e`).** Coincident 0, concentric 1, perpendicular 2,
  parallel 3, tangent 4, distance 5, angle 6, unknown 7, symmetric 8, cam follower 9,
  gear 10, width 11, lock to sketch 12, rack and pinion 13, max mates 14, path 15,
  lock 16, screw 17, linear coupler 18, universal joint 19, coordinate 20, slot 21,
  hinge 22, slider 23, profile centre 24, magnetic 25.
- **Inferred.** Limit mates have no type of their own: they are distance or angle mates
  whose minimum and maximum differ (S30, S34).
- **Observed (S32, S33).** A component has a name with instance number, a fixed flag, a
  constrained status and a transform (rotation, translation, scale).
- **Unknown.** Whether the 3×3 rotation in `ArrayData` is stored by rows or by columns,
  and whether scale is ever not 1 for a component.

## AnvilCAD decisions

AnvilCAD's names: an **Assembly** document holds **Components**, positioned by **Mates**.
The mate types keep their generic geometric names (coincident, parallel, perpendicular,
tangent, concentric, distance, angle, lock). Diagnostics are AnvilCAD's own **Mate
diagnostics** panel, not a copy of any SOLIDWORKS tool.

### Data model

- **Decision.** An assembly is a list of components and a list of mates, plus its own
  origin and three planes:
  - **Component:** a stable id; a file reference (path relative to the assembly, plus a
    content hash for checking); a configuration name; an instance number (shown as
    `name<n>`, numbered per file); a rigid transform to the assembly frame (rotation as a
    unit quaternion or orthonormal matrix, and a translation, in millimetres; no scale); a
    fixed flag; a suppressed flag.
  - **Mate:** a stable id and name (`Coincident1`); a type; two entity references (one
    component for lock); an alignment (aligned, anti-aligned; *closest* is resolved to one
    of these when the mate is created); for distance and angle a value and a flip flag;
    for angle an optional reference axis; for concentric a lock-rotation flag; a
    suppressed flag.
  - **Entity reference:** the component id (or the assembly itself, for its origin and
    planes) plus a stable reference into that part: a reference plane, axis, point or
    coordinate system feature, the part origin, or a face, edge or vertex by its
    persistent topological name. Alongside it the file caches the entity's last known
    geometry record (kind, point, direction, radius) in the part's own coordinates, so a
    mate can report itself as dangling, with what it used to point at, when the reference
    no longer resolves.
- **Decision.** Every mate entity is reduced, at solve time, to one of: point, line
  (point and unit direction), plane (point and unit normal), cylinder (axis line and
  radius), cone (axis, apex, half-angle), sphere (centre and radius). The solver sees only
  these, transformed by the component's current placement. Mates are on infinite geometry
  (S22).
- **Decision.** Subassemblies are rigid by default: one component with one transform.
  A subassembly can be made flexible; see "Subassemblies" below.
- **Decision.** Inserting a component puts it at the point clicked, unrotated, or at the
  assembly origin with identity rotation if dropped there (S7). The first component is
  fixed; later ones float (S4, S5).

### First mates

- **Decision.** First: coincident, parallel, perpendicular, concentric (with lock
  rotation), distance, angle and lock, with aligned / anti-aligned and flip, over the
  point, line, plane, cylinder and sphere pairs in the tables above. Then tangent. Cones,
  circular edges, curves, extrusions and non-analytic surfaces come later, as do limit,
  width and the mechanical mates.
- **Decision.** A mate is offered only for valid pairs (S10). Its value defaults to the
  current measurement (S13, S16), and its alignment and flip default to the ones nearer
  the current positions.

### Solver

- **Decision.** A numeric least-squares solver over the floating components' rigid
  transforms:
  - unknowns: for each floating component, a small twist (three translations, three
    rotations) applied to its current placement; fixed components have none;
  - each mate gives residual equations from its constraint (signed distances, dot or
    cross products of directions, signed angle about the reference axis), with the
    alignment and flip choosing the branch;
  - Levenberg–Marquardt (damped Gauss–Newton) iterations from the current positions, so
    the answer nearest to where the user left things is found (S23, S27);
  - components joined by no mates are solved independently, and a lock mate merges two
    components into one rigid group before solving.
- **Decision.** After solving, rank the Jacobian (singular value decomposition with a
  tolerance) to find each component's remaining DOF and redundant equations. Report:
  - **under defined** `(-)` when its DOF are above zero;
  - **fully defined** (no prefix) at zero;
  - **fixed** `(f)` for fixed components;
  - **over defined** `(+)` when a satisfied mate is redundant and is a distance or angle,
    as a warning (S26);
  - **not satisfied** when a residual stays above tolerance, as an error naming the mate,
    the residual (for example "faces are 2.40 mm apart") and the other mates on the same
    components (S24, S26);
  - **dangling** when an entity reference does not resolve (S24).

  Redundant coincident, parallel, perpendicular, concentric and lock mates that are
  satisfied are allowed silently (S26).
- **Decision.** A failed mate is a reported error, never a panic, and leaves the other
  components solved as well as they can be.
- **Decision.** Dragging solves with the dragged point pulled towards the pointer as a soft
  goal, so a component moves only within its remaining DOF (S28).
- **Decision.** A new mate that conflicts asks, as in S21, whether to keep it and break the
  older mate or leave the new one unsolved.

## Interference and clearance

What AnvilCAD's interference check should do: find components whose bodies overlap, with
the volume of each overlap, and optionally pairs that are closer than a clearance. It was
written for issue #66 and `crates/anvil-model/src/interference.rs`, and studied the same
way as the rest of this file (published help only, retrieved and paraphrased as described
under "How this was studied", REA's method of marking each finding, REA's analysis tools
not run because the inputs are prose). Sources S36 to S46 were read on 2026-11-25.

| # | Topic | URL | Version |
|---|---|---|---|
| S36 | Interference Detection (overview) | A + `c_interference_detection.htm` | 2025 |
| S37 | Interference Detection PropertyManager | A + `HIDD_DVE_INTERFERENCE.htm` | 2025 |
| S38 | Detecting interferences between assembly components | A + `t_detecting_interferences.htm` | 2025 |
| S39 | Excluding components and bodies | A + `c_Excluding_Components_and_Bodies.htm` | 2025 |
| S40 | Ignoring selected interferences | A + `t_interference_detection_wn.htm` | 2025 |
| S41 | Clearance Verification (overview) | A + `c_Clearance_Verification_SWassy.htm` | 2025 |
| S42 | Clearance Verification PropertyManager | A + `HIDD_DVE_CLEARANCE.htm` | 2025 |
| S43 | Checking static clearances between components | A + `t_Clearances_verifying_in_assemblies.htm` | 2025 |
| S44 | Component suppression states | A + `c_Component_Suppression_States_SWassy.htm` | 2025 |
| S45 | `IInterference` members, `Volume`, `IsPossibleInterference` | B + `IInterference_members`, `IInterference~Volume`, `IInterference~IsPossibleInterference` | 2024 |
| S46 | `IInterferenceDetectionMgr` members, `TreatCoincidenceAsInterference`, `IgnoreHiddenBodies` | B + `IInterferenceDetectionMgr_members`, `IInterferenceDetectionMgr~<member>` | 2024 |

### What is checked

- **Observed (S37).** By default the whole top-level assembly is checked. With one
  component selected, only interferences involving it are reported; with two or more,
  only those between the selected components.
- **Observed (S37, S39).** Components can also be excluded from the check.
- **Observed (S37, S36).** Interferences between bodies of one multibody part are reported
  only when asked for. A subassembly can be treated as one component, so interferences
  inside it are not reported.
- **Observed (S37, S39).** *Ignore hidden bodies/components* leaves out interferences that
  involve hidden components (including ones hidden by Isolate) and between a multibody
  part's hidden bodies and other components. **Observed (S46).** The API calls it
  `IgnoreHiddenBodies`; its setting does not persist between runs.
- **Unknown.** Whether *Ignore hidden bodies/components* is on or off by default.
- **Observed (S44).** A suppressed component is not loaded and is not a functional member
  of the assembly; it cannot be seen or selected.
- **Inferred.** So a suppressed component takes no part in an interference check.
- **Observed (S37).** Surface bodies can be included, but their interferences have no
  volume.
- **Observed (S37).** The check can be cancelled while it runs (Cancel or Esc).

### What is reported

- **Observed (S36, S37, S38).** Each interference is listed with its volume. Expanding it
  shows the two interfering components; a component view lists by component instead of
  by interference number. **Observed (S45).** An interference has its components, a
  volume, an ignore flag, a fastener flag and a "possible interference" flag, and its
  interfering volume can be fetched as a body.
- **Observed (S36, S37).** The true volume of interference can be shown as a shaded volume;
  selecting an interference highlights it in red. The other components can be shown
  wireframe, hidden, transparent or as they are, and the interfering ones transparent.
- **Observed (S38, S40).** Results can be sorted by volume, zoomed to, ignored (ignores are
  remembered between runs), and every interference smaller than a chosen one ignored.
  They can be saved to a spreadsheet.
- **Observed (S37).** Interferences between fasteners can go to a folder of their own.
- **Observed (S37).** In its reduced large-design mode the check is approximate and has no
  volumes, and curved faces that coincide or nearly touch can give false positives where
  their facets collide.
- **Inferred.** The full mode therefore works on the exact geometry, not on facets.

### Coincident versus overlapping

- **Observed (S36, S37, S46).** Coincidence (faces or other entities that touch without
  material overlapping) is told apart from interference. *Treat coincidence as
  interference* reports coincident entities as interferences as well.
- **Unknown.** What volume a coincidence carries in the results (presumably none), how it
  is drawn, and the option's default.
- **Unknown.** What `IsPossibleInterference` (S45) means; it may be the coincidence case.

### Clearance

- **Observed (S41, S42).** Clearance verification is a separate tool. It measures the
  minimum distance between components (or chosen faces), between the selected items or
  between them and the rest of the assembly, and reports every clearance that is less
  than or equal to a minimum acceptable value. An option drops the "equal" case.
- **Observed (S42, S43).** Each failed clearance is listed with its value; selecting one
  highlights it. Clearances can be ignored, and subassemblies treated as components.
- **Unknown.** What clearance verification reports for components that touch or overlap.
- **Unknown.** Whether the minimum distance is exact or found on facets.

### AnvilCAD decisions

AnvilCAD's names: **Interference** for the command and its panel; each result is a
**finding** of kind **overlap**, **touching** or **clearance**.

- **Decision.** One command checks both: overlaps always, touching pairs on request, and
  pairs closer than a clearance when one is given. Each pair of components appears at
  most once, as its most serious kind: overlap, then touching, then clearance.
- **Decision.** An overlap's volume is the exact volume common to the two components,
  from the exact kernel's boolean intersection and its exact mass integrals. Where a part
  is only faceted, or the exact intersection fails, the faceted bodies are intersected
  instead and the finding is marked approximate. A component's own bodies are not checked
  against each other.
- **Decision.** Two components *touch* when no volume above 1e-6 mm³ is common to them
  and their boundaries are within 1e-6 mm. Touching is listed only when asked for (off by
  default in the app, as the coincident faces of a mated assembly are expected), and
  never as an overlap.
- **Decision.** A clearance finding is a pair whose minimum distance is above the touching
  tolerance and at most the clearance (inclusive, as S42). The distance is found on the
  tessellations and refined on the exact surfaces, as Measure does
  (`docs/re/measure-and-section.md`), so it is exact for planes, cylinders and spheres.
  The two closest points are reported.
- **Decision.** Hidden components are left out unless asked for. No component is ever
  suppressed in AnvilCAD yet; when suppression comes it will leave the component out
  (S44).
- **Decision.** Like S37, the check covers every pair, or with a list of components, the
  pairs involving one of them when one is given and the pairs among them when several are.
- **Decision.** The check runs off the UI thread, in parallel over pairs, with progress and
  Cancel. Results are dropped when the assembly changes, since they no longer describe
  it.
- **Decision.** In the view, while results are shown, components are drawn as wireframe
  (S37's "wireframe" option; the renderer has no transparency) and overlap volumes shaded,
  the selected one red. Touching and clearance findings show a line between their closest
  points. Clicking a finding in the list or its volume in the view selects it.
- **Decision.** Ignore lists, sorting, spreadsheet export, fastener folders and surface
  bodies are not part of the first version. Subassemblies (#93) will be checked as their
  leaf components, with an option to treat one as a single component.

## Subassemblies

What AnvilCAD's subassemblies should do: an assembly placed as a component of another,
rigid or flexible, with mates in the parent that reach geometry inside it, and bills of
materials that list it at the top level, as its parts, or indented. It was written for
issue #93 and studied the same way as the rest of this file: published help only,
retrieved and paraphrased as described under "How this was studied", with REA's method of
marking each finding (`rea-agents` 6.10.1, `third_party/rea` at `3fc0075`). REA's analysis
tools were not run, because the inputs are prose; no SOLIDWORKS file was read. Sources S47
to S60 were read on 2026-11-25.

| # | Topic | URL | Version |
|---|---|---|---|
| S47 | Subassemblies (section index) | A + `c_Subassemblies_Intro.htm` | 2025 |
| S48 | Working with subassemblies (creating, nesting) | A + `c_Working_with_Sub-assemblies.htm` | 2025 |
| S49 | Flexible subassemblies; making a subassembly flexible | A + `c_Flexible_Sub-Assemblies.htm` | 2025 |
| S50 | Effects of assembly structure editing (where mates live) | A + `c_Effects_of_Assembly_Structure_Editing.htm` | 2025 |
| S51 | Forming a new assembly from existing components | A + `t_forming_new_assembly.htm` | 2025 |
| S52 | The design tree in an assembly; component icons | A + `c_featuremanager_design_tree_assembly.htm`, `c_component_icons_fmdt.htm` | 2025 |
| S53 | Search routine for referenced documents | A + `c_Search_Routine_for_Referenced_Documents.htm` | 2025 |
| S54 | Bill of Materials PropertyManager (BOM type, numbering) | A + `HIDD_DVE_INSERT_BOM.htm` | 2025 |
| S55 | Bill of Materials overview; table-based BOMs; default BOMs | A + `c_Bill_of_Materials1.htm`, `c_table_based_bom_top.htm`, `c_Default_BOMS.htm` | 2025 |
| S56 | Indented BOMs; item numbers; expanding and collapsing | A + `c_indented_boms.htm`, `t_setting_item_numbers_boms.htm`, `t_expanding_assemblies_bom.htm` | 2025 |
| S57 | Component Options dialog box (child components show, hide, promote) | A + `HIDD_BOM_COMPONENT_OPTIONS.htm` | 2025 |
| S58 | `IComponent2::Solving`, `IComponent2::GetChildren` | B + `IComponent2~Solving`, `IComponent2~GetChildren` | 2024 |
| S59 | `swComponentSolvingOption_e` | C + `swComponentSolvingOption_e` | 2024 |
| S60 | `swBomType_e`, `swNumberingType_e` | C + `swBomType_e`, `swNumberingType_e` | 2024 |

### Inserting an assembly

- **Observed (S47, S48).** An assembly that is a component of another is a subassembly.
  Subassemblies nest to any number of levels. One way to make one is to save an assembly
  on its own and insert it into a higher-level assembly as a component.
- **Observed (S1, S3).** As with parts, the component links to the subassembly's file;
  changes to that file show in the parent.
- **Observed (S32, S58).** Through the API a component's children are listed
  (`GetChildren`), and a component inside a subassembly is named by its path from the
  top (`subAssem1-2/Part1-1`).
- **Observed (S52).** The tree shows subassemblies and parts together under the top-level
  assembly, each expandable, with its own icon for a subassembly and another for a
  flexible one.
- **Observed (S51).** Forming a subassembly from components already in the assembly moves
  those components down one level. **Decision.** Forming, dissolving and reorganising are
  later work.
- **Unknown.** What SOLIDWORKS does when an assembly is inserted into itself, directly or
  through another subassembly. Nothing read describes it. **Inferred.** Such a structure
  has no finite expansion, so it cannot be opened, and must be refused.

### Missing files

- **Observed (S53).** When a referenced document cannot be found where it was saved, a
  fixed search order is tried: open documents of the same name, a list of configured
  folders, the active document's folder and the folders around it, the last folder used,
  then the saved path with and without its drive, and finally the user is asked to browse.
  A file that is found has its new path written into the parent on the next save.
- **Unknown.** What the assembly shows for a component whose file is never found.
- **Decision.** AnvilCAD looks for a component's file only at its path relative to the
  assembly (paths are relative, so moving a whole project keeps working). A missing file
  never stops the assembly opening: the component stays in the tree marked with an error
  that names the missing file, everything else loads and solves, and mates to the missing
  component are reported as unresolved. The same holds at every level of nesting. The
  search routine and a "find it" prompt are later work.

### Rigid and flexible

- **Observed (S49, S58, S59).** A subassembly is solved either *rigid* (0) or *flexible*
  (1); the setting applies to subassembly components only (a part reports −1). It is set
  per component in the parent, and per configuration.
- **Inferred (S17, S49).** Rigid means the subassembly moves as one body: its own mates
  are solved inside it and its components keep their positions relative to it.
- **Observed (S49).** In a flexible subassembly, its mates are solved at the same time as
  the parent's, so its components can move within their own degrees of freedom inside the
  parent without breaking the subassembly's mates or the parent's.
- **Observed (S49).** Several instances of one subassembly can be in the same parent; some
  can be rigid and others flexible, and flexible instances can hold different positions
  of their components.
- **Inferred.** So the positions of a flexible instance's components belong to the parent,
  not to the subassembly's file, which keeps its own positions for when it is opened on its
  own or used rigid.
- **Observed (S32).** Seen from the root, a subassembly's fixed or floating flags are read
  from the subassembly's own document.
- **Inferred.** A component fixed in the subassembly is fixed relative to the
  subassembly's frame, which itself moves with the subassembly component in the parent.
- **Unknown.** Whether a flexible subassembly nested in a flexible subassembly is also
  solved in the top-level assembly, and how the solve treats a fixed component inside a
  flexible subassembly. **Decision** below.

### Mates to geometry inside a subassembly

- **Observed (S50).** A mate lives in the mates folder of the lowest common parent of the
  components it joins: a mate between components in two different branches lives in the
  top-level assembly.
- **Inferred (S50, S31).** So a mate in a parent can pick a face, edge or plane of a part
  nested inside a subassembly, and the mate entity's reference component is that nested
  part, not the subassembly.
- **Inferred.** Against a rigid subassembly, such a mate moves the whole subassembly; it
  cannot move the nested part on its own. Against a flexible one, it moves the nested part
  within its freedom in the subassembly.

### Bills of materials

- **Observed (S54, S60).** The BOM types are *top-level only* (2), *parts only* (1) and
  *indented* (3), plus a *flattened* option for indented BOMs.
  - *Top-level only* lists the parts and subassemblies of the assembly itself, but not the
    components of its subassemblies.
  - *Parts only* lists subassembly components as items of their own and does not list
    subassemblies.
  - *Indented* lists subassemblies, with their components indented below them. Item
    numbering for indented BOMs is none (0), detailed (1) or flat (2).
- **Observed (S55).** The default BOM has the columns item number, part number,
  description and quantity.
- **Observed (S54, S55).** Quantity is the number of instances, unless a custom property
  is chosen as a multiplier. Quantities are per configuration.
- **Observed (S56).** In an indented BOM, a subassembly can be collapsed to show only its
  own row.
- **Observed (S57).** A subassembly's child components can be shown, hidden (the
  subassembly is one item) or promoted (the subassembly row is dissolved and its children
  move up a level).
- **Unknown.** What *detailed* and *flat* numbering look like (the help shows them as
  pictures), and whether the quantity on an indented child row counts per parent or in
  all. **Decision** below.

### AnvilCAD decisions

AnvilCAD's names: a **subassembly** is a component whose file is an `.anvasm`; it is
**rigid** or **flexible**. The BOM views are **Top level**, **Parts only** and
**Indented**.

- **Decision.** A component's file can be an `.anvasm` as well as an `.anvprt`, with the
  same relative path, configuration-free for now (assemblies have no configurations yet).
  Its placement places the subassembly's own frame (its origin and planes). A nested
  component's path is resolved relative to the file of the assembly that holds it.
- **Decision.** Opening or inserting an assembly that contains itself, at any depth, is
  refused with a message that names the chain of files ("a.anvasm contains itself through
  b.anvasm"). The rest of the assembly still opens; the offending component is marked like
  a missing file.
- **Decision.** A subassembly is solved when it is loaded, with the mates in its own file,
  and is then rigid in the parent. A rigid subassembly is one body to the parent's solver.
- **Decision.** A flexible subassembly's direct components are solved in the parent, with
  the subassembly's own mates. The subassembly component itself stands for the
  subassembly's frame: components fixed in the subassembly's file are held to that frame,
  and mates in the subassembly to its own planes or origin use it. The solved positions
  of the flexible instance's components are stored in the parent, per instance, relative
  to the subassembly's frame; the subassembly's file is not changed. One level is
  flexible: a subassembly nested inside a flexible subassembly is rigid within it.
- **Decision.** A mate side names a component of the assembly and, when the entity is
  inside a subassembly, the path of component ids down to the part that owns it. Against a
  rigid subassembly the entity's shape is carried up through the nested placements, so the
  mate moves the subassembly; against a flexible one it acts on the nested part.
- **Decision.** The tree lists a subassembly's components under it, indented and read
  only (their own mates are edited in the subassembly's file). Hiding a subassembly hides
  all of it.
- **Decision.** The BOM has three views. *Top level* is the existing parts list, now with
  subassemblies as rows of their own. *Parts only* lists every part at every depth, one
  row per file and configuration, with the total quantity in the whole assembly.
  *Indented* lists the top level, with each subassembly's rows below it, indented one
  level per depth, numbered in detailed style (`2`, `2.1`, `2.2`), and the quantity on a
  child row counts it per one of its parent subassembly. Rows group identical files within
  one parent. A subassembly's part number is its own `PartNo` property or its file name,
  as for parts. Quantity multipliers, collapsing, hiding and promoting are later work.
- **Decision.** The interference check sees through subassemblies: it checks the parts
  inside them, at every depth, against each other and against everything else, and names
  each by its path (`arm<1>/link<2>`). An option checks each top-level subassembly as one
  component instead, so interferences inside it are not reported (S37).
- **Decision.** Assembly files move to format version 2: components gain a `flexible`
  flag and the stored positions of a flexible instance's components, and mate sides an
  optional path inside a subassembly. Version 1 files read unchanged; they hold no
  subassemblies.

## Unknown or not covered

1. How `.sldasm` files store components, transforms and mates. Nothing here was learnt
   from files.
2. The DOF counts: all inferred; the help gives none.
3. Whether concentric circular edges are also made coplanar.
4. Whether tangent cylinders, and distance mates between lines or axes, require parallel
   axes.
5. The exact meaning of *closest* alignment, and of alignment for point and line entities.
6. The row or column order of `IMathTransform::ArrayData`'s rotation.
7. How SOLIDWORKS chooses among solutions when nothing (alignment, flip, reference entity)
   decides, beyond "nearest to the current position".
8. Component patterns, mirrored components, mate references and automatic mates on
   dragging, none of which were studied. For subassemblies, see the unknowns under
   "Subassemblies".
9. Interference and clearance: the defaults of *Treat coincidence as interference* and
   *Ignore hidden bodies/components*, what a coincidence reports, what a possible
   interference is, and what clearance verification reports for touching or overlapping
   components (see "Interference and clearance").
10. Subassemblies: what SOLIDWORKS does with an assembly inserted into itself, what it
    shows for a component whose file is never found, how flexible subassemblies nest and
    treat their fixed components, and what detailed and flat BOM numbering look like (see
    "Subassemblies").
