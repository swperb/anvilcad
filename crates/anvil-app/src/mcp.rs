//! A Model Context Protocol server, so AI agents (Claude Code, Cursor and others) can model
//! parts with no window: `anvilcad-agent mcp`. See docs/MCP.md.
//!
//! It speaks JSON-RPC 2.0 over stdio, one message per line, and keeps one headless app
//! ([`Headless`]) open for the whole session. The tools call the same [`Host`] API that Lua
//! scripts drive (docs/SCRIPTING.md), so a tool and `anvil.*` always agree. A failing tool
//! returns a tool result with `isError`, never a crash: each call also runs inside
//! `catch_unwind`, so even a bug in the app is reported to the agent rather than ending the
//! session.

use crate::headless::Headless;
use crate::script::{Depth, ExtrudeSpec, Host, PlaneSpec, SketchSpec};
use serde_json::{json, Map, Value};
use std::io::{BufRead, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

/// The protocol revisions this server speaks, newest first. It answers `initialize` with the
/// client's revision when it is one of these, and with the newest otherwise.
pub const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "\
AnvilCAD models parts in millimetres. A typical session: new_part, add_sketch (rectangles \
and circles on the front, top or right plane, or on an extrusion's end face), add_extrude \
(a boss, or a cut with cut=true), then mass_properties to check the result, set_dimension \
or set_variable to change it, export (STEP or STL) and screenshot to look at it. \
list_features shows the feature ids that sketches, extrusions and dimensions refer to. \
run_lua runs any script in AnvilCAD's Lua API for anything the other tools do not cover.";

/// One MCP session over a headless AnvilCAD.
pub struct Server {
    h: Headless,
}

impl Default for Server {
    fn default() -> Self {
        Server::new()
    }
}

impl Server {
    /// Starts on a new, empty part.
    pub fn new() -> Server {
        Server { h: Headless::new() }
    }

    /// The headless app the tools act on, for tests.
    pub fn headless(&mut self) -> &mut Headless {
        &mut self.h
    }

    /// Handles one line of input and returns the reply line, if the message needs one.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let reply = match serde_json::from_str::<Value>(line) {
            Ok(msg) => self.handle(&msg),
            Err(e) => Some(error_reply(Value::Null, -32700, &format!("parse error: {e}"))),
        };
        reply.map(|r| r.to_string())
    }

    /// Handles one JSON-RPC message. Notifications get no reply.
    pub fn handle(&mut self, msg: &Value) -> Option<Value> {
        let Some(obj) = msg.as_object() else {
            return Some(error_reply(Value::Null, -32600, "a message is one JSON-RPC object"));
        };
        let id = obj.get("id").cloned();
        let Some(method) = obj.get("method").and_then(Value::as_str) else {
            // A response to something we never send, or garbage: answer requests only.
            return id.map(|id| error_reply(id, -32600, "the message has no \"method\""));
        };
        let params = obj.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = id else {
            // Notifications (initialized, cancelled, ...) need no answer.
            return None;
        };
        let result = match method {
            "initialize" => Ok(self.initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => self.call(&params),
            "resources/list" => Ok(json!({ "resources": [] })),
            "prompts/list" => Ok(json!({ "prompts": [] })),
            other => Err((-32601, format!("method \"{other}\" is not supported"))),
        };
        Some(match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err((code, message)) => error_reply(id, code, &message),
        })
    }

    fn initialize(&self, params: &Value) -> Value {
        let asked = params.get("protocolVersion").and_then(Value::as_str);
        let version = asked.filter(|v| PROTOCOL_VERSIONS.contains(v)).unwrap_or(PROTOCOL_VERSIONS[0]);
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "anvilcad", "title": "AnvilCAD", "version": env!("CARGO_PKG_VERSION") },
            "instructions": INSTRUCTIONS,
        })
    }

    fn call(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or((-32602, "tools/call needs \"name\"".to_string()))?;
        if !TOOLS.iter().any(|t| t.name == name) {
            return Err((-32602, format!("there is no tool called \"{name}\"")));
        }
        let args = match params.get("arguments") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(m)) => m.clone(),
            Some(_) => return Err((-32602, "\"arguments\" is an object".to_string())),
        };
        let outcome = catch_unwind(AssertUnwindSafe(|| self.tool(name, &Args(&args))))
            .unwrap_or_else(|p| Err(format!("internal error in {name}: {}", panic_text(&p))));
        Ok(match outcome {
            Ok(Output::Json(v)) => json!({
                "content": [{ "type": "text", "text": pretty(&v) }],
                "structuredContent": v,
                "isError": false,
            }),
            Ok(Output::Image { info, png }) => json!({
                "content": [
                    { "type": "text", "text": pretty(&info) },
                    { "type": "image", "data": base64(&png), "mimeType": "image/png" },
                ],
                "structuredContent": info,
                "isError": false,
            }),
            Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
        })
    }

    /// Runs `f` against the scripting host, then lets the UI catch up so screenshots and
    /// `get_state` show the change.
    fn host<R>(&mut self, f: impl FnOnce(&mut dyn Host) -> Result<R, String>) -> Result<R, String> {
        let ctx = self.h.ctx.clone();
        let r = self.h.app.with_host(&ctx, f);
        self.h.settle();
        r
    }

    /// The body after a change, so each modelling tool shows its effect without another call.
    fn summary(&self) -> Value {
        let s = self.h.app.snapshot();
        json!({ "body": s["body"], "errors": errors(&s) })
    }

    fn tool(&mut self, name: &str, a: &Args) -> Result<Output, String> {
        let out = match name {
            "new_part" => {
                self.host(|h| h.new_part(true))?;
                if let Some(m) = a.str("material")? {
                    self.host(|h| h.set_material(m))?;
                }
                json!({ "ok": true, "features": self.h.app.feature_list()["features"] })
            }
            "open_part" => {
                let path = a.path("path")?;
                self.h.app.take_message();
                self.host(|h| h.open(&path, true))?;
                // Opening reports through the message dialog: a failure, or import notes.
                let message = self.h.app.take_message();
                self.h.settle();
                if let Some((title, text)) = &message {
                    if title.ends_with("failed") {
                        return Err(format!("{title}: {text}"));
                    }
                }
                let s = self.h.app.snapshot();
                let mut tree = self.h.app.feature_list();
                tree["path"] = s["path"].clone();
                tree["body"] = s["body"].clone();
                tree["material"] = s["material"].clone();
                tree["message"] = json!(message.map(|(_, text)| text));
                tree
            }
            "save_part" => {
                let path = match a.opt_path("path")? {
                    Some(_) => Some(a.out_path("path")?),
                    None => None,
                };
                self.host(|h| h.save(path.as_deref()))?;
                json!({ "path": self.h.app.snapshot()["path"] })
            }
            "list_features" => self.h.app.feature_list(),
            "get_state" => self.h.app.snapshot(),
            "add_sketch" => {
                let spec = sketch_spec(a)?;
                let id = self.host(|h| h.add_sketch(&spec))?;
                json!({ "sketch": id })
            }
            "add_extrude" => {
                let depth = match a.get("depth") {
                    Some(Value::Number(n)) => Depth::Value(n.as_f64().unwrap_or(f64::NAN)),
                    Some(Value::String(s)) if s.eq_ignore_ascii_case("through") => Depth::ThroughAll,
                    Some(Value::String(s)) => match s.trim().parse::<f64>() {
                        Ok(v) => Depth::Value(v),
                        Err(_) => Depth::Expr(s.clone()),
                    },
                    _ => return Err("depth is a number of mm, an equation, or \"through\"".into()),
                };
                let spec = ExtrudeSpec {
                    sketch: a.id("sketch")?,
                    depth,
                    cut: a.bool("cut")?,
                    reverse: a.bool("reverse")?,
                };
                let before = self.h.app.snapshot()["body"]["volume"].as_f64();
                let id = self.host(|h| h.add_extrude(&spec))?;
                let mut s = self.summary();
                s["feature"] = json!(id);
                let after = s["body"]["volume"].as_f64();
                if let (Some(b), Some(a)) = (before, after) {
                    if (a - b).abs() <= 1e-9 * b.abs().max(1.0) {
                        let way = if spec.reverse { "without reverse" } else { "with reverse: true" };
                        s["warning"] = json!(format!(
                            "the {} did not change the body: it may point away from it; try {way}",
                            if spec.cut { "cut" } else { "extrusion" }
                        ));
                    }
                }
                s
            }
            "set_variable" => {
                let name = a.req_str("name")?;
                let value = a.number_or_text("value")?;
                self.host(|h| h.set_variable(name, &value))?;
                let mut s = self.summary();
                s["variables"] = self.h.app.feature_list()["variables"].clone();
                s
            }
            "set_dimension" => {
                let feature = a.number_or_text("feature")?;
                let dimension = a.req_str("dimension")?;
                let value = a.number_or_text("value")?;
                self.host(|h| h.set_dimension(&feature, dimension, &value))?;
                self.summary()
            }
            "delete_feature" => {
                let feature = a.number_or_text("feature")?;
                let gone = self.host(|h| h.delete_feature(&feature))?;
                let mut s = self.summary();
                s["deleted"] = json!(gone);
                s
            }
            "set_material" => {
                let name = a.req_str("name")?;
                self.host(|h| h.set_material(name))?;
                self.h.app.mass_properties().unwrap_or_else(|_| json!({ "material": name }))
            }
            "rebuild" => {
                self.host(|h| h.run_builtin("Rebuild"))?;
                self.summary()
            }
            "mass_properties" => self.h.app.mass_properties()?,
            "export" => {
                let path = a.out_path("path")?;
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
                if !["step", "stp", "stl", "obj"].contains(&ext.as_str()) {
                    return Err(format!(
                        "export writes .step, .stp, .stl or .obj files, not {}",
                        path.display()
                    ));
                }
                self.host(|h| h.export(&path))?;
                let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                json!({ "path": path.display().to_string(), "bytes": bytes })
            }
            "screenshot" => return self.screenshot(a),
            "run_lua" => {
                let code = a.req_str("code")?;
                let ctx = self.h.ctx.clone();
                let r = self.h.app.eval_lua(&ctx, code, "mcp");
                self.h.settle();
                let (value, printed) = r?;
                let mut s = self.summary();
                s["result"] = value;
                s["printed"] = json!(printed);
                s
            }
            "run_command" => {
                let name = a.req_str("name")?;
                self.host(|h| h.run_builtin(name))?;
                let s = self.h.app.snapshot();
                json!({ "status": s["status"], "body": s["body"], "errors": errors(&s) })
            }
            other => return Err(format!("there is no tool called \"{other}\"")),
        };
        Ok(Output::Json(out))
    }

    fn screenshot(&mut self, a: &Args) -> Result<Output, String> {
        let path = a.out_path("path")?;
        if let (Some(w), Some(h)) = (a.num("width")?, a.num("height")?) {
            self.h.command(&json!({ "cmd": "resize", "width": w, "height": h }));
        }
        if let Some(view) = a.str("view")? {
            let names = ["front", "back", "left", "right", "top", "bottom", "isometric"];
            let Some(v) = names.iter().find(|n| n.eq_ignore_ascii_case(view)) else {
                return Err(format!("view is one of {}", names.join(", ")));
            };
            let mut title = v.to_string();
            title[..1].make_ascii_uppercase();
            self.host(|h| h.run_builtin(&title))?;
        }
        if a.opt_bool("fit")?.unwrap_or(true) {
            self.host(|h| h.run_builtin("Zoom to Fit"))?;
        }
        let mut cmd = json!({ "cmd": "screenshot", "path": path.display().to_string() });
        let whole = a.str("crop")?.map(|c| c.eq_ignore_ascii_case("window")).unwrap_or(false);
        if !whole {
            cmd["region"] = self.h.app.snapshot()["viewport"].clone();
        }
        let mut r = self.h.command(&cmd);
        if r["ok"] != true {
            return Err(format!("screenshot: {}", r["error"].as_str().unwrap_or("failed")));
        }
        if let Some(o) = r.as_object_mut() {
            o.remove("ok");
        }
        if !a.opt_bool("inline")?.unwrap_or(true) {
            return Ok(Output::Json(r));
        }
        let png = std::fs::read(&path).map_err(|e| format!("could not read back {}: {e}", path.display()))?;
        Ok(Output::Image { info: r, png })
    }
}

