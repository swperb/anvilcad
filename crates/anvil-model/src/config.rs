//! Configurations: named variants of a part kept in one document.
//!
//! The behaviour follows the specification in `docs/re/configurations.md`. In short:
//!
//! - The feature tree always holds the active configuration's values, so every editor
//!   works on the active configuration without knowing about configurations.
//! - A parameter is either unconfigured (one value, the tree's, for every configuration)
//!   or configured, with a value per configuration in [`ConfiguredParam::values`].
//! - The stored value for the active configuration can lag behind the tree until
//!   [`Document::capture_configuration`] records it; switching, saving and every table
//!   operation capture first.
//! - A derived configuration stores only the values it overrides and inherits the rest
//!   from its parent.

use crate::document::{CustomProperty, Dim, Document, FeatureKind};
use crate::material::Material;
use crate::rebuild::BuildState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The name a new part's only configuration gets.
pub const DEFAULT_CONFIGURATION: &str = "Default";

/// One configuration of a part.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Configuration {
    pub id: u32,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// For a derived configuration, the configuration it inherits from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u32>,
    /// Features added while another configuration is active are suppressed in this one.
    #[serde(default)]
    pub suppress_new_features: bool,
    /// Configuration-specific custom properties.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub properties: Vec<CustomProperty>,
}

impl Configuration {
    pub fn new(id: u32, name: &str) -> Configuration {
        Configuration {
            id,
            name: name.to_string(),
            description: String::new(),
            comment: String::new(),
            parent: None,
            suppress_new_features: false,
            properties: Vec::new(),
        }
    }
}

/// Which dimension of a feature a configured parameter is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DimSlot {
    /// Extrude depth.
    Depth,
    /// Revolve angle or circular pattern total angle.
    Angle,
    /// Reference plane offset.
    Offset,
    /// Linear pattern spacing.
    Spacing,
    /// Pattern instance count.
    Count,
    /// Fillet radius.
    Radius,
    /// Chamfer distance.
    Distance,
    /// Angle of an angle-and-distance chamfer.
    ChamferAngle,
    /// Shell wall thickness.
    Thickness,
}

impl DimSlot {
    pub fn label(self) -> &'static str {
        match self {
            DimSlot::Depth => "depth",
            DimSlot::Angle => "angle",
            DimSlot::Offset => "offset",
            DimSlot::Spacing => "spacing",
            DimSlot::Count => "count",
            DimSlot::Radius => "radius",
            DimSlot::Distance => "distance",
            DimSlot::ChamferAngle => "chamfer angle",
            DimSlot::Thickness => "thickness",
        }
    }

    /// The dimensions a feature of this kind has that can be configured.
    pub fn of(kind: &FeatureKind) -> Vec<DimSlot> {
        use crate::document::ChamferKind;
        match kind {
            FeatureKind::Extrude(e) if e.end.uses_depth() => vec![DimSlot::Depth],
            FeatureKind::Revolve(_) => vec![DimSlot::Angle],
            FeatureKind::Plane(_) => vec![DimSlot::Offset],
            FeatureKind::LinearPattern(_) => vec![DimSlot::Spacing, DimSlot::Count],
            FeatureKind::CircularPattern(_) => vec![DimSlot::Angle, DimSlot::Count],
            FeatureKind::Fillet(_) => vec![DimSlot::Radius],
            FeatureKind::Chamfer(c) => match c.kind {
                ChamferKind::AngleDistance { .. } => vec![DimSlot::Distance, DimSlot::ChamferAngle],
                _ => vec![DimSlot::Distance],
            },
            FeatureKind::Shell(_) => vec![DimSlot::Thickness],
            _ => Vec::new(),
        }
    }

    /// The dimension this slot names in a feature, if the feature has it.
    pub fn get(self, kind: &FeatureKind) -> Option<&Dim> {
        use crate::document::ChamferKind;
        match (self, kind) {
            (DimSlot::Depth, FeatureKind::Extrude(e)) => Some(&e.depth),
            (DimSlot::Angle, FeatureKind::Revolve(r)) => Some(&r.angle),
            (DimSlot::Angle, FeatureKind::CircularPattern(p)) => Some(&p.total_angle),
            (DimSlot::Offset, FeatureKind::Plane(p)) => Some(&p.offset),
            (DimSlot::Spacing, FeatureKind::LinearPattern(p)) => Some(&p.spacing),
            (DimSlot::Count, FeatureKind::LinearPattern(p)) => Some(&p.count),
            (DimSlot::Count, FeatureKind::CircularPattern(p)) => Some(&p.count),
            (DimSlot::Radius, FeatureKind::Fillet(f)) => Some(&f.radius),
            (DimSlot::Distance, FeatureKind::Chamfer(c)) => Some(&c.distance),
            (DimSlot::ChamferAngle, FeatureKind::Chamfer(c)) => match &c.kind {
                ChamferKind::AngleDistance { angle, .. } => Some(angle),
                _ => None,
            },
            (DimSlot::Thickness, FeatureKind::Shell(s)) => Some(&s.thickness),
            _ => None,
        }
    }

    pub fn get_mut(self, kind: &mut FeatureKind) -> Option<&mut Dim> {
        use crate::document::ChamferKind;
        match (self, kind) {
            (DimSlot::Depth, FeatureKind::Extrude(e)) => Some(&mut e.depth),
            (DimSlot::Angle, FeatureKind::Revolve(r)) => Some(&mut r.angle),
            (DimSlot::Angle, FeatureKind::CircularPattern(p)) => Some(&mut p.total_angle),
            (DimSlot::Offset, FeatureKind::Plane(p)) => Some(&mut p.offset),
            (DimSlot::Spacing, FeatureKind::LinearPattern(p)) => Some(&mut p.spacing),
            (DimSlot::Count, FeatureKind::LinearPattern(p)) => Some(&mut p.count),
            (DimSlot::Count, FeatureKind::CircularPattern(p)) => Some(&mut p.count),
            (DimSlot::Radius, FeatureKind::Fillet(f)) => Some(&mut f.radius),
            (DimSlot::Distance, FeatureKind::Chamfer(c)) => Some(&mut c.distance),
            (DimSlot::ChamferAngle, FeatureKind::Chamfer(c)) => match &mut c.kind {
                ChamferKind::AngleDistance { angle, .. } => Some(angle),
                _ => None,
            },
            (DimSlot::Thickness, FeatureKind::Shell(s)) => Some(&mut s.thickness),
            _ => None,
        }
    }
}

