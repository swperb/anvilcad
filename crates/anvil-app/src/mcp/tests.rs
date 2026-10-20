//! Drives the MCP server with JSON-RPC lines, as an agent would, and checks exact values.

use super::*;

/// A server that has done the `initialize` handshake.
fn server() -> Server {
    let mut s = Server::new();
    let r = rpc(&mut s, "initialize", json!({ "protocolVersion": "2025-06-18", "capabilities": {} }));
    assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#), None);
    s
}

/// Sends one request as a line of JSON and parses the reply line.
fn rpc(s: &mut Server, method: &str, params: Value) -> Value {
    let line = json!({ "jsonrpc": "2.0", "id": 7, "method": method, "params": params }).to_string();
    let reply = s.handle_line(&line).expect("a request gets a reply");
    let v: Value = serde_json::from_str(&reply).expect("the reply is JSON");
    assert_eq!(v["jsonrpc"], "2.0");
    assert_eq!(v["id"], 7);
    v
}

/// Calls a tool and returns its structured result, failing the test on a tool error.
fn call(s: &mut Server, name: &str, args: Value) -> Value {
    let r = rpc(s, "tools/call", json!({ "name": name, "arguments": args }));
    let result = &r["result"];
    assert_eq!(result["isError"], false, "{name} {args} -> {r}");
    // The text content carries the same JSON as the structured content.
    let text = result["content"][0]["text"].as_str().expect("text content");
    assert_eq!(serde_json::from_str::<Value>(text).expect("JSON text"), result["structuredContent"]);
    result["structuredContent"].clone()
}

/// Calls a tool that should fail and returns its error text.
fn call_err(s: &mut Server, name: &str, args: Value) -> String {
    let r = rpc(s, "tools/call", json!({ "name": name, "arguments": args }));
    assert_eq!(r["result"]["isError"], true, "{name} {args} should fail -> {r}");
    r["result"]["content"][0]["text"].as_str().expect("error text").to_string()
}

fn close(a: &Value, b: f64) -> bool {
    a.as_f64().is_some_and(|a| (a - b).abs() < 1e-6)
}

fn assert_vec(v: &Value, want: [f64; 3]) {
    let got: Vec<f64> = v.as_array().expect("array").iter().filter_map(Value::as_f64).collect();
    assert_eq!(got.len(), 3, "{v}");
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() < 1e-6, "{v} is not {want:?}");
    }
}

