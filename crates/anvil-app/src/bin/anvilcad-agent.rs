//! Drives AnvilCAD with no window, for agents and scripted tests.
//!
//! Usage:
//!   anvilcad-agent [--open FILE | --sample] [--size WxH] [--scale S]
//!                  [--config INIT.lua] [--lua SCRIPT.lua]
//!                  [--script FILE.jsonl] [--keep-going] [--screenshot OUT.png]
//!
//! Reads one JSON command per line (from `--script`, or stdin) and writes one JSON reply
//! per line to stdout. It never opens a window, takes focus or reads the real mouse and
//! keyboard, so it can run while you use the machine. See docs/AGENT_HARNESS.md.

use anvil_app::headless::Headless;
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

const USAGE: &str = "\
Usage: anvilcad-agent [--open FILE | --sample] [--size WxH] [--scale S]
                      [--config INIT.lua] [--lua SCRIPT.lua]
                      [--script FILE.jsonl] [--keep-going] [--screenshot OUT.png]

No init.lua is read unless --config names one, so runs are repeatable. --lua runs a Lua
script (docs/SCRIPTING.md) after the part opens and before any JSON commands.

Reads one JSON command per line (from --script, or stdin) and writes one JSON reply per
line. No window is opened, so it never takes focus or uses the real mouse and keyboard.
Send {\"cmd\":\"help\"} for the commands; docs/AGENT_HARNESS.md has the details.";

fn main() {
    let mut size = (1400.0, 880.0);
    let mut scale = 1.0;
    let mut open = None;
    let mut sample = false;
    let mut script = None;
    let mut keep_going = false;
    let mut screenshot = None;
    let mut config = None;
    let mut lua = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--open" => open = args.next().map(PathBuf::from),
            "--sample" => sample = true,
            "--script" => script = args.next().map(PathBuf::from),
            "--keep-going" => keep_going = true,
            "--config" => config = args.next().map(PathBuf::from),
            "--lua" => lua = args.next().map(PathBuf::from),
            "--screenshot" => screenshot = args.next(),
            "--scale" => scale = args.next().and_then(|s| s.parse().ok()).unwrap_or(1.0),
            "--size" => {
                let s = args.next().unwrap_or_default();
                match s.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))) {
                    Some(wh) => size = wh,
                    None => fail(&format!("--size takes WIDTHxHEIGHT in points, got {s:?}")),
                }
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            other => fail(&format!("unknown argument {other:?} (try --help)")),
        }
    }

    let mut h = Headless::with_size(egui::vec2(size.0, size.1), scale);
    if sample {
        h.app.open_sample();
    }
    if let Some(p) = &open {
        h.app.open_file(p);
    }
    let ctx = h.ctx.clone();
    if let Some(p) = config {
        h.app.load_config(&ctx, Some(p));
    }
    if let Some(p) = &lua {
        let src = std::fs::read_to_string(p)
            .unwrap_or_else(|e| fail(&format!("could not read {}: {e}", p.display())));
        if let Err(e) = h.app.run_lua(&ctx, &src, &p.display().to_string()) {
            fail(&format!("{}: {e}", p.display()));
        }
    }
    h.settle();

    let out = std::io::stdout();
    let mut out = out.lock();
    let mut failed = false;
    let mut handle = |line: &str, out: &mut dyn Write| -> bool {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            return true;
        }
        let reply = match serde_json::from_str::<Value>(line) {
            Ok(cmd) if cmd.get("cmd").and_then(Value::as_str) == Some("quit") => {
                let _ = writeln!(out, "{}", json!({ "ok": true, "id": cmd.get("id") }));
                return false;
            }
            Ok(cmd) => h.command(&cmd),
            Err(e) => json!({ "ok": false, "error": format!("not JSON: {e}") }),
        };
        let ok = reply["ok"] == true;
        let _ = writeln!(out, "{reply}");
        let _ = out.flush();
        if !ok {
            failed = true;
        }
        ok || keep_going || script.is_none()
    };

    match &script {
        Some(path) => {
            let text =
                std::fs::read_to_string(path).unwrap_or_else(|e| fail(&format!("{}: {e}", path.display())));
            for line in text.lines() {
                if !handle(line, &mut out) {
                    break;
                }
            }
        }
        None if screenshot.is_some() => {}
        None => {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if !handle(&line, &mut out) {
                    break;
                }
            }
        }
    }

    if let Some(path) = &screenshot {
        let r = h.command(&json!({ "cmd": "screenshot", "path": path }));
        let _ = writeln!(out, "{r}");
        failed |= r["ok"] != true;
        if r["ok"] == true {
            eprintln!("wrote {}", Path::new(path).display());
        }
    }
    if failed && script.is_some() {
        std::process::exit(1);
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("anvilcad-agent: {msg}");
    std::process::exit(2)
}
