# Sketch Dimension tool

What AnvilCAD's Dimension tool does: one sketch command that works out which dimension to
add from what the user picks and where they place the value. SOLIDWORKS users know the
equivalent tool well, so AnvilCAD matches its behaviour where the published documentation
describes it. This specification was written before the implementation in
`crates/anvil-sketch/src/dimension.rs` and `crates/anvil-app/src/sketcher.rs`.

## How this was studied

- **Inputs.** Only published SOLIDWORKS information: Dassault's public online help for
  SOLIDWORKS 2025, one 2017 help page, and the 2024 API help (sources below). No SOLIDWORKS
  software, installer, library or file was downloaded, installed, run or analysed, and no
  SOLIDWORKS licence was accepted on the machine used.
- **Retrieval.** The help site renders its articles with JavaScript, so a plain page fetch
  returns only the navigation shell. Each article was fetched over HTTPS and its text read
  from the page's embedded `__NEXT_DATA__` JSON (`props.pageProps.helpContentData.helpText`).
  The text was then paraphrased. None of it is copied into this repository.
- **REA.** REA (`rea-agents` 6.5.0, with the `third_party/rea` submodule at `3fc0075`) was
  set up and its research method followed:
  - each finding below is marked **Observed** (stated in a cited source), **Inferred** (our
    reasoning from the sources), **Decision** (AnvilCAD's own choice where the sources are
    silent) or **Unknown**;
  - every observation cites its source.

  REA's analysis tools were **not** run. They inspect artifacts (binaries, packages,
  captures, files), and the only lawful inputs for a user-interface behaviour like this one
  are prose documents. Pointing REA at SOLIDWORKS software is ruled out by
  [INTEROPERABILITY.md](../../INTEROPERABILITY.md). No SOLIDWORKS sample file carries this
  behaviour.

## Sources

Base A is `https://help.solidworks.com/2025/English/SolidWorks/sldworks/` (SOLIDWORKS 2025
user help). Base B is `https://help.solidworks.com/2024/English/api/sldworksapi/`
(SOLIDWORKS 2024 API help). All were read on 2026-10-17.

