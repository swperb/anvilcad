//! The app's side of Lua configuration and scripting (see `crate::script` and
//! docs/SCRIPTING.md): named built-in commands, user key bindings, the command palette,
//! loading and live-reloading `init.lua`, and the [`Host`] that scripts drive.

use super::*;
use crate::keymap::{Matcher, Step};
use crate::script::{
    self, Action, BlendSpec, CircularSpec, CombineHow, CombineSpec, Depth, DirPick, DraftSpec, Engine,
    ExtrudeSpec, HoleSpec, Host, InsertSpec, InterferenceSpec, MateSide, MateSpec, MirrorSpec, PatternSpec,
    PlaneDef, PlanePick, PlaneSpec, RevolveSpec, RibSpec, ShellSpec, SketchSpec, SplitSpec,
};
use anvil_model::assembly::{MateEntity, MateRef};
use anvil_model::mates::{Alignment, MateKind};
use std::time::SystemTime;

/// Every built-in command a key binding, script (`anvil.run`) or the command palette can run,
/// with a short description. Names match the toolbar and menus.
pub const BUILTIN_COMMANDS: &[(&str, &str)] = &[
    ("New", "Start a new part"),
    ("New Assembly", "Start a new assembly"),
    ("New Drawing", "Start a drawing of the open part"),
    ("Update Views", "Project the drawing's views again"),
    ("Change Part", "Point the drawing at another part"),
    ("Model Dimensions", "Bring the part's dimensions into the drawing views"),
    ("Reference Dimension", "Dimension the drawing views"),
    ("Export PDF", "Export the drawing as PDF"),
    ("Export DXF", "Export the drawing as DXF"),
    ("Insert Component", "Place a part in the assembly"),
    ("Mate", "Add a mate between two components"),
    ("Bill of Materials", "Show the assembly's parts list"),
    ("Interference", "Find components that overlap, touch or are too close"),
    ("Open", "Open a part, template, STEP or SOLIDWORKS file"),
    ("Save", "Save the part"),
    ("Save As", "Save the part under a new name"),
    ("Save as Part Template", "Save the part as a template for new parts"),
    ("Export", "Export STEP, 3MF, STL or OBJ"),
    ("Open Sample Part", "Open the sample mounting block"),
    ("Undo", "Undo the last change"),
    ("Redo", "Redo the last undone change"),
    ("Rebuild", "Rebuild the part"),
    ("Delete", "Delete the selected feature"),
    ("Extruded Boss/Base", "Extrude a sketch into a body"),
    ("Revolved Boss/Base", "Revolve a sketch into a body"),
    ("Extruded Cut", "Cut by extruding a sketch"),
    ("Revolved Cut", "Cut by revolving a sketch"),
    ("Linear Pattern", "Repeat features along directions"),
    ("Circular Pattern", "Repeat features around an axis"),
    ("Mirror", "Mirror features about a plane"),
    ("Fillet", "Round edges"),
    ("Chamfer", "Bevel edges"),
    ("Shell", "Hollow the body"),
    ("Hole", "Add a hole at the picked face point"),
    ("Sweep", "Sweep a profile along a path"),
    ("Loft", "Loft between profiles"),
    ("Draft", "Taper faces"),
    ("Dome", "Dome a face"),
    ("Rib", "Add a rib from an open sketch"),
    ("Combine", "Combine bodies"),
    ("Move/Copy Bodies", "Move or copy bodies"),
    ("Delete/Keep Bodies", "Delete or keep bodies"),
    ("Split", "Split bodies"),
    ("Scale", "Scale bodies"),
    ("Plane", "Add a reference plane"),
    ("Axis", "Add a reference axis"),
    ("Point", "Add a reference point"),
    ("Coordinate System", "Add a coordinate system"),
    ("Sketch", "Start a sketch on the selected plane or face"),
    ("Exit Sketch", "Finish the open sketch"),
    ("Convert Entities", "Project edges into the open sketch"),
    ("Front", "Front view"),
    ("Back", "Back view"),
    ("Left", "Left view"),
    ("Right", "Right view"),
    ("Top", "Top view"),
    ("Bottom", "Bottom view"),
    ("Isometric", "Isometric view"),
    ("Normal To", "Look straight at the open sketch"),
    ("Zoom to Fit", "Fit the part in the view"),
    ("Shaded with edges", "Show or hide model edges"),
    ("Dark Theme", "Switch to the dark theme"),
    ("Light Theme", "Switch to the light theme"),
    ("Toggle Theme", "Switch between light and dark"),
    ("Features Tab", "Show the Features commands"),
    ("Sketch Tab", "Show the Sketch commands"),
    ("Evaluate Tab", "Show the Evaluate commands"),
    ("Mass Properties", "Show mass, volume and centroid"),
    ("Check Design (DFM)", "Check the part for manufacturability"),
    ("Measure", "Measure between picks"),
    ("Section View", "Show or hide the section view"),
    ("Equations", "Edit global variables and equations"),
    ("Custom Properties", "Edit the part's custom properties"),
    ("Configuration Table", "Edit configurations"),
    ("Command Palette", "Find and run any command"),
    ("Run Script...", "Run a Lua script"),
    ("Reload Config", "Read init.lua again"),
    ("Edit Config", "Create init.lua if needed and show where it is"),
];

/// The built-in commands that work while a drawing is open.
const DRAWING_COMMANDS: &[&str] = &[
    "New",
    "New Assembly",
    "New Drawing",
    "Open",
    "Save",
    "Save As",
    "Update Views",
    "Change Part",
    "Model Dimensions",
    "Reference Dimension",
    "Export PDF",
    "Export DXF",
    "Delete",
    "Open Sample Part",
    "Undo",
    "Redo",
    "Rebuild",
    "Zoom to Fit",
    "Dark Theme",
    "Light Theme",
    "Toggle Theme",
    "Command Palette",
    "Run Script...",
    "Reload Config",
    "Edit Config",
];

/// The command palette's state while it is open.
#[derive(Default)]
pub(super) struct CommandPalette {
    query: String,
    selected: usize,
    focus: bool,
}

/// Scripting state kept by the app.
#[derive(Default)]
pub(super) struct Scripting {
    pub(super) engine: Option<Engine>,
    /// A copy of the loaded config's options, readable while a script has the engine.
    pub(super) options: script::Options,
    /// The config file, if the app reads one (the headless harness does not by default).
    path: Option<PathBuf>,
    modified: Option<SystemTime>,
    /// When the file was last checked for changes (egui time, seconds).
    checked: f64,
    matcher: Matcher,
    pub(super) palette: Option<CommandPalette>,
    /// Whether the startup options (what to open, the starting view) were applied yet.
    started: bool,
    /// A problem with the template or material options, shown once the new part is loaded.
    new_part_warning: Option<String>,
    /// The last `anvil.status` text of the running script.
    script_status: Option<String>,
}

/// The palette's list: built-in and user commands matching `query`, user commands first.
fn palette_items(engine: Option<&Engine>, query: &str) -> Vec<(String, String, bool)> {
    let q = query.to_ascii_lowercase();
    let words: Vec<&str> = q.split_whitespace().collect();
    let hit = |name: &str, desc: &str| {
        let hay = format!("{} {}", name.to_ascii_lowercase(), desc.to_ascii_lowercase());
        words.iter().all(|w| hay.contains(w))
    };
    let mut out = Vec::new();
    if let Some(e) = engine {
        for c in &e.config.commands {
            if hit(&c.name, &c.description) {
                out.push((c.name.clone(), c.description.clone(), true));
            }
        }
    }
    for (n, d) in BUILTIN_COMMANDS {
        if hit(n, d) {
            out.push((n.to_string(), d.to_string(), false));
        }
    }
    out
}

