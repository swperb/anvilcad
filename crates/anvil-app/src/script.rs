//! Lua configuration and scripting: `init.lua` sets options, theme colours, key bindings and
//! user commands, and scripts and commands drive the app through the `anvil` table. See
//! docs/SCRIPTING.md for the user-facing reference.
//!
//! `init.lua` only declares things (it runs when the app starts and whenever the file
//! changes). Calls that change the model, such as `anvil.run` or `anvil.extrude`, exist only
//! while a command, key binding or script runs: they are scoped functions over a [`Host`],
//! the app, so the engine never holds on to it.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::Color32;
use mlua::{Function, HookTriggers, Lua, LuaSerdeExt, Table, Value, VmState};

use crate::keymap::{self, Chord};

/// The `anvil` table before `init.lua` runs.
const PRELUDE: &str = r#"
anvil = {
  opt = {},
  theme = { light = {}, dark = {} },
  leader = nil,
  _keys = {},
  _commands = {},
}
function anvil.key(keys, action, description)
  if type(keys) ~= "string" then error("anvil.key: the first argument is the keys, such as \"ctrl+e\"", 2) end
  if type(action) ~= "string" and type(action) ~= "function" then
    error("anvil.key: the second argument is a command name or a function", 2)
  end
  table.insert(anvil._keys, { keys = keys, action = action, description = description or "" })
end
function anvil.command(name, fn, opts)
  if type(name) ~= "string" or name == "" then error("anvil.command: the first argument is the command's name", 2) end
  if type(fn) ~= "function" then error("anvil.command: the second argument is a function", 2) end
  opts = opts or {}
  table.insert(anvil._commands, { name = name, fn = fn, description = opts.description or "" })
end
"#;

/// Options `anvil.opt` accepts, with what each takes (for error messages and the docs).
pub const OPTIONS: &[(&str, &str)] = &[
    ("theme", "\"light\" or \"dark\""),
    ("ui_scale", "a number from 0.5 to 3, such as 1.25"),
    ("font", "the path of a .ttf or .otf file for the interface"),
    ("mono_font", "the path of a .ttf or .otf file for numbers and code"),
    ("material", "a library material for new parts, such as \"6061 Alloy\""),
    ("template", "a part template (.anvprtdot) new parts start from"),
    ("open", "what opens at launch with no file given: a path, or \"sample\""),
    ("view", "the starting view: front, back, left, right, top, bottom or isometric"),
    ("edges", "true to draw model edges (Shaded with edges), false for shaded only"),
    ("snap_px", "the sketch snap and pick radius in pixels, 2 to 40"),
];

/// Colours `anvil.theme.light` and `anvil.theme.dark` accept; the names are [`crate::theme::Palette`]'s.
pub const THEME_COLORS: &[&str] = &[
    "chrome",
    "panel",
    "inset",
    "border",
    "text",
    "text_weak",
    "hover",
    "accent",
    "on_accent",
    "select_fill",
    "select",
    "error",
    "warning",
    "ok",
    "view_top",
    "view_bottom",
    "float_fill",
];

/// Model colours `anvil.theme` accepts directly (both themes).
pub const MODEL_COLORS: &[&str] = &["body", "hover_face", "selected_face", "edge"];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    pub dark: Option<bool>,
    pub ui_scale: Option<f32>,
    pub font: Option<PathBuf>,
    pub mono_font: Option<PathBuf>,
    pub material: Option<String>,
    pub template: Option<PathBuf>,
    /// A path, or `"sample"`.
    pub open: Option<String>,
    pub view: Option<String>,
    pub edges: Option<bool>,
    pub snap_px: Option<f64>,
}

/// Colour overrides from `anvil.theme`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThemeOverrides {
    pub light: Vec<(String, Color32)>,
    pub dark: Vec<(String, Color32)>,
    /// body, hover_face, selected_face, edge, as linear 0..1 RGB.
    pub model: Vec<(String, [f32; 3])>,
}

impl ThemeOverrides {
    pub fn is_empty(&self) -> bool {
        self.light.is_empty() && self.dark.is_empty() && self.model.is_empty()
    }
}

/// What a binding or command does.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// A built-in command, by name.
    Builtin(String),
    /// A Lua function: the index in `anvil._keys`.
    Lua(usize),
}

