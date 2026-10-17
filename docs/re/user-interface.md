# Main window layout

What AnvilCAD's main window should offer so that SOLIDWORKS users find their way around,
written before the October 2026 interface refresh. The aim is a familiar *arrangement*
(where things are and how they behave), not a copy of SOLIDWORKS's look: AnvilCAD keeps its
own names, colours, icons and artwork.

## How this was studied

Only published SOLIDWORKS help pages were read. No SOLIDWORKS software, files or binaries
were installed, opened or analysed.

REA 6.5.0 (`npx rea-agents capture-browser-scenario`) loaded each page in a provider-owned
headless Chromium 155 (launch mode, temporary profile, 1280x720, light scheme, service
workers blocked), waited 2.5 s and captured the page's accessibility tree and final URL.
Each capture is recorded as REA Evidence:

| Page (SOLIDWORKS Design Help 2025, help.solidworks.com) | REA evidence id |
|---|---|
| `sldworks/c_user_interface_overview.htm` | `ev_c9e9266cc65ed82534c4fbf2857f8a7b8fefa972d5e76c4f579a23a7dc854073` |
| `sldworks/c_commandmanager.htm` | `ev_6f7577575b411b1de99bc1a604b9372918ef06fb3f19eff4714eca5f1c9c30ef` |
| `sldworks/c_featuremanager_design_tree_overview.htm` | `ev_8cac778d3de76ab63d60c9e91aebcfdf824f6356409d6881a914177b67c36295` |
| `sldworks/c_heads_up_view_toolbar.htm` | `ev_8ab4b8b19e51257377c3612c792e54052cc72f69f4f8b205cd712a04c2dc20cf` |
| `sldworks/c_propertymanager_overview.htm` | `ev_1f90cc71…` (page not found; nothing learned) |

The accessibility tree holds the pages' text and the captions of their illustrations, not
the illustrations themselves. Nothing here is taken from the pictures.

## Observations (stated by the help pages)

- **Window elements.** The overview names, in order: menu bar, toolbars, CommandManager,
  FeatureManager design tree tab, PropertyManager tab, ConfigurationManager tab, design
  tree filter, FeatureManager design tree, selection breadcrumbs, heads-up view toolbar.
- **Command bar.** A context-sensitive toolbar whose content changes with the tab chosen
  below it ("Sketch" tab shows the sketch tools). Buttons can show their names below the
  icon ("large buttons with text") or beside it, and can be grouped. Ctrl+Page Up / Page
  Down step through the tabs. Its purpose is to keep the tools in one place and leave room
  for the graphics area.
- **Feature tree.** Linked both ways with the graphics area: selecting in either selects
  in both. Double-clicking a feature shows its dimensions; slow double-click renames;
  features can be suppressed, reordered and rolled back with a rollback bar. Fixed items
  near the top: material, origin, reference planes; folders for equations, bodies and so
  on. The tree, the property panel and other managers share the left pane and are
  switched by tabs at its top. F9 hides the pane.
- **Property panel.** While a feature is created or edited, its options replace the tree
  in the left pane (the tree is still reachable as a fly-out).
- **Heads-up view toolbar.** A transparent toolbar inside each viewport with the common
  view tools: view orientation (standard and saved views), display style, zoom to fit.

## Derivations for AnvilCAD

- Keep the four zones and their places: menus at the top, a tabbed command bar under them,
  the left pane for the feature tree or a feature's property panel, and the viewport with
  a floating view toolbar. These are what users' habits depend on.
- Command buttons carry an icon and their name. Names must stay exactly as they are today
  (`Extruded Boss/Base`, `Fillet`, …): the headless harness and its scripts find widgets by
  label (see [AGENT_HARNESS.md](../AGENT_HARNESS.md)).
- Group buttons in the command bar with small captions, so the bar reads as sets of
  related tools rather than one long row.
- The view toolbar offers the standard views, zoom to fit and the edges toggle, matching
  the View menu and its Ctrl+1…7 and F shortcuts.
- Ctrl+Page Up / Page Down step through command tabs.

## Deliberate differences (AnvilCAD's own design)

- A flat, quiet look: neutral warm greys, one accent colour (AnvilCAD's iron oxide, from
  its logo and website), rounded corners, generous spacing, no bevels or gradients on
  controls. Selection is steel blue.
- Icons are AnvilCAD's own line drawings, painted by the app; no SOLIDWORKS icons,
  artwork or colour scheme.
- A light and a dark theme, switched from the View menu.
- The window title, part name and units sit in AnvilCAD's own status and header layout.

## Unknowns and limits

- The pages' illustrations were not read, so exact sizes, spacing and colours of
  SOLIDWORKS's window are unknown; this is intended, since AnvilCAD does not copy them.
- The PropertyManager overview page was not found at the URL tried; its behaviour above
  comes from the FeatureManager page's description of the shared left pane.
- Configurations, DimXpert, display manager, design tree filter and selection breadcrumbs
  have no AnvilCAD counterpart yet and are left out.