| # | Topic | URL | Version |
|---|---|---|---|
| S1 | Dimensioning a 2D sketch | A + `t_Dimensioning_a_2D_Sketch.htm` | 2025 |
| S2 | Dimensions overview | A + `c_dimensions_top.htm` | 2025 |
| S3 | Angular dimensions | A + `c_Angular_Dimensions.htm` | 2025 |
| S4 | Angular dimensions between two lines | A + `t_Creating_Angular_Dimensions_Between_Two_Lines.htm` | 2025 |
| S5 | Angular dimensions between three points | A + `t_Creating_Angular_Dimensions_Between_Three_Points.htm` | 2025 |
| S6 | Angular dimensions using an imaginary line | A + `t_ang_dims_imag_line.htm` | 2025 |
| S7 | Symmetrical angular dimensions | A + `t_create_sym_ang_dims.htm` | 2025 |
| S8 | Circular dimensions | A + `t_Creating_Circular_Dimensions.htm` | 2025 |
| S9 | Arc dimensions | A + `t_creating_arc_dimensions.htm` | 2025 |
| S10 | Dimensions between two points | A + `t_Creating_Dimensions_Between_Two_Points.htm` | 2025 |
| S11 | Dimensions between arcs or circles | A + `c_Dimensions_Between_Arcs_or_Circles.htm` | 2025 |
| S12 | Dimensions between two arcs | A + `t_creating_dimensions_edges_arcs.htm` | 2025 |
| S13 | Changing how a distance is measured | A + `t_changing_distance_measured.htm` | 2025 |
| S14 | Concentric circles | A + `t_creating_dimensions_concentric_circles.htm` | 2025 |
| S15 | Two points of the same arc | A + `t_dimension_two_points_same_arc.htm` | 2025 |
| S16 | Centerlines for radial and diametric dimensions | A + `t_dimensioning_centerlines.htm` | 2025 |
| S17 | Maintaining proportions | A + `c_maintain_proportion.htm` | 2025 |
| S18 | Horizontal dimensions | A + `t_Creating_Horizontal_Dimensions.htm` | 2025 |
| S19 | Modify dimension box | `https://help.solidworks.com/2017/English/SolidWorks/Sldworks/HIDD_DYNPARAM_TOOLS_DLG.htm` | 2017 (the 2024 and 2025 pages were empty) |
| S20 | Equations in the modify box | A + `c_create_equations_modify_dialog.htm` | 2025 |
| S21 | Creating an equation | A + `t_creating_equation.htm` | 2025 |
| S22 | Creating a global variable | A + `c_create_global_variable_on_the_fly.htm` | 2025 |
| S23 | Modifying an equation | A + `t_modifying_an_equation.htm` | 2025 |
| S24 | Zero and negative values | A + `t_zero_negative_dimensions.htm` | 2025 |
| S25 | Editing dimensions in place | A + `t_Modifying_2D_Sketch_Dimension_Instant2D.htm` | 2025 |
| S26 | Relations and snaps options (over-define prompt) | A + `r_relations_snaps_options.htm` | 2025 |
| S27 | Sketch options | A + `HIDD_OPTIONS_SKETCH.htm` | 2025 |
| S28 | Resolving over-defined sketches | A + `c_Resolving_Over_Defined_Sketches.htm` | 2025 |
| S29 | Automatic driven dimensions | A + `c_driven_dimensions.htm` | 2025 |
| S30 | Reference dimensions | A + `c_Reference_Dimensions.htm` | 2025 |
| S31 | Inserting driven dimensions | A + `t_Inserting_Reference_Dimensions.htm` | 2025 |
| S32 | Sketch status conventions | A + `c_Sketch_Status_Conventions.htm` | 2025 |
| S33 | Sketch geometry status | A + `c_Sketch_Geometry_Status.htm` | 2025 |
| S34 | Overriding dimensions on drag | A + `c_Override_Dims_on_Drag_Move.htm` | 2025 |
| S35 | `IModelDoc2::AddDimension2` | B + `SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.IModelDoc2~AddDimension2.html` | 2024 |
| S36 | `IModelDoc2::AddDiameterDimension2` | B + `SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.IModelDoc2~AddDiameterDimension2.html` | 2024 |
| S37 | `IModelDoc2::AddHorizontalDimension2` | B + `SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.IModelDoc2~AddHorizontalDimension2.html` | 2024 |
| S38 | `IDimension::SystemValue` | B + `SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.IDimension~SystemValue.html` | 2024 |
| S39 | `IDimension::DrivenState` | B + `SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.IDimension~DrivenState.html` | 2024 |
| S40 | `IDimension::SetSystemValue3` | B + `SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.IDimension~SetSystemValue3.html` | 2024 |
| S41 | `IDisplayDimension` | B + `SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.IDisplayDimension.html` | 2024 |

## Behaviour

### One tool, the type follows the picks

- **Observed (S1).** What the user picks decides the kind of dimension. For distances
  between points, angles and circles, where the value is placed also decides it. The
  default linear kind is aligned. The preview snaps to the nearest kind as the pointer
  moves. Esc takes back the last pick.
- **Observed (S1, S11).** Distances to a circle or arc go to its centre by default, even
  when the circumference was clicked.

| Picks | Dimension | Placement | Status |
|---|---|---|---|
| One line | Its length | Beside the line: aligned length. Inferred: above or below its span, its horizontal extent; left or right of it, its vertical extent | Observed (length, S1); Inferred (placement) |
| Two points | Distance | Horizontal, vertical or aligned by placement | Observed (S1, S10) |
| Point and line | Perpendicular distance | — | Observed (S1) |
| Two parallel lines | Distance between them | — | Observed (S1) |
| Two crossing lines | Angle | Which of the angles depends on the pointer | Observed (S1, S3, S4) |
| Circle | Diameter | — | Observed (S1) |
| Arc | Radius | — | Observed (S9) |
| Arc and both its end points | Length along the arc | — | Observed (S1, S9) |
| Circle or arc with a point, line, circle or arc | Distance from the centre | As for two points | Observed (S1, S11, S12) |
| Point or line and a centerline | Distance to the centerline on the near side, diameter (twice the distance) on the far side | Side of the centerline | Observed (S1, S16) |

### Placement zones

- **Unknown.** The sources show the zones only in pictures. They give no thresholds.
- **Decision.** Take the axis-aligned box spanned by the two measured points:
  - placed above or below it (inside its x-span, outside its y-span): horizontal;
  - placed left or right of it (inside its y-span, outside its x-span): vertical;
  - otherwise: aligned.
- **Decision.** Two points that already line up horizontally or vertically only have the
  one distance.
