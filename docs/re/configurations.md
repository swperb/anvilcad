# Configurations and the configuration manager

What AnvilCAD's configurations do. A configuration is a named variant of a part, kept in
the same file: the same feature tree with different dimensions, suppressed features,
global variable values, material or properties. SOLIDWORKS users know the idea from its
configurations and ConfigurationManager, so AnvilCAD follows their published behaviour
where that makes sense. This specification was written for the implementation in
`crates/anvil-model/src/config.rs` (model, storage, switching), `crates/anvil-model/src/document.rs`
(hooks on adding and deleting features) and `crates/anvil-app/src/app.rs` (the
configuration manager, the configuration table and the properties editor).

## How this was studied

- **Inputs.** Only published information was used: Dassault's public online help for
  SOLIDWORKS 2025 and its 2024 API help. No SOLIDWORKS software, installer, library or
  file was downloaded, installed, run or analysed. No SOLIDWORKS licence was accepted on
  the machine used.
- **Retrieval.** As for [hole-wizard.md](hole-wizard.md): each article was fetched over
  HTTPS with a browser user agent, its text read from the page's embedded `__NEXT_DATA__`
  JSON (`props.pageProps.helpContentData.helpText`) and stripped of HTML by a script run
  with `python3 -I`. A missing page still returns HTTP 200, so each page's content was
  checked for "cannot be found". Links between articles were followed from the overview
  page. The text was paraphrased; none of it is copied into this repository.
- **REA.** REA (`rea-agents` 6.5.0, with the `third_party/rea` submodule at `3fc0075`) was
  set up and its research method followed:
  - each finding below is marked **Observed** (stated in a cited source), **Inferred**
    (our reasoning from the sources), **Decision** (AnvilCAD's own choice where the
    sources are silent) or **Unknown**;
  - every observation cites its source.

  REA's analysis tools were **not** run. They inspect artifacts (binaries, packages,
  captures, files), and the only lawful inputs for this behaviour are prose documents.
  How `.sldprt` files store configurations was not studied; reading them is future work
  that needs sample files and its own specification.

## Sources

Base A is `https://help.solidworks.com/2025/English/SolidWorks/sldworks/`. Base B is
`https://help.solidworks.com/2024/English/api/sldworksapi/`. Base C is
`https://help.solidworks.com/2024/English/api/swconst/`. All were read on 2026-10-18.

| # | Topic | URL | Version |
|---|---|---|---|
| S1 | Configurations overview | A + `c_Configurations_Overview.htm` | 2025 |
| S2 | Configuration views (the ConfigurationManager tab, sort order) | A + `r_ConfigurationManager_configurations.htm` | 2025 |
| S3 | Derived configurations | A + `c_Derived_Configurations.htm` | 2025 |
| S4 | Configurable items for parts | A + `c_Configurable_Items_for_Parts.htm` | 2025 |
| S5 | Configurable parameters (index) | A + `r_Configurable_Parameters.htm` | 2025 |
| S6 | Activating a configuration | A + `t_Activating_a_Configuration.htm` | 2025 |
| S7 | Creating configurations manually | A + `t_Creating_a_Configuration_Manually.htm` | 2025 |
| S8 | Configuration Properties / Add Configuration PropertyManager | A + `HIDD_NEW_ADD_CONFIG.htm` | 2025 |
| S9 | Specify Configurations dialog box | A + `HIDD_APPLY_TO_CONFIGS.htm` | 2025 |
| S10 | Deleting a configuration manually | A + `t_Deleting_a_Configuration_Manually.htm` | 2025 |
| S11 | Modify Configurations (overview) | A + `c_ModifyCfg_Modify_Configurations.htm` | 2025 |
| S12 | Modify Configurations dialog box | A + `r_modify_configurations_dialog.htm` | 2025 |
| S13 | Adding and modifying parameters in the table | A + `t_modifying_parameters_configurations.htm` | 2025 |
| S14 | Adding and modifying features in the table | A + `t_modifying_features_configurations.htm` | 2025 |
| S15 | Dimensions in configurations | A + `r_Dimensions_in_Configurations.htm` | 2025 |
| S16 | Feature suppression state in configurations | A + `r_feature_suppression_state.htm` | 2025 |
| S17 | Global variables in configurations | A + `c_DT_Global_Variables.htm` | 2025 |
| S18 | Materials in configurations | A + `r_Materials_in_Configurations.htm` | 2025 |
| S19 | Custom properties in configurations | A + `r_Custom_Properties_in_Configurations.htm` | 2025 |
| S20 | Custom and configuration properties | A + `HIDD_CUSTOM_PROPERTIES.htm` | 2025 |
| S21 | Design table configurations | A + `c_Design_Table_Configurations.htm` | 2025 |
| S22 | `IConfiguration` members | B + `SolidWorks.Interop.sldworks~SolidWorks.Interop.sldworks.IConfiguration_members.html` | 2024 |
| S23 | `swInConfigurationOpts_e` | C + `SOLIDWORKS.Interop.swconst~SOLIDWORKS.Interop.swconst.swInConfigurationOpts_e.html` | 2024 |
| S24 | `swConfigurationOptions2_e` | C + `SOLIDWORKS.Interop.swconst~SOLIDWORKS.Interop.swconst.swConfigurationOptions2_e.html` | 2024 |