#[derive(Clone, Debug)]
pub struct Binding {
    pub keys: String,
    pub sequence: Vec<Chord>,
    pub action: Action,
    pub description: String,
}

#[derive(Clone, Debug)]
pub struct UserCommand {
    pub name: String,
    pub description: String,
    /// Its index in `anvil._commands`.
    index: usize,
}

/// What `init.lua` declared.
#[derive(Clone, Debug, Default)]
pub struct Config {
    pub options: Options,
    pub theme: ThemeOverrides,
    pub bindings: Vec<Binding>,
    pub commands: Vec<UserCommand>,
}

/// Where `init.lua` lives: `$ANVILCAD_CONFIG` if set; otherwise `%APPDATA%\AnvilCAD\init.lua`
/// on Windows and `$XDG_CONFIG_HOME/anvilcad/init.lua` (default `~/.config/anvilcad/init.lua`)
/// on Linux and macOS. `env` reads an environment variable.
pub fn config_path(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let set = |k: &str| env(k).filter(|v| !v.is_empty());
    if let Some(p) = set("ANVILCAD_CONFIG") {
        return Some(PathBuf::from(p));
    }
    if cfg!(windows) {
        return set("APPDATA").map(|d| Path::new(&d).join("AnvilCAD").join("init.lua"));
    }
    let base = set("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| set("HOME").map(|h| Path::new(&h).join(".config")))?;
    Some(base.join("anvilcad").join("init.lua"))
}

/// The default config path for this process.
pub fn default_config_path() -> Option<PathBuf> {
    config_path(|k| std::env::var(k).ok())
}

/// What scripts can do to the app. The app implements it; tests can use a fake.
pub trait Host {
    /// Runs a built-in command by its name (see `docs/SCRIPTING.md`).
    fn run_builtin(&mut self, name: &str) -> Result<(), String>;
    /// The app's state, as the headless harness's `state` command reports it.
    fn state(&self) -> serde_json::Value;
    /// Starts a new part. Refuses if there are unsaved changes, unless `force`.
    fn new_part(&mut self, force: bool) -> Result<(), String>;
    /// Opens a file. Refuses if there are unsaved changes, unless `force`.
    fn open(&mut self, path: &Path, force: bool) -> Result<(), String>;
    fn save(&mut self, path: Option<&Path>) -> Result<(), String>;
    fn export(&mut self, path: &Path) -> Result<(), String>;
    /// Sets (or adds) a global variable, then rebuilds.
    fn set_variable(&mut self, name: &str, expr: &str) -> Result<(), String>;
    fn set_material(&mut self, name: &str) -> Result<(), String>;
    /// Adds a sketch and returns its feature id.
    fn add_sketch(&mut self, spec: &SketchSpec) -> Result<u32, String>;
    /// Adds an extrusion and returns its feature id.
    fn add_extrude(&mut self, spec: &ExtrudeSpec) -> Result<u32, String>;
    fn set_status(&mut self, text: &str);
}

/// A sketch from `anvil.sketch{...}`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SketchSpec {
    /// `front`, `top`, `right`, or a feature's end or start face.
    pub plane: PlaneSpec,
    /// Corner rectangles: x0, y0, x1, y1.
    pub rects: Vec<[f64; 4]>,
    /// Circles: centre x, centre y, radius.
    pub circles: Vec<[f64; 3]>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum PlaneSpec {
    #[default]
    Front,
    Top,
    Right,
    /// The end (or, with `start`, the start) cap of an extrusion.
    Face {
        feature: u32,
        start: bool,
    },
}

/// An extrusion from `anvil.extrude{...}`.
#[derive(Clone, Debug, PartialEq)]
pub struct ExtrudeSpec {
    pub sketch: u32,
    pub depth: Depth,
    pub cut: bool,
    pub reverse: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Depth {
    Value(f64),
    /// An equation, such as `"thickness * 2"`.
    Expr(String),
    ThroughAll,
}

/// The Lua state and what `init.lua` declared.
pub struct Engine {
    lua: Lua,
    pub config: Config,
    /// How long one command or script may run before it is stopped.
    pub time_limit: Duration,
}

fn lua_err(e: mlua::Error) -> String {
    // Keep the message and the Lua traceback's first useful line, without mlua's wrapping.
    let text = match &e {
        mlua::Error::CallbackError { cause, .. } => return lua_err((**cause).clone()),
        _ => e.to_string(),
    };
    let first = text.lines().next().unwrap_or("");
    first.strip_prefix("runtime error: ").unwrap_or(first).to_string()
}

impl Engine {
    /// Runs `source` (named `name` in messages) as `init.lua`. `dir`, if given, is searched by
    /// `require` (`dir/?.lua` and `dir/lua/?.lua`).
    pub fn load(source: &str, name: &str, dir: Option<&Path>) -> Result<Engine, String> {
        let lua = Lua::new();
        if let Some(dir) = dir {
            let d = dir.to_string_lossy().replace('\\', "/");
            let package: Table = lua.globals().get("package").map_err(lua_err)?;
            let path: String = package.get("path").map_err(lua_err)?;
            package
                .set("path", format!("{d}/?.lua;{d}/lua/?.lua;{d}/lua/?/init.lua;{path}"))
                .map_err(lua_err)?;
        }
        lua.load(PRELUDE).set_name("prelude").exec().map_err(lua_err)?;
        let mut engine = Engine { lua, config: Config::default(), time_limit: Duration::from_secs(10) };
        engine.guarded(|lua| lua.load(source).set_name(format!("@{name}")).exec())?;
        engine.config = engine.read_config()?;
        Ok(engine)
    }

    /// Loads a config file. A missing file is an empty config.
    pub fn load_file(path: &Path) -> Result<Engine, String> {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(format!("could not read {}: {e}", path.display())),
        };
        Engine::load(&source, &path.display().to_string(), path.parent())
    }

    /// Runs `f` with a time limit, so a runaway loop stops instead of freezing the app.
    fn guarded<R>(&self, f: impl FnOnce(&Lua) -> mlua::Result<R>) -> Result<R, String> {
        let start = Instant::now();
        let limit = self.time_limit;
        self.lua
            .set_hook(HookTriggers::new().every_nth_instruction(20_000), move |_, _| {
                if start.elapsed() > limit {
                    Err(mlua::Error::RuntimeError(format!(
                        "stopped after {} s: the script ran too long",
                        limit.as_secs_f32()
                    )))
                } else {
                    Ok(VmState::Continue)
                }
            })
            .map_err(lua_err)?;
        let r = f(&self.lua).map_err(lua_err);
        self.lua.remove_hook();
        r
    }

    fn anvil(&self) -> Result<Table, String> {
        self.lua.globals().get("anvil").map_err(|_| "init.lua replaced the `anvil` table".to_string())
    }

    fn read_config(&self) -> Result<Config, String> {
        let anvil = self.anvil()?;
        let opt: Table = anvil.get("opt").map_err(|_| "anvil.opt must be a table".to_string())?;
        let options = read_options(&opt)?;
        let theme = read_theme(&anvil.get("theme").map_err(|_| "anvil.theme must be a table".to_string())?)?;

        let leader = match anvil.get::<Value>("leader").map_err(lua_err)? {
            Value::Nil => Vec::new(),
            Value::String(s) => {
                keymap::parse_sequence(&s.to_string_lossy(), &[]).map_err(|e| format!("anvil.leader: {e}"))?
            }
            _ => return Err("anvil.leader must be a string, such as \"space\"".into()),
        };
        let mut bindings = Vec::new();
        let keys: Table = anvil.get("_keys").map_err(lua_err)?;
        for (i, entry) in keys.sequence_values::<Table>().enumerate() {
            let entry = entry.map_err(lua_err)?;
            let text: String = entry.get("keys").map_err(lua_err)?;
            let sequence = keymap::parse_sequence(&text, &leader).map_err(|e| format!("anvil.key: {e}"))?;
            let action = match entry.get::<Value>("action").map_err(lua_err)? {
                Value::String(s) => {
                    let name = s.to_string_lossy();
                    if !crate::app::BUILTIN_COMMANDS.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name))
                        && !self.has_user_command(&name)?
                    {
                        return Err(format!(
                            "anvil.key(\"{text}\", \"{name}\"): no command is called \"{name}\" \
                             (see the command palette, Ctrl+Shift+P, for the names)"
                        ));
                    }
                    Action::Builtin(name)
                }
                _ => Action::Lua(i + 1),
            };
            let description: String = entry.get("description").map_err(lua_err)?;
            bindings.push(Binding { keys: text, sequence, action, description });
        }
        let mut commands = Vec::new();
        let list: Table = anvil.get("_commands").map_err(lua_err)?;
        for (i, entry) in list.sequence_values::<Table>().enumerate() {
            let entry = entry.map_err(lua_err)?;
            commands.push(UserCommand {
                name: entry.get("name").map_err(lua_err)?,
                description: entry.get("description").map_err(lua_err)?,
                index: i + 1,
            });
        }
        Ok(Config { options, theme, bindings, commands })
    }

    fn has_user_command(&self, name: &str) -> Result<bool, String> {
        let list: Table = self.anvil()?.get("_commands").map_err(lua_err)?;
        for entry in list.sequence_values::<Table>() {
            let n: String = entry.map_err(lua_err)?.get("name").map_err(lua_err)?;
            if n.eq_ignore_ascii_case(name) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The user command called `name`, if any (case-insensitive).
    pub fn user_command(&self, name: &str) -> Option<&UserCommand> {
        self.config.commands.iter().find(|c| c.name.eq_ignore_ascii_case(name))
    }

    /// Runs a user command.
    pub fn run_command(&self, cmd: &UserCommand, host: &mut dyn Host) -> Result<(), String> {
        let entry: Table =
            self.anvil()?.get::<Table>("_commands").and_then(|t| t.get(cmd.index)).map_err(lua_err)?;
        let f: Function = entry.get("fn").map_err(lua_err)?;
        self.with_host(host, |_| f.call::<()>(()))
    }

    /// Runs the Lua function of a key binding.
    pub fn run_binding(&self, index: usize, host: &mut dyn Host) -> Result<(), String> {
        let entry: Table = self.anvil()?.get::<Table>("_keys").and_then(|t| t.get(index)).map_err(lua_err)?;
        let f: Function = entry.get("action").map_err(lua_err)?;
        self.with_host(host, |_| f.call::<()>(()))
    }

    /// Runs a script (a chunk of Lua) with the whole `anvil` API.
    pub fn run_script(&self, source: &str, name: &str, host: &mut dyn Host) -> Result<(), String> {
        self.with_host(host, |lua| lua.load(source).set_name(format!("@{name}")).exec())
    }

    /// Calls `f` with the model-changing half of the API installed over `host`, then removes it.
    fn with_host(&self, host: &mut dyn Host, f: impl FnOnce(&Lua) -> mlua::Result<()>) -> Result<(), String> {
        let host = RefCell::new(host);
        let anvil = self.anvil()?;
        let names = [
            "run", "state", "new", "open", "save", "export", "var", "material", "sketch", "extrude", "status",
        ];
        let result = self.guarded(|lua| {
            lua.scope(|scope| {
                let h = &host;
                let fail = |e: String| mlua::Error::RuntimeError(e);
                anvil.set(
                    "run",
                    scope.create_function(move |_, name: String| {
                        h.borrow_mut().run_builtin(&name).map_err(fail)
                    })?,
                )?;
                anvil
                    .set("state", scope.create_function(move |lua, ()| lua.to_value(&h.borrow().state()))?)?;
                anvil.set(
                    "new",
                    scope.create_function(move |_, opts: Option<Table>| {
                        let force = force_flag(opts.as_ref())?;
                        h.borrow_mut().new_part(force).map_err(fail)
                    })?,
                )?;
                anvil.set(
                    "open",
                    scope.create_function(move |_, (p, opts): (String, Option<Table>)| {
                        let force = force_flag(opts.as_ref())?;
                        h.borrow_mut().open(Path::new(&p), force).map_err(fail)
                    })?,
                )?;
                anvil.set(
                    "save",
                    scope.create_function(move |_, p: Option<String>| {
                        h.borrow_mut().save(p.as_deref().map(Path::new)).map_err(fail)
                    })?,
                )?;
                anvil.set(
                    "export",
                    scope.create_function(move |_, p: String| {
                        h.borrow_mut().export(Path::new(&p)).map_err(fail)
                    })?,
                )?;
                anvil.set(
                    "var",
                    scope.create_function(move |_, (name, expr): (String, Value)| {
                        let expr = match expr {
                            Value::Integer(i) => i.to_string(),
                            Value::Number(n) => n.to_string(),
                            Value::String(s) => s.to_string_lossy(),
                            _ => {
                                return Err(fail(
                                    "anvil.var(name, value): the value is a number or an equation".into(),
                                ))
                            }
                        };
                        h.borrow_mut().set_variable(&name, &expr).map_err(fail)
                    })?,
                )?;
                anvil.set(
                    "material",
                    scope.create_function(move |_, name: String| {
                        h.borrow_mut().set_material(&name).map_err(fail)
                    })?,
                )?;
                anvil.set(
                    "sketch",
                    scope.create_function(move |_, t: Table| {
                        let spec = read_sketch(&t).map_err(fail)?;
                        h.borrow_mut().add_sketch(&spec).map_err(fail)
                    })?,
                )?;
                anvil.set(
                    "extrude",
                    scope.create_function(move |_, t: Table| {
                        let spec = read_extrude(&t).map_err(fail)?;
                        h.borrow_mut().add_extrude(&spec).map_err(fail)
                    })?,
                )?;
                anvil.set(
                    "status",
                    scope.create_function(move |_, text: String| {
                        h.borrow_mut().set_status(&text);
                        Ok(())
                    })?,
                )?;
                f(lua)
            })
        });
        // Calls after the scope ends would fail anyway; make the message say why.
        for n in names {
            let _ = anvil.set(n, Value::Nil);
        }
        result
    }
}

/// `{ force = true }` in `anvil.new` and `anvil.open`.
fn force_flag(opts: Option<&Table>) -> mlua::Result<bool> {
    Ok(match opts {
        Some(t) => t.get::<Option<bool>>("force")?.unwrap_or(false),
        None => false,
    })
}

fn read_options(opt: &Table) -> Result<Options, String> {
    let mut o = Options::default();
    for pair in opt.pairs::<String, Value>() {
        let (key, value) =
            pair.map_err(|_| "anvil.opt keys are names, such as anvil.opt.theme".to_string())?;
        let bad = |what: &str| format!("anvil.opt.{key} must be {what}");
        let string = |v: &Value| match v {
            Value::String(s) => Some(s.to_string_lossy()),
            _ => None,
        };
        let number = |v: &Value| match v {
            Value::Integer(i) => Some(*i as f64),
            Value::Number(n) => Some(*n),
            _ => None,
        };
        let help = OPTIONS.iter().find(|(n, _)| *n == key).map(|(_, h)| *h);
        let Some(help) = help else {
            let known: Vec<&str> = OPTIONS.iter().map(|(n, _)| *n).collect();
            return Err(format!("unknown option anvil.opt.{key} (options: {})", known.join(", ")));
        };
        match key.as_str() {
            "theme" => {
                o.dark = Some(match string(&value).as_deref() {
                    Some("dark") => true,
                    Some("light") => false,
                    _ => return Err(bad(help)),
                })
            }
            "ui_scale" => {
                let s = number(&value).filter(|s| (0.5..=3.0).contains(s)).ok_or_else(|| bad(help))?;
                o.ui_scale = Some(s as f32);
            }
            "font" => o.font = Some(PathBuf::from(expand(&string(&value).ok_or_else(|| bad(help))?))),
            "mono_font" => {
                o.mono_font = Some(PathBuf::from(expand(&string(&value).ok_or_else(|| bad(help))?)))
            }
            "material" => o.material = Some(string(&value).ok_or_else(|| bad(help))?),
            "template" => o.template = Some(PathBuf::from(expand(&string(&value).ok_or_else(|| bad(help))?))),
            "open" => o.open = Some(expand(&string(&value).ok_or_else(|| bad(help))?)),
            "view" => {
                let v = string(&value).map(|v| v.to_ascii_lowercase()).ok_or_else(|| bad(help))?;
                if !["front", "back", "left", "right", "top", "bottom", "isometric"].contains(&v.as_str()) {
                    return Err(bad(help));
                }
                o.view = Some(v);
            }
            "edges" => match value {
                Value::Boolean(b) => o.edges = Some(b),
                _ => return Err(bad(help)),
            },
            "snap_px" => {
                o.snap_px =
                    Some(number(&value).filter(|s| (2.0..=40.0).contains(s)).ok_or_else(|| bad(help))?)
            }
            _ => unreachable!("every name in OPTIONS is handled"),
        }
    }
    Ok(o)
}

/// Expands a leading `~` to the home directory.
fn expand(p: &str) -> String {
    match p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")) {
        Some(rest) => match std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            Ok(home) => Path::new(&home).join(rest).to_string_lossy().into_owned(),
            Err(_) => p.to_string(),
        },
        None => p.to_string(),
    }
}

