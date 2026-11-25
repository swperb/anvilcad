# Drawings: sheets, standard views and PDF

What an AnvilCAD drawing is and does in its first slice: a drawing document that refers to
a part, one sheet with a size, a scale and a projection method, the three standard views and
an isometric projected from the part's exact geometry with hidden lines removed or dashed and
tangent edges shown, thin or removed, views that follow the part when it changes, and vector
PDF output. SOLIDWORKS users know the equivalent workflow well, so AnvilCAD follows its
published behaviour where that makes sense. Dimensions, title blocks and custom properties
(issue #97) and section, detail and auxiliary views (#98) come later and are noted only
where the data model must leave room for them.

This specification was written before the implementation, which is in
`crates/anvil-drawing` (document, projection, layout and PDF) and
`crates/anvil-app/src/app/drawing.rs` (drawing mode).

## How this was studied

- **Inputs.** Only published information was used: Dassault's public online help for
  SOLIDWORKS 2025 (with one 2017, one 2022 and one 2024 page where the 2025 page is missing
  or says less) and the 2024 API help. No SOLIDWORKS software, installer, library or file was
  downloaded, installed, run or analysed, and no `.slddrw` file was studied. No SOLIDWORKS
  licence was accepted on the machine used.
- **Retrieval.** The help site renders its articles with JavaScript. Each article was fetched
  over HTTPS with a browser user agent and its text read from the page's embedded
  `__NEXT_DATA__` JSON (`props.pageProps.helpContentData.helpText`) by a script run with
  `python3 -I`. A missing page still returns HTTP 200 with "page cannot be found" in its
  data, so each page was checked; the missing ones are listed under "Unknown or not
  covered". Page names came from links inside fetched pages and from web searches limited to
  `help.solidworks.com`. The text was paraphrased; none of it is copied into this
  repository. Pictures on the pages were not available to the script, so nothing here is
  taken from them.
- **REA.** REA (`rea-agents` 6.10.1, with the `third_party/rea` submodule at `3fc0075`) was
  set up and its research method followed:
  - each finding below is marked **Observed** (stated in a cited source), **Inferred** (our
    reasoning from the sources), **Decision** (AnvilCAD's own choice where the sources are
    silent or where AnvilCAD does better) or **Unknown**;
  - every observation cites its source.

  REA's analysis tools were **not** run. They inspect artifacts (binaries, packages,
  captures, files), and the only lawful inputs for this behaviour are prose documents.
  Pointing REA at SOLIDWORKS software is ruled out by
  [INTEROPERABILITY.md](../../INTEROPERABILITY.md). The drawing file format is not covered;
  AnvilCAD's `.anvdrw` format is its own.
- **Standards.** Paper sizes and line widths are not given in the SOLIDWORKS help. They come
  from the published standards ISO 216 (A sizes), ASME Y14.1 (inch sizes) and ISO 128 (line
  types and widths), and are marked as such.

## Sources

Base A is `https://help.solidworks.com/2025/English/SolidWorks/sldworks/`. Base B is
`https://help.solidworks.com/2024/English/api/swconst/` and base C is
`https://help.solidworks.com/2024/English/api/sldworksapi/`; enumeration pages there are
named `SOLIDWORKS.Interop.swconst~SOLIDWORKS.Interop.swconst.<Enum>.html` and interface
pages `SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.<Interface>.html`. All were
read on 2026-11-24.