impl AnvilApp {
    /// Runs a built-in command by name (case-insensitive).
    pub(super) fn run_builtin(&mut self, ctx: &egui::Context, name: &str) -> Result<(), String> {
        let Some((name, _)) = BUILTIN_COMMANDS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) else {
            return Err(format!("no built-in command is called \"{name}\""));
        };
        if self.in_drawing() && !DRAWING_COMMANDS.contains(name) {
            return Err(format!("\"{name}\" works on parts; open a part to use it"));
        }
        match *name {
            "New" => self.request(PendingAction::New),
            "New Drawing" => self.request(PendingAction::NewDrawing),
            "Update Views" => self.update_views(),
            "Change Part" => self.change_drawing_part(),
            "Export PDF" => self.export_drawing_pdf(),
            "Export DXF" => self.export_drawing_dxf(),
            "Model Dimensions" => self.drawing_model_dimensions(),
            "Reference Dimension" => self.drawing_reference_dimension(),
            "New Assembly" => self.request(PendingAction::NewAssembly),
            "Insert Component" => self.insert_component(),
            "Mate" => self.start_mate(),
            "Interference" => self.open_interference(),
            "Bill of Materials" => {
                if let Some(s) = &mut self.asm {
                    s.bom = true;
                }
            }
            "Open" => self.request(PendingAction::Open),
            "Save" => self.save(false),
            "Save As" => self.save(true),
            "Save as Part Template" => self.save_template(),
            "Export" => self.export(),
            "Open Sample Part" => self.request(PendingAction::OpenSample),
            "Undo" => self.undo(),
            "Redo" => self.redo(),
            "Rebuild" => self.rebuild(),
            "Delete" if self.asm.is_some() => self.delete_assembly_selection(),
            "Delete" if self.in_drawing() => {
                self.drawing_delete();
            }
            "Delete" => {
                if matches!(self.selection, Selection::Feature(_)) && self.editor.is_none() {
                    self.delete_selected_feature();
                }
            }
            "Extruded Boss/Base" => self.cmd_extrude(BodyOp::Boss),
            "Revolved Boss/Base" => self.cmd_revolve(BodyOp::Boss),
            "Extruded Cut" => self.cmd_extrude(BodyOp::Cut),
            "Revolved Cut" => self.cmd_revolve(BodyOp::Cut),
            "Linear Pattern" => self.cmd_pattern(false),
            "Circular Pattern" => self.cmd_pattern(true),
            "Mirror" => self.cmd_mirror(),
            "Fillet" => self.cmd_edge_treatment(false),
            "Chamfer" => self.cmd_edge_treatment(true),
            "Shell" => self.cmd_shell(),
            "Hole" => self.cmd_hole(),
            "Sweep" => self.cmd_sweep(),
            "Loft" => self.cmd_loft(),
            "Draft" => self.cmd_draft(),
            "Dome" => self.cmd_dome(),
            "Rib" => self.cmd_rib(),
            "Combine" => self.cmd_body(0),
            "Move/Copy Bodies" => self.cmd_body(1),
            "Delete/Keep Bodies" => self.cmd_body(2),
            "Split" => self.cmd_body(3),
            "Scale" => self.cmd_body(4),
            "Plane" => self.cmd_plane(),
            "Axis" => self.cmd_axis(),
            "Point" => self.cmd_point(),
            "Coordinate System" => self.cmd_coordsys(),
            "Sketch" => self.cmd_sketch(),
            "Exit Sketch" => {
                if self.sketch.is_some() {
                    self.exit_sketch();
                }
            }
            "Convert Entities" => self.cmd_convert(),
            "Front" | "Back" | "Left" | "Right" | "Top" | "Bottom" | "Isometric" => {
                if let Some(v) = StdView::ALL.into_iter().find(|v| v.label() == *name) {
                    self.camera.set_view(v);
                    self.fit_pending = true;
                }
            }
            "Normal To" => {
                if let Some(s) = &self.sketch {
                    let f = s.frame;
                    self.camera.normal_to(&f);
                }
            }
            "Zoom to Fit" => {
                self.fit_pending = true;
                if let Some(s) = &mut self.drw {
                    s.fit_view();
                }
            }
            "Shaded with edges" => self.show_edges = !self.show_edges,
            "Dark Theme" | "Light Theme" | "Toggle Theme" => {
                self.dark = match *name {
                    "Dark Theme" => true,
                    "Light Theme" => false,
                    _ => !self.dark,
                };
                theme::apply(ctx, self.dark);
            }
            "Features Tab" => self.tab = Tab::Features,
            "Sketch Tab" => self.tab = Tab::Sketch,
            "Evaluate Tab" => self.tab = Tab::Evaluate,
            "Mass Properties" => self.windows.mass = true,
            "Check Design (DFM)" => self.open_dfm(),
            "Measure" => self.measure = Some(Vec::new()),
            "Section View" => self.toggle_section(),
            "Equations" => self.windows.equations = true,
            "Custom Properties" => self.windows.properties = true,
            "Configuration Table" => self.cfg.table = true,
            "Command Palette" => {
                self.scripting.palette = Some(CommandPalette { focus: true, ..Default::default() })
            }
            "Run Script..." => self.run_script_dialog(ctx),
            "Reload Config" => self.reload_config(ctx),
            "Edit Config" => self.edit_config(),
            _ => return Err(format!("\"{name}\" is listed but not wired up")),
        }
        Ok(())
    }

    /// Runs a command by name: a user command from `init.lua` first, then a built-in one.
    pub fn run_command(&mut self, ctx: &egui::Context, name: &str) -> Result<(), String> {
        let user = self.scripting.engine.as_ref().and_then(|e| e.user_command(name)).cloned();
        match user {
            Some(cmd) => self.with_engine(ctx, |engine, host| engine.run_command(&cmd, host)),
            None => self.run_builtin(ctx, name),
        }
    }

    /// Runs `f` with the engine and the app as its host. The engine is taken out of the app
    /// while it runs, so a script cannot reach it re-entrantly.
    pub(super) fn with_engine(
        &mut self,
        ctx: &egui::Context,
        f: impl FnOnce(&Engine, &mut dyn Host) -> Result<(), String>,
    ) -> Result<(), String> {
        let Some(engine) = self.scripting.engine.take() else {
            return Err("no Lua engine is loaded".into());
        };
        let result = self.with_host(ctx, |host| f(&engine, host));
        self.scripting.engine = Some(engine);
        result
    }

    /// Calls `f` with the [`Host`] that Lua scripts drive, for callers in Rust such as the MCP
    /// server (`crate::mcp`). Rebuilds afterwards if the part changed.
    pub fn with_host<R>(
        &mut self,
        ctx: &egui::Context,
        f: impl FnOnce(&mut dyn Host) -> Result<R, String>,
    ) -> Result<R, String> {
        let before = self.doc.clone();
        self.scripting.script_status = None;
        let result = f(&mut AppHost { app: self, ctx });
        if self.doc != before {
            self.rebuild();
            self.scene_dirty = true;
        }
        // What the script said with anvil.status outlasts the rebuild's own message.
        if let Some(s) = self.scripting.script_status.take() {
            self.status = s;
        }
        result
    }

    /// The feature tree with each feature's settable dimensions, for agents (`crate::mcp`).
    pub fn feature_list(&self) -> serde_json::Value {
        use serde_json::json;
        let features: Vec<_> = self
            .doc
            .features
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let dims: serde_json::Map<String, serde_json::Value> = DimSlot::of(&f.kind)
                    .into_iter()
                    .filter_map(|slot| {
                        let d = slot.get(&f.kind)?;
                        let value = d.eval(&self.state.variables).ok();
                        let expr = match d {
                            Dim::Expr(e) => Some(e.clone()),
                            Dim::Value(_) => None,
                        };
                        Some((slot.label().to_string(), json!({ "value": value, "equation": expr })))
                    })
                    .collect();
                json!({
                    "id": f.id,
                    "name": f.name,
                    "kind": kind_name(&f.kind),
                    "suppressed": f.suppressed,
                    "rolled_back": self.doc.rollback.is_some_and(|r| i >= r),
                    "error": self.state.errors.get(&f.id).map(|e| e.message()),
                    "dimensions": dims,
                })
            })
            .collect();
        json!({
            "features": features,
            "variables": self.state.variables,
            "variable_errors": self.state.variable_errors,
        })
    }

    /// Closes the message dialog, if one is open, and returns its title and text, so an
    /// agent can read it without a dialog left over its next screenshot.
    pub fn take_message(&mut self) -> Option<(String, String)> {
        self.message.take()
    }

    /// The body's mass properties, in millimetres and grams, for agents (`crate::mcp`).
    pub fn mass_properties(&self) -> Result<serde_json::Value, String> {
        use serde_json::json;
        if self.state.body.is_empty() {
            return Err("there is no solid body yet; add a sketch and an extrusion".into());
        }
        let m = self.state.mass_report(&self.doc);
        let b = self.state.body.bounds();
        let material = self.doc.material.as_ref().map(|m| m.resolved());
        let c = m.props.centroid;
        Ok(json!({
            "units": { "length": "mm", "volume": "mm^3", "area": "mm^2", "mass": "g" },
            "volume": m.props.volume,
            "surface_area": m.props.surface_area,
            "centroid": [c.x, c.y, c.z],
            "bounds": { "min": [b.min.x, b.min.y, b.min.z], "max": [b.max.x, b.max.y, b.max.z] },
            "size": [b.max.x - b.min.x, b.max.y - b.min.y, b.max.z - b.min.z],
            "material": material.as_ref().map(|m| m.name.clone()),
            "density_g_per_cm3": material.as_ref().map(|m| m.density),
            "mass": m.mass,
            "principal_moments_g_mm2": m.principal,
            "open_edges": self.state.body.open_edges(),
            "exact": self.state.exact.is_some(),
        }))
    }

    /// Runs Lua like [`AnvilApp::run_lua`] and returns the chunk's return value (as JSON) and
    /// the lines it printed, for agents (`crate::mcp`).
    pub fn eval_lua(
        &mut self,
        ctx: &egui::Context,
        source: &str,
        name: &str,
    ) -> Result<(serde_json::Value, Vec<String>), String> {
        if self.scripting.engine.is_none() {
            self.scripting.engine = Some(Engine::load("", "init.lua", None)?);
        }
        let mut out = (serde_json::Value::Null, Vec::new());
        let result = self.with_engine(ctx, |engine, host| {
            out = engine.eval_script(source, name, host)?;
            Ok(())
        });
        if let Err(e) = &result {
            self.status = format!("Script error: {e}");
        }
        result.map(|()| out)
    }

    /// Runs a Lua script with the whole `anvil` API. Without a loaded config it runs in a
    /// fresh engine.
    pub fn run_lua(&mut self, ctx: &egui::Context, source: &str, name: &str) -> Result<(), String> {
        if self.scripting.engine.is_none() {
            self.scripting.engine = Some(Engine::load("", "init.lua", None)?);
        }
        let result = self.with_engine(ctx, |engine, host| engine.run_script(source, name, host));
        if let Err(e) = &result {
            self.status = format!("Script error: {e}");
        }
        result
    }

    fn run_script_dialog(&mut self, ctx: &egui::Context) {
        let Some(path) = self.file_dialog("Run Script", || {
            rfd::FileDialog::new().add_filter("Lua script", &["lua"]).pick_file()
        }) else {
            return;
        };
        match std::fs::read_to_string(&path) {
            Ok(src) => {
                if self.run_lua(ctx, &src, &path.display().to_string()).is_ok() {
                    self.status = format!("Ran {}", path.display());
                }
            }
            Err(e) => {
                self.message = Some(("Run Script".into(), format!("Could not read {}: {e}", path.display())))
            }
        }
    }

    /// Reads `init.lua` from `path` and applies it; `None` uses the platform's default path.
    /// At startup, applies the options that only make sense then (what to open, the view).
    pub fn load_config(&mut self, ctx: &egui::Context, path: Option<PathBuf>) {
        let path = path.or_else(script::default_config_path);
        self.scripting.path = path.clone();
        self.scripting.modified = path.as_deref().and_then(modified);
        match path {
            Some(p) => {
                let result = Engine::load_file(&p);
                self.install_config(ctx, result, &p.display().to_string());
            }
            None => self.apply_options(ctx),
        }
    }

    /// Applies a config from source text (the headless harness and tests).
    pub fn load_config_source(&mut self, ctx: &egui::Context, source: &str) -> Result<(), String> {
        let result = Engine::load(source, "init.lua", None);
        let err = result.as_ref().err().cloned();
        self.install_config(ctx, result, "init.lua");
        err.map_or(Ok(()), Err)
    }

    fn install_config(&mut self, ctx: &egui::Context, result: Result<Engine, String>, name: &str) {
        match result {
            Ok(engine) => {
                let n = engine.config.bindings.len() + engine.config.commands.len();
                self.scripting.options = engine.config.options.clone();
                self.scripting.engine = Some(engine);
                self.apply_options(ctx);
                if n > 0 || self.scripting.started {
                    self.status = format!("Loaded {name}");
                }
            }
            // Keep the previous config, so a typo in a live edit does not undo everything.
            Err(e) => {
                self.status = format!("Config error: {e}");
                self.message =
                    Some(("init.lua".into(), format!("{e}\n\nThe previous settings are still in use.")));
                if !self.scripting.started {
                    self.apply_options(ctx);
                }
            }
        }
    }

    fn reload_config(&mut self, ctx: &egui::Context) {
        let path = self.scripting.path.clone();
        if path.is_none() && script::default_config_path().is_none() {
            self.status = "No config file: set HOME or ANVILCAD_CONFIG".into();
            return;
        }
        self.load_config(ctx, path);
    }

    fn edit_config(&mut self) {
        let Some(path) = self.scripting.path.clone().or_else(script::default_config_path) else {
            self.message =
                Some(("Edit Config".into(), "No config folder: set HOME or ANVILCAD_CONFIG.".into()));
            return;
        };
        if !path.exists() {
            let created = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(&path, STARTER_CONFIG));
            if let Err(e) = created {
                self.message =
                    Some(("Edit Config".into(), format!("Could not create {}: {e}", path.display())));
                return;
            }
        }
        self.scripting.path = Some(path.clone());
        self.message = Some((
            "Edit Config".into(),
            format!(
                "Your config is {}\n\nEdit it in any editor; AnvilCAD reloads it when you save. \
                 See docs/SCRIPTING.md for everything it can set.",
                path.display()
            ),
        ));
    }

    /// Applies the loaded options, theme, fonts and colours.
    fn apply_options(&mut self, ctx: &egui::Context) {
        let config = self.scripting.engine.as_ref().map(|e| e.config.clone()).unwrap_or_default();
        let o = &config.options;
        theme::set_overrides(ctx, config.theme.clone());
        if let Some(dark) = o.dark {
            self.dark = dark;
        }
        theme::apply(ctx, self.dark);
        if let Some(s) = o.ui_scale {
            ctx.set_zoom_factor(s);
        }
        if let Err(e) = apply_fonts(ctx, o.font.as_deref(), o.mono_font.as_deref()) {
            self.status = format!("Config error: {e}");
        }
        if let Some(e) = o.edges {
            self.show_edges = e;
        }
        self.sync_3d_mouse(ctx);
        self.autosave.every = match o.autosave {
            Some(0.0) => None,
            Some(m) => Some(std::time::Duration::from_secs_f64(m * 60.0)),
            None => Some(crate::recovery::DEFAULT_INTERVAL),
        };
        let px = o.snap_px.unwrap_or(sketcher::PICK_PX);
        if let Some(s) = &mut self.sketch {
            s.pick_px = px;
        }
        self.scene_dirty = true;

        if !self.scripting.started {
            self.scripting.started = true;
            if let Some(v) = &o.view {
                if let Some(v) = StdView::ALL.into_iter().find(|s| s.label().eq_ignore_ascii_case(v)) {
                    self.camera.set_view(v);
                    self.fit_pending = true;
                }
            }
        }
    }

    /// The snap radius for new sketches.
    pub(super) fn snap_px(&self) -> f64 {
        self.scripting.options.snap_px.unwrap_or(sketcher::PICK_PX)
    }

    /// Starts a new part from the template and material options, then reports any problem
    /// with them (after loading, so the rebuild's status does not hide it).
    pub(super) fn start_new_part(&mut self) {
        let doc = self.new_part_document();
        let warning = std::mem::take(&mut self.scripting.new_part_warning);
        self.load_document(doc, None);
        if let Some(w) = warning {
            self.status = w;
        }
    }

    /// A new part: from the configured template if any, with the configured material.
    fn new_part_document(&mut self) -> Document {
        let o = self.scripting.options.clone();
        let mut doc = match &o.template {
            Some(t) => match Document::load(t) {
                Ok(d) => Document::from_template(&d),
                Err(e) => {
                    self.scripting.new_part_warning =
                        Some(format!("anvil.opt.template: {} could not be read: {e}", t.display()));
                    Document::new()
                }
            },
            None => Document::new(),
        };
        if let Some(m) = &o.material {
            if doc.material.is_none() {
                doc.material = Material::library().into_iter().find(|x| &x.name == m);
                if doc.material.is_none() {
                    self.scripting.new_part_warning =
                        Some(format!("anvil.opt.material: no library material is called \"{m}\""));
                }
            }
        }
        doc
    }

    /// What to show at launch when no file was given: `anvil.opt.open`, or a new part from the
    /// template and material options.
    pub fn open_startup(&mut self) {
        let open = self.scripting.options.open.clone();
        match open.as_deref() {
            Some("sample") => self.load_document(crate::sample::sample_part(), None),
            Some(p) => self.open_path(Path::new(p)),
            None => self.start_new_part(),
        }
    }

    /// Reloads `init.lua` when it changes on disk (checked about once a second).
    pub(super) fn poll_config(&mut self, ctx: &egui::Context) {
        let Some(path) = self.scripting.path.clone() else { return };
        let now = ctx.input(|i| i.time);
        if now - self.scripting.checked < 1.0 {
            return;
        }
        self.scripting.checked = now;
        let m = modified(&path);
        if m != self.scripting.modified {
            self.scripting.modified = m;
            let result = Engine::load_file(&path);
            self.install_config(ctx, result, &path.display().to_string());
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }

    /// Handles the user's key bindings before the built-in shortcuts. Keys that complete or
    /// continue a binding are taken from egui's input, so nothing else sees them.
    pub(super) fn user_keys(&mut self, ctx: &egui::Context) {
        let Some(engine) = &self.scripting.engine else { return };
        if engine.config.bindings.is_empty() || ctx.wants_keyboard_input() {
            return;
        }
        let sequences: Vec<Vec<crate::keymap::Chord>> =
            engine.config.bindings.iter().map(|b| b.sequence.clone()).collect();
        let presses: Vec<(Key, Modifiers)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } => {
                        Some((*key, *modifiers))
                    }
                    _ => None,
                })
                .collect()
        });
        let now = ctx.input(|i| i.time);
        let mut fired = Vec::new();
        for (key, mods) in presses {
            match self.scripting.matcher.press(&sequences, key, mods, now) {
                Step::NoMatch => continue,
                Step::Pending => {}
                Step::Fire(i) => fired.push(i),
            }
            ctx.input_mut(|i| i.consume_key(mods, key));
        }
        for i in fired {
            let Some(binding) = self.scripting.engine.as_ref().map(|e| e.config.bindings[i].clone()) else {
                return;
            };
            let result = match &binding.action {
                Action::Builtin(name) => self.run_command(ctx, name),
                Action::Lua(index) => self.with_engine(ctx, |engine, host| engine.run_binding(*index, host)),
            };
            if let Err(e) = result {
                self.status = format!("{}: {e}", binding.keys);
            }
        }
    }

    /// The command palette window.
    pub(super) fn command_palette(&mut self, ctx: &egui::Context) {
        let Some(mut pal) = self.scripting.palette.take() else { return };
        let items = palette_items(self.scripting.engine.as_ref(), &pal.query);
        let shortcut = |name: &str| -> Option<String> {
            let e = self.scripting.engine.as_ref()?;
            e.config
                .bindings
                .iter()
                .find(|b| matches!(&b.action, Action::Builtin(n) if n.eq_ignore_ascii_case(name)))
                .map(|b| b.keys.clone())
        };
        let mut run = None;
        let mut open = true;
        let (up, down, enter, esc) = ctx.input(|i| {
            (
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::Enter),
                i.key_pressed(Key::Escape),
            )
        });
        if down && !items.is_empty() {
            pal.selected = (pal.selected + 1).min(items.len() - 1);
        }
        if up {
            pal.selected = pal.selected.saturating_sub(1);
        }
        pal.selected = pal.selected.min(items.len().saturating_sub(1));
        egui::Window::new("Command Palette")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 80.0))
            .fixed_size(egui::vec2(460.0, 360.0))
            .open(&mut open)
            .show(ctx, |ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut pal.query)
                        .hint_text("Type to find a command")
                        .desired_width(f32::INFINITY),
                );
                if pal.focus {
                    field.request_focus();
                    pal.focus = false;
                }
                if field.changed() {
                    pal.selected = 0;
                }
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    let p = Palette::of(ui);
                    for (i, (name, desc, user)) in items.iter().enumerate() {
                        let label = match shortcut(name) {
                            Some(k) => format!("{name}    {k}"),
                            None => name.clone(),
                        };
                        let r = ui.selectable_label(i == pal.selected, RichText::new(label).strong());
                        let r = r.on_hover_text(desc.as_str());
                        if *user {
                            ui.label(
                                RichText::new(format!("  {desc} (yours)")).size(11.5).color(p.text_weak),
                            );
                        }
                        if i == pal.selected && (up || down) {
                            r.scroll_to_me(None);
                        }
                        if r.clicked() {
                            run = Some(name.clone());
                        }
                    }
                    if items.is_empty() {
                        ui.label(RichText::new("No command matches").color(p.text_weak));
                    }
                });
            });
        if enter {
            run = run.or_else(|| items.get(pal.selected).map(|(n, _, _)| n.clone()));
        }
        if let Some(name) = run {
            if let Err(e) = self.run_command(ctx, &name) {
                self.status = format!("{name}: {e}");
            }
            return;
        }
        if open && !esc && self.scripting.palette.is_none() {
            self.scripting.palette = Some(pal);
        }
    }
}