/// Serves MCP over these streams until the input ends.
pub fn serve(input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
    let mut server = Server::new();
    for line in input.lines() {
        let line = line?;
        if let Some(reply) = server.handle_line(&line) {
            writeln!(output, "{reply}")?;
            output.flush()?;
        }
    }
    Ok(())
}

enum Output {
    Json(Value),
    Image { info: Value, png: Vec<u8> },
}

fn error_reply(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

fn panic_text(p: &Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic with no message".into())
}

/// Features that failed to rebuild, by name.
fn errors(snapshot: &Value) -> Value {
    let list: Vec<_> = snapshot["features"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| !f["error"].is_null())
        .map(|f| json!({ "id": f["id"], "name": f["name"], "error": f["error"] }))
        .collect();
    json!(list)
}

fn sketch_spec(a: &Args) -> Result<SketchSpec, String> {
    let plane = match (a.get("on_feature"), a.str("plane")?) {
        (Some(_), Some(_)) => return Err("give plane or on_feature, not both".into()),
        (Some(_), None) => {
            let start = match a.str("face")? {
                None | Some("end") => false,
                Some("start") => true,
                Some(f) => return Err(format!("face is \"end\" or \"start\", not \"{f}\"")),
            };
            PlaneSpec::Face { feature: a.id("on_feature")?, start }
        }
        (None, p) => match p.unwrap_or("front").to_ascii_lowercase().as_str() {
            "front" => PlaneSpec::Front,
            "top" => PlaneSpec::Top,
            "right" => PlaneSpec::Right,
            other => return Err(format!("plane is front, top or right, not \"{other}\"")),
        },
    };
    let rects = a.number_lists::<4>("rects", "rects: [[x0, y0, x1, y1], ...]")?;
    let circles = a.number_lists::<3>("circles", "circles: [[x, y, radius], ...]")?;
    if rects.is_empty() && circles.is_empty() {
        return Err("give rects: [[x0, y0, x1, y1], ...] or circles: [[x, y, radius], ...]".into());
    }
    Ok(SketchSpec { plane, rects, circles })
}

/// A tool's arguments, with readers that say what was wrong.
struct Args<'a>(&'a Map<String, Value>);