| # | Topic | URL | Version |
|---|---|---|---|
| S1 | Sheet Properties dialog | A + `HIDD_DRAWING1.htm` | 2025 |
| S2 | Sheet format and size | A + `r_drawing_sheet_format_size.htm` | 2025 |
| S3 | Paper size enumeration (`swDwgPaperSizes_e`) | B + enum page | 2024 API |
| S4 | Drafting standard | A + `HIDD_OPTIONS_DRAFTING_STD.htm` | 2025 |
| S5 | Standard 3 View | A + `c_standard_3_view.htm` | 2025 |
| S6 | Creating a Standard 3 View | A + `t_creating_3view_standard_method.htm` | 2025 |
| S7 | Standard 3 View in a new drawing | A + `t_creating_3view_new_document.htm` | 2025 |
| S9 | First and third angle projection | A + `c_first_angle_and_third_angle_projection.htm` | 2025 |
| S10 | Projected views | A + `c_projected_view.htm` | 2025 |
| S11 | Aligning views | A + `c_align_views.htm` | 2025 |
| S12 | Tutorial: creating a drawing of a part | `https://help.solidworks.com/2024/english/swconntutorial/t_tut_lesson3_createdrawing.htm` | 2024 |
| S13 | Model views | A + `c_model_view.htm` | 2025 |
| S14 | Model View PropertyManager | A + `HIDD_DVE_NAMED_VIEW.htm` | 2025 |
| S15 | Orientation dialog (standard views, up axis) | A + `c_orientation_dialog_box.htm` | 2025 |
| S16 | Standard view enumeration (`swStandardViews_e`) | B + enum page | 2024 API |
| S17 | Front, Top and Right plane directions | `https://help.solidworks.com/2025/English/SolidWorks/cworks/c_Specifying_Directions.htm` | 2025 |
| S19 | Glossary | `https://help.solidworks.com/2017/english/SolidWorks/sldworks/g_glossary.htm` | 2017 |
| S20 | Drawing display style options | A + `HIDD_OPTIONS_DRAWING_EDGES.htm` | 2025 |
| S22 | Display mode enumeration (`swDisplayMode_e`) | B + enum page | 2024 API |
| S23 | Hidden Lines Removed | A + `c_hidden_lines_removed_fundamentals.htm` | 2025 |
| S24 | Hidden Lines Visible | A + `c_hidden_lines_visible.htm` | 2025 |
| S26 | Tangent edge display | A + `c_tangent_edge_display_overview.htm` | 2025 |
| S27 | Tangent edge enumeration (`swDisplayTangentEdges_e`) | B + enum page | 2024 API |
| S28 | Image quality (tangent edge definition) | A + `HIDD_OPTIONS_IMAGE_QUALITY_display.htm` | 2025 |
| S29 | Silhouette edges | A + `t_Selecting_Silhouette_Edges.htm` | 2025 |
| S30 | Line font | A + `HIDD_OPTIONS_LINE_FONT.htm` | 2025 |
| S32 | Line style enumeration (`swLineStyles_e`) | B + enum page | 2024 API |
| S33 | Line weight enumeration (`swLineWeights_e`) | B + enum page | 2024 API |
| S34 | Line thickness | A + `HIDD_PRINT_LINE_WEIGHTS_props.htm` | 2025 |
| S37 | Controlling how views update | A + `t_control_update_behavior_of_views.htm` | 2025 |
| S38 | Out-of-date views | A + `c_out_of_date_view.htm` | 2025 |
| S41 | Drawing options | A + `r_drawings_options_all.htm` | 2025 |
| S43 | Detached drawings | `https://help.solidworks.com/2022/english/SolidWorks/sldworks/c_detached_drawings.htm` | 2022 |
| S44 | PDF files and PDF export options | A + `c_Adobe_Portable_Document_Format_pdf_Files.htm` | 2025 |
| S46 | PDF export data (`IExportPdfData`) | C + interface page | 2024 API |
| S49 | Printing drawings | A + `c_print_drawings.htm` | 2025 |
| S50 | Draft quality hidden lines | A + `t_draft_quality_HLR_HLV.htm` | 2025 |
| S51 | Types of drawing documents | A + `c_drawing_documents.htm` | 2025 |
| S52 | Drawings overview | A + `c_drawings_overview.htm` | 2025 |
| S53 | Creating a drawing | A + `c_create_drawing.htm` | 2025 |
| S54 | Drawing view properties | A + `HIDD_DRVIEW_PROP.htm` | 2025 |

Numbers missing from the table are pages that were read but added nothing used here.

## The drawing document

- **Observed.** A drawing is a 2D representation of a part or assembly, linked to its
  models, so that changing a model changes the drawing (S19, S52). A part must be saved
  before a drawing of it is made (S53). A new drawing takes the name of the first model put
  in it (S53).