fn modified(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Loads font files ahead of egui's own, or restores the defaults when none are set.
fn apply_fonts(ctx: &egui::Context, font: Option<&Path>, mono: Option<&Path>) -> Result<(), String> {
    let mut defs = egui::FontDefinitions::default();
    for (path, family, name) in [
        (font, egui::FontFamily::Proportional, "user-font"),
        (mono, egui::FontFamily::Monospace, "user-mono-font"),
    ] {
        let Some(path) = path else { continue };
        let bytes = std::fs::read(path).map_err(|e| format!("font {}: {e}", path.display()))?;
        defs.font_data.insert(name.into(), Arc::new(egui::FontData::from_owned(bytes)));
        defs.families.entry(family).or_default().insert(0, name.into());
    }
    ctx.set_fonts(defs);
    Ok(())
}

/// The app as a script's [`Host`].
struct AppHost<'a> {
    app: &'a mut AnvilApp,
    ctx: &'a egui::Context,
}

impl AppHost<'_> {
    /// A feature by its id or (case-insensitive) name.
    fn feature_named(&self, what: &str, feature: &str) -> Result<&Feature, String> {
        let doc = &self.app.doc;
        feature
            .parse::<u32>()
            .ok()
            .and_then(|id| doc.feature(id))
            .or_else(|| doc.features.iter().find(|f| f.name.eq_ignore_ascii_case(feature)))
            .ok_or_else(|| format!("{what}: there is no feature \"{feature}\""))
    }

    /// Scripts never throw away unsaved work unless they say so.
    fn check_unsaved(&self, what: &str, force: bool) -> Result<(), String> {
        if self.app.is_dirty() && !force {
            return Err(format!(
                "{what}: the part has unsaved changes; save it first, or pass {{ force = true }} to discard them"
            ));
        }
        Ok(())
    }
}