impl Args<'_> {
    fn get(&self, k: &str) -> Option<&Value> {
        self.0.get(k).filter(|v| !v.is_null())
    }

    fn str(&self, k: &str) -> Result<Option<&str>, String> {
        match self.get(k) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s)),
            Some(_) => Err(format!("{k} is a string")),
        }
    }

    fn req_str(&self, k: &str) -> Result<&str, String> {
        self.str(k)?.ok_or_else(|| format!("{k} is required"))
    }

    fn num(&self, k: &str) -> Result<Option<f64>, String> {
        match self.get(k) {
            None => Ok(None),
            Some(v) => v.as_f64().map(Some).ok_or_else(|| format!("{k} is a number")),
        }
    }

    fn opt_bool(&self, k: &str) -> Result<Option<bool>, String> {
        match self.get(k) {
            None => Ok(None),
            Some(v) => v.as_bool().map(Some).ok_or_else(|| format!("{k} is true or false")),
        }
    }

    fn bool(&self, k: &str) -> Result<bool, String> {
        Ok(self.opt_bool(k)?.unwrap_or(false))
    }

    fn id(&self, k: &str) -> Result<u32, String> {
        self.get(k)
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| format!("{k} is a feature id (see list_features)"))
    }

    /// A number or an equation, as text.
    fn number_or_text(&self, k: &str) -> Result<String, String> {
        match self.get(k) {
            Some(Value::Number(n)) => Ok(n.to_string()),
            Some(Value::String(s)) if !s.trim().is_empty() => Ok(s.clone()),
            _ => Err(format!("{k} is a number or an equation")),
        }
    }

    fn opt_path(&self, k: &str) -> Result<Option<PathBuf>, String> {
        Ok(self.str(k)?.filter(|s| !s.trim().is_empty()).map(PathBuf::from))
    }

    fn path(&self, k: &str) -> Result<PathBuf, String> {
        self.opt_path(k)?.ok_or_else(|| format!("{k} is required"))
    }

    /// A file to write; its folder is created if need be.
    fn out_path(&self, k: &str) -> Result<PathBuf, String> {
        let p = self.path(k)?;
        if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        }
        Ok(p)
    }

    fn number_lists<const N: usize>(&self, k: &str, usage: &str) -> Result<Vec<[f64; N]>, String> {
        let Some(v) = self.get(k) else { return Ok(Vec::new()) };
        let rows = v.as_array().ok_or_else(|| usage.to_string())?;
        rows.iter()
            .map(|row| {
                let nums: Option<Vec<f64>> =
                    row.as_array().and_then(|r| r.iter().map(Value::as_f64).collect());
                nums.and_then(|n| <[f64; N]>::try_from(n).ok())
                    .filter(|n| n.iter().all(|x| x.is_finite()))
                    .ok_or_else(|| usage.to_string())
            })
            .collect()
    }
}