/// A parameter that can take a different value in each configuration.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ConfigParam {
    /// Whether a feature (or sketch) is suppressed.
    Suppression { feature: u32 },
    /// A dimension of a feature.
    Dimension { feature: u32, slot: DimSlot },
    /// A global variable's expression.
    Variable { name: String },
    /// The part's material.
    Material,
}

impl ConfigParam {
    /// The feature this parameter belongs to, if any.
    pub fn feature(&self) -> Option<u32> {
        match self {
            ConfigParam::Suppression { feature } | ConfigParam::Dimension { feature, .. } => Some(*feature),
            _ => None,
        }
    }
}

/// A parameter's value in one configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ConfigValue {
    Suppressed(bool),
    Dim(Dim),
    /// A global variable's expression.
    Expr(String),
    Material(Option<Material>),
}

impl ConfigValue {
    /// Short text for tables.
    pub fn text(&self) -> String {
        match self {
            ConfigValue::Suppressed(true) => "Suppressed".into(),
            ConfigValue::Suppressed(false) => "Unsuppressed".into(),
            ConfigValue::Dim(d) => d.text(),
            ConfigValue::Expr(e) => e.clone(),
            ConfigValue::Material(Some(m)) => m.name.clone(),
            ConfigValue::Material(None) => "<not specified>".into(),
        }
    }
}

/// A configured parameter and its values. A top-level configuration always has a value;
/// a derived one has one only where it overrides its parent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConfiguredParam {
    pub param: ConfigParam,
    pub values: BTreeMap<u32, ConfigValue>,
}

