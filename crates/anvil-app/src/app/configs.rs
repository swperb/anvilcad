//! The configuration manager: the Configurations tab of the left pane, the configuration
//! properties window and the configuration table. Behaviour follows
//! `docs/re/configurations.md`.

use super::AnvilApp;
use crate::theme::{self, Icon, Palette, RowLook};
use anvil_model::*;
use eframe::egui::{self, RichText};

/// Which tree the left pane shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Manager {
    #[default]
    Features,
    Configurations,
}

/// The configuration manager's window state.
#[derive(Default)]
pub(super) struct ConfigUi {
    pub manager: Manager,
    pub table: bool,
    pub props: Option<PropsWindow>,
    /// The table cell being typed in, with its text.
    editing: Option<((ConfigParam, u32), String)>,
    /// Each configuration rebuilt by "Rebuild All Configurations", and the document they
    /// were rebuilt from.
    rebuilt: Option<(Document, Vec<Rebuilt>)>,
}

/// One configuration rebuilt for the table.
#[derive(Clone)]
struct Rebuilt {
    id: u32,
    volume: f64,
    mass: Option<f64>,
    errors: usize,
}

/// The Add Configuration and Configuration Properties window.
pub(super) struct PropsWindow {
    /// The configuration being edited, or `None` when adding one.
    id: Option<u32>,
    /// For a new derived configuration, its parent.
    parent: Option<u32>,
    name: String,
    description: String,
    comment: String,
    suppress_new_features: bool,
    properties: Vec<CustomProperty>,
    new_prop: String,
    error: Option<String>,
}

impl ConfigUi {
    /// Closes the windows and forgets results, keeping which tree is shown.
    pub fn reset(&mut self) {
        *self = ConfigUi { manager: self.manager, ..ConfigUi::default() };
    }
}

