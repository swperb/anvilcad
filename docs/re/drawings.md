# Drawings: sheets, views, dimensions, title blocks, PDF and DXF

What an AnvilCAD drawing is and does in its first slice: a drawing document that refers to
a part, one sheet with a size, a scale and a projection method, the three standard views and
an isometric projected from the part's exact geometry with hidden lines removed or dashed and
tangent edges shown, thin or removed, views that follow the part when it changes, and vector
PDF output. SOLIDWORKS users know the equivalent workflow well, so AnvilCAD follows its
published behaviour where that makes sense. The second slice (issue #97) adds model and
reference dimensions, title blocks filled from the part's custom properties and
configuration, and DXF output; it is specified in the sections from "Dimensions" on. The
third slice (issue #98) adds section views (full and offset) hatched by material, detail
views and hole callouts; it is specified in "Section views", "Hatching", "Detail views" and
"Hole callouts". Assembly drawings with a bill of materials and balloons, the rest of #98,
are specified in "Assembly drawings, BOM tables and balloons" but not yet built (#210).

This specification was written before the implementation, which is in
`crates/anvil-drawing` (document, projection, layout and PDF) and
`crates/anvil-app/src/app/drawing.rs` (drawing mode). The second slice is in
`crates/anvil-drawing/src/dims.rs` (dimensions), `title.rs` (title blocks), `dxf.rs` (DXF)
and the same app module. The third slice is in `section.rs` (section views and hatching),
`detail.rs` (detail views) and `callout.rs` (hole callouts).

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
- **Second slice.** The dimension, title-block and DXF sections were studied the same way on
  2026-11-25, with the same REA version and pin and the same method: only published help
  pages were read, no `.slddrw` or `.dxf` file written by SOLIDWORKS was studied, and REA's
  analysis tools were not run. The DXF structure comes from Autodesk's published DXF
  reference (D1), not from SOLIDWORKS output; it was used for group codes only.
- **Third slice.** Section views, hatching, detail views, hole callouts and BOM tables were
  studied the same way on 2026-11-25 (S80 to S106), with the same REA version (6.10.1) and
  submodule pin, following the `reverse-engineer-anything` skill's habit of separating
  observations, inferences, decisions and unknowns. Pages were found from the links inside
  "Hole Callouts", "Detail View" and "Area Hatch/Fill" and from there onward. The 2025 pages
  `c_section_views.htm`, `HIDD_DVE_SECTION_VIEW.htm`, `c_offset_section.htm`,
  `c_detail_views.htm`, `HIDD_DVE_HOLE_CALLOUT.htm`, `c_crosshatch.htm` and
  `HIDD_OPTIONS_VIEW_LABELS.htm` were tried and are missing. Pictures were again not
  available, so nothing here describes what a SOLIDWORKS section line or callout looks like
  beyond what the text says. No `calloutformat.txt` or `sldwks.ptn` file (both ship with the
  installed product, S86, S95) was read. Material hatch families come from no SOLIDWORKS
  source; see "Hatching".
- **Standards.** Paper sizes and line widths are not given in the SOLIDWORKS help. They come
  from the published standards ISO 216 (A sizes), ASME Y14.1 (inch sizes) and ISO 128 (line
  types and widths), and are marked as such.

## Sources

Base A is `https://help.solidworks.com/2025/English/SolidWorks/sldworks/`. Base B is
`https://help.solidworks.com/2024/English/api/swconst/` and base C is
`https://help.solidworks.com/2024/English/api/sldworksapi/`; enumeration pages there are
named `SOLIDWORKS.Interop.swconst~SOLIDWORKS.Interop.swconst.<Enum>.html` and interface
pages `SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.<Interface>.html`. S1 to S54 were
read on 2026-11-24 and S60 to S72 on 2026-11-25, in the same way (the second slice also
tried `c_dangling_dimensions.htm`, `c_title_blocks.htm`, `c_sheet_formats.htm`,
`c_dxf_dwg_files.htm`, `HIDD_OPTIONS_DIMS.htm`, `c_marked_for_drawing.htm` and
`c_custom_properties.htm` under base A; all were missing). D1 is Autodesk's published DXF
reference, used for the group codes of the DXF subset written; it is not SOLIDWORKS
material.

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
| S60 | Model items | A + `c_insert_model_items.htm` | 2025 |
| S61 | Model Items PropertyManager | A + `HIDD_DVE_INSERT_MODEL_ITEMS.htm` | 2025 |
| S62 | Dimensions in drawings | A + `c_dimensions_in_drawings.htm` | 2025 |
| S63 | Inserting dimensions into drawings | A + `c_Inserting_Dimensions_into_Drawings.htm` | 2025 |
| S64 | Reference dimensions | A + `c_Reference_Dimensions.htm` | 2025 |
| S65 | Linking notes to document properties | A + `t_link_notes_to_document_properties.htm` | 2025 |
| S66 | Link to Property | A + `HIDD_SELECT_PROPERTY.htm` | 2025 |
| S67 | Title blocks | `https://help.solidworks.com/2025/English/SolidWorks/acadhelp/c_title_blocks_acadhelp.htm` | 2025 |
| S68 | Defining or editing title blocks | A + `t_defining_editing_title_blocks.htm` | 2025 |
| S69 | Title block tables (configuration-specific properties) | A + `c_title_block_tables_2.htm` | 2025 |
| S70 | Model View PropertyManager, configuration information | A + `HIDD_DVE_NAMED_VIEW.htm` | 2025 |
| S71 | DXF/DWG file export options | A + `r_dxf_dwg_export_options.htm` | 2025 |
| S72 | Setting DXF/DWG export options | A + `HIDD_EXPORT_OPTION_DXF.htm` | 2025 |
| S80 | Section views in drawings | A + `c_section_views_in_drawings.htm` | 2025 |
| S81 | Section View PropertyManager (new view) | A + `HIDD_DVE_SECTION_PROP_NEW.htm` | 2025 |
| S82 | Section View PropertyManager (drawings) | A + `HIDD_DVE_SECTION_PROP.htm` | 2025 |
| S83 | Inserting a section view | A + `t_Inserting_Section_View_drawing.htm` | 2025 |
| S84 | Inserting a section view with offset | A + `t_Inserting_Section_View_Offset.htm` | 2025 |
| S85 | Examples: section view, and with offset | A + `c_Examples_Section_View.htm`, `c_Examples_Section_View_Offset.htm` | 2025 |
| S86 | Area hatch/fill | A + `c_area_hatch_fill.htm` | 2025 |
| S87 | Area Hatch/Fill PropertyManager, new and edit | A + `HIDD_DVE_AREAHATCH.htm`, `HIDD_DVE_AREAHATCH_PROPS.htm` | 2025 |
| S88 | Section view labels | A + `HIDD_Options_section_label.htm` | 2025 |
| S89 | Additional features of section views | A + `c_additional_section_views_in_drawings.htm` | 2025 |
| S90 | Troubleshooting section views | A + `c_troubleshooting_section_views.htm` | 2025 |
| S91 | Modifying section views | A + `t_modify_section_views.htm` | 2025 |
| S92 | Detail view | A + `c_detail_view.htm` | 2025 |
| S93 | Detail View PropertyManager | A + `HIDD_DVE_DETAIL_CIRCLE.htm` | 2025 |
| S94 | Detail view labels | A + `HIDD_Options_detail_label.htm` | 2025 |
| S95 | Creating a detail view | A + `t_create_detail_view.htm` | 2025 |
| S96 | Derived drawing views | A + `c_derived_drawing_views.htm` | 2025 |
| S97 | Hole callouts | A + `c_hole_callouts.htm` | 2025 |
| S98 | Adding hole callouts in a drawing | A + `t_adding_hole_callouts_drawing.htm` | 2025 |
| S99 | Modifying the hole callout format | A + `c_modify_hole_callout_format.htm` | 2025 |
| S100 | Callout variables | A + `HIDD_CALLOUT_VARIABLE.htm` | 2025 |
| S101 | Instance indicators | A + `t_instance_indicators.htm` | 2025 |
| S102 | View labels | A + `c_view_labels.htm` | 2025 |
| S103 | Balloons | A + `c_balloons.htm` | 2025 |
| S104 | Inserting balloons; balloon styles and sizes | A + `t_inserting_balloons.htm`, `c_Balloon_Styles_and_Sizes.htm` | 2025 |
| S105 | Auto balloons and the Auto Balloon PropertyManager | A + `c_Auto_Balloons.htm`, `HIDD_DVE_AUTO_BALLOON.htm` | 2025 |
| S106 | Bill of materials overview and PropertyManager | A + `c_Bill_of_Materials_-_Overview2.htm`, `HIDD_DVE_INSERT_BOM.htm` | 2025 |
| D1 | DXF Reference (Autodesk, AutoCAD 2012 edition) | `https://images.autodesk.com/adsk/files/autocad_2012_pdf_dxf-reference_enu.pdf` | AutoCAD 2012 |

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
- **Decision (second slice).** The format moves to version 2, which adds the dimensions,
  the title block and the configuration shown (see below). Version 1 files open, with no
  dimensions, the standard title block and the part's saved configuration; a build that
  only knows version 1 refuses a version 2 file instead of dropping its dimensions.
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
- **Decision.** The sheet shows a frame 10 mm inside the paper's edge and a title block in
  its lower right corner (see "Title blocks"), which gives the scale and the projection
  method among its fields. The first slice showed a plain note there instead.

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

## Dimensions

### Model dimensions

- **Observed.** Dimensions in a drawing are associated with the model: changing the model
  changes the drawing (S62, S63). The usual practice is to dimension each feature while
  modelling and then insert those dimensions into the drawing views; changing an inserted
  dimension in the drawing changes the model (S62, S63).
- **Observed.** Model Items inserts dimensions (and annotations and reference geometry) from
  the model, for the entire model or a selected feature, into all views or chosen views; in
  all views each dimension goes to "the most appropriate view" (S60, S61, S63). A dimension
  inserted in one view is not inserted again into another, and an option removes duplicates
  (S61, S63). Dimensions go only into views where they are appropriate, and can be moved
  only to a view whose orientation suits them (S62, S63).
- **Observed.** Dimensions can be filtered by "marked for drawing"; new model dimensions are
  marked by default (S61). An option keeps the placement the dimension had in the sketch
  (S61). Dimensions of a sketch that is not absorbed into a feature are inserted only if the
  sketch is shown (S60).
- **Observed.** Inserted dimensions are drawn black, including those that are blue in the
  model, such as an extrusion's depth; reference dimensions are grey with parentheses (S63).
  Model items can be deleted with Delete (S60).
- **Unknown.** How "the most appropriate view" is chosen, and what "appropriate" means for a
  view's orientation.
- **Inferred.** A dimension measured in a plane belongs in a view that looks square onto that
  plane, where it is seen at its true size; a depth belongs in a view that sees the depth
  direction side on.
- **Decision.** Model Dimensions (AnvilCAD's name for Model Items, dimensions only) brings
  in, for the whole part, the dimensions of every sketch and the depth of every extrude:
  - a sketch dimension goes to the first orthographic view, in the order front, top, side,
    that looks square onto the sketch's plane; a depth goes to the first such view that
    sees the extrude direction side on (Blind and Mid Plane depths only, since the others
    have no depth value). The isometric view never gets model dimensions, because lengths
    are foreshortened there;
  - each dimension is inserted once: running Model Dimensions again adds only those not
    yet in the drawing, and says so when there are none;
  - reference dimensions of the sketch are left out (AnvilCAD has no "marked for drawing"
    flag, and a sketch's reference dimension is a check, not a manufacturing size); every
    driving dimension, including those set by equations, comes in, except a distance of
    zero (two points held level), which says nothing on a drawing;
  - placement: a dimension the user moved in the sketch keeps that placement (S61's
    option, always on); one left at its default place is moved 8 sheet millimetres out
    from the view's centre, so its value does not sit on the part's lines;
  - suppressed and rolled-back features bring nothing.
- **Decision.** A model dimension stores only the feature and the dimension it is (the
  sketch dimension's index and kind, or "depth") and where its value is placed. Its value
  and its geometry come from the rebuilt part every time the views update, so the drawing
  always shows what the model is. If the dimension is gone from the model (deleted,
  suppressed, or a different kind at that index), it is left out and the left pane counts
  it as no longer in the model; Delete removes it for good.
- **Decision.** Editing a model dimension's value from the drawing (which S62 says changes
  the model) is not in this slice. The value is shown; the part is changed in the part.

### Reference dimensions

- **Observed.** Dimensions added in the drawing are reference dimensions: they are driven,
  their value cannot be edited, and they update when the model changes (S62, S64). They are
  enclosed in parentheses by default (except ordinate dimensions), with a document option to
  turn that off (S64). They are made with the same tool, and the same methods, as sketch
  dimensions, including horizontal, vertical and parallel dimensions, and they can measure
  to silhouette edges (S62, S64). A reference dimension is hidden when its feature is
  suppressed (S64).
- **Unknown.** What a reference dimension does when the edge it measured disappears
  ("dangling" dimensions; the 2025 page `c_dangling_dimensions.htm` is missing).
- **Decision.** Reference Dimension in drawing mode is the sketch Dimension tool on a view's
  lines, with the same inference rules (`docs/re/sketch-dimension.md`): one line gives its
  length, or its horizontal or vertical extent by placement; two points give a horizontal,
  vertical or aligned distance by placement; a point and a line or two parallel lines give
  the distance between them; two crossing lines give the angle of the sector the value is
  placed in; a circle gives its diameter, an arc its radius; a circle or arc with anything
  else measures from its centre. Pickable things are the ends of lines and arcs, circle and
  arc centres, straight lines (pieces of one edge split into seen and hidden parts count as
  one line) and circles and arcs seen square on. Ellipses and traced curves cannot be
  picked. Only the front, top and side views take reference dimensions; in the isometric
  the tool says why not.
- **Decision.** Values are shown in parentheses. There is no option to turn that off yet.
- **Decision.** A reference dimension stores what it measured as points in model space on
  the view's plane (so the side view turning from left to right with the projection method
  keeps it), and the inferred measurement. When the views update, each pick is found again
  among the new lines: the same point, a line with the same ends, a circle or arc with the
  same centre and radius. If it is found the value is measured again; if not, the dimension
  no longer attaches: it is drawn in the error colour on screen at its old place, counted in
  the left pane, and left out of PDF and DXF output, so a stale value is never printed.
  Following an edge that moved (true associativity) needs edge identities that survive a
  rebuild and is later work.

### Drawing dimensions

- **Unknown.** Factory arrow, text and gap sizes, and precision (the Document Properties -
  Dimensions page, `HIDD_OPTIONS_DIMS.htm`, is missing for 2025).
- **Decision (after ISO 129-1 practice).** Dimension and extension lines are thin (0.25 mm);
  arrowheads are filled, 3 mm long and 1 mm wide; text is 3.5 mm high, upright on the sheet
  (unidirectional), centred on its place, with the dimension and extension lines broken
  round it. Lengths are in millimetres to two decimals with trailing zeros dropped;
  diameters start with Ø, radii with R, angles end with °.

## Title blocks

- **Observed.** A new drawing starts from a template with a paper size and a sheet format;
  standard formats hold a title block, whose lines and text can be edited; notes in it can
  be linked to document properties or custom properties (S67). A title block can be defined
  so that chosen notes are fields a user fills in, and saved as a template (S68).
- **Observed.** A note links to a property with `$PRP:"name"` (the current document) or
  `$PRPSHEET:"name"` (the model in the view named in Sheet Properties, by default the first
  view); a note can mix text and several links; if the property cannot be found the note
  shows an error naming it (S65, S66). Properties can be the document's own, its custom
  properties or configuration-specific properties (S66); configuration-specific values are
  those of the configuration shown (S69). System properties include the configuration
  name, file name and the sheet's scale and size (S65).
- **Observed.** A drawing view uses either the model's in-use or last saved configuration or
  a named configuration (S70).
- **Decision.** Each drawing has a title block: a list of fields, each a label and a text.
  The text mixes plain words and `{Name}` placeholders (AnvilCAD's syntax, not `$PRP`);
  `{{` and `}}` give braces. A placeholder is filled, in this order, from:
  - the part's custom properties as they apply in the configuration the drawing shows
    (configuration-specific ones replace file ones of the same name, as AnvilCAD's
    Custom Properties already do), matched without regard to case;
  - built-in names: `Configuration`, `FileName` (the part's file name without extension),
    `Material` (the part's material), `DrawingName`, `Scale`, `Size` and `Projection`.
  A name found nowhere prints nothing, and the left pane lists it as not set in the part,
  which is clearer on paper than an `ERROR!` text.
- **Decision.** The standard title block, for new drawings and for version 1 files: Title
  `{Description}`; Part number `{PartNo}`; Revision `{Revision}`; Material `{Material}`;
  Configuration `{Configuration}`; Drawn by `{DrawnBy}`; Scale `{Scale}`; Projection
  `{Projection}`; Sheet `{Size}`. These are the custom properties every new AnvilCAD part
  has.
- **Decision (layout, after ISO 7200).** The block is 180 mm wide (the width of the frame
  when that is less), in the lower right corner of the frame: the first field spans the
  width in a 12 mm row with 5 mm text, the others follow two to a row in 9 mm rows with a
  1.8 mm label above 3.5 mm text. Text that does not fit its cell is made smaller, down to
  2 mm, and then shortened with "...". Views are placed above the block.
- **Decision.** The fields are edited in the left pane, in the drawing: add, remove, rename,
  reorder, edit text, and go back to the standard block. Saving a block as a template of its
  own for other drawings is later work.
- **Decision.** The drawing chooses which configuration of the part it shows (by name; none
  means the part's active configuration as saved). The views, the model dimensions and the
  title block all come from that configuration. A name the part no longer has is reported
  with what to do.

## DXF export

- **Observed.** Drawings can be saved as DXF or DWG. Options: the target AutoCAD version;
  AutoCAD standard or TrueType fonts; mapping SOLIDWORKS line fonts to AutoCAD stock line
  types (and, from R2000, line weights) or keeping SOLIDWORKS styles; custom layer and
  colour maps; "scale output 1:1", which writes model geometry at full size based on the
  most common view scale instead of the sheet scale; end point merging; splines as splines
  or polylines; views as blocks; and the active sheet, all sheets to separate files, all
  sheets to one file, or all sheets to paper space (S71, S72).
- **Unknown.** The default of each option, and how dimensions and title-block text are
  written (as DXF dimension entities or as plain lines and text).
- **Standard (D1).** DXF is a tagged text format of group-code and value pairs in sections
  (`HEADER`, `TABLES`, `ENTITIES`); R12 (`AC1009`) is read by every DXF reader and has
  `LINE`, `CIRCLE`, `ARC`, `POLYLINE`/`VERTEX`/`SEQEND`, `SOLID` and `TEXT` entities and
  `LTYPE` and `LAYER` tables.
- **Decision.** Export DXF writes the sheet as R12 ASCII DXF at sheet size (one drawing unit
  is one sheet millimetre, `$INSUNITS` 4 and `$MEASUREMENT` 1), the same picture as the PDF:
  - lines are `LINE`; circles and circular arcs are exact `CIRCLE` and `ARC`; ellipses
    (circles seen at an angle) and traced curves are 2D `POLYLINE`s within 0.01 mm, since
    R12 has no ellipse;
  - layers by role: `VISIBLE`, `HIDDEN` (linetype `DASHED`, 3 mm dashes and 1 mm gaps),
    `THIN`, `FRAME`, `DIMENSIONS` and `TITLE`, all colour 7 (black on white, white on
    black);
  - dimensions are written as plain lines, filled `SOLID` arrowheads and `TEXT`, so every
    reader shows them as they are on the sheet; Ø and ° are written as `%%c` and `%%d`;
  - the title block as lines and `TEXT`.
  "Scale output 1:1", other versions, DWG and several sheets are not offered: a drawing has
  one sheet, and DXF at sheet size matches the PDF.

## Section views

- **Observed.** A section view cuts a parent view with a cutting line (section line). The
  line can be straight or an offset section defined by a stepped line, and can include
  concentric arcs (S80). The tool offers vertical, horizontal, auxiliary (any angle) and
  aligned lines, with arc, single and notch offsets that can be combined (S80, S81, S83).
  A single offset is set by a first point on the cutting line and a second point giving
  the depth of the offset; a notch by two points on the line and a third for its depth
  (S84, S85).
- **Observed.** The cut direction is shown by arrows and is reversed with Flip direction or
  by double-clicking the line (S82, S91). Each section line and view has a letter label
  that can be edited (S82, S91). Cutting lines are not always required by international
  standards and can be hidden (S80).
- **Observed.** A section of a section view is calculated from the original model, and
  section views update when the model changes (S89). A section view needing an update is
  marked with hatching (S91).
- **Observed.** Options include a partial section limited to the line's length, a slice
  showing only the cut faces, hiding the "shoulders" (the edges the line's corners make in
  the view, S90), emphasizing the outline of cut faces after ISO 128-50, and a depth that
  limits what is shown behind the cut (S80, S82). A section view uses the parent's scale,
  the sheet's scale or its own, and the parent's display style unless set otherwise (S82).
- **Observed.** If the line does not cross the whole model, portions are shown uncut;
  a cut that leaves zero-thickness geometry is reported (S90).
- **Observed.** Section view labels follow a drafting standard and show a name, the letter,
  and the scale; the label can be shown above the view (S88). The line style choices
  include a continuous line with thick ends and changes of direction (S88).
- **Unknown.** The default name text of the label, the arrow shape and size, the default
  cut direction, and where a new section view is placed relative to its parent beyond
  "drag the preview to a location" (S83).
- **Inferred.** The cutting line belongs to the parent view; the cut removes the material
  between the viewer and the cutting surface, and the section view looks in the direction
  the arrows point. An offset section's cut faces lie at several depths but are all seen
  square on, so the view is still orthographic.
- **Decision.** Section View cuts a front, top or side view, or another section view, with
  a horizontal or vertical cutting line through points the user clicks on the view. One
  point makes a full section. Each further point makes an offset section: the line runs
  through every point, in order along the line, and steps square to itself halfway between
  neighbouring points (a single or notch offset in S84's terms, as many as needed). Arcs,
  auxiliary and aligned lines, partial, slice and depth-limited sections are later work.
- **Decision.** The arrows point away from the material kept: a horizontal line looks
  down, a vertical line looks to the left, as seen on the parent; Flip direction reverses
  it. The section view looks the way the arrows point, so it is one of the six standard
  orthographic directions (front, back, top, bottom, left or right). It is cut from the
  part's exact bodies again whenever the views update; the jog faces of an offset section
  are seen edge on, and the lines where they meet the cut faces (the shoulders) are left
  out, as S82's "hide cutting line shoulders" does.
- **Decision.** If the cut fails (the kernel cannot cut the body there), the view shows
  nothing and the left pane says why and what to try (move the line); the rest of the
  drawing is unaffected.
- **Decision (after ISO 128-40 and 128-44).** The cutting line is a thin long-dash dot line
  (12 mm dashes, then a 0.5 mm dot with 1.25 mm gaps either side) with thick 0.5 mm strokes
  6 mm long at its ends and 3 mm either side of each corner; it runs 6 mm past the parent
  view's lines, so the thick ends stay outside them. Each end
  has a filled arrow (shaft 7 mm, head 4 mm by 1.6 mm) pointing the way the view looks,
  with the letter (5 mm) beside it. Letters run A, B, C... skipping I, O and Q (ASME
  Y14.2 practice, to avoid confusion with numbers), shared with detail views, and can be
  changed. The view's label is `SECTION A-A` in 5 mm text centred under the view; when its
  scale differs from the sheet's, `SCALE 2:1` follows on a second line in 3.5 mm text.
- **Decision.** Section views use the sheet's scale and their own display style, which
  starts as hidden lines removed and tangent edges removed. Model dimensions are not put in
  section views; reference dimensions can be.

## Hatching

- **Observed.** Hatching is applied automatically to the cut faces of section views
  (S86). Its pattern, scale and angle can be the material's crosshatch as specified in the
  part document (S87). The pattern alternates between bodies of a multibody part and
  between components of an assembly (S82). An option applies the view's scale to the
  hatch (S82), so by default the spacing does not follow the view scale. Patterns come
  from a file of named patterns that ships with the product (S86).
- **Unknown.** The patterns, spacing and angle SOLIDWORKS uses for each material, and the
  default of "scale hatch pattern".
- **Standard.** ISO 128-50 draws hatching as thin lines, usually at 45 degrees, with the
  spacing chosen for the size of the area; adjacent parts are hatched at different angles
  or spacings. ASME Y14.2 gives symbols for families of materials (iron, steel, copper
  alloys, light alloys, plastics and others). The standard texts are paid and were not
  read; the families below are AnvilCAD's own patterns in that spirit, not copies of the
  standard's drawings.
- **Decision.** Every cut face is hatched with thin 0.25 mm lines (as the "Thin" pen), at a
  spacing fixed on paper whatever the view's scale, after the part's material:
  - no material, titanium and anything not listed below: single lines 3 mm apart;
  - steels and irons (name containing steel, iron, AISI, ASTM or 17-4): pairs of lines
    0.8 mm apart, every 4 mm;
  - copper alloys (brass, bronze, copper): solid lines alternating with dashed lines
    (2 mm dashes, 1 mm gaps), 2 mm apart;
  - light alloys (aluminium, magnesium, and the 1060, 5052, 6061 and 7075 alloys): solid
    lines alternating with long-dash dot lines, 2 mm apart;
  - plastics (the material class): lines both ways (crossed at 90 degrees), 3 mm apart.

  Lines run at 45 degrees for the first body and 135 degrees for the next, alternating
  (S82's alternation). All hatch lines lie on one grid per view, so neighbouring cut faces
  of one body line up across a step.

## Detail views

- **Observed.** A detail view shows a portion of a view, usually enlarged; the portion is
  enclosed by a circle or another closed sketch; the parent can be an orthographic,
  isometric, section, crop or exploded view, or another detail view (S92, S95). A default
  scaling factor sets the detail's scale as a factor of the parent's (S92). The view can
  have no outline, a full outline or a jagged outline; the detail circle has a style and a
  letter label (S93, S94). Labels show a name, letter and scale (S94).
- **Unknown.** The default scaling factor and the default outline.
- **Decision.** Detail View takes a circle on any view: click its centre, then a point on
  the circle. The detail starts at twice the parent's scale, rounded up to the next
  standard scale, and its scale can be typed. It shows the parent's lines and hatching
  clipped to the circle, with a thin full outline (an AnvilCAD choice, so the edge of what
  is shown is clear). The parent shows the circle as a thin line with the letter (5 mm)
  outside it; the view's label is `DETAIL B` with `SCALE 2:1` under it. Other profiles and
  jagged outlines are later work. Reference dimensions can be put on detail views (at the
  detail's scale); model dimensions cannot.

## Placing derived views

- **Unknown.** S83 and S95 only say the user drags the view to where it goes.
- **Decision.** Views do not store positions yet (see "The drawing document"). Section and
  detail views are placed in columns to the right of the standard views, top to bottom in
  the order they were made, each with its label below it; Fit and the scale chosen for a
  new drawing take them into account. When a new section or detail view does not fit at
  the sheet's scale, the scale is lowered to the largest standard one at which everything
  fits, as part of the same undo step, and the status line says so. Moving views by hand
  is later work.
- **Decision.** A section or detail view can be deleted; views made from it go with it,
  with their dimensions and callouts. Standard views cannot be deleted in this slice.

## Hole callouts

- **Observed.** The Hole Callout tool adds driven diameter dimensions to holes made by the
  hole feature or by circular cuts; it uses the hole feature's information and updates
  when the hole changes (S97). The hole's axis must be normal to the sheet (S97). The
  callout is placed by clicking a hole's edge and then where the text goes (S98). It holds
  the diameter, the depth when the hole has one, and for hole-feature holes more, such as
  a countersink or the number of instances (S97, S98). Callouts can be put in section and
  detail views too (S98).
- **Observed.** The text for each hole type comes from a format file with one definition
  per type and standard; a countersunk through hole is the hole diameter and "thru" on one
  line and the countersink symbol, its diameter, `X` and its angle on the next (S99). The
  variables include the type, standard, fastener size, counterbore depth and diameter and
  countersink angle (S99, S100). The number of instances is included for holes of one
  feature or pattern; JIS can write it with `x` or `-` (S97, S101).
- **Unknown.** The exact default text of each hole type (the format file was not read),
  the default instance indicator outside JIS, and how tapped hole threads are written.
- **Decision.** Callout text, one line per item, top to bottom, in the drawing's
  dimension text (3.5 mm), millimetres to two decimals with trailing zeros dropped. Words
  stand in for the counterbore, countersink and depth symbols, because the PDF's
  Helvetica and the DXF's text styles have no such glyphs (ASME Y14.5's older word forms):
  - hole: `Ø6.6 THRU` or `Ø6.6 X 10 DEEP`;
  - counterbore: the hole's line, then `CBORE Ø11 X 6.4 DEEP`;
  - countersink: the hole's line, then `CSK Ø12.4 X 90°`;
  - tapped hole: the tap drill's line (`Ø5 X 15 DEEP` or `Ø5 THRU`), then the thread:
    `M6x1 X 12 DEEP` for ISO metric sizes and holes with no standard size, `1/4-20 UNC X
    12.7 DEEP` for ANSI inch sizes (the size name and the coarse series). The thread depth
    is always given, as the Hole feature records it, even on a through hole;
  - a near-side countersink adds `CSK Ø.. X ..°` as the last line;
  - when the feature makes more than one hole, the first line starts with the count and
    `X`, as `4X Ø6.6 THRU`.

  A blind hole's drill point is not in the callout (its depth is to the shoulder, as the
  Hole feature measures it).
- **Decision.** Hole Callouts (like Model Dimensions, for holes) puts one callout per Hole
  feature into the first front, top or side view that looks along the hole's axis from
  the side it is drilled from, or failing that from the other side. A feature already
  called out is skipped. The leader runs from the text, with a 4 mm shoulder, to the edge
  of the feature's first hole with a filled arrow; the text sits beside the shoulder. By
  default the leader rises steeply to the right until the text clears the top of the view
  by 2 mm, so it sits in the space above the view and not over the part or the view beside
  it. The
  callout's text is dragged like a dimension's value. A callout whose feature is gone or
  suppressed is counted in the left pane like a missing model dimension and left out of
  PDF and DXF. Clicking a single hole, callouts on holes made by cuts, section and detail
  views, and tolerances are later work.

## Assembly drawings, BOM tables and balloons (not yet built, #210)

- **Observed.** A bill of materials can be inserted into the drawing of an assembly; it
  updates when components are added, deleted or replaced or their properties change
  (with automatic update on); rows combine when every field but the item number is the
  same; the part number column cannot be deleted (S106). The table has a stationary
  corner and an anchor; BOM types are top-level only, parts only and indented (S106).
- **Observed.** Balloons label components with their item numbers in the BOM; a balloon is
  attached by clicking a component in a view of the assembly and placing it; styles
  include circular, and sizes are fitted to the number of characters (S103, S104). Auto
  balloons put balloons in views without duplicates, numbered in assembly order or
  sequentially, with layouts (square, circular, top, bottom, left, right) and an option to
  balloon one instance only of repeated components (S105).
- **Decision (for the next slice).** A drawing of an `.anvasm` shows the placed exact
  bodies of every component. A BOM table, fed by AnvilCAD's existing bill of materials
  (`anvil-model` `Assembly::bill_of_materials`: item, part number, description,
  quantity), sits above the title block with its stationary corner at the lower right.
  Auto Balloons puts one circular balloon per BOM row, numbered with the row's item, in the
  first view where one of its components has a visible line, with a leader ending in a dot
  on that line. This waits on subassemblies (#93), which changes the bill of materials.

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
6. How "the most appropriate view" for a model dimension is chosen (S60, S63), and how
   dangling reference dimensions behave.
7. Factory dimension styles (arrow and text sizes, precision) and DXF export defaults.
8. Section line arrow shapes, label wording and placement defaults (S83, S88); hatch
   patterns per material (S87); the detail scale factor and outline defaults (S92, S93);
   the hole callout format file's default text (S99).
9. Later slices: assembly drawings with BOM tables and balloons; auxiliary, aligned,
   partial and broken views and arc offsets; callouts on cut holes; editing model dimensions from the drawing; dimensions of other features (fillet
   radii, hole sizes, pattern counts, revolve angles); title-block templates saved apart
   from a drawing; annotations; several sheets, portrait and custom sizes, moving views by
   hand, shaded views, assemblies, and keeping projected geometry in the drawing so it opens
   without its part.