/// Why a configuration operation was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    EmptyName,
    /// The name has a character configuration names cannot have.
    BadCharacter(char),
    /// The name starts or ends with a space.
    OuterSpace,
    DuplicateName(String),
    NotFound(u32),
    DeleteActive,
    DeleteLast,
    /// Only a derived configuration can link a value to its parent.
    NotDerived,
    /// The parameter does not exist in the part (a deleted feature or variable, say).
    NoSuchParameter,
    /// The value is the wrong kind for the parameter.
    WrongValue,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::EmptyName => write!(f, "a configuration needs a name"),
            ConfigError::BadCharacter(c) => write!(f, "configuration names cannot contain '{c}'"),
            ConfigError::OuterSpace => write!(f, "configuration names cannot start or end with a space"),
            ConfigError::DuplicateName(n) => write!(f, "there is already a configuration named '{n}'"),
            ConfigError::NotFound(id) => write!(f, "configuration #{id} does not exist"),
            ConfigError::DeleteActive => {
                write!(f, "the active configuration cannot be deleted; show another one first")
            }
            ConfigError::DeleteLast => write!(f, "a part needs at least one configuration"),
            ConfigError::NotDerived => write!(f, "only a derived configuration can link to its parent"),
            ConfigError::NoSuchParameter => write!(f, "that parameter is not in the part"),
            ConfigError::WrongValue => write!(f, "that value does not fit the parameter"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Document {
    /// Gives a document with no configurations (from a file saved before they existed)
    /// its one "Default" configuration, and makes sure the active one exists.
    pub fn normalize_configurations(&mut self) {
        if self.configurations.is_empty() {
            self.configurations.push(Configuration::new(1, DEFAULT_CONFIGURATION));
        }
        if self.configuration(self.active_configuration).is_none() {
            self.active_configuration = self.configurations[0].id;
        }
    }

    /// True when the part has more than its one implicit configuration, so the file needs
    /// a format version that knows about configurations.
    pub fn uses_configurations(&self) -> bool {
        self.configurations.len() > 1 || !self.configured.is_empty()
    }

    pub fn configuration(&self, id: u32) -> Option<&Configuration> {
        self.configurations.iter().find(|c| c.id == id)
    }

    pub fn configuration_mut(&mut self, id: u32) -> Option<&mut Configuration> {
        self.configurations.iter_mut().find(|c| c.id == id)
    }

    pub fn configuration_by_name(&self, name: &str) -> Option<&Configuration> {
        self.configurations.iter().find(|c| c.name == name)
    }

    /// The active configuration's name.
    pub fn active_configuration_name(&self) -> &str {
        self.configuration(self.active_configuration)
            .map(|c| c.name.as_str())
            .unwrap_or(DEFAULT_CONFIGURATION)
    }

    /// Configurations in tree order: each top-level one followed by its derived ones,
    /// with their depth.
    pub fn configuration_tree(&self) -> Vec<(u32, usize)> {
        let mut out = Vec::new();
        fn walk(doc: &Document, parent: Option<u32>, depth: usize, out: &mut Vec<(u32, usize)>) {
            for c in doc.configurations.iter().filter(|c| c.parent == parent) {
                // Guard against a damaged file with a parent cycle.
                if out.iter().any(|(id, _)| *id == c.id) {
                    continue;
                }
                out.push((c.id, depth));
                walk(doc, Some(c.id), depth + 1, out);
            }
        }
        walk(self, None, 0, &mut out);
        // Configurations whose parent is missing (a damaged file) still get listed.
        for c in &self.configurations {
            if !out.iter().any(|(id, _)| *id == c.id) {
                out.push((c.id, 0));
            }
        }
        out
    }

    /// Checks a name for a configuration (`except` is the one being renamed).
    pub fn check_configuration_name(&self, name: &str, except: Option<u32>) -> Result<(), ConfigError> {
        if name.trim().is_empty() {
            return Err(ConfigError::EmptyName);
        }
        if name.starts_with(' ') || name.ends_with(' ') {
            return Err(ConfigError::OuterSpace);
        }
        if let Some(c) = name.chars().find(|c| matches!(c, '/' | '@')) {
            return Err(ConfigError::BadCharacter(c));
        }
        if self.configurations.iter().any(|c| c.name == name && Some(c.id) != except) {
            return Err(ConfigError::DuplicateName(name.to_string()));
        }
        Ok(())
    }

    /// A name not yet used by a configuration: `base`, or `base2`, `base3`...
    pub fn unused_configuration_name(&self, base: &str) -> String {
        if self.configuration_by_name(base).is_none() {
            return base.to_string();
        }
        (2..)
            .map(|i| format!("{base}{i}"))
            .find(|n| self.configuration_by_name(n).is_none())
            .unwrap_or_else(|| base.to_string())
    }

    fn next_configuration_id(&self) -> u32 {
        self.configurations.iter().map(|c| c.id).max().unwrap_or(0) + 1
    }

    /// Adds a configuration and makes it active. A top-level one starts with the active
    /// configuration's values; a derived one (with `parent`) inherits its parent's.
    pub fn add_configuration(&mut self, name: &str, parent: Option<u32>) -> Result<u32, ConfigError> {
        self.normalize_configurations();
        self.check_configuration_name(name, None)?;
        if let Some(p) = parent {
            if self.configuration(p).is_none() {
                return Err(ConfigError::NotFound(p));
            }
        }
        self.capture_configuration();
        let id = self.next_configuration_id();
        let mut c = Configuration::new(id, name);
        c.parent = parent;
        if parent.is_none() {
            let active = self.active_configuration;
            for i in 0..self.configured.len() {
                if let Some(v) = self.resolved_value(i, active) {
                    self.configured[i].values.insert(id, v);
                }
            }
        }
        self.configurations.push(c);
        self.activate_configuration(id)?;
        Ok(id)
    }

    pub fn rename_configuration(&mut self, id: u32, name: &str) -> Result<(), ConfigError> {
        self.check_configuration_name(name, Some(id))?;
        let c = self.configuration_mut(id).ok_or(ConfigError::NotFound(id))?;
        c.name = name.to_string();
        Ok(())
    }

    /// Deletes a configuration (never a feature). Its derived configurations keep the
    /// values they inherited and move up to its parent.
    pub fn delete_configuration(&mut self, id: u32) -> Result<(), ConfigError> {
        let parent = self.configuration(id).ok_or(ConfigError::NotFound(id))?.parent;
        if id == self.active_configuration {
            return Err(ConfigError::DeleteActive);
        }
        if self.configurations.len() <= 1 {
            return Err(ConfigError::DeleteLast);
        }
        self.capture_configuration();
        let children: Vec<u32> =
            self.configurations.iter().filter(|c| c.parent == Some(id)).map(|c| c.id).collect();
        for i in 0..self.configured.len() {
            let Some(v) = self.resolved_value(i, id) else { continue };
            for &child in &children {
                self.configured[i].values.entry(child).or_insert_with(|| v.clone());
            }
            self.configured[i].values.remove(&id);
        }
        for c in &mut self.configurations {
            if c.parent == Some(id) {
                c.parent = parent;
            }
        }
        // A top-level configuration needs every value; children promoted to the top
        // already have them from the loop above.
        self.configurations.retain(|c| c.id != id);
        Ok(())
    }

    /// Makes a configuration active: records the tree's values into the current one,
    /// then writes the new one's into the tree. The caller rebuilds.
    pub fn activate_configuration(&mut self, id: u32) -> Result<(), ConfigError> {
        if self.configuration(id).is_none() {
            return Err(ConfigError::NotFound(id));
        }
        self.capture_configuration();
        self.active_configuration = id;
        self.apply_configuration();
        Ok(())
    }

    /// The value a parameter has in the tree now.
    pub fn tree_value(&self, param: &ConfigParam) -> Option<ConfigValue> {
        match param {
            ConfigParam::Suppression { feature } => {
                self.feature(*feature).map(|f| ConfigValue::Suppressed(f.suppressed))
            }
            ConfigParam::Dimension { feature, slot } => {
                slot.get(&self.feature(*feature)?.kind).cloned().map(ConfigValue::Dim)
            }
            ConfigParam::Variable { name } => {
                self.variables.iter().find(|v| &v.name == name).map(|v| ConfigValue::Expr(v.expr.clone()))
            }
            ConfigParam::Material => Some(ConfigValue::Material(self.material.clone())),
        }
    }

    /// Writes a value into the tree. Fails if the parameter is gone or the value is the
    /// wrong kind.
    fn set_tree_value(&mut self, param: &ConfigParam, value: &ConfigValue) -> Result<(), ConfigError> {
        match (param, value) {
            (ConfigParam::Suppression { feature }, ConfigValue::Suppressed(s)) => {
                self.feature_mut(*feature).ok_or(ConfigError::NoSuchParameter)?.suppressed = *s;
            }
            (ConfigParam::Dimension { feature, slot }, ConfigValue::Dim(d)) => {
                let f = self.feature_mut(*feature).ok_or(ConfigError::NoSuchParameter)?;
                *slot.get_mut(&mut f.kind).ok_or(ConfigError::NoSuchParameter)? = d.clone();
            }
            (ConfigParam::Variable { name }, ConfigValue::Expr(e)) => {
                let v = self
                    .variables
                    .iter_mut()
                    .find(|v| &v.name == name)
                    .ok_or(ConfigError::NoSuchParameter)?;
                v.expr = e.clone();
            }
            (ConfigParam::Material, ConfigValue::Material(m)) => self.material = m.clone(),
            _ => return Err(ConfigError::WrongValue),
        }
        Ok(())
    }

    /// The value parameter `i` has in configuration `cfg`, following derived
    /// configurations up to the one that sets it. The active configuration's value is
    /// read from the tree, so this is current without a capture.
    pub fn resolved_value(&self, i: usize, cfg: u32) -> Option<ConfigValue> {
        let cp = self.configured.get(i)?;
        let mut cur = Some(cfg);
        let mut steps = 0;
        while let Some(c) = cur {
            if c == self.active_configuration {
                return self.tree_value(&cp.param);
            }
            if let Some(v) = cp.values.get(&c) {
                return Some(v.clone());
            }
            cur = self.configuration(c).and_then(|c| c.parent);
            steps += 1;
            if steps > self.configurations.len() {
                break;
            }
        }
        None
    }

    /// True when a derived configuration sets parameter `i` itself rather than
    /// inheriting it. Always true for a top-level configuration.
    pub fn is_overridden(&self, i: usize, cfg: u32) -> bool {
        match self.configuration(cfg) {
            Some(c) if c.parent.is_some() => {
                self.configured.get(i).is_some_and(|p| p.values.contains_key(&cfg))
            }
            _ => true,
        }
    }

    /// Records the tree's values of configured parameters into the active configuration
    /// and drops parameters whose feature, dimension or variable no longer exists.
    pub fn capture_configuration(&mut self) {
        self.normalize_configurations();
        let active = self.active_configuration;
        let parent = self.configuration(active).and_then(|c| c.parent);
        let mut keep = Vec::with_capacity(self.configured.len());
        for i in 0..self.configured.len() {
            let Some(v) = self.tree_value(&self.configured[i].param) else {
                keep.push(false);
                continue;
            };
            keep.push(true);
            match parent {
                None => {
                    self.configured[i].values.insert(active, v);
                }
                Some(p) => {
                    // A derived configuration records a value only when it differs from
                    // what it inherits, or when it already overrides it.
                    let inherited = self.resolved_value(i, p);
                    if self.configured[i].values.contains_key(&active) || inherited.as_ref() != Some(&v) {
                        self.configured[i].values.insert(active, v);
                    }
                }
            }
        }
        let mut k = keep.into_iter();
        self.configured.retain(|_| k.next().unwrap_or(true));
    }

    /// Writes the active configuration's values into the tree.
    fn apply_configuration(&mut self) {
        let active = self.active_configuration;
        for i in 0..self.configured.len() {
            let param = self.configured[i].param.clone();
            let mut cur = Some(active);
            let mut value = None;
            let mut steps = 0;
            while let Some(c) = cur {
                if let Some(v) = self.configured[i].values.get(&c) {
                    value = Some(v.clone());
                    break;
                }
                cur = self.configuration(c).and_then(|c| c.parent);
                steps += 1;
                if steps > self.configurations.len() {
                    break;
                }
            }
            if let Some(v) = value {
                // A stale parameter is dropped at the next capture.
                let _ = self.set_tree_value(&param, &v);
            }
        }
    }

    /// The index of a configured parameter.
    pub fn configured_index(&self, param: &ConfigParam) -> Option<usize> {
        self.configured.iter().position(|p| &p.param == param)
    }

    /// Configures a parameter: every top-level configuration starts with the value it
    /// has now, and derived configurations inherit it. Returns its index.
    pub fn configure(&mut self, param: ConfigParam) -> Result<usize, ConfigError> {
        self.normalize_configurations();
        if let Some(i) = self.configured_index(&param) {
            return Ok(i);
        }
        let v = self.tree_value(&param).ok_or(ConfigError::NoSuchParameter)?;
        let values =
            self.configurations.iter().filter(|c| c.parent.is_none()).map(|c| (c.id, v.clone())).collect();
        self.configured.push(ConfiguredParam { param, values });
        Ok(self.configured.len() - 1)
    }

    /// Configures a feature's suppression and each of its dimensions.
    pub fn configure_feature(&mut self, id: u32) -> Result<(), ConfigError> {
        let kind = self.feature(id).ok_or(ConfigError::NoSuchParameter)?.kind.clone();
        self.configure(ConfigParam::Suppression { feature: id })?;
        for slot in DimSlot::of(&kind) {
            self.configure(ConfigParam::Dimension { feature: id, slot })?;
        }
        Ok(())
    }

    /// Stops configuring parameter `i`: the active configuration's value (the tree's)
    /// then applies to every configuration.
    pub fn unconfigure(&mut self, i: usize) {
        if i < self.configured.len() {
            self.configured.remove(i);
        }
    }

    /// Sets parameter `i` in configuration `cfg`. `None` links a derived configuration's
    /// value back to its parent. Setting the active configuration changes the tree; the
    /// caller rebuilds.
    pub fn set_configured_value(
        &mut self,
        i: usize,
        cfg: u32,
        value: Option<ConfigValue>,
    ) -> Result<(), ConfigError> {
        let c = self.configuration(cfg).ok_or(ConfigError::NotFound(cfg))?;
        let derived = c.parent.is_some();
        let param = self.configured.get(i).ok_or(ConfigError::NoSuchParameter)?.param.clone();
        if let Some(v) = &value {
            let kind_ok = matches!(
                (&param, v),
                (ConfigParam::Suppression { .. }, ConfigValue::Suppressed(_))
                    | (ConfigParam::Dimension { .. }, ConfigValue::Dim(_))
                    | (ConfigParam::Variable { .. }, ConfigValue::Expr(_))
                    | (ConfigParam::Material, ConfigValue::Material(_))
            );
            if !kind_ok {
                return Err(ConfigError::WrongValue);
            }
        } else if !derived {
            return Err(ConfigError::NotDerived);
        }
        self.capture_configuration();
        let i = self.configured_index(&param).ok_or(ConfigError::NoSuchParameter)?;
        match value {
            Some(v) => {
                self.configured[i].values.insert(cfg, v);
            }
            None => {
                self.configured[i].values.remove(&cfg);
            }
        }
        self.apply_configuration();
        Ok(())
    }

    /// A short label for a configured parameter, e.g. "Boss-Extrude1 depth".
    pub fn param_label(&self, param: &ConfigParam) -> String {
        let fname = |id: u32| self.feature(id).map(|f| f.name.clone()).unwrap_or_else(|| format!("#{id}"));
        match param {
            ConfigParam::Suppression { feature } => format!("{} suppressed", fname(*feature)),
            ConfigParam::Dimension { feature, slot } => format!("{} {}", fname(*feature), slot.label()),
            ConfigParam::Variable { name } => format!("\"{name}\""),
            ConfigParam::Material => "Material".into(),
        }
    }

    /// Keeps a configured global variable when the variable is renamed.
    pub fn rename_configured_variable(&mut self, old: &str, new: &str) {
        for p in &mut self.configured {
            if let ConfigParam::Variable { name } = &mut p.param {
                if name == old {
                    *name = new.to_string();
                }
            }
        }
    }

    /// Every parameter of the part that could be configured and is not yet.
    pub fn configurable_params(&self) -> Vec<ConfigParam> {
        let mut out = vec![ConfigParam::Material];
        out.extend(self.variables.iter().map(|v| ConfigParam::Variable { name: v.name.clone() }));
        for f in &self.features {
            out.push(ConfigParam::Suppression { feature: f.id });
            out.extend(
                DimSlot::of(&f.kind).into_iter().map(|slot| ConfigParam::Dimension { feature: f.id, slot }),
            );
        }
        out.retain(|p| self.configured_index(p).is_none());
        out
    }

    /// Called when a feature has been added while other configurations exist: it is
    /// suppressed in those that suppress new features.
    pub(crate) fn feature_added(&mut self, id: u32) {
        let active = self.active_configuration;
        let suppress: Vec<u32> = self
            .configurations
            .iter()
            .filter(|c| c.id != active && c.suppress_new_features)
            .map(|c| c.id)
            .collect();
        if suppress.is_empty() {
            return;
        }
        let mut values = BTreeMap::new();
        for c in &self.configurations {
            let s = suppress.contains(&c.id);
            // Derived configurations without the option inherit from their parent.
            if c.parent.is_none() || s {
                values.insert(c.id, ConfigValue::Suppressed(s));
            }
        }
        // The active configuration keeps the new feature, even when derived from one
        // that suppresses it.
        values.insert(active, ConfigValue::Suppressed(false));
        self.configured.push(ConfiguredParam { param: ConfigParam::Suppression { feature: id }, values });
    }

    /// Called when features are deleted: their configured parameters go too.
    pub(crate) fn features_deleted(&mut self, ids: &[u32]) {
        self.configured.retain(|p| p.param.feature().is_none_or(|f| !ids.contains(&f)));
    }

    /// The custom properties that apply in a configuration: the file's, with the
    /// configuration's own replacing any of the same name and added after them.
    pub fn properties_for(&self, cfg: u32) -> Vec<CustomProperty> {
        let mut out = self.custom_properties.clone();
        if let Some(c) = self.configuration(cfg) {
            for p in &c.properties {
                match out.iter_mut().find(|q| q.name.eq_ignore_ascii_case(&p.name)) {
                    Some(q) => *q = p.clone(),
                    None => out.push(p.clone()),
                }
            }
        }
        out
    }

    /// Rebuilds the part as it is in another configuration, leaving this document as it
    /// is.
    pub fn rebuild_configuration(&self, id: u32) -> Result<BuildState, ConfigError> {
        let mut d = self.clone();
        d.activate_configuration(id)?;
        Ok(d.rebuild())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::*;
    use anvil_kernel::v2;
    use anvil_sketch::{Constraint, ORIGIN};

    /// A 40 x 20 plate, 10 deep, whose width is the global variable "width". Returns the
    /// document and the extrude's id.
    fn plate() -> (Document, u32) {
        let mut doc = Document::new();
        doc.variables.push(GlobalVariable {
            name: "width".into(),
            expr: "40".into(),
            comment: String::new(),
        });
        let mut sf = SketchFeature::new(PlaneRef::Front);
        let s = &mut sf.sketch;
        let l = s.add_rectangle(v2(0.0, 0.0), v2(40.0, 20.0));
        let Some(anvil_sketch::Geom::Line { a, .. }) = s.entities.get(l[0]).map(|e| e.geom) else {
            unreachable!()
        };
        s.add_constraint(Constraint::Coincident { p: a, q: ORIGIN });
        let w = s.add_constraint(Constraint::Length { line: l[0], value: 40.0 });
        s.add_constraint(Constraint::Length { line: l[1], value: 20.0 });
        sf.set_driven(w, Some("width".into()));
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        let ex = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::Blind,
            depth: Dim::Value(10.0),
            reverse: false,
            op: BodyOp::Boss,
            ..Default::default()
        }));
        (doc, ex)
    }

    fn volume(doc: &mut Document) -> f64 {
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        st.body.mass_properties().volume
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6 * b.abs().max(1.0)
    }

    fn set_depth(doc: &mut Document, ex: u32, d: f64) {
        if let Some(FeatureKind::Extrude(e)) = doc.feature_mut(ex).map(|f| &mut f.kind) {
            e.depth = Dim::Value(d);
        }
    }

    fn cut_hole(doc: &mut Document, ex: u32) -> u32 {
        let top = anvil_kernel::FaceId { feature: ex, index: anvil_kernel::FaceId::END_CAP };
        let mut sf = SketchFeature::new(PlaneRef::Face(top));
        let c = sf.sketch.add_point(v2(20.0, 10.0));
        sf.sketch.add_circle(c, 5.0);
        let sk = doc.add_feature(FeatureKind::Sketch(sf));
        doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: sk,
            end: EndCondition::ThroughAll,
            depth: Dim::Value(0.0),
            reverse: false,
            op: BodyOp::Cut,
            ..Default::default()
        }))
    }

    #[test]
    fn new_part_has_a_default_configuration() {
        let doc = Document::new();
        assert_eq!(doc.configurations.len(), 1);
        assert_eq!(doc.active_configuration_name(), "Default");
        assert!(!doc.uses_configurations());
    }

    #[test]
    fn configured_depth_differs_per_configuration() {
        let (mut doc, ex) = plate();
        assert!(close(volume(&mut doc), 8000.0));
        doc.configure(ConfigParam::Dimension { feature: ex, slot: DimSlot::Depth }).unwrap();
        let default = doc.active_configuration;
        let long = doc.add_configuration("Long", None).unwrap();
        assert_eq!(doc.active_configuration, long, "a new configuration is activated");
        // An ordinary edit changes only the active configuration.
        set_depth(&mut doc, ex, 25.0);
        assert!(close(volume(&mut doc), 20000.0));
        doc.activate_configuration(default).unwrap();
        assert!(close(volume(&mut doc), 8000.0));
        // Rebuilding another configuration leaves the document alone.
        let st = doc.rebuild_configuration(long).unwrap();
        assert!(close(st.body.mass_properties().volume, 20000.0));
        assert_eq!(doc.active_configuration, default);
        assert!(close(volume(&mut doc), 8000.0));
    }

    #[test]
    fn unconfigured_edits_apply_to_every_configuration() {
        let (mut doc, ex) = plate();
        let default = doc.active_configuration;
        doc.add_configuration("Other", None).unwrap();
        set_depth(&mut doc, ex, 5.0);
        doc.activate_configuration(default).unwrap();
        assert!(close(volume(&mut doc), 4000.0));
    }

    #[test]
    fn unconfigure_keeps_the_active_value_everywhere() {
        let (mut doc, ex) = plate();
        let i = doc.configure(ConfigParam::Dimension { feature: ex, slot: DimSlot::Depth }).unwrap();
        let default = doc.active_configuration;
        let other = doc.add_configuration("Other", None).unwrap();
        set_depth(&mut doc, ex, 30.0);
        doc.activate_configuration(default).unwrap();
        doc.unconfigure(i);
        assert!(doc.configured.is_empty());
        doc.activate_configuration(other).unwrap();
        assert!(close(volume(&mut doc), 8000.0));
    }

    #[test]
    fn suppression_and_variables_and_material() {
        let (mut doc, ex) = plate();
        let cut = cut_hole(&mut doc, ex);
        let hole = std::f64::consts::PI * 25.0 * 10.0;
        doc.configure(ConfigParam::Suppression { feature: cut }).unwrap();
        doc.configure(ConfigParam::Variable { name: "width".into() }).unwrap();
        doc.configure(ConfigParam::Material).unwrap();
        let default = doc.active_configuration;
        let wide = doc.add_configuration("Wide solid", None).unwrap();
        doc.feature_mut(cut).unwrap().suppressed = true;
        doc.variables[0].expr = "60".into();
        doc.material = Material::library().into_iter().find(|m| m.name.contains("Steel"));
        assert!(doc.material.is_some());
        assert!(close(volume(&mut doc), 12000.0));

        doc.activate_configuration(default).unwrap();
        let v = volume(&mut doc);
        assert!((v - (8000.0 - hole)).abs() / v < 2e-3, "{v}");
        assert!(doc.material.is_none());
        assert_eq!(doc.variables[0].expr, "40");
        assert!(!doc.feature(cut).unwrap().suppressed);

        // The table reads every configuration's value without switching.
        let w = doc.configured_index(&ConfigParam::Variable { name: "width".into() }).unwrap();
        assert_eq!(doc.resolved_value(w, wide), Some(ConfigValue::Expr("60".into())));
        assert_eq!(doc.resolved_value(w, default), Some(ConfigValue::Expr("40".into())));
    }

    #[test]
    fn derived_configurations_inherit_until_overridden() {
        let (mut doc, ex) = plate();
        let i = doc.configure(ConfigParam::Dimension { feature: ex, slot: DimSlot::Depth }).unwrap();
        let parent = doc.active_configuration;
        let child = doc.add_configuration("Child", Some(parent)).unwrap();
        assert!(!doc.is_overridden(i, child));
        assert!(close(volume(&mut doc), 8000.0));

        // A change in the parent reaches the child.
        doc.activate_configuration(parent).unwrap();
        set_depth(&mut doc, ex, 15.0);
        doc.activate_configuration(child).unwrap();
        assert!(close(volume(&mut doc), 12000.0));
        assert!(!doc.is_overridden(i, child));

        // A change in the child overrides it there only.
        set_depth(&mut doc, ex, 2.0);
        doc.activate_configuration(parent).unwrap();
        assert!(doc.is_overridden(i, child));
        assert!(close(volume(&mut doc), 12000.0));
        set_depth(&mut doc, ex, 20.0);
        doc.activate_configuration(child).unwrap();
        assert!(close(volume(&mut doc), 1600.0));

        // Linking it back to the parent picks the parent's value up again.
        doc.set_configured_value(i, child, None).unwrap();
        assert!(!doc.is_overridden(i, child));
        assert!(close(volume(&mut doc), 16000.0));
        assert_eq!(doc.set_configured_value(i, parent, None), Err(ConfigError::NotDerived));
    }

    #[test]
    fn table_edits_set_values_in_any_configuration() {
        let (mut doc, ex) = plate();
        let i = doc.configure(ConfigParam::Dimension { feature: ex, slot: DimSlot::Depth }).unwrap();
        let a = doc.active_configuration;
        let b = doc.add_configuration("B", None).unwrap();
        doc.activate_configuration(a).unwrap();
        doc.set_configured_value(i, b, Some(ConfigValue::Dim(Dim::Value(3.0)))).unwrap();
        assert!(close(volume(&mut doc), 8000.0), "the active configuration is unchanged");
        doc.set_configured_value(i, a, Some(ConfigValue::Dim(Dim::Expr("width / 8".into())))).unwrap();
        assert!(close(volume(&mut doc), 4000.0), "setting the active one changes the tree");
        assert_eq!(
            doc.set_configured_value(i, a, Some(ConfigValue::Suppressed(true))),
            Err(ConfigError::WrongValue)
        );
        doc.activate_configuration(b).unwrap();
        assert!(close(volume(&mut doc), 2400.0));
    }

    #[test]
    fn new_features_are_suppressed_where_asked() {
        let (mut doc, ex) = plate();
        let default = doc.active_configuration;
        let blank = doc.add_configuration("Blank", None).unwrap();
        let keeps = doc.add_configuration("Keeps", None).unwrap();
        doc.configuration_mut(blank).unwrap().suppress_new_features = true;
        doc.activate_configuration(default).unwrap();
        let cut = cut_hole(&mut doc, ex);
        let hole = std::f64::consts::PI * 25.0 * 10.0;
        let v = volume(&mut doc);
        assert!((v - (8000.0 - hole)).abs() / v < 2e-3, "{v}");
        let in_blank = doc.rebuild_configuration(blank).unwrap().body.mass_properties().volume;
        // The cut's sketch is a new feature too, so it is suppressed as well.
        assert!(close(in_blank, 8000.0), "{in_blank}");
        let in_keeps = doc.rebuild_configuration(keeps).unwrap().body.mass_properties().volume;
        assert!((in_keeps - (8000.0 - hole)).abs() / in_keeps < 2e-3);
        assert!(doc.configured_index(&ConfigParam::Suppression { feature: cut }).is_some());

        // Deleting the feature drops its configured parameters.
        doc.delete_feature(cut);
        assert!(doc.configured_index(&ConfigParam::Suppression { feature: cut }).is_none());
    }

    #[test]
    fn names_are_checked() {
        let mut doc = Document::new();
        assert_eq!(doc.add_configuration("", None), Err(ConfigError::EmptyName));
        assert_eq!(doc.add_configuration(" A", None), Err(ConfigError::OuterSpace));
        assert_eq!(doc.add_configuration("A/B", None), Err(ConfigError::BadCharacter('/')));
        assert_eq!(doc.add_configuration("A@B", None), Err(ConfigError::BadCharacter('@')));
        assert_eq!(doc.add_configuration("Default", None), Err(ConfigError::DuplicateName("Default".into())));
        let a = doc.add_configuration("A", None).unwrap();
        assert_eq!(doc.rename_configuration(a, "Default"), Err(ConfigError::DuplicateName("Default".into())));
        doc.rename_configuration(a, "A").unwrap();
        doc.rename_configuration(a, "Short").unwrap();
        assert_eq!(doc.active_configuration_name(), "Short");
        assert_eq!(doc.unused_configuration_name("Default"), "Default2");
    }

    #[test]
    fn deleting_a_configuration() {
        let (mut doc, ex) = plate();
        let i = doc.configure(ConfigParam::Dimension { feature: ex, slot: DimSlot::Depth }).unwrap();
        let default = doc.active_configuration;
        assert_eq!(doc.delete_configuration(default), Err(ConfigError::DeleteActive));
        let mid = doc.add_configuration("Mid", None).unwrap();
        set_depth(&mut doc, ex, 12.0);
        let child = doc.add_configuration("Child", Some(mid)).unwrap();
        doc.activate_configuration(default).unwrap();
        doc.delete_configuration(mid).unwrap();
        let c = doc.configuration(child).unwrap();
        assert_eq!(c.parent, None, "the child moves up");
        assert_eq!(doc.resolved_value(i, child), Some(ConfigValue::Dim(Dim::Value(12.0))));
        assert!(doc.configured[i].values.keys().all(|&k| k != mid));
        doc.delete_configuration(child).unwrap();
        assert_eq!(doc.delete_configuration(default), Err(ConfigError::DeleteActive));
        assert_eq!(doc.configurations.len(), 1);
    }

    #[test]
    fn saving_records_the_active_values_and_the_format_version() {
        let plain = Document::new();
        let v: serde_json::Value = serde_json::from_str(&plain.to_json().unwrap()).unwrap();
        assert_eq!(v["format_version"], 1, "a part without configurations stays readable by older builds");

        let (mut doc, ex) = plate();
        doc.configure(ConfigParam::Dimension { feature: ex, slot: DimSlot::Depth }).unwrap();
        let default = doc.active_configuration;
        let deep = doc.add_configuration("Deep", None).unwrap();
        doc.configuration_mut(deep).unwrap().description = "Deep plate".into();
        // Edited and not yet captured: saving must still record it.
        set_depth(&mut doc, ex, 40.0);
        let text = doc.to_json().unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["format_version"], 2);
        let mut back = Document::from_json(&text).unwrap();
        assert_eq!(back.active_configuration, deep);
        assert_eq!(back.configuration(deep).unwrap().description, "Deep plate");
        assert!(close(volume(&mut back), 32000.0));
        back.activate_configuration(default).unwrap();
        assert!(close(volume(&mut back), 8000.0));

        // A file from before configurations opens with one "Default".
        let mut old: serde_json::Value = serde_json::from_str(&plain.to_json().unwrap()).unwrap();
        let o = old.as_object_mut().unwrap();
        o.remove("configurations");
        o.remove("active_configuration");
        let d = Document::from_json(&old.to_string()).unwrap();
        assert_eq!(d.configurations.len(), 1);
        assert_eq!(d.active_configuration_name(), "Default");
        assert_eq!(d.active_configuration, d.configurations[0].id);
    }

    #[test]
    fn configuration_properties_override_file_properties() {
        let mut doc = Document::new();
        doc.custom_properties[0].value = "Plate".into(); // Description
        let id = doc.active_configuration;
        let c = doc.configuration_mut(id).unwrap();
        c.properties.push(CustomProperty {
            name: "description".into(),
            kind: PropertyKind::Text,
            value: "Long plate".into(),
        });
        c.properties.push(CustomProperty {
            name: "Cost".into(),
            kind: PropertyKind::Number,
            value: "12.5".into(),
        });
        let p = doc.properties_for(id);
        assert_eq!(p.len(), 6);
        assert_eq!(p[0].value, "Long plate");
        assert_eq!(p[5].name, "Cost");
    }

    #[test]
    fn configure_feature_takes_suppression_and_dimensions() {
        let (mut doc, ex) = plate();
        doc.configure_feature(ex).unwrap();
        let labels: Vec<String> = doc.configured.iter().map(|p| doc.param_label(&p.param)).collect();
        assert_eq!(labels, ["Boss-Extrude1 suppressed", "Boss-Extrude1 depth"]);
        assert!(!doc.configurable_params().contains(&ConfigParam::Suppression { feature: ex }));
        assert!(doc.configurable_params().contains(&ConfigParam::Material));
        assert!(doc.configure(ConfigParam::Suppression { feature: 999 }).is_err());
    }
}