- **Observed.** Each view records its model and configuration (S54). Drawings can keep
  display data and can be opened without their models (detached, S43; lightweight, S51).
- **Inferred.** A drawing stores references to model files and per-view settings, not a copy
  of the model's solid geometry.
- **Decision.** An AnvilCAD drawing is a `.anvdrw` file of versioned JSON, like `.anvprt`
  and `.anvasm`: `format` is `"anvil-drawing"` and `format_version` is 1. It holds the part
  file's path (relative to the drawing's folder where possible, with `/` separators), one
  sheet, and a list of views with ids. Opening a file with a newer version is refused with a
  message naming the version. Saving writes a temporary file and renames it, so a crash
  never leaves half a drawing. Ids are kept stable so that dimensions (#97) can refer to
  views; a view does not yet store a position, and adding one later is a new optional field.
- **Decision.** A drawing holds no geometry of its own in this slice: views are projected
  again whenever the drawing is opened. Keeping projected geometry in the file, so a drawing
  can be shown and printed without its part, is left for later (see "Unknown or not
  covered").
- **Decision.** "New Drawing" makes a drawing of the open part, which must have been saved;
  AnvilCAD says so in plain words otherwise. A new drawing is named `Drawing1` until it is
  saved. Drawings of assemblies are not in this slice and the command says so.

## Sheets

- **Observed.** A sheet has a scale, a type of projection (first angle or third angle) and a
  sheet format or size: a standard size from a list, or a custom width and height (S1, S2).
- **Observed.** The API lists the paper sizes A0, A1, A2, A3, A4 and A4 portrait, ANSI A and
  A portrait, B, C, D and E, and user-defined (S3).
- **Observed.** The help says first angle is typical in Europe and third angle in the United
  States (S1). Drafting standards offered include ANSI and ISO (S4).
- **Observed.** A view uses the sheet's scale unless it is given its own (S12, S14). An
  option scales new standard views to fit the sheet whatever its size, which the help warns
  can give unusual scales (S41).
- **Standard.** Sizes, landscape, width by height: A4 297 x 210, A3 420 x 297, A2 594 x 420,
  A1 841 x 594, A0 1189 x 841 mm (ISO 216); A 11 x 8.5, B 17 x 11, C 22 x 17, D 34 x 22,
  E 44 x 34 in (ASME Y14.1).
- **Decision.** Sheets are A4 to A0 and ANSI A to E, landscape. Portrait A4 and A and custom
  sizes are later work. A new drawing starts on A3, first angle, which suit AnvilCAD's
  millimetre units; both can be changed at any time.
- **Decision.** The scale is written `sheet:model` (`1:2` is half size, `2:1` twice). A new
  drawing gets the largest ISO 5455 standard scale (50:1, 20:1, 10:1, 5:1, 2:1, 1:1, 1:2,
  1:5, 1:10, 1:20, 1:50, 1:100, 1:200, 1:500, 1:1000) at which all its views fit inside the
  frame, so the "unusual scale" S41 warns about never happens. Fit sets that scale again;
  any whole-number ratio can be typed.
- **Decision.** The sheet shows a frame 10 mm inside the paper's edge and, until title blocks
  arrive (#97), a note in the lower right corner giving the scale and the projection method.

## Standard views

- **Observed.** Standard 3 View makes three related orthographic views at once, oriented from
  the model's Front, Right and Top; their orientations cannot be changed (S5). The front view
  is a model view and the other two are projected from it with the sheet's projection
  method (S10). The top view can move only up and down and the side view only sideways,
  staying aligned with the front view (S5, S11).
- **Observed.** In third angle projection the front view is at the lower left and the other
  two views are the top and right views; in first angle the front view is at the upper left
  and the others are the top and left views (S9). Placing projected views by hand confirms
  it: in third angle the view above the front is the top view and the one beside it the
  right view; in first angle the view above is the bottom view and the one beside it the
  left view (S12).
- **Observed.** Model views can take the standard orientations Front, Back, Left, Right, Top,
  Bottom and Isometric, and also Trimetric and Dimetric (S14, S16). The Front plane holds X
  and Y with normal Z, the Right plane Z and Y with normal X, and the Top plane X and Z with
  normal Y (S17). An "up axis" option chooses Y up or Z up (S15).
- **Inferred.** With Y up: the front view looks along -Z with X to the right; the top view
  looks down -Y with X to the right and -Z up the sheet; the right view looks along -X with
  -Z to the right; the left view looks along +X with +Z to the right. This matches
  AnvilCAD's own standard planes and named views (`crates/anvil-app/src/camera.rs`).
- **Inferred.** Third angle: top view above the front, right view to its right. First angle:
  top view below the front, left view to its right (S9 with S12).
- **Unknown.** The isometric direction, and the spacing between the standard views, are not
  given.
- **Decision.** A new drawing has four views: front, top, side and isometric. The side view
  is the right view in third angle and the left view in first angle, placed to the right of
  the front either way; the top view is above the front in third angle and below it in
  first angle. Changing the projection method moves the views and turns the side view.
  Views are placed by these rules, with the space between them 6% of the sheet's height (at
  least 12 mm), and the group is centred in the frame.
- **Decision.** The isometric looks from (1, 1, 1) with Y up, as AnvilCAD's 3D isometric view
  does. It is a true orthographic projection at the sheet's scale (lengths along the axes
  are foreshortened). It goes in the corner the other views leave free: to the right of the
  front view, below it in first angle and above it in third, clear of the top and side views.

## Display style, hidden lines and tangent edges

- **Observed.** A drawing view's display style is one of Wireframe (all edges), Hidden Lines
  Visible (visible edges and hidden edges as set in Line Font), Hidden Lines Removed (only
  edges visible from the view), Shaded With Edges and Shaded (S14, S20, S22). Hidden edges
  use the Hidden Edges line font, whose style the API calls dashed (S20, S30, S32).
- **Observed.** Tangent edges are the transitions between faces that meet smoothly, most often
  at fillets (S19, S26). A view shows them as Visible (a solid line), Use font (the tangent
  edge line font) or Removed (S20, S26, S27). Drafting practice often shows them in pictorial
  views and removes them in orthographic views (S26). Since 2009 an edge is tangent when the
  angle between its two faces is under one degree (S28).
- **Observed.** A silhouette edge is the curve showing the extent of a cylindrical or other
  curved face seen from the side (S19, S29).
- **Observed.** High quality views use the fully loaded, exact model; draft quality uses a
  faceted model and is faster (S20, S22, S50, S51).
- **Unknown.** The factory default display style and tangent edge mode for new views.
- **Decision.** Views offer Hidden lines removed, Hidden lines dashed and Wireframe, and
  tangent edges Visible, Thin lines or Removed (AnvilCAD's names). New orthographic views
  have hidden lines and tangent edges removed; the new isometric has hidden lines removed
  and tangent edges visible, following S26's drafting practice. Shaded views are later work.
- **Decision.** There is only one quality: views are always projected from the exact
  B-rep, never from the display mesh, and fast enough that a draft mode is not needed for
  parts.
- **Decision (how views are projected).** Each view draws the part's edges and the
  silhouettes of its curved faces:
  - an edge between faces meeting at under 1 degree is a tangent edge; an edge between two
    pieces of the same surface, or a face's seam, is never drawn;
  - silhouettes are exact: two lines on a cylinder or cone, a circle on a sphere, the tube
    and side circles of a torus seen square to its axis, and otherwise curves traced on the
    torus where its normal is square to the view; on freeform faces they are found on the
    display mesh and moved onto the surface;
  - each curve is cut where it passes behind a face, by testing points along it with a ray
    towards the viewer against the exact faces of every body and finding each change by
    bisection; a ray that only touches a face from outside, at a silhouette, does not hide;
  - pieces are projected exactly: a line stays a line, a circle becomes a circle, an
    ellipse or (seen edge-on) a line; a piece lying on a line drawn before it is dropped,
    visible lines first, so an edge hidden behind a coincident visible edge is not drawn;
    straight pieces that continue one another are joined.
- **Decision.** Projection runs on a background thread using every core. The sheet shows the
  progress, with a Cancel button, and the window stays responsive; a cancelled update leaves
  the old views with a note that they may be out of date.

## Line styles

- **Observed.** Each kind of line (visible, hidden, tangent, hidden tangent and others) has a
  style, a thickness and an end cap (S30). Styles include solid, dashed, phantom and chain
  (S32); thicknesses are named steps (thin, normal, thick and more) whose printed widths are
  set per document (S33, S34).
- **Unknown.** The default style and thickness of each kind, and the widths of the steps.
- **Standard.** ISO 128 uses a thick and a thin line in the ratio 2:1, for example 0.5 mm and
  0.25 mm; hidden lines are thin and dashed.
- **Decision.** Visible edges and silhouettes 0.5 mm continuous; hidden edges 0.25 mm dashed
  (3 mm dashes, 1 mm gaps); tangent edges in the Thin lines mode 0.25 mm continuous; the
  frame 0.5 mm. Line ends and joins are round. Paper is white and lines black in both of
  AnvilCAD's themes, as they print.

## Updating views

- **Observed.** If the drawing is open when its model changes, the "automatic view update"
  setting decides whether its views update; views can be left out of automatic updates; a
  separate option decides whether views update when a drawing is opened (S37). A view that is
  out of date shows hatching on its border and can be updated by hand (Update View, Update
  All Views, Rebuild); pressing Esc stops an update (S37, S38). A detached drawing opens
  without its model and prints an out-of-sync watermark (S43).
- **Unknown.** What happens when a drawing's model file is missing when it is opened
  (outside detached drawings).
- **Decision.** Views always follow the part. Opening a drawing projects its views from the
  part file; while it is open the part file is checked every second and saving the part
  updates the views. Update Views (and Rebuild) projects them again by hand. There is no
  setting to turn updating off: projection is fast and never blocks the window, so the
  reason for that setting (slow updates) does not apply.
- **Decision.** If the part file is missing, cannot be read, or is not exact (a feature fell
  back to the faceted kernel), the sheet says which and what to do, the views are left
  empty rather than wrong, and Change Part points the drawing at another file.

## PDF export

- **Observed.** Drawings can be saved as PDF. Options include colour, embedded fonts,
  high quality (vector, double precision) lines, the resolution of shaded and draft views,
  vector edges on shaded views, and printer line weights (S44). Sheets to export can be all,
  the current one or named ones (S46). Printing defaults to black and white (S49).
- **Inferred.** Lines go into the PDF as vectors; only shaded and draft views are raster.
- **Unknown.** The default state of each PDF option.
- **Decision.** Export PDF writes one page the size of the sheet, black on white, with every
  line as a vector path: straight lines as line segments, circles and ellipses as cubic
  Bezier curves, each pen's width and dash pattern set in the PDF. Text uses the standard
  Helvetica font, which every PDF reader has, so no font is embedded. Nothing is drawn as an
  image. It is written by AnvilCAD's own writer, with no new dependency.

## Unknown or not covered

1. The `.slddrw` format and how SOLIDWORKS stores model references and cached view
   geometry. Nothing here was learnt from files.
2. Factory defaults for display style, tangent edges, line styles and widths, view spacing,
   projection method per standard, and PDF options.
3. The exact isometric, dimetric and trimetric directions.
4. Behaviour of a normal (not detached) drawing whose model is missing on open.
5. View labels on orthographic views: the 2025 and 2019 "Orthographic View Labels" pages
   (`HIDD_OPTIONS_ORTHO_LABEL.htm`) are missing, as are the 2025 pages
   `c_scales_in_drawings.htm`, `c_replace_model_drawing_views.htm` and
   `HIDD_SHOW_HIDDEN_EDGES.htm`.
6. Later slices: dimensions, annotations, title blocks and custom properties (#97); section,
   detail, auxiliary and broken views (#98); several sheets, portrait and custom sizes,
   moving views by hand, shaded views, assemblies, DXF output, and keeping projected geometry
   in the drawing so it opens without its part.