impl Host for AppHost<'_> {
    fn run_builtin(&mut self, name: &str) -> Result<(), String> {
        self.app.run_builtin(self.ctx, name)
    }

    fn state(&self) -> serde_json::Value {
        self.app.snapshot()
    }

    fn new_part(&mut self, force: bool) -> Result<(), String> {
        self.check_unsaved("anvil.new", force)?;
        self.app.start_new_part();
        Ok(())
    }

    fn open(&mut self, path: &Path, force: bool) -> Result<(), String> {
        if !path.exists() {
            return Err(format!("{} does not exist", path.display()));
        }
        self.check_unsaved("anvil.open", force)?;
        self.app.open_path(path);
        Ok(())
    }

    fn save(&mut self, path: Option<&Path>) -> Result<(), String> {
        let path = path.map(Path::to_path_buf).or_else(|| self.app.path.clone()).ok_or_else(|| {
            "anvil.save: the part has no file yet; give a path, anvil.save(\"part.anvprt\")".to_string()
        })?;
        self.app.doc.save(&path).map_err(|e| e.to_string())?;
        self.app.path = Some(path.clone());
        self.app.saved = self.app.doc.clone();
        self.app.status = format!("Saved {}", path.display());
        Ok(())
    }

    fn export(&mut self, path: &Path) -> Result<(), String> {
        self.app.rebuild();
        if self.app.state.body.is_empty() {
            return Err("there is no solid body to export".into());
        }
        self.app.write_export(path).map(|_| ()).map_err(|e| format!("export to {}: {e}", path.display()))
    }

    fn set_variable(&mut self, name: &str, expr: &str) -> Result<(), String> {
        if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(format!("anvil.var: \"{name}\" is not a variable name (letters, digits, _)"));
        }
        match self.app.doc.variables.iter_mut().find(|v| v.name == name) {
            Some(v) => v.expr = expr.to_string(),
            None => self.app.doc.variables.push(GlobalVariable {
                name: name.to_string(),
                expr: expr.to_string(),
                comment: String::new(),
            }),
        }
        let (_, errors) = self.app.doc.evaluate_variables();
        match errors.get(name) {
            Some(e) => Err(format!("anvil.var(\"{name}\", \"{expr}\"): {e}")),
            None => Ok(()),
        }
    }

    fn delete_feature(&mut self, feature: &str) -> Result<Vec<u32>, String> {
        let id = self.feature_named("anvil.delete", feature)?.id;
        let gone = self.app.doc.delete_feature(id);
        self.app.selection = Selection::None;
        self.app.rebuild();
        Ok(gone)
    }

    fn set_dimension(&mut self, feature: &str, dimension: &str, value: &str) -> Result<(), String> {
        let f = self.feature_named("anvil.dim", feature)?;
        let (id, name) = (f.id, f.name.clone());
        let slots = DimSlot::of(&f.kind);
        let wanted = dimension.trim().to_ascii_lowercase().replace('_', " ");
        let Some(slot) = slots.iter().copied().find(|s| s.label() == wanted) else {
            let have: Vec<_> = slots.iter().map(|s| s.label()).collect();
            return Err(match have.is_empty() {
                true => format!("anvil.dim: {name} has no dimension that can be set"),
                false => format!("anvil.dim: {name} has no \"{dimension}\"; it has {}", have.join(", ")),
            });
        };
        let value = value.trim();
        let dim = match value.parse::<f64>() {
            Ok(v) if v.is_finite() => Dim::Value(v),
            Ok(_) => return Err(format!("anvil.dim: {value} is not a finite number")),
            Err(_) if value.is_empty() => return Err("anvil.dim: the value is empty".into()),
            Err(_) => Dim::Expr(value.to_string()),
        };
        let old = self.app.doc.feature(id).and_then(|f| slot.get(&f.kind)).cloned();
        if let Some(d) = self.app.doc.feature_mut(id).and_then(|f| slot.get_mut(&mut f.kind)) {
            *d = dim;
        }
        self.app.rebuild();
        if let Some(e) = self.app.state.errors.get(&id).cloned() {
            // Put the old value back, so one bad value does not leave the part broken.
            if let (Some(old), Some(d)) =
                (old, self.app.doc.feature_mut(id).and_then(|f| slot.get_mut(&mut f.kind)))
            {
                *d = old;
            }
            self.app.rebuild();
            return Err(format!("anvil.dim: {name} {}: {e}", slot.label()));
        }
        Ok(())
    }

    fn set_material(&mut self, name: &str) -> Result<(), String> {
        let m = Material::library().into_iter().find(|m| m.name.eq_ignore_ascii_case(name));
        let m = m.ok_or_else(|| format!("anvil.material: no library material is called \"{name}\""))?;
        self.app.doc.material = Some(m);
        Ok(())
    }

    fn add_sketch(&mut self, spec: &SketchSpec) -> Result<u32, String> {
        let plane = match spec.plane {
            PlaneSpec::Front => PlaneRef::Front,
            PlaneSpec::Top => PlaneRef::Top,
            PlaneSpec::Right => PlaneRef::Right,
            PlaneSpec::Face { feature, start } => match self.app.doc.feature(feature).map(|f| &f.kind) {
                None => return Err(format!("anvil.sketch: there is no feature {feature}")),
                // A reference plane: sketch on the plane itself.
                Some(FeatureKind::Plane(_)) => PlaneRef::Feature(feature),
                Some(_) => {
                    let index = if start { FaceId::START_CAP } else { FaceId::END_CAP };
                    PlaneRef::Face(FaceId { feature, index })
                }
            },
        };
        let mut sf = SketchFeature::new(plane);
        for r in &spec.rects {
            if (r[0] - r[2]).abs() < 1e-9 || (r[1] - r[3]).abs() < 1e-9 {
                return Err(format!(
                    "anvil.sketch: rectangle {{{}, {}, {}, {}}} has no area",
                    r[0], r[1], r[2], r[3]
                ));
            }
            sf.sketch.add_rectangle(anvil_kernel::v2(r[0], r[1]), anvil_kernel::v2(r[2], r[3]));
        }
        for c in &spec.circles {
            if c[2].is_nan() || c[2] <= 0.0 {
                return Err(format!("anvil.sketch: circle radius {} is not positive", c[2]));
            }
            let p = sf.sketch.add_point(anvil_kernel::v2(c[0], c[1]));
            sf.sketch.add_circle(p, c[2]);
        }
        for line in &spec.polylines {
            if line.len() < 2 {
                return Err("anvil.sketch: a polyline needs at least two points".into());
            }
            for w in line.windows(2) {
                let (a, b) = (anvil_kernel::v2(w[0][0], w[0][1]), anvil_kernel::v2(w[1][0], w[1][1]));
                if a.distance(b) < 1e-9 {
                    return Err(format!(
                        "anvil.sketch: a polyline repeats the point {{{}, {}}}",
                        w[0][0], w[0][1]
                    ));
                }
                let (pa, pb) = (sf.sketch.add_point(a), sf.sketch.add_point(b));
                sf.sketch.add_line(pa, pb);
            }
        }
        for a in &spec.arcs {
            let v = |i: usize| anvil_kernel::v2(a[i], a[i + 1]);
            sf.sketch.add_3pt_arc(v(0), v(4), v(2)).map_err(|_| {
                format!(
                    "anvil.sketch: the arc {{{}, {}, {}, {}, {}, {}}} has its three points in a line or two of them the same",
                    a[0], a[1], a[2], a[3], a[4], a[5]
                )
            })?;
        }
        join_coincident_ends(&mut sf.sketch);
        // Free points go in after the joining, so a point that lands on a line's end stays free.
        for p in &spec.points {
            sf.sketch.add_point(anvil_kernel::v2(p[0], p[1]));
        }
        let before = self.app.doc.clone();
        let id = self.app.doc.add_feature(FeatureKind::Sketch(sf));
        // Rebuild now, so a face that cannot carry a sketch fails here rather than at the
        // extrusion that uses it.
        self.app.rebuild();
        if let Some(e) = self.app.state.errors.get(&id).cloned() {
            self.app.doc = before;
            self.app.rebuild();
            let hint = match spec.plane {
                PlaneSpec::Face { feature, start } => format!(
                    " (feature {feature} has no planar {} face in the body; sketch on a boss's \
                     face or a standard plane instead)",
                    if start { "start" } else { "end" }
                ),
                _ => String::new(),
            };
            return Err(format!("anvil.sketch: {e}{hint}"));
        }
        Ok(id)
    }

    fn add_extrude(&mut self, spec: &ExtrudeSpec) -> Result<u32, String> {
        if self.app.doc.sketch(spec.sketch).is_none() {
            return Err(format!("anvil.extrude: feature {} is not a sketch", spec.sketch));
        }
        let (end, depth) = match &spec.depth {
            Depth::Value(v) if *v > 0.0 => (EndCondition::Blind, Dim::Value(*v)),
            Depth::Value(v) => return Err(format!("anvil.extrude: depth {v} is not positive")),
            Depth::Expr(e) => (EndCondition::Blind, Dim::Expr(e.clone())),
            Depth::ThroughAll => (EndCondition::ThroughAll, Dim::Value(0.0)),
        };
        let before = self.app.doc.clone();
        let id = self.app.doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: spec.sketch,
            end,
            depth,
            reverse: spec.reverse,
            op: if spec.cut { BodyOp::Cut } else { BodyOp::Boss },
            merge: !spec.separate,
            ..Default::default()
        }));
        // Rebuild now, so a later sketch can sit on this feature's faces and errors show here.
        self.app.rebuild();
        if let Some(e) = self.app.state.errors.get(&id).cloned() {
            // A failed call adds nothing, so a retry does not leave a broken twin behind.
            self.app.doc = before;
            self.app.rebuild();
            return Err(format!("anvil.extrude: {e}"));
        }
        Ok(id)
    }

    fn add_revolve(&mut self, spec: &RevolveSpec) -> Result<u32, String> {
        let what = "anvil.revolve";
        let sk = &self
            .app
            .doc
            .sketch(spec.sketch)
            .ok_or_else(|| format!("{what}: feature {} is not a sketch", spec.sketch))?
            .sketch;
        let (a, b) =
            (anvil_kernel::v2(spec.axis[0], spec.axis[1]), anvil_kernel::v2(spec.axis[2], spec.axis[3]));
        if a.distance(b) < 1e-9 {
            return Err(format!(
                "{what}: the axis {{{}, {}, {}, {}}} has no length",
                spec.axis[0], spec.axis[1], spec.axis[2], spec.axis[3]
            ));
        }
        // A line of the sketch along the axis, either way round; else a new centerline.
        let near = |p: anvil_sketch::PointId, q: anvil_kernel::Vec2| sk.pos(p).distance(q) < 1e-6;
        let found = sk.entities.iter().position(|e| match e.geom {
            anvil_sketch::Geom::Line { a: pa, b: pb } => {
                (near(pa, a) && near(pb, b)) || (near(pa, b) && near(pb, a))
            }
            _ => false,
        });
        let before = self.app.doc.clone();
        let axis = match found {
            Some(e) => e,
            None => {
                let sk = &mut self
                    .app
                    .doc
                    .sketch_mut(spec.sketch)
                    .ok_or_else(|| format!("{what}: no sketch {}", spec.sketch))?
                    .sketch;
                let (pa, pb) = (sk.add_point(a), sk.add_point(b));
                let e = sk.add_line(pa, pb);
                sk.entities[e].construction = true;
                e
            }
        };
        let angle = match &spec.angle {
            Depth::Value(v) => Dim::Value(*v),
            Depth::Expr(e) => Dim::Expr(e.clone()),
            Depth::ThroughAll => return Err(format!("{what}: angle is a number of degrees or an equation")),
        };
        let kind = FeatureKind::Revolve(RevolveFeature {
            sketch: spec.sketch,
            axis,
            angle,
            op: if spec.cut { BodyOp::Cut } else { BodyOp::Boss },
            kind: RevolveKind::OneDirection,
            reverse: false,
            thin: None,
            flip_side: false,
        });
        self.commit_feature(what, before, kind)
    }

    fn add_blend(&mut self, spec: &BlendSpec) -> Result<u32, String> {
        let what = if spec.chamfer { "anvil.chamfer" } else { "anvil.fillet" };
        let edges = spec.edges.iter().map(|p| self.edge_at(*p, what)).collect::<Result<Vec<_>, _>>()?;
        let size = match &spec.size {
            Depth::Value(v) => Dim::Value(*v),
            Depth::Expr(e) => Dim::Expr(e.clone()),
            Depth::ThroughAll => return Err(format!("{what}: the size is a number or an equation")),
        };
        let kind = if spec.chamfer {
            FeatureKind::Chamfer(ChamferFeature {
                edges,
                distance: size,
                kind: ChamferKind::EqualDistance,
                tangent_propagation: false,
            })
        } else {
            FeatureKind::Fillet(FilletFeature {
                edges,
                radius: size,
                profile: FilletProfile::Circular,
                tangent_propagation: false,
            })
        };
        let before = self.app.doc.clone();
        self.commit_feature(what, before, kind)
    }

    fn add_shell(&mut self, spec: &ShellSpec) -> Result<u32, String> {
        let what = "anvil.shell";
        let remove = spec.remove.iter().map(|p| self.face_at(*p, what)).collect::<Result<Vec<_>, _>>()?;
        let thickness = match &spec.thickness {
            Depth::Value(v) => Dim::Value(*v),
            Depth::Expr(e) => Dim::Expr(e.clone()),
            Depth::ThroughAll => return Err(format!("{what}: thickness is a number or an equation")),
        };
        let before = self.app.doc.clone();
        self.commit_feature(what, before, FeatureKind::Shell(ShellFeature { remove, thickness }))
    }

    fn add_mirror(&mut self, spec: &MirrorSpec) -> Result<u32, String> {
        let what = "anvil.mirror";
        let features = self.feature_ids(what, &spec.features)?;
        let plane = self.plane_of(&spec.plane, what)?;
        let before = self.app.doc.clone();
        let kind = FeatureKind::Mirror(MirrorFeature {
            plane,
            features,
            bodies: false,
            plane2: None,
            seed_only: false,
        });
        self.commit_feature(what, before, kind)
    }

    fn add_pattern(&mut self, spec: &PatternSpec) -> Result<u32, String> {
        let what = "anvil.pattern";
        let features = self.feature_ids(what, &spec.features)?;
        let dim = |d: &Depth| dim_of(d, what);
        let second = match &spec.second {
            Some(r) => Some(PatternDirection {
                direction: axis_of(&r.direction, [0.0; 3], what)?,
                spacing: dim(&r.spacing)?,
                count: dim(&r.count)?,
                reverse: false,
            }),
            None => None,
        };
        let kind = FeatureKind::LinearPattern(LinearPatternFeature {
            features,
            direction: axis_of(&spec.first.direction, [0.0; 3], what)?,
            spacing: dim(&spec.first.spacing)?,
            count: dim(&spec.first.count)?,
            direction2: second,
            ..Default::default()
        });
        let before = self.app.doc.clone();
        self.commit_feature(what, before, kind)
    }

    fn add_circular_pattern(&mut self, spec: &CircularSpec) -> Result<u32, String> {
        let what = "anvil.circular_pattern";
        let features = self.feature_ids(what, &spec.features)?;
        let kind = FeatureKind::CircularPattern(CircularPatternFeature {
            features,
            axis: axis_of(&spec.axis, spec.through, what)?,
            total_angle: dim_of(&spec.angle, what)?,
            count: dim_of(&spec.count, what)?,
            equal_spacing: true,
            ..Default::default()
        });
        let before = self.app.doc.clone();
        self.commit_feature(what, before, kind)
    }

    fn add_hole(&mut self, spec: &HoleSpec) -> Result<u32, String> {
        let what = "anvil.hole";
        let Some(sf) = self.app.doc.sketch(spec.sketch) else {
            return Err(format!("{what}: feature {} is not a sketch", spec.sketch));
        };
        if anvil_model::hole::positions(&sf.sketch).is_empty() {
            return Err(format!(
                "{what}: sketch {} has no free points to place holes at; add them with points = {{{{x, y}}, ...}}",
                spec.sketch
            ));
        }
        let (end, depth) = match &spec.depth {
            Depth::ThroughAll => (anvil_model::HoleEnd::ThroughAll, Dim::Value(0.0)),
            d => (anvil_model::HoleEnd::Blind, dim_of(d, what)?),
        };
        let kind = match (&spec.counterbore, &spec.countersink) {
            (Some((d, h)), _) => {
                anvil_model::HoleKind::Counterbore { diameter: dim_of(d, what)?, depth: dim_of(h, what)? }
            }
            (None, Some((d, a))) => anvil_model::HoleKind::Countersink(anvil_model::Sink {
                diameter: dim_of(d, what)?,
                angle: dim_of(a, what)?,
            }),
            (None, None) => anvil_model::HoleKind::Simple,
        };
        let hole = anvil_model::HoleFeature {
            sketch: spec.sketch,
            kind,
            diameter: dim_of(&spec.diameter, what)?,
            end,
            depth,
            drill_angle: None,
            near_sink: None,
            reverse: false,
            size: None,
        };
        let before = self.app.doc.clone();
        self.commit_feature(what, before, FeatureKind::Hole(hole))
    }

    fn add_draft(&mut self, spec: &DraftSpec) -> Result<u32, String> {
        let what = "anvil.draft";
        let neutral = self.plane_of(&spec.neutral, what)?;
        let faces = spec.faces.iter().map(|p| self.face_at(*p, what)).collect::<Result<Vec<_>, _>>()?;
        let kind = FeatureKind::Draft(anvil_model::DraftFeature {
            neutral,
            faces,
            angle: dim_of(&spec.angle, what)?,
            reverse: false,
            propagation: anvil_model::Propagation::None,
        });
        let before = self.app.doc.clone();
        self.commit_feature(what, before, kind)
    }

    fn add_rib(&mut self, spec: &RibSpec) -> Result<u32, String> {
        let what = "anvil.rib";
        if self.app.doc.sketch(spec.sketch).is_none() {
            return Err(format!("{what}: feature {} is not a sketch", spec.sketch));
        }
        let kind = FeatureKind::Rib(anvil_model::rib::RibFeature {
            sketch: spec.sketch,
            thickness: dim_of(&spec.thickness, what)?,
            sides: anvil_model::rib::RibSides::Both,
            normal_to_sketch: false,
            flip: spec.flip,
            natural: false,
        });
        let before = self.app.doc.clone();
        self.commit_feature(what, before, kind)
    }

    fn add_combine(&mut self, spec: &CombineSpec) -> Result<u32, String> {
        let what = "anvil.combine";
        let bodies = self.body_refs(what, &spec.bodies)?;
        let main = match &spec.main {
            Some(m) => Some(self.body_refs(what, std::slice::from_ref(m))?[0]),
            None => None,
        };
        let op = match spec.how {
            CombineHow::Add => anvil_model::CombineOp::Add,
            CombineHow::Subtract => anvil_model::CombineOp::Subtract,
            CombineHow::Common => anvil_model::CombineOp::Common,
        };
        if op == anvil_model::CombineOp::Subtract && main.is_none() {
            return Err(format!("{what}: a subtraction needs main = <the body to subtract from>"));
        }
        let kind = FeatureKind::Combine(anvil_model::CombineFeature { op, main, bodies });
        let before = self.app.doc.clone();
        self.commit_feature(what, before, kind)
    }

    fn add_split(&mut self, spec: &SplitSpec) -> Result<u32, String> {
        let what = "anvil.split";
        let planes = spec.planes.iter().map(|p| self.plane_of(p, what)).collect::<Result<Vec<_>, _>>()?;
        let bodies = self.body_refs(what, &spec.bodies)?;
        let before = self.app.doc.clone();
        self.commit_feature(what, before, FeatureKind::Split(anvil_model::SplitFeature { planes, bodies }))
    }

    fn new_assembly(&mut self, force: bool) -> Result<(), String> {
        if !force && self.app.is_dirty() {
            return Err(
                "anvil.assembly: there are unsaved changes; save first, or pass { force = true }".into()
            );
        }
        self.app.new_assembly();
        Ok(())
    }

    fn insert_component(&mut self, spec: &InsertSpec) -> Result<u32, String> {
        let what = "anvil.insert";
        if !self.app.in_assembly() {
            return Err(format!("{what}: no assembly is open; start one with anvil.assembly()"));
        }
        let at = spec.at.map(|a| anvil_kernel::Vec3 { x: a[0], y: a[1], z: a[2] });
        self.app.insert_part_at(&spec.path, at).map_err(|e| format!("{what}: {e}"))
    }

    fn add_mate(&mut self, spec: &MateSpec) -> Result<u32, String> {
        let what = "anvil.mate";
        if !self.app.in_assembly() {
            return Err(format!("{what}: no assembly is open; start one with anvil.assembly()"));
        }
        let value =
            || spec.value.ok_or_else(|| format!("{what}: a {} mate needs value = <number>", spec.kind));
        let kind = match spec.kind.to_ascii_lowercase().as_str() {
            "coincident" => MateKind::Coincident,
            "concentric" => MateKind::Concentric,
            "parallel" => MateKind::Parallel,
            "perpendicular" => MateKind::Perpendicular,
            "tangent" => MateKind::Tangent,
            "distance" => MateKind::Distance { value: value()?, flip: spec.flip },
            "angle" => MateKind::Angle { degrees: value()? },
            other => {
                return Err(format!(
                    "{what}: kind \"{other}\" is not coincident, concentric, parallel, perpendicular, tangent, distance or angle"
                ))
            }
        };
        let alignment = match spec.align.as_deref().map(str::to_ascii_lowercase).as_deref() {
            None | Some("closest") => Alignment::Closest,
            Some("aligned") => Alignment::Aligned,
            Some("anti-aligned") | Some("anti_aligned") | Some("antialigned") => Alignment::AntiAligned,
            Some(o) => return Err(format!("{what}: align \"{o}\" is not aligned, anti-aligned or closest")),
        };
        let a = self.mate_ref(&spec.a, what, "a")?;
        let b = self.mate_ref(&spec.b, what, "b")?;
        self.app.script_mate(kind, alignment, a, b).map_err(|e| format!("{what}: {e}"))
    }

    fn add_plane(&mut self, spec: &PlaneDef) -> Result<u32, String> {
        let what = "anvil.plane";
        let kind = FeatureKind::Plane(PlaneFeature {
            base: self.plane_of(&spec.from, what)?,
            offset: dim_of(&spec.offset, what)?,
            flip: spec.flip,
            ..Default::default()
        });
        let before = self.app.doc.clone();
        self.commit_feature(what, before, kind)
    }

    fn interference(&mut self, spec: &InterferenceSpec) -> Result<serde_json::Value, String> {
        let what = "anvil.interference";
        if !self.app.in_assembly() {
            return Err(format!("{what}: no assembly is open; open one or start one with anvil.assembly()"));
        }
        let clearance = match spec.clearance {
            Some(c) if !(c.is_finite() && c > 0.0) => {
                return Err(format!("{what}: clearance must be more than 0 mm"));
            }
            c => c,
        };
        let components =
            spec.components.iter().map(|c| self.component_named(what, c)).collect::<Result<Vec<_>, _>>()?;
        let opts = anvil_model::interference::Options {
            clearance,
            touching: spec.touching,
            include_hidden: spec.hidden,
            components,
        };
        let report = self.app.run_interference(&opts).map_err(|e| format!("{what}: {e}"))?;
        let s = self.app.asm.as_ref().ok_or(format!("{what}: no assembly is open"))?;
        Ok(super::interference::report_json(&report, &|c| s.component_label(c)))
    }

    fn set_status(&mut self, text: &str) {
        self.app.status = text.to_string();
        self.app.scripting.script_status = Some(text.to_string());
    }
}