## What a configuration is

- **Observed (S1).** Configurations are variations of a model within one document. In a
  part they vary dimensions, features and properties, including custom properties.
- **Observed (S1, S7).** A configuration is made by giving it a name and properties, then
  changing the model while it is active.
- **Observed (S8, S22).** A configuration has a name, an optional description and an
  optional comment. It has configuration-specific custom properties (S20, S22
  `CustomPropertyManager`).
- **Observed (S8).** A name must not be blank, must not contain `/` or `@`, and must not
  start or end with a space. It must not repeat another configuration's name.
- **Decision.** A new part has one configuration, named "Default". Parts saved before
  configurations existed open with that one configuration.

## The parameters a part configuration can vary

- **Observed (S4, S5).** For parts: feature and sketch dimensions, tolerances, suppression
  of features and equations, end conditions, mass overrides, sketch planes and relations,
  face colours, base and split parts, the driving state of sketch dimensions, derived
  configurations, configuration-specific properties, hole sizes and material. Global
  variables can be configured (S17).
- **Observed (S11).** The Modify Configurations table configures, in parts, dimensions of
  features and sketches, suppression states, material and custom properties.
- **Implemented.** Four kinds of configured parameter:
  - the suppression state of a feature or sketch (S16);
  - a feature dimension: extrude depth, revolve angle, reference plane offset, pattern
    spacing, count and angle, fillet radius, chamfer distance and angle, shell thickness
    (S15). A dimension may be a number or an expression;
  - a global variable's expression (S17);
  - the material (S18).
- **Decision.** Sketch dimensions are not configured directly. A sketch dimension can
  already be driven by an expression over global variables; configuring that variable
  configures the dimension. This also avoids pinning a configured value to a constraint's
  position in the sketch, which changes when constraints are deleted.
- **Not implemented.** End conditions, hole sizes, sketch planes and relations, colours,
  tolerances, mass overrides, base and split parts. The help names them (S5); they are
  future work.

### Configured and unconfigured parameters

- **Observed (S13).** A parameter is either configured, with a value per configuration, or
  unconfigured, with one value for all. *Unconfigure* applies the active configuration's
  value to all configurations and removes the parameter's column.
- **Observed (S9, S15, S16).** When a dimension or a suppression state is changed in a
  model with several configurations, the change applies to this configuration, all
  configurations, or the ones specified. The active configuration is always included.