/// Parses `#rgb`, `#rrggbb` or `#rrggbbaa`.
pub fn parse_color(text: &str) -> Option<Color32> {
    let hex = text.strip_prefix('#')?;
    let digits: Vec<u8> = hex.chars().map(|c| c.to_digit(16).map(|d| d as u8)).collect::<Option<_>>()?;
    match digits.len() {
        3 => Some(Color32::from_rgb(digits[0] * 17, digits[1] * 17, digits[2] * 17)),
        6 | 8 => {
            let b = |i: usize| digits[i] * 16 + digits[i + 1];
            let a = if digits.len() == 8 { b(6) } else { 255 };
            Some(Color32::from_rgba_unmultiplied(b(0), b(2), b(4), a))
        }
        _ => None,
    }
}

fn read_theme(theme: &Table) -> Result<ThemeOverrides, String> {
    let mut out = ThemeOverrides::default();
    for pair in theme.pairs::<String, Value>() {
        let (key, value) = pair.map_err(|_| "anvil.theme keys are names".to_string())?;
        match key.as_str() {
            "light" | "dark" => {
                let Value::Table(t) = value else { return Err(format!("anvil.theme.{key} must be a table")) };
                for pair in t.pairs::<String, String>() {
                    let (name, color) = pair
                        .map_err(|_| format!("anvil.theme.{key} maps colour names to \"#rrggbb\" strings"))?;
                    if !THEME_COLORS.contains(&name.as_str()) {
                        return Err(format!(
                            "unknown colour anvil.theme.{key}.{name} (colours: {})",
                            THEME_COLORS.join(", ")
                        ));
                    }
                    let c = parse_color(&color).ok_or_else(|| {
                        format!("anvil.theme.{key}.{name} = \"{color}\" is not a #rrggbb colour")
                    })?;
                    if key == "light" { &mut out.light } else { &mut out.dark }.push((name, c));
                }
            }
            name if MODEL_COLORS.contains(&name) => {
                let Value::String(s) = value else {
                    return Err(format!("anvil.theme.{name} must be \"#rrggbb\""));
                };
                let c = parse_color(&s.to_string_lossy())
                    .ok_or_else(|| format!("anvil.theme.{name} is not a #rrggbb colour"))?;
                out.model.push((name.to_string(), [c.r(), c.g(), c.b()].map(|v| v as f32 / 255.0)));
            }
            other => {
                return Err(format!(
                    "unknown anvil.theme.{other} (use anvil.theme.light.<colour>, anvil.theme.dark.<colour>, \
                     or one of {})",
                    MODEL_COLORS.join(", ")
                ))
            }
        }
    }
    // Lua tables have no order; sort so the result does not depend on hashing.
    out.light.sort_by(|a, b| a.0.cmp(&b.0));
    out.dark.sort_by(|a, b| a.0.cmp(&b.0));
    out.model.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn numbers<const N: usize>(t: &Table, what: &str) -> Result<[f64; N], String> {
    let v: Vec<f64> = t
        .sequence_values::<f64>()
        .collect::<mlua::Result<_>>()
        .map_err(|_| format!("{what}: numbers only"))?;
    v.try_into().map_err(|v: Vec<f64>| format!("{what}: {N} numbers, not {}", v.len()))
}

fn read_sketch(t: &Table) -> Result<SketchSpec, String> {
    let plane = match t.get::<Value>("plane").map_err(lua_err)? {
        Value::Nil => PlaneSpec::Front,
        Value::String(p) => match p.to_string_lossy().to_ascii_lowercase().as_str() {
            "front" => PlaneSpec::Front,
            "top" => PlaneSpec::Top,
            "right" => PlaneSpec::Right,
            other => return Err(format!("anvil.sketch: plane \"{other}\" is not front, top or right")),
        },
        Value::Table(f) => {
            let feature: u32 =
                f.get("on").map_err(|_| "anvil.sketch: plane = { on = <feature id> }".to_string())?;
            let face: Option<String> = f.get("face").map_err(lua_err)?;
            PlaneSpec::Face { feature, start: face.as_deref() == Some("start") }
        }
        _ => return Err("anvil.sketch: plane is \"front\", \"top\", \"right\" or { on = id }".into()),
    };
    let mut s = SketchSpec { plane, ..SketchSpec::default() };
    if let Some(rects) = t.get::<Option<Table>>("rects").map_err(lua_err)? {
        for r in rects.sequence_values::<Table>() {
            s.rects.push(numbers(&r.map_err(lua_err)?, "anvil.sketch rects: {x0, y0, x1, y1}")?);
        }
    }
    if let Some(circles) = t.get::<Option<Table>>("circles").map_err(lua_err)? {
        for c in circles.sequence_values::<Table>() {
            s.circles.push(numbers(&c.map_err(lua_err)?, "anvil.sketch circles: {x, y, radius}")?);
        }
    }
    if s.rects.is_empty() && s.circles.is_empty() {
        return Err("anvil.sketch: give rects = {{x0, y0, x1, y1}, ...} or circles = {{x, y, r}, ...}".into());
    }
    Ok(s)
}

fn read_extrude(t: &Table) -> Result<ExtrudeSpec, String> {
    let sketch: u32 = t.get("sketch").map_err(|_| "anvil.extrude: sketch = <sketch id>".to_string())?;
    let depth = match t.get::<Value>("depth").map_err(lua_err)? {
        Value::Integer(i) => Depth::Value(i as f64),
        Value::Number(n) => Depth::Value(n),
        Value::String(s) if s.to_string_lossy() == "through" => Depth::ThroughAll,
        Value::String(s) => Depth::Expr(s.to_string_lossy()),
        _ => return Err("anvil.extrude: depth is a number, an equation, or \"through\"".into()),
    };
    let flag = |k: &str| t.get::<Option<bool>>(k).map(|b| b.unwrap_or(false)).map_err(lua_err);
    Ok(ExtrudeSpec { sketch, depth, cut: flag("cut")?, reverse: flag("reverse")? })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_follows_platform_conventions() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
        };
        assert_eq!(
            config_path(env(&[("ANVILCAD_CONFIG", "/tmp/x.lua"), ("HOME", "/home/a")])),
            Some(PathBuf::from("/tmp/x.lua"))
        );
        if cfg!(windows) {
            assert_eq!(
                config_path(env(&[("APPDATA", r"C:\Users\a\AppData\Roaming")])),
                Some(Path::new(r"C:\Users\a\AppData\Roaming").join("AnvilCAD").join("init.lua"))
            );
        } else {
            assert_eq!(
                config_path(env(&[("HOME", "/home/a")])),
                Some(PathBuf::from("/home/a/.config/anvilcad/init.lua"))
            );
            assert_eq!(
                config_path(env(&[("HOME", "/home/a"), ("XDG_CONFIG_HOME", "/cfg")])),
                Some(PathBuf::from("/cfg/anvilcad/init.lua"))
            );
            assert_eq!(config_path(env(&[])), None);
        }
    }

    #[test]
    fn reads_options_theme_keys_and_commands() {
        let e = Engine::load(
            r##"
            anvil.opt.theme = "dark"
            anvil.opt.ui_scale = 1.25
            anvil.opt.material = "6061 Alloy"
            anvil.opt.view = "Top"
            anvil.opt.edges = false
            anvil.opt.snap_px = 12
            anvil.theme.dark.accent = "#112233"
            anvil.theme.light.view_top = "#fff"
            anvil.theme.body = "#ff0000"
            anvil.leader = "space"
            anvil.command("Plate", function() end, { description = "A plate" })
            anvil.key("ctrl+shift+e", "Extruded Boss/Base")
            anvil.key("<leader>p", "plate")
            anvil.key("g g", function() end, "zoom")
            "##,
            "init.lua",
            None,
        )
        .unwrap();
        let c = &e.config;
        assert_eq!(c.options.dark, Some(true));
        assert_eq!(c.options.ui_scale, Some(1.25));
        assert_eq!(c.options.material.as_deref(), Some("6061 Alloy"));
        assert_eq!(c.options.view.as_deref(), Some("top"));
        assert_eq!(c.options.edges, Some(false));
        assert_eq!(c.options.snap_px, Some(12.0));
        assert_eq!(c.theme.dark, vec![("accent".to_string(), Color32::from_rgb(0x11, 0x22, 0x33))]);
        assert_eq!(c.theme.light, vec![("view_top".to_string(), Color32::WHITE)]);
        assert_eq!(c.theme.model, vec![("body".to_string(), [1.0, 0.0, 0.0])]);
        assert_eq!(c.commands.len(), 1);
        assert_eq!(c.commands[0].description, "A plate");
        assert_eq!(c.bindings.len(), 3);
        assert_eq!(c.bindings[0].action, Action::Builtin("Extruded Boss/Base".into()));
        assert_eq!(c.bindings[1].sequence.len(), 2);
        assert_eq!(c.bindings[2].action, Action::Lua(3));
        assert_eq!(c.bindings[2].description, "zoom");
    }

    #[test]
    fn mistakes_are_errors_that_say_what_to_write() {
        let err = |src: &str| Engine::load(src, "init.lua", None).err().unwrap();
        assert!(err("anvil.opt.thme = 'dark'").contains("unknown option anvil.opt.thme"));
        assert!(err("anvil.opt.theme = 'blue'").contains("\"light\" or \"dark\""));
        assert!(err("anvil.opt.ui_scale = 9").contains("0.5 to 3"));
        assert!(err("anvil.theme.dark.acent = '#fff'").contains("unknown colour anvil.theme.dark.acent"));
        assert!(err("anvil.theme.dark.accent = 'red'").contains("not a #rrggbb colour"));
        assert!(err("anvil.key('ctrl+e', 'Extrood')").contains("no command is called \"Extrood\""));
        assert!(err("anvil.key('hyper+e', 'Fillet')").contains("unknown modifier"));
        // Lua errors carry the file and line.
        assert!(err("local x = \n\n nil + 1").contains("init.lua:3"));
        assert!(err("this is not lua").contains("init.lua:1"));
    }

    #[test]
    fn a_runaway_config_is_stopped() {
        let lua = Lua::new();
        let mut e = Engine { lua, config: Config::default(), time_limit: Duration::from_millis(200) };
        e.lua.load(PRELUDE).exec().unwrap();
        let start = Instant::now();
        let r = e.guarded(|lua| lua.load("while true do end").exec());
        assert!(r.unwrap_err().contains("ran too long"));
        assert!(start.elapsed() < Duration::from_secs(5));
        e.time_limit = Duration::from_secs(10);
        assert!(e.guarded(|lua| lua.load("local n = 0 for i = 1, 1000 do n = n + i end").exec()).is_ok());
    }

    #[test]
    fn require_finds_modules_next_to_init_lua() {
        let dir = std::env::temp_dir().join(format!("anvil-require-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("lua")).unwrap();
        std::fs::write(dir.join("lua").join("mykeys.lua"), "anvil.key('ctrl+shift+f', 'Fillet')").unwrap();
        std::fs::write(dir.join("init.lua"), "require('mykeys')\nanvil.opt.theme = 'dark'").unwrap();
        let e = Engine::load_file(&dir.join("init.lua")).unwrap();
        assert_eq!(e.config.bindings.len(), 1);
        assert_eq!(e.config.options.dark, Some(true));
        // A missing file is an empty config, not an error.
        assert!(Engine::load_file(&dir.join("absent.lua")).unwrap().config.bindings.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn parses_colors() {
        assert_eq!(parse_color("#abc"), Some(Color32::from_rgb(0xaa, 0xbb, 0xcc)));
        assert_eq!(parse_color("#102030"), Some(Color32::from_rgb(0x10, 0x20, 0x30)));
        assert_eq!(parse_color("#10203080"), Some(Color32::from_rgba_unmultiplied(0x10, 0x20, 0x30, 0x80)));
        assert_eq!(parse_color("102030"), None);
        assert_eq!(parse_color("#12345"), None);
        assert_eq!(parse_color("#gg0000"), None);
    }
}