/// A number or an equation as a feature dimension.
fn dim_of(d: &Depth, what: &str) -> Result<Dim, String> {
    match d {
        Depth::Value(v) => Ok(Dim::Value(*v)),
        Depth::Expr(e) => Ok(Dim::Expr(e.clone())),
        Depth::ThroughAll => Err(format!("{what}: \"through\" is not a size here")),
    }
}

/// A direction through the point `through`: a model axis's direction, or a vector. Through
/// the origin, a model axis is that axis itself.
fn axis_of(d: &DirPick, through: [f64; 3], what: &str) -> Result<Axis, String> {
    let v = match d {
        DirPick::X => [1.0, 0.0, 0.0],
        DirPick::Y => [0.0, 1.0, 0.0],
        DirPick::Z => [0.0, 0.0, 1.0],
        DirPick::Vector(v) => *v,
    };
    if v.iter().all(|x| x.abs() < 1e-12) || v.iter().chain(&through).any(|x| !x.is_finite()) {
        return Err(format!("{what}: the direction {{{}, {}, {}}} has no length", v[0], v[1], v[2]));
    }
    let at_origin = through.iter().all(|x| *x == 0.0);
    Ok(match d {
        DirPick::X if at_origin => Axis::X,
        DirPick::Y if at_origin => Axis::Y,
        DirPick::Z if at_origin => Axis::Z,
        _ => {
            let p = |a: [f64; 3]| anvil_kernel::Vec3 { x: a[0], y: a[1], z: a[2] };
            Axis::Line { origin: p(through), dir: p(v) }
        }
    })
}

