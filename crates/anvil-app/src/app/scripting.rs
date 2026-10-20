//! The app's side of Lua configuration and scripting (see `crate::script` and
//! docs/SCRIPTING.md): named built-in commands, user key bindings, the command palette,
//! loading and live-reloading `init.lua`, and the [`Host`] that scripts drive.

use super::*;
use crate::keymap::{Matcher, Step};
use crate::script::{self, Action, Depth, Engine, ExtrudeSpec, Host, PlaneSpec, SketchSpec};
use std::time::SystemTime;

/// Every built-in command a key binding, script (`anvil.run`) or the command palette can run,
/// with a short description. Names match the toolbar and menus.
pub const BUILTIN_COMMANDS: &[(&str, &str)] = &[
    ("New", "Start a new part"),
    ("New Assembly", "Start a new assembly"),
    ("Insert Component", "Place a part in the assembly"),
    ("Mate", "Add a mate between two components"),
    ("Bill of Materials", "Show the assembly's parts list"),
    ("Open", "Open a part, template, STEP or SOLIDWORKS file"),
    ("Save", "Save the part"),
    ("Save As", "Save the part under a new name"),
    ("Save as Part Template", "Save the part as a template for new parts"),
    ("Export", "Export STEP, STL or OBJ"),
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
    options: script::Options,
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
        match *name {
            "New" => self.request(PendingAction::New),
            "New Assembly" => self.request(PendingAction::NewAssembly),
            "Insert Component" => self.insert_component(),
            "Mate" => self.start_mate(),
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
            "Zoom to Fit" => self.fit_pending = true,
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
    fn with_engine(
        &mut self,
        ctx: &egui::Context,
        f: impl FnOnce(&Engine, &mut dyn Host) -> Result<(), String>,
    ) -> Result<(), String> {
        let Some(engine) = self.scripting.engine.take() else {
            return Err("no Lua engine is loaded".into());
        };
        let before = self.doc.clone();
        self.scripting.script_status = None;
        let result = f(&engine, &mut AppHost { app: self, ctx });
        self.scripting.engine = Some(engine);
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
        let name = self.app.part_name();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let exact = (ext == "step" || ext == "stp")
            .then(|| anvil_model::step::exact_step(&self.app.state, &name))
            .flatten();
        match exact {
            Some(text) => std::fs::write(path, text),
            None => anvil_io::export_by_extension(&self.app.state.body, path, &name),
        }
        .map_err(|e| format!("export to {}: {e}", path.display()))
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
            PlaneSpec::Face { feature, start } => {
                if self.app.doc.feature(feature).is_none() {
                    return Err(format!("anvil.sketch: there is no feature {feature}"));
                }
                let index = if start { FaceId::START_CAP } else { FaceId::END_CAP };
                PlaneRef::Face(FaceId { feature, index })
            }
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
        Ok(self.app.doc.add_feature(FeatureKind::Sketch(sf)))
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
        let id = self.app.doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: spec.sketch,
            end,
            depth,
            reverse: spec.reverse,
            op: if spec.cut { BodyOp::Cut } else { BodyOp::Boss },
            ..Default::default()
        }));
        // Rebuild now, so a later sketch can sit on this feature's faces and errors show here.
        self.app.rebuild();
        if let Some(e) = self.app.state.errors.get(&id) {
            return Err(format!("anvil.extrude: {e}"));
        }
        Ok(id)
    }

    fn set_status(&mut self, text: &str) {
        self.app.status = text.to_string();
        self.app.scripting.script_status = Some(text.to_string());
    }
}

/// Written by Edit Config when there is no init.lua yet.
pub const STARTER_CONFIG: &str = r##"-- AnvilCAD configuration. AnvilCAD reloads this file when you save it.
-- Everything it can set is in docs/SCRIPTING.md.

-- anvil.opt.theme = "dark"
-- anvil.opt.ui_scale = 1.1
-- anvil.opt.material = "6061 Alloy"
-- anvil.opt.view = "isometric"

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