struct Tool {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    /// The `properties` of the input schema, as JSON text.
    properties: &'static str,
    required: &'static [&'static str],
    read_only: bool,
}

const TOOLS: &[Tool] = &[
    Tool {
        name: "new_part",
        title: "New part",
        description: "Start a new, empty part, discarding the current one. Units are millimetres.",
        properties: r#"{ "material": { "type": "string", "description": "A library material, such as \"6061 Alloy\" or \"ABS\"" } }"#,
        required: &[],
        read_only: false,
    },
    Tool {
        name: "open_part",
        title: "Open part",
        description: "Open an AnvilCAD part (.anvprt), template, STEP or SOLIDWORKS file, discarding the current part. Returns its feature tree.",
        properties: r#"{ "path": { "type": "string", "description": "The file to open" } }"#,
        required: &["path"],
        read_only: false,
    },
    Tool {
        name: "save_part",
        title: "Save part",
        description: "Save the part as .anvprt, to path or to the file it came from.",
        properties: r#"{ "path": { "type": "string", "description": "Where to save, such as part.anvprt; optional once the part has a file" } }"#,
        required: &[],
        read_only: false,
    },
    Tool {
        name: "list_features",
        title: "List features",
        description: "The feature tree: each feature's id, name, kind, rebuild error and settable dimensions (with their values), and the global variables.",
        properties: "{}",
        required: &[],
        read_only: true,
    },
    Tool {
        name: "get_state",
        title: "Get state",
        description: "Everything the app reports about itself: features, body, material, variables, configurations, status bar, open dialogs.",
        properties: "{}",
        required: &[],
        read_only: true,
    },
    Tool {
        name: "add_sketch",
        title: "Add sketch",
        description: "Add a sketch of rectangles and circles and return its feature id. Coordinates are mm in the sketch plane. Front is the XY plane, top is XZ and right is YZ; on_feature sketches on an extrusion's end (or start) face instead.",
        properties: r#"{
            "plane": { "type": "string", "enum": ["front", "top", "right"], "description": "Default front" },
            "on_feature": { "type": "integer", "description": "Sketch on this extrusion's face instead of a plane" },
            "face": { "type": "string", "enum": ["end", "start"], "description": "Which face of on_feature; default end" },
            "rects": { "type": "array", "items": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4 }, "description": "Rectangles by opposite corners: [[x0, y0, x1, y1], ...]" },
            "circles": { "type": "array", "items": { "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3 }, "description": "Circles: [[x, y, radius], ...]" }
        }"#,
        required: &[],
        read_only: false,
    },
    Tool {
        name: "add_extrude",
        title: "Add extrusion",
        description: "Extrude a sketch into a boss, or cut with cut=true. A boss goes along the sketch plane's normal (+Z for front, +Y for top, +X for right, out of a face); a cut goes the opposite way (into a face); reverse flips either. Returns the feature id and the body after the rebuild, with a warning if the body did not change.",
        properties: r#"{
            "sketch": { "type": "integer", "description": "The sketch's feature id" },
            "depth": { "type": ["number", "string"], "description": "mm, an equation such as \"thickness * 2\", or \"through\" (through all)" },
            "cut": { "type": "boolean", "description": "Remove material instead of adding it" },
            "reverse": { "type": "boolean", "description": "Extrude the other way" }
        }"#,
        required: &["sketch", "depth"],
        read_only: false,
    },
    Tool {
        name: "set_variable",
        title: "Set variable",
        description: "Set or add a global variable to a number or an equation; dimensions written as equations follow it. Rebuilds.",
        properties: r#"{ "name": { "type": "string" }, "value": { "type": ["number", "string"], "description": "A number or an equation such as \"width / 2\"" } }"#,
        required: &["name", "value"],
        read_only: false,
    },
    Tool {
        name: "set_dimension",
        title: "Set dimension",
        description: "Set one of a feature's dimensions (depth, angle, offset, spacing, count, radius, distance, chamfer angle, thickness: list_features shows which each has) to a number or an equation, then rebuild. If the rebuild fails, the old value is kept and the error is returned.",
        properties: r#"{
            "feature": { "type": ["integer", "string"], "description": "The feature's id or name" },
            "dimension": { "type": "string", "description": "Such as \"depth\"" },
            "value": { "type": ["number", "string"], "description": "mm (degrees for angles), or an equation" }
        }"#,
        required: &["feature", "dimension", "value"],
        read_only: false,
    },
    Tool {
        name: "delete_feature",
        title: "Delete feature",
        description: "Delete a feature, by id or name, together with the features that depend on it (such as the extrusion of a deleted sketch). Returns the ids removed and the body after the rebuild.",
        properties: r#"{ "feature": { "type": ["integer", "string"], "description": "The feature's id or name" } }"#,
        required: &["feature"],
        read_only: false,
    },
    Tool {
        name: "set_material",
        title: "Set material",
        description: "Assign a library material, so mass_properties reports mass.",
        properties: r#"{ "name": { "type": "string", "description": "Such as \"6061 Alloy\", \"Plain Carbon Steel\" or \"ABS\"" } }"#,
        required: &["name"],
        read_only: false,
    },
    Tool {
        name: "rebuild",
        title: "Rebuild",
        description: "Rebuild the part and report the body and any feature errors.",
        properties: "{}",
        required: &[],
        read_only: false,
    },
    Tool {
        name: "mass_properties",
        title: "Mass properties",
        description: "Measure the body: volume (mm^3), surface area (mm^2), centroid, bounding box and size (mm), and mass (g) and principal moments when a material is set. These come from the tessellated body, so curved faces make them slightly approximate; exact says whether STEP export can write an exact B-rep.",
        properties: "{}",
        required: &[],
        read_only: true,
    },
    Tool {
        name: "export",
        title: "Export",
        description: "Export the body by extension: .step or .stp (exact B-rep where possible), .stl or .obj (mesh).",
        properties: r#"{ "path": { "type": "string", "description": "Such as out/part.step" } }"#,
        required: &["path"],
        read_only: false,
    },
    Tool {
        name: "screenshot",
        title: "Screenshot",
        description: "Render the part to a PNG file and return the image. Crops to the 3D view unless crop is \"window\".",
        properties: r#"{
            "path": { "type": "string", "description": "Where to write the PNG" },
            "view": { "type": "string", "enum": ["front", "back", "left", "right", "top", "bottom", "isometric"] },
            "fit": { "type": "boolean", "description": "Zoom to fit first; default true" },
            "crop": { "type": "string", "enum": ["viewport", "window"], "description": "Default viewport" },
            "width": { "type": "number", "description": "Window width in points (with height)" },
            "height": { "type": "number", "description": "Window height in points (with width)" },
            "inline": { "type": "boolean", "description": "Return the image in the result as well as writing it; default true" }
        }"#,
        required: &["path"],
        read_only: false,
    },
    Tool {
        name: "run_lua",
        title: "Run Lua",
        description: "Run Lua with AnvilCAD's anvil API (anvil.sketch, anvil.extrude, anvil.var, anvil.dim, anvil.delete, anvil.run, anvil.state, anvil.export...; see docs/SCRIPTING.md), for anything the other tools do not cover. Returns the chunk's return value as result, the lines it printed, and the body.",
        properties: r#"{ "code": { "type": "string" } }"#,
        required: &["code"],
        read_only: false,
    },
    Tool {
        name: "run_command",
        title: "Run command",
        description: "Run a built-in command by its toolbar or menu name, such as \"Undo\", \"Redo\", \"Isometric\" or \"Zoom to Fit\".",
        properties: r#"{ "name": { "type": "string" } }"#,
        required: &["name"],
        read_only: false,
    },
];

/// The `tools/list` entries.
fn tools() -> Vec<Value> {
    TOOLS
        .iter()
        .map(|t| {
            let properties: Value = serde_json::from_str(t.properties).unwrap_or_else(|_| json!({}));
            json!({
                "name": t.name,
                "title": t.title,
                "description": t.description,
                "inputSchema": { "type": "object", "properties": properties, "required": t.required },
                "annotations": { "readOnlyHint": t.read_only, "openWorldHint": false },
            })
        })
        .collect()
}

/// Standard base64 with padding, for image results.
fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(A[(n >> shift & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