/// How far a picked point may be from the edge or face it names, in mm.
const PICK_TOLERANCE: f64 = 0.05;

impl AppHost<'_> {
    /// A standard plane, or the face through a point.
    fn plane_of(&self, p: &PlanePick, what: &str) -> Result<PlaneRef, String> {
        Ok(match p {
            PlanePick::Front => PlaneRef::Front,
            PlanePick::Top => PlaneRef::Top,
            PlanePick::Right => PlaneRef::Right,
            PlanePick::Face(q) => PlaneRef::Face(self.face_at(*q, what)?),
            PlanePick::Feature(n) => {
                let f = self.feature_named(what, n)?;
                match f.kind {
                    FeatureKind::Plane(_) => PlaneRef::Feature(f.id),
                    _ => return Err(format!("{what}: {} is not a reference plane", f.name)),
                }
            }
        })
    }

    /// Bodies by name, as `state().bodies` lists them: the feature that last changed a
    /// body, with `[n]` for its place when that feature left several.
    fn body_refs(&self, what: &str, names: &[String]) -> Result<Vec<anvil_model::BodyRef>, String> {
        names
            .iter()
            .map(|n| {
                let (feature, index) = match n.trim().strip_suffix(']').and_then(|t| t.rsplit_once('[')) {
                    Some((f, i)) => (
                        f.trim(),
                        i.trim().parse::<u32>().map_err(|_| format!("{what}: \"{n}\" is not a body name"))?,
                    ),
                    None => (n.trim(), 1),
                };
                let id = self.feature_named(what, feature)?.id;
                let body = anvil_model::BodyRef { feature: id, index };
                if self.app.state.body_names.contains(&body) {
                    Ok(body)
                } else {
                    let have = self.app.body_names();
                    Err(format!(
                        "{what}: there is no body \"{n}\"; the part's bodies are {}",
                        if have.is_empty() { "none yet".to_string() } else { have.join(", ") }
                    ))
                }
            })
            .collect()
    }

    /// One side of a mate: a component's face through a point, or a standard plane.
    fn mate_ref(&self, side: &MateSide, what: &str, key: &str) -> Result<MateRef, String> {
        match side {
            MateSide::Face(p) => {
                let q = anvil_kernel::Vec3 { x: p[0], y: p[1], z: p[2] };
                let (component, face) = self
                    .app
                    .assembly_face_at(q)
                    .map_err(|e| format!("{what}: {key}: {e} at {{{}, {}, {}}}", p[0], p[1], p[2]))?;
                Ok(MateRef { component: Some(component), entity: MateEntity::Face(face) })
            }
            MateSide::Plane { component, plane } => {
                let plane = match plane {
                    PlanePick::Front => PlaneRef::Front,
                    PlanePick::Top => PlaneRef::Top,
                    PlanePick::Right => PlaneRef::Right,
                    _ => return Err(format!("{what}: {key}.plane is \"front\", \"top\" or \"right\"")),
                };
                let component = match component {
                    None => None,
                    Some(c) => Some(self.component_named(what, c)?),
                };
                Ok(MateRef { component, entity: MateEntity::Plane(plane) })
            }
        }
    }

    /// A component of the open assembly by id or name ("bracket<1>").
    fn component_named(&self, what: &str, name: &str) -> Result<u32, String> {
        let comps = self.app.asm.as_ref().map(|s| &s.asm.components).ok_or("no assembly is open")?;
        let found = comps.iter().find(|c| c.id.to_string() == name || c.name == name);
        found.map(|c| c.id).ok_or_else(|| {
            let have: Vec<_> = comps.iter().map(|c| c.name.clone()).collect();
            format!("{what}: there is no component \"{name}\"; the components are {}", have.join(", "))
        })
    }

    /// The ids of the features named by id or name, in order.
    fn feature_ids(&self, what: &str, names: &[String]) -> Result<Vec<u32>, String> {
        names.iter().map(|n| self.feature_named(what, n).map(|f| f.id)).collect()
    }

    /// Adds `kind` and rebuilds; if the feature fails, puts the part back as it was
    /// (`before`) and says why, so a failed call leaves nothing behind.
    fn commit_feature(&mut self, what: &str, before: Document, kind: FeatureKind) -> Result<u32, String> {
        let id = self.app.doc.add_feature(kind);
        self.app.rebuild();
        if let Some(e) = self.app.state.errors.get(&id).cloned() {
            self.app.doc = before;
            self.app.rebuild();
            return Err(format!("{what}: {e}"));
        }
        Ok(id)
    }

    /// The model edge through point `p`: the one within [`PICK_TOLERANCE`] of it, refusing
    /// a point near two edges (a corner).
    fn edge_at(&self, p: [f64; 3], what: &str) -> Result<anvil_kernel::EdgeRef, String> {
        let q = anvil_kernel::Vec3 { x: p[0], y: p[1], z: p[2] };
        let body = &self.app.state.body;
        let mut near: Vec<anvil_kernel::EdgeRef> = Vec::new();
        let mut nearest = f64::INFINITY;
        for (seg, i, j) in body.shared_segments() {
            let (a, b) = (body.polygons[i].tag.id, body.polygons[j].tag.id);
            if a == b {
                continue; // Inside one face: not a model edge.
            }
            let d = point_segment_distance(q, seg);
            nearest = nearest.min(d);
            let e = anvil_kernel::EdgeRef::new(a, b);
            if d <= PICK_TOLERANCE && !near.contains(&e) {
                near.push(e);
            }
        }
        match near.as_slice() {
            [e] => Ok(*e),
            [] if nearest.is_finite() => Err(format!(
                "{what}: no edge passes through {{{}, {}, {}}}; the nearest is {:.3} mm away (give a point on the edge, within {PICK_TOLERANCE} mm)",
                p[0], p[1], p[2], nearest
            )),
            [] => Err(format!("{what}: there is no body yet to pick edges on")),
            _ => Err(format!(
                "{what}: {{{}, {}, {}}} is on {} edges (a corner?); give a point along one edge, away from its ends",
                p[0], p[1], p[2], near.len()
            )),
        }
    }

    /// The face through point `p`, within [`PICK_TOLERANCE`], refusing a point on an edge.
    fn face_at(&self, p: [f64; 3], what: &str) -> Result<anvil_kernel::FaceId, String> {
        let q = anvil_kernel::Vec3 { x: p[0], y: p[1], z: p[2] };
        face_through(&self.app.state.body, q).map_err(|e| format!("{what}: {e}"))
    }
}