- **Decision.** A single line that is already horizontal or vertical always gets its
  length, and so does a line whose value is placed in an aligned zone. Its length is what
  the user means, and it survives the line later being rotated.

### Angles

- **Inferred.** The two lines divide the plane into four sectors. The angle shown is the
  angle of the sector the pointer is in, so it is either θ or 180° − θ.
- **Decision.** AnvilCAD stores the angle as a counter-clockwise angle between two chosen
  rays (`RayAngle`), so the dimension keeps driving the sector it was placed in.
- **Not implemented here.** Angles from three points (S5), to an imaginary horizontal or
  vertical (S6), and symmetric angles about a centerline (S7).

### Circles and arcs

- **Observed (S12, S13, S14).** Distances between circles or arcs can measure to the
  edges instead of the centres, and each end can be set to its minimum or maximum point.
- **Decision.** Not implemented. AnvilCAD always measures centre to centre.

### Value entry

- **Observed (S19).** The value box opens when the dimension is placed. It accepts numbers
  and arithmetic, and units can be typed with the number.
- **Observed (S20–S23).** Input starting with `=` is an equation, which can use global
  variables and stays linked to the dimension.
- **Observed (S19).** Cancelling restores the original value.
- **Observed (S24).** A negative value reverses the dimension's direction.
- **Inferred.** The box shows the value the geometry measures now.
- **Unknown.** Which operators and which units are accepted.
- **Decision.** Plain input is evaluated once to a number:
  - arithmetic uses AnvilCAD's expression grammar (`+ - * / ^`, parentheses and functions);
  - lengths accept `mm`, `cm`, `m`, `in`, `inch` and `ft`;
  - angles accept `deg`, `°` and `rad`;
  - a unit applies to the number just before it.

  Only `=` input is kept as a linked equation. (Before this change, AnvilCAD linked any
  non-numeric input, such as `10+5`, as an equation.)
- **Decision.** If the box is cancelled, a new dimension keeps its measured value.
- **Not implemented here.** Spin wheels, the rebuild preview button and per-configuration
  scope.

### Over-defining dimensions and reference dimensions

- **Observed (S26, S27).** A dimension that would over-define the sketch brings up a
  choice: make it driven (it then only reports the size), leave it driving (the sketch
  stays over-defined), or cancel it. An option can make either choice the default without
  asking.
- **Observed (S30, S33).** Driven dimensions show in parentheses and in grey. They cannot
  be edited, and they update when the geometry changes.
- **Observed (S29).** A driven dimension can be switched back to driving later.
- **Inferred.** Both redundant dimensions (consistent, but removing no freedom) and
  conflicting ones over-define the sketch.
- **Decision.** AnvilCAD calls these **reference dimensions**, because "driven" already
  means "driven by an equation" in AnvilCAD.
  - **Detection.** The solver is run with and without the new dimension. The dimension
    over-defines if the sketch stops converging, or if the degrees of freedom do not drop.
  - **While the user decides,** the dimension is held as a reference, so the sketch never
    shows as broken.
  - **The panel** offers Make Reference, Keep Driving and Cancel Dimension.
  - **Switching later.** Make Reference and Make Driving switch an existing dimension.
  - **Not implemented here.** The option to skip the prompt.

### Sketch state

- **Observed (S32, S33).** Sketches are under-defined (blue), fully defined (black) or
  over-defined (red). Driven dimensions are grey.
- **Decision.** AnvilCAD already shows these states. It does not have a separate colour
  for "redundant but consistent". A driving dimension kept on purpose that way still
  solves, and shows as defined.

### Not taken from the sources

- **First dimension.** The first dimension in a sketch scales the whole profile (S17).
  This is not implemented; AnvilCAD solves to the new value with minimal movement.
- **API.** The `AddDimension2` family (S35–S37) places a dimension for the current
  selection at a given text position. AnvilCAD's `dimension::infer(sketch, picks, at)` has
  the same shape, but it is AnvilCAD's own API. The `anvil-migrate` exporter maps
  dimensions from SOLIDWORKS separately.

## Unknowns

- Exact placement thresholds and angle sectors (pictures only).
- Whether a single line's length is placement-sensitive, beyond what S1 implies.
- What is shown in the value box for a new dimension, and what cancel does to a new one.
- Which units and operators the value box accepts.
- How the first-dimension scaling picks its fixed point.
- Ellipses, splines and slots were not studied.
- S1 and S9 differ on whether the arc's end points are Ctrl-picked.