impl AnvilApp {
    /// The Feature Tree / Configurations switch at the top of the left pane.
    pub(super) fn manager_tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (m, label) in
                [(Manager::Features, "Feature Tree"), (Manager::Configurations, "Configurations")]
            {
                if theme::tab(ui, label, self.cfg.manager == m).clicked() {
                    self.cfg.manager = m;
                }
            }
        });
        let pal = Palette::of(ui);
        let y = ui.cursor().top();
        ui.painter().hline(ui.max_rect().x_range(), y, egui::Stroke::new(1.0_f32, pal.border));
        ui.add_space(8.0);
    }

    /// Makes a configuration active and rebuilds.
    pub(super) fn show_configuration(&mut self, id: u32) {
        let unchanged = self.cfg.rebuilt.as_ref().is_some_and(|(d, _)| *d == self.doc);
        match self.doc.activate_configuration(id) {
            Ok(()) => {
                self.cfg.editing = None;
                self.rebuild();
                // Switching changes no configuration, so the table's results still hold.
                if unchanged {
                    if let Some((d, _)) = &mut self.cfg.rebuilt {
                        *d = self.doc.clone();
                    }
                }
                self.status = format!("Showing configuration {}", self.doc.active_configuration_name());
            }
            Err(e) => self.message = Some(("Configuration".into(), e.to_string())),
        }
    }

    /// Configures a feature's suppression and dimensions and opens the table.
    pub(super) fn configure_feature(&mut self, id: u32) {
        match self.doc.configure_feature(id) {
            Ok(()) => self.cfg.table = true,
            Err(e) => self.message = Some(("Configure Feature".into(), e.to_string())),
        }
    }

    fn open_config_props(&mut self, id: Option<u32>, parent: Option<u32>) {
        let w = match id.and_then(|id| self.doc.configuration(id)) {
            Some(c) => PropsWindow {
                id: Some(c.id),
                parent: c.parent,
                name: c.name.clone(),
                description: c.description.clone(),
                comment: c.comment.clone(),
                suppress_new_features: c.suppress_new_features,
                properties: c.properties.clone(),
                new_prop: String::new(),
                error: None,
            },
            None => {
                let base = match parent.and_then(|p| self.doc.configuration(p)) {
                    Some(p) => format!("{} derived", p.name),
                    None => "Configuration".into(),
                };
                PropsWindow {
                    id: None,
                    parent,
                    name: self.doc.unused_configuration_name(&base),
                    description: String::new(),
                    comment: String::new(),
                    suppress_new_features: false,
                    properties: Vec::new(),
                    new_prop: String::new(),
                    error: None,
                }
            }
        };
        self.cfg.props = Some(w);
    }

    /// The Configurations tab of the left pane.
    pub(super) fn configuration_manager(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.horizontal(|ui| {
            let pal = Palette::of(ui);
            let (r, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
            theme::paint_icon(ui.painter(), r, Icon::Part, pal.text, pal.accent);
            ui.label(RichText::new(self.part_name()).size(15.0).strong());
        })
        .response
        .context_menu(|ui| {
            if ui.button("Add Configuration...").clicked() {
                ui.close_menu();
                self.open_config_props(None, None);
            }
        });
        ui.add_space(6.0);
        theme::section_label(ui, "Configurations");

        let mut action: Option<(u32, &'static str)> = None;
        let active = self.doc.active_configuration;
        for (id, depth) in self.doc.configuration_tree() {
            let Some(c) = self.doc.configuration(id) else { continue };
            let label = if c.description.is_empty() {
                c.name.clone()
            } else {
                format!("{} [{}]", c.name, c.description)
            };
            let look = RowLook { selected: id == active, indent: depth.min(8) as u8, ..Default::default() };
            let mut r = theme::tree_row(ui, Icon::Configuration, &label, look);
            if id == active {
                r = r.on_hover_text("The active configuration");
            } else {
                r = r.on_hover_text("Double-click to show this configuration");
            }
            if r.double_clicked() {
                action = Some((id, "show"));
            }
            r.context_menu(|ui| {
                if ui.button("Show Configuration").clicked() {
                    action = Some((id, "show"));
                    ui.close_menu();
                }
                if ui.button("Add Derived Configuration...").clicked() {
                    action = Some((id, "derive"));
                    ui.close_menu();
                }
                if ui.button("Properties...").clicked() {
                    action = Some((id, "props"));
                    ui.close_menu();
                }
                ui.separator();
                if ui.add_enabled(id != active, egui::Button::new("Delete")).clicked() {
                    action = Some((id, "delete"));
                    ui.close_menu();
                }
            });
        }
        ui.add_space(6.0);
        if ui.button("Add Configuration...").clicked() {
            self.open_config_props(None, None);
        }
        ui.add_space(4.0);
        theme::section_label(ui, "Tables");
        let n = self.doc.configured.len();
        let label = format!("Configuration Table ({n} parameter{})", if n == 1 { "" } else { "s" });
        if theme::tree_row(
            ui,
            Icon::Equations,
            &label,
            RowLook { selected: self.cfg.table, ..Default::default() },
        )
        .on_hover_text("Values of configured parameters in each configuration")
        .clicked()
        {
            self.cfg.table = true;
        }

        if let Some((id, what)) = action {
            match what {
                "show" => self.show_configuration(id),
                "derive" => self.open_config_props(None, Some(id)),
                "props" => self.open_config_props(Some(id), None),
                "delete" => match self.doc.delete_configuration(id) {
                    Ok(()) => self.rebuild(),
                    Err(e) => self.message = Some(("Delete Configuration".into(), e.to_string())),
                },
                _ => {}
            }
        }
    }

    pub(super) fn config_windows(&mut self, ctx: &egui::Context) {
        self.config_props_window(ctx);
        self.config_table_window(ctx);
    }

    fn config_props_window(&mut self, ctx: &egui::Context) {
        let Some(w) = &mut self.cfg.props else { return };
        let title = if w.id.is_some() { "Configuration Properties" } else { "Add Configuration" };
        let parent_name = w.parent.and_then(|p| self.doc.configuration(p)).map(|p| p.name.clone());
        let mut open = true;
        let mut result: Option<bool> = None;
        egui::Window::new(title).open(&mut open).default_width(420.0).collapsible(false).show(ctx, |ui| {
            egui::Grid::new("cfg_props").num_columns(2).show(ui, |ui| {
                ui.label("Configuration name");
                ui.add(egui::TextEdit::singleline(&mut w.name).desired_width(240.0));
                ui.end_row();
                ui.label("Description");
                ui.add(egui::TextEdit::singleline(&mut w.description).desired_width(240.0));
                ui.end_row();
                ui.label("Comment");
                ui.add(egui::TextEdit::multiline(&mut w.comment).desired_rows(2).desired_width(240.0));
                ui.end_row();
                if let Some(p) = &parent_name {
                    ui.label("Derived from");
                    ui.label(p);
                    ui.end_row();
                }
            });
            ui.checkbox(&mut w.suppress_new_features, "Suppress new features").on_hover_text(
                "Features added while another configuration is active are suppressed in this one",
            );
            ui.separator();
            ui.label(RichText::new("Configuration-specific properties").strong());
            let mut del = None;
            egui::Grid::new("cfg_custom").striped(true).num_columns(3).show(ui, |ui| {
                for (i, p) in w.properties.iter_mut().enumerate() {
                    ui.add(egui::TextEdit::singleline(&mut p.name).desired_width(120.0));
                    ui.add(egui::TextEdit::singleline(&mut p.value).desired_width(200.0));
                    if ui.small_button("x").clicked() {
                        del = Some(i);
                    }
                    ui.end_row();
                }
            });
            if let Some(i) = del {
                w.properties.remove(i);
            }
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut w.new_prop)
                        .hint_text("new property name")
                        .desired_width(160.0),
                );
                if ui.button("Add").clicked() && !w.new_prop.trim().is_empty() {
                    w.properties.push(CustomProperty {
                        name: w.new_prop.trim().into(),
                        kind: PropertyKind::Text,
                        value: String::new(),
                    });
                    w.new_prop.clear();
                }
            });
            ui.weak("These replace the file's properties of the same name in this configuration.");
            if let Some(e) = &w.error {
                ui.colored_label(Palette::of(ui).error, e);
            }
            ui.separator();
            ui.horizontal(|ui| {
                if theme::primary_button(ui, "OK").clicked() {
                    result = Some(true);
                }
                if theme::secondary_button(ui, "Cancel").clicked() {
                    result = Some(false);
                }
            });
        });
        if !open {
            result = Some(false);
        }
        match result {
            Some(true) => self.apply_config_props(),
            Some(false) => self.cfg.props = None,
            None => {}
        }
    }

    fn apply_config_props(&mut self) {
        let Some(w) = &mut self.cfg.props else { return };
        let name = w.name.clone();
        let id = match w.id {
            Some(id) => self.doc.rename_configuration(id, &name).map(|_| id),
            None => self.doc.add_configuration(&name, w.parent),
        };
        let id = match id {
            Ok(id) => id,
            Err(e) => {
                w.error = Some(e.to_string());
                return;
            }
        };
        if let Some(c) = self.doc.configuration_mut(id) {
            c.description = w.description.trim().to_string();
            c.comment = w.comment.clone();
            c.suppress_new_features = w.suppress_new_features;
            c.properties = w.properties.iter().filter(|p| !p.name.trim().is_empty()).cloned().collect();
        }
        let added = w.id.is_none();
        self.cfg.props = None;
        if added {
            self.cfg.editing = None;
            self.rebuild();
            self.status = format!("Added and showing configuration {name}");
        }
    }

    /// Rebuilds every configuration for the table's volume and mass columns.
    fn rebuild_all_configurations(&mut self) {
        self.doc.capture_configuration();
        let mut out = Vec::new();
        for c in &self.doc.configurations {
            let mut d = self.doc.clone();
            if d.activate_configuration(c.id).is_err() {
                continue;
            }
            let st = d.rebuild();
            let m = st.mass_report(&d);
            out.push(Rebuilt { id: c.id, volume: m.props.volume, mass: m.mass, errors: st.errors.len() });
        }
        self.cfg.rebuilt = Some((self.doc.clone(), out));
        self.status = format!("Rebuilt {} configurations", self.doc.configurations.len());
    }

    fn config_table_window(&mut self, ctx: &egui::Context) {
        if !self.cfg.table {
            return;
        }
        let mut open = true;
        let mut changed = false;
        let mut show: Option<u32> = None;
        let mut unconfigure: Option<usize> = None;
        let mut add: Option<ConfigParam> = None;
        let mut rebuild_all = false;
        let results: Option<Vec<Rebuilt>> = match &self.cfg.rebuilt {
            Some((d, r)) if *d == self.doc => Some(r.clone()),
            _ => None,
        };
        let tree = self.doc.configuration_tree();
        let active = self.doc.active_configuration;
        egui::Window::new("Configuration Table").open(&mut open).default_width(640.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                let params = self.doc.configurable_params();
                egui::ComboBox::from_id_salt("cfg_add")
                    .selected_text("Add parameter...")
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for p in params {
                            if ui.selectable_label(false, self.doc.param_label(&p)).clicked() {
                                add = Some(p);
                            }
                        }
                    });
                if ui.button("Rebuild All Configurations").clicked() {
                    rebuild_all = true;
                }
            });
            ui.add_space(4.0);
            let pal = Palette::of(ui);
            egui::ScrollArea::both().max_height(420.0).show(ui, |ui| {
                egui::Grid::new("cfg_table").striped(true).show(ui, |ui| {
                    ui.strong("Configuration");
                    for (i, p) in self.doc.configured.iter().enumerate() {
                        ui.strong(self.doc.param_label(&p.param)).context_menu(|ui| {
                            if ui.button("Unconfigure").on_hover_text("Use the active configuration's value in all").clicked() {
                                unconfigure = Some(i);
                                ui.close_menu();
                            }
                        });
                    }
                    if results.is_some() {
                        ui.strong("Volume (mm³)");
                        ui.strong("Mass (g)");
                    }
                    ui.end_row();

                    for &(cfg, depth) in &tree {
                        let Some(c) = self.doc.configuration(cfg) else { continue };
                        let mut name = RichText::new(format!("{}{}", "    ".repeat(depth), c.name));
                        if cfg == active {
                            name = name.strong().color(pal.accent);
                        }
                        let r = ui.add(egui::Label::new(name).sense(egui::Sense::click()));
                        if r.double_clicked() {
                            show = Some(cfg);
                        }
                        r.on_hover_text(if cfg == active { "Active" } else { "Double-click to show" });
                        for i in 0..self.doc.configured.len() {
                            let param = self.doc.configured[i].param.clone();
                            let value = self.doc.resolved_value(i, cfg);
                            let inherited = !self.doc.is_overridden(i, cfg);
                            let set = self.table_cell(ui, &param, cfg, value, inherited);
                            if let Some(v) = set {
                                if let Err(e) = self.doc.set_configured_value(i, cfg, v) {
                                    self.status = e.to_string();
                                }
                                changed = true;
                            }
                        }
                        if let Some(r) = results.as_ref().and_then(|rs| rs.iter().find(|r| r.id == cfg)) {
                            if r.errors > 0 {
                                ui.colored_label(pal.error, format!("{} error(s)", r.errors));
                            } else {
                                ui.label(format!("{:.2}", r.volume));
                            }
                            ui.label(r.mass.map(|m| format!("{m:.2}")).unwrap_or_else(|| "-".into()));
                        }
                        ui.end_row();
                    }
                });
            });
            if self.doc.configured.is_empty() {
                ui.weak("Nothing is configured yet. Add a parameter, or right-click a feature and choose Configure Feature.");
            } else {
                ui.weak("Faded values are inherited from the parent configuration. Right-click a value to link it back.");
            }
        });
        if !open {
            self.cfg.table = false;
            self.cfg.editing = None;
        }
        if let Some(p) = add {
            if let Err(e) = self.doc.configure(p) {
                self.status = e.to_string();
            }
        }
        if let Some(i) = unconfigure {
            self.doc.unconfigure(i);
        }
        if let Some(id) = show {
            self.show_configuration(id);
        } else if changed {
            self.rebuild();
        }
        if rebuild_all {
            self.rebuild_all_configurations();
        }
    }

    /// One cell of the table. Returns `Some(new value)` when the user changed it, where
    /// `Some(None)` links a derived configuration's value back to its parent.
    fn table_cell(
        &mut self,
        ui: &mut egui::Ui,
        param: &ConfigParam,
        cfg: u32,
        value: Option<ConfigValue>,
        inherited: bool,
    ) -> Option<Option<ConfigValue>> {
        let pal = Palette::of(ui);
        // Inherited values are drawn faded.
        let faded = pal.text_weak.gamma_multiply(0.6);
        let derived = self.doc.configuration(cfg).is_some_and(|c| c.parent.is_some());
        let mut out = None;
        let id = ("cfg_cell", param.clone(), cfg);
        let r = match &value {
            Some(ConfigValue::Suppressed(s)) => {
                let mut b = *s;
                let r = ui.checkbox(
                    &mut b,
                    if inherited {
                        RichText::new("Suppressed").color(faded)
                    } else {
                        RichText::new("Suppressed")
                    },
                );
                if r.changed() {
                    out = Some(Some(ConfigValue::Suppressed(b)));
                }
                r
            }
            Some(ConfigValue::Material(m)) => {
                let current = m.as_ref().map(|m| m.name.clone()).unwrap_or_else(|| "<not specified>".into());
                let text =
                    if inherited { RichText::new(current).color(faded) } else { RichText::new(current) };
                egui::ComboBox::from_id_salt(id.clone())
                    .selected_text(text)
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(m.is_none(), "<not specified>").clicked() {
                            out = Some(Some(ConfigValue::Material(None)));
                        }
                        for lib in Material::library() {
                            let sel = m.as_ref().is_some_and(|c| c.name == lib.name);
                            if ui.selectable_label(sel, &lib.name).clicked() {
                                out = Some(Some(ConfigValue::Material(Some(lib))));
                            }
                        }
                    })
                    .response
            }
            Some(v @ (ConfigValue::Dim(_) | ConfigValue::Expr(_))) => {
                let key = (param.clone(), cfg);
                let mut buf = match &self.cfg.editing {
                    Some((k, b)) if *k == key => b.clone(),
                    _ => v.text(),
                };
                let mut te = egui::TextEdit::singleline(&mut buf).id_salt(id.clone()).desired_width(90.0);
                if inherited {
                    te = te.text_color(faded);
                }
                let r = ui.add(te);
                if r.lost_focus() {
                    self.cfg.editing = None;
                    let t = buf.trim();
                    if t != v.text() {
                        if t.is_empty() {
                            if derived {
                                out = Some(None);
                            }
                        } else {
                            out = Some(Some(match v {
                                ConfigValue::Dim(_) => ConfigValue::Dim(Dim::parse(t)),
                                _ => ConfigValue::Expr(t.trim_start_matches('=').trim().to_string()),
                            }));
                        }
                    }
                } else if r.has_focus() {
                    self.cfg.editing = Some((key, buf));
                }
                r
            }
            None => ui.weak("-"),
        };
        if derived && !inherited {
            r.context_menu(|ui| {
                if ui.button("Link to Parent").clicked() {
                    out = Some(None);
                    ui.close_menu();
                }
            });
        }
        out
    }
}