/// The face of `body` through `q`, within [`PICK_TOLERANCE`], refusing a point on an edge.
pub(super) fn face_through(
    body: &anvil_kernel::Solid,
    q: anvil_kernel::Vec3,
) -> Result<anvil_kernel::FaceId, String> {
    let mut found: Vec<anvil_kernel::FaceId> = Vec::new();
    for poly in &body.polygons {
        if (poly.normal.dot(q) - poly.w).abs() > PICK_TOLERANCE || found.contains(&poly.tag.id) {
            continue;
        }
        let u = poly.normal.any_perpendicular();
        let v = poly.normal.cross(u);
        let flat = |x: anvil_kernel::Vec3| anvil_kernel::v2(x.dot(u), x.dot(v));
        let pts: Vec<anvil_kernel::Vec2> = poly.verts.iter().map(|&x| flat(x)).collect();
        if anvil_kernel::profile::point_in_polygon(flat(q), &pts) {
            found.push(poly.tag.id);
        }
    }
    match found.as_slice() {
        [f] => Ok(*f),
        [] => Err(format!("no face passes through {{{}, {}, {}}}", q.x, q.y, q.z)),
        _ => Err(format!(
            "{{{}, {}, {}}} is on {} faces (an edge?); give a point inside one face",
            q.x,
            q.y,
            q.z,
            found.len()
        )),
    }
}