/// A 10 x 20 mm rectangle on the front plane, extruded 30 mm. Returns the extrusion's id.
fn block(s: &mut Server) -> u64 {
    call(s, "new_part", json!({}));
    let sk = call(s, "add_sketch", json!({ "plane": "front", "rects": [[0, 0, 10, 20]] }));
    let sketch = sk["sketch"].as_u64().expect("sketch id");
    let ex = call(s, "add_extrude", json!({ "sketch": sketch, "depth": 30 }));
    assert!(close(&ex["body"]["volume"], 6000.0), "{ex}");
    ex["feature"].as_u64().expect("feature id")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("anvil-mcp-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn handshake_and_tool_list() {
    let mut s = Server::new();
    let r = rpc(&mut s, "initialize", json!({ "protocolVersion": "2024-11-05" }));
    assert_eq!(r["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(r["result"]["serverInfo"]["name"], "anvilcad");
    assert!(r["result"]["capabilities"]["tools"].is_object());
    // A revision the server does not know is answered with its newest.
    let r = rpc(&mut s, "initialize", json!({ "protocolVersion": "1999-01-01" }));
    assert_eq!(r["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
    assert_eq!(rpc(&mut s, "ping", json!({}))["result"], json!({}));

    let tools = rpc(&mut s, "tools/list", json!({}))["result"]["tools"].clone();
    let names: Vec<&str> = tools.as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    for want in [
        "new_part",
        "open_part",
        "save_part",
        "list_features",
        "add_sketch",
        "add_extrude",
        "set_variable",
        "set_dimension",
        "rebuild",
        "mass_properties",
        "export",
        "screenshot",
        "run_lua",
    ] {
        assert!(names.contains(&want), "{want} missing from {names:?}");
    }
    for t in tools.as_array().unwrap() {
        assert_eq!(t["inputSchema"]["type"], "object", "{t}");
        assert!(t["inputSchema"]["properties"].is_object(), "{} has a broken schema", t["name"]);
        assert!(t["description"].as_str().is_some_and(|d| d.len() > 20));
    }
}

#[test]
fn extrudes_a_10_by_20_by_30_block() {
    let mut s = server();
    let id = block(&mut s);
    let m = call(&mut s, "mass_properties", json!({}));
    assert!(close(&m["volume"], 6000.0), "{m}");
    assert!(close(&m["surface_area"], 2.0 * (10.0 * 20.0 + 10.0 * 30.0 + 20.0 * 30.0)), "{m}");
    assert_vec(&m["centroid"], [5.0, 10.0, 15.0]);
    assert_vec(&m["bounds"]["min"], [0.0, 0.0, 0.0]);
    assert_vec(&m["bounds"]["max"], [10.0, 20.0, 30.0]);
    assert_vec(&m["size"], [10.0, 20.0, 30.0]);
    assert_eq!(m["open_edges"], 0);
    assert_eq!(m["mass"], Value::Null);

    // Plain carbon steel is 7.8 g/cm^3: 6 cm^3 weighs 46.8 g.
    let m = call(&mut s, "set_material", json!({ "name": "Plain Carbon Steel" }));
    assert!(close(&m["mass"], 46.8), "{m}");

    let tree = call(&mut s, "list_features", json!({}));
    let features = tree["features"].as_array().unwrap();
    assert_eq!(features.len(), 2);
    assert_eq!(features[0]["kind"], "Sketch");
    assert_eq!(features[1]["id"], id);
    assert_eq!(features[1]["kind"], "Extrude");
    assert!(close(&features[1]["dimensions"]["depth"]["value"], 30.0), "{tree}");
}

#[test]
fn dimensions_and_variables_drive_the_model() {
    let mut s = server();
    let id = block(&mut s);
    let r = call(&mut s, "set_dimension", json!({ "feature": id, "dimension": "depth", "value": 50 }));
    assert!(close(&r["body"]["volume"], 10000.0), "{r}");

    // By name, and as an equation of a global variable.
    let name = call(&mut s, "list_features", json!({}))["features"][1]["name"].clone();
    call(&mut s, "set_variable", json!({ "name": "t", "value": 12 }));
    let r = call(&mut s, "set_dimension", json!({ "feature": name, "dimension": "depth", "value": "t * 2" }));
    assert!(close(&r["body"]["volume"], 200.0 * 24.0), "{r}");
    let r = call(&mut s, "set_variable", json!({ "name": "t", "value": "5" }));
    assert!(close(&r["body"]["volume"], 200.0 * 10.0), "{r}");
    assert!(close(&r["variables"]["t"], 5.0), "{r}");
    let tree = call(&mut s, "list_features", json!({}));
    assert_eq!(tree["features"][1]["dimensions"]["depth"]["equation"], "t * 2");
    assert!(close(&tree["features"][1]["dimensions"]["depth"]["value"], 10.0));

    // A value the rebuild rejects is an error, and the old value stays.
    let e = call_err(&mut s, "set_dimension", json!({ "feature": id, "dimension": "depth", "value": -5 }));
    assert!(e.contains("depth"), "{e}");
    let m = call(&mut s, "mass_properties", json!({}));
    assert!(close(&m["volume"], 2000.0), "{m}");
    let e = call_err(&mut s, "set_dimension", json!({ "feature": id, "dimension": "radius", "value": 1 }));
    assert!(e.contains("it has depth"), "{e}");
    let e = call_err(&mut s, "set_dimension", json!({ "feature": 999, "dimension": "depth", "value": 1 }));
    assert!(e.contains("no feature"), "{e}");
}

#[test]
fn a_failed_extrusion_adds_nothing() {
    let mut s = server();
    let sk = call(&mut s, "add_sketch", json!({ "plane": "top", "rects": [[-50, -30, 50, 30]] }));
    let e = call_err(&mut s, "add_extrude", json!({ "sketch": sk["sketch"], "depth": "thickness" }));
    assert!(e.contains("unknown variable"), "{e}");
    assert_eq!(call(&mut s, "list_features", json!({}))["features"].as_array().unwrap().len(), 1);
    call(&mut s, "set_variable", json!({ "name": "thickness", "value": 10 }));
    let r = call(&mut s, "add_extrude", json!({ "sketch": sk["sketch"], "depth": "thickness" }));
    assert!(close(&r["body"]["volume"], 100.0 * 60.0 * 10.0), "{r}");
    let r = call(&mut s, "set_variable", json!({ "name": "thickness", "value": 12 }));
    assert!(close(&r["body"]["volume"], 100.0 * 60.0 * 12.0), "{r}");
    assert_eq!(call(&mut s, "list_features", json!({}))["features"].as_array().unwrap().len(), 2);
}

#[test]
fn cuts_and_sketches_on_faces() {
    let mut s = server();
    let id = block(&mut s);
    // A 2 x 2 mm square hole through the block's end face.
    let sk = call(&mut s, "add_sketch", json!({ "on_feature": id, "rects": [[2, 2, 4, 4]] }));
    let r = call(&mut s, "add_extrude", json!({ "sketch": sk["sketch"], "depth": "through", "cut": true }));
    assert!(close(&r["body"]["volume"], 6000.0 - 4.0 * 30.0), "{r}");
    // A 5 mm boss on the same face.
    let sk = call(&mut s, "add_sketch", json!({ "on_feature": id, "rects": [[0, 10, 10, 20]] }));
    let r = call(&mut s, "add_extrude", json!({ "sketch": sk["sketch"], "depth": 5 }));
    assert!(close(&r["body"]["volume"], 5880.0 + 10.0 * 10.0 * 5.0), "{r}");
    let m = call(&mut s, "mass_properties", json!({}));
    assert!(close(&m["bounds"]["max"][2], 35.0), "{m}");
}

#[test]
fn lua_reaches_the_same_part() {
    let mut s = server();
    let id = block(&mut s);
    let r = call(&mut s, "run_lua", json!({ "code": format!("anvil.dim({id}, 'depth', 15)") }));
    assert!(close(&r["body"]["volume"], 3000.0), "{r}");
    let r = call(
        &mut s,
        "run_lua",
        json!({ "code": "local k = anvil.sketch{ plane = 'top', rects = {{20, 0, 30, 10}} }\n\
                          anvil.extrude{ sketch = k, depth = 10 }" }),
    );
    assert!(close(&r["body"]["volume"], 4000.0), "{r}");
    let e = call_err(&mut s, "run_lua", json!({ "code": "anvil.dim(1, 'depth')" }));
    assert!(e.contains("anvil.dim"), "{e}");
    let e = call_err(&mut s, "run_lua", json!({ "code": "this is not lua" }));
    assert!(!e.is_empty());
    let r = call(&mut s, "run_command", json!({ "name": "Undo" }));
    assert!(r["body"].is_object(), "{r}");
}

#[test]
fn exports_step_and_stl_and_reopens_them() {
    let mut s = server();
    block(&mut s);
    let dir = scratch("export");
    let step = dir.join("sub/block.step");
    let r = call(&mut s, "export", json!({ "path": step.display().to_string() }));
    assert!(r["bytes"].as_u64().is_some_and(|b| b > 200), "{r}");
    let text = std::fs::read_to_string(&step).unwrap();
    assert!(text.starts_with("ISO-10303-21;"), "{}", &text[..40.min(text.len())]);
    let stl = dir.join("block.stl");
    call(&mut s, "export", json!({ "path": stl.display().to_string() }));
    assert!(std::fs::metadata(&stl).unwrap().len() > 84);

    let saved = dir.join("block.anvprt");
    let r = call(&mut s, "save_part", json!({ "path": saved.display().to_string() }));
    assert_eq!(r["path"], saved.display().to_string());

    for path in [&step, &saved] {
        let r = call(&mut s, "open_part", json!({ "path": path.display().to_string() }));
        assert!(close(&r["body"]["volume"], 6000.0), "{} -> {r}", path.display());
    }

    let e = call_err(&mut s, "export", json!({ "path": dir.join("block.dwg").display().to_string() }));
    assert!(e.contains(".step"), "{e}");
    let e =
        call_err(&mut s, "open_part", json!({ "path": dir.join("missing.anvprt").display().to_string() }));
    assert!(e.contains("does not exist"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mistakes_are_tool_errors_and_protocol_errors() {
    let mut s = server();
    // Tool errors: the call ran, and says what was wrong.
    let e = call_err(&mut s, "mass_properties", json!({}));
    assert!(e.contains("no solid body"), "{e}");
    let e = call_err(&mut s, "export", json!({ "path": "nothing.step" }));
    assert!(e.contains("no solid body"), "{e}");
    let e = call_err(&mut s, "add_sketch", json!({ "plane": "sideways", "rects": [[0, 0, 1, 1]] }));
    assert!(e.contains("front, top or right"), "{e}");
    let e = call_err(&mut s, "add_sketch", json!({ "rects": [[0, 0, 1]] }));
    assert!(e.contains("x0, y0, x1, y1"), "{e}");
    let e = call_err(&mut s, "add_sketch", json!({}));
    assert!(e.contains("rects"), "{e}");
    let e = call_err(&mut s, "add_sketch", json!({ "circles": [[0, 0, -1]] }));
    assert!(e.contains("radius"), "{e}");
    let e = call_err(&mut s, "add_extrude", json!({ "sketch": 42, "depth": 10 }));
    assert!(e.contains("not a sketch"), "{e}");
    let e = call_err(&mut s, "add_extrude", json!({ "sketch": 1, "depth": [1] }));
    assert!(e.contains("depth"), "{e}");
    let e = call_err(&mut s, "set_material", json!({ "name": "Unobtainium" }));
    assert!(e.contains("Unobtainium"), "{e}");
    let e = call_err(&mut s, "set_variable", json!({ "name": "bad name", "value": 1 }));
    assert!(e.contains("not a variable name"), "{e}");
    let e = call_err(&mut s, "run_command", json!({ "name": "Frobnicate" }));
    assert!(e.contains("Frobnicate"), "{e}");
    let e = call_err(&mut s, "screenshot", json!({ "path": "x.png", "view": "under" }));
    assert!(e.contains("isometric"), "{e}");

    // Protocol errors.
    let r = rpc(&mut s, "tools/call", json!({ "name": "make_coffee" }));
    assert_eq!(r["error"]["code"], -32602);
    let r = rpc(&mut s, "tools/call", json!({ "name": "rebuild", "arguments": [1] }));
    assert_eq!(r["error"]["code"], -32602);
    let r = rpc(&mut s, "no/such/method", json!({}));
    assert_eq!(r["error"]["code"], -32601);
    let r: Value = serde_json::from_str(&s.handle_line("{not json").unwrap()).unwrap();
    assert_eq!(r["error"]["code"], -32700);
    assert_eq!(r["id"], Value::Null);
    let r: Value = serde_json::from_str(&s.handle_line("[1, 2]").unwrap()).unwrap();
    assert_eq!(r["error"]["code"], -32600);
    assert_eq!(s.handle_line(""), None);
    // The session survives all of that.
    block(&mut s);
}

/// Needs a GPU or a software adapter; skipped (with a note) where there is neither.
#[test]
fn screenshots_to_a_file_and_inline() {
    let mut s = server();
    block(&mut s);
    let dir = scratch("shot");
    let path = dir.join("block.png");
    let r = rpc(
        &mut s,
        "tools/call",
        json!({ "name": "screenshot", "arguments": { "path": path.display().to_string(), "view": "isometric" } }),
    );
    let result = &r["result"];
    if result["isError"] == true {
        let e = result["content"][0]["text"].as_str().unwrap_or("");
        assert!(e.contains("adapter"), "{r}");
        eprintln!("skipping MCP screenshot: {e}");
        return;
    }
    let png = std::fs::read(&path).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let image = &result["content"][1];
    assert_eq!(image["type"], "image");
    assert_eq!(image["mimeType"], "image/png");
    assert_eq!(image["data"].as_str().unwrap(), base64(&png));
    // Cropped to the 3D view, which is smaller than the 1400 x 880 window.
    let info = &result["structuredContent"];
    assert!(info["width"].as_u64().is_some_and(|w| w > 100 && w < 1400), "{info}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn base64_matches_rfc_4648() {
    for (raw, enc) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(base64(raw.as_bytes()), enc);
    }
    assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
}
