//! Runs `anvilcad-agent mcp` as an MCP client would: a child process spoken to over stdio.
//! This checks what the in-process tests cannot: that stdout carries only protocol lines.

use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn anvilcad_agent_mcp_models_a_block_over_stdio() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_anvilcad-agent"))
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start anvilcad-agent mcp");
    let call = |id: u32, name: &str, args: Value| json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": name, "arguments": args } });
    let messages = [
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": { "protocolVersion": "2025-06-18", "capabilities": {},
                            "clientInfo": { "name": "test", "version": "0" } } }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        call(2, "add_sketch", json!({ "plane": "front", "rects": [[0, 0, 10, 20]] })),
        call(3, "add_extrude", json!({ "sketch": 1, "depth": 30 })),
        call(4, "mass_properties", json!({})),
        call(5, "set_dimension", json!({ "feature": 2, "dimension": "depth", "value": "0 - 1" })),
    ];
    {
        let mut stdin = child.stdin.take().expect("stdin");
        for m in &messages {
            writeln!(stdin, "{m}").expect("write a request");
        }
        // Closing stdin ends the session.
    }
    let out = child.wait_with_output().expect("anvilcad-agent mcp exits");
    assert!(out.status.success(), "{:?}", out.status);
    let text = String::from_utf8(out.stdout).expect("UTF-8");
    let replies: Vec<Value> =
        text.lines().map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}"))).collect();
    // One reply per request, none for the notification, and nothing else on stdout.
    let ids: Vec<u64> = replies.iter().filter_map(|r| r["id"].as_u64()).collect();
    assert_eq!(ids, [1, 2, 3, 4, 5]);
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "anvilcad");
    let m = &replies[3]["result"]["structuredContent"];
    assert_eq!(m["volume"].as_f64(), Some(6000.0), "{m}");
    assert_eq!(m["size"], json!([10.0, 20.0, 30.0]));
    // A depth of -1 fails the rebuild: a tool error, not a crash.
    assert_eq!(replies[4]["result"]["isError"], true, "{}", replies[4]);
}