fn point_segment_distance(p: anvil_kernel::Vec3, s: [anvil_kernel::Vec3; 2]) -> f64 {
    let d = s[1] - s[0];
    let t = ((p - s[0]).dot(d) / d.dot(d).max(1e-300)).clamp(0.0, 1.0);
    (s[0] + d * t - p).length()
}

/// Merges sketch points that sit at the same place, so lines and arcs given end to end
/// share their ends and close into profiles. Arc and circle centres are left alone.
fn join_coincident_ends(sk: &mut anvil_sketch::Sketch) {
    loop {
        let centres: Vec<anvil_sketch::PointId> = sk
            .entities
            .iter()
            .filter_map(|e| match e.geom {
                anvil_sketch::Geom::Arc { c, .. } | anvil_sketch::Geom::Circle { c, .. } => Some(c),
                _ => None,
            })
            .collect();
        let ends: Vec<anvil_sketch::PointId> =
            (0..sk.points.len()).filter(|p| !centres.contains(p) && *p != anvil_sketch::ORIGIN).collect();
        let pair = ends.iter().enumerate().find_map(|(k, &a)| {
            ends[k + 1..].iter().find(|&&b| sk.pos(a).distance(sk.pos(b)) < 1e-9).map(|&b| (a, b))
        });
        match pair {
            Some((a, b)) => {
                if sk.merge_points(a, b).is_err() {
                    return;
                }
            }
            None => return,
        }
    }
}

/// Written by Edit Config when there is no init.lua yet.
pub const STARTER_CONFIG: &str = r##"-- AnvilCAD configuration. AnvilCAD reloads this file when you save it.
-- Everything it can set is in docs/SCRIPTING.md.

-- anvil.opt.theme = "dark"
-- anvil.opt.ui_scale = 1.1
-- anvil.opt.material = "6061 Alloy"
-- anvil.opt.view = "isometric"
-- anvil.opt.mouse_3d_speed = 1.5

-- anvil.theme.dark.accent = "#e3825a"
-- anvil.theme.body = "#b8bfcc"

-- anvil.leader = "space"
-- anvil.key("<leader>e", "Extruded Boss/Base")
-- anvil.key("ctrl+shift+f", "Fillet")

-- anvil.command("Plate 100 x 60 x 10", function()
--   local s = anvil.sketch{ plane = "top", rects = { { -50, -30, 50, 30 } } }
--   anvil.extrude{ sketch = s, depth = 10 }
-- end, { description = "A test plate" })
"##;