- **Decision.** In AnvilCAD an ordinary edit to a configured parameter (in the feature
  editor, the tree's Suppress command, the equations window or the material picker)
  changes only the active configuration. An edit to an unconfigured parameter changes
  all configurations. To vary a parameter, the user configures it first (*Configure
  Feature* on a feature, or adding it to the configuration table). This gives "this
  configuration" and "all configurations" without a dialog on every edit; "specify
  configurations" is done in the configuration table.
- **Observed (S11, S14).** Removing a column from the table leaves the model unchanged.
  **Decision.** AnvilCAD's table shows every configured parameter, so the only way to
  remove a column is to unconfigure it.

## Activating

- **Observed (S6).** Double-clicking a configuration, or *Show Configuration* on it, makes
  it the active configuration and the model updates to it.
- **Observed (S24).** `swConfigOption_DontActivate` exists to stop a new configuration
  becoming active, so by default **(Inferred)** a newly added configuration is activated.
  AnvilCAD activates a new configuration.
- **Inferred (S1, S7).** A new configuration starts as a copy of the model as it is when
  it is made, then is changed. AnvilCAD copies the active configuration's values of every
  configured parameter into a new top-level configuration.

## Derived configurations

- **Observed (S3).** A derived configuration is a child of another. Its parameters are
  linked to the parent: changing the parent changes the child. Changing a parameter while
  the child is active overrides it in the child only, and that parameter is no longer
  linked.
- **Observed (S12).** The table highlights a derived configuration's overridden values.
- **Observed (S23).** `swLinkedToParent` is valid only for derived configurations.
  **Decision.** In the table a derived configuration's cell can be set back to *Link to
  parent*, which removes its override.
- **Decision.** Deleting a configuration that has derived children keeps the children:
  each takes on the values it was inheriting, and becomes a child of the deleted
  configuration's parent (or a top-level configuration).

## Suppressing new features

- **Observed (S8, S24).** A part configuration's *Suppress features* option suppresses in
  it new features added while another configuration is active. With the option off, new
  features are unsuppressed in it.
- **Implemented.** Adding a feature while several configurations exist configures that
  feature's suppression when any other configuration has the option on: unsuppressed in
  the active configuration and in those without the option, suppressed in those with it.

## Deleting

- **Observed (S10).** Deleting a configuration deletes no features. The active
  configuration cannot be deleted.
- **Decision.** The last configuration cannot be deleted either.

## The configuration manager

- **Observed (S2).** The ConfigurationManager is a tab at the top of the manager pane,
  beside the feature tree. It lists the configurations, with derived ones under their
  parents, and a folder of saved tables.
- **Observed (S8).** *Add Configuration* is on the part's context menu. *Properties* on a
  configuration edits its name, description, comment, options and custom properties.
- **Observed (S12).** The Modify Configurations table lists configurations as rows and
  configured parameters as columns, with the active configuration in bold. Cells hold
  numbers, check boxes for suppression and a list for materials. Its commands include
  *Rebuild all configurations*.
- **Implemented.** A *Configurations* tab beside *Features* in the left pane: the
  configurations as a tree (active one highlighted), with *Show Configuration*, *Add
  Derived Configuration*, *Properties* and *Delete* on each, and *Add Configuration*. A
  *Configuration Table* window lists configurations as rows and every configured parameter
  as a column. It edits values, links derived values back to their parent, adds and
  unconfigures parameters, and rebuilds all configurations to show each one's volume and
  mass.
- **Not implemented.** Saved table views, sort orders (S2), configuration previews (S2),
  design tables in a spreadsheet (S21) and the configuration publisher.

## Custom properties

- **Observed (S20).** Properties are held on a *Custom* tab (the file's) and a
  *Configuration Specific* tab (each configuration's).
- **Observed (S19).** In a design table a configuration-specific property left blank is
  undefined for that configuration.
- **Decision.** Where a configuration-specific property has the same name as a file
  property, the configuration's value is used for that configuration.
- **Unknown.** Which of the two SOLIDWORKS shows in a bill of materials when both exist.
  The help describes the tabs but not a precedence.

## Bill of materials options

- **Observed (S8).** A configuration chooses its BOM part number (document name,
  configuration name, parent's, or a user name) and whether it is excluded from a BOM.
- **Not implemented.** AnvilCAD has no assemblies or BOMs yet.

## Storage in AnvilCAD's part file

- **Decision.** The feature tree in memory and on disk always holds the active
  configuration's values. Alongside it the file stores the configurations and, for each
  configured parameter, its value in each configuration. A derived configuration stores
  only the values it overrides. Switching configurations first records the tree's current
  values into the active configuration, then writes the new one's into the tree.
- **Decision.** A part with one configuration and nothing configured is saved as format
  version 1, which earlier builds read. A part with more is saved as version 2, so earlier
  builds refuse it instead of silently dropping its configurations.

## Unknown or not covered

1. How `.sldprt` stores configurations and configured values. Nothing here was learnt
   from files.
2. SOLIDWORKS's default choice between "this configuration" and "all configurations" when
   a dimension is changed with no prompt. The help lists the options, not the default.
3. Whether configuration-specific or file properties win in a BOM.
4. Whether deleting a parent deletes its derived configurations in SOLIDWORKS.
