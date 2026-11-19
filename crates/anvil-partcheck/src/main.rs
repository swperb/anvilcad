//! `anvil-partcheck`: runs Part Check on a STEP file and prints the JSON result.
//!
//! ```text
//! anvil-partcheck <file.step> --material <name> [--quantity <n>] [--compact]
//! anvil-partcheck --schema
//! ```
//!
//! Exit codes: 0 on success, 1 when the file cannot be checked, 2 on bad arguments.
//! Nothing is written anywhere but stdout and stderr, and stderr never carries geometry.

use anvil_partcheck::{Request, check_step, json_schema};
use std::process::ExitCode;

const USAGE: &str = "usage: anvil-partcheck <file.step> --material <name> [--quantity <n>] [--compact]\n       anvil-partcheck --schema";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--schema") {
        return print(&json_schema(), false);
    }
    let mut file = None;
    let mut material = None;
    let mut quantity = 1u32;
    let mut compact = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--material" => material = it.next().cloned(),
            "--quantity" => match it.next().and_then(|q| q.parse().ok()) {
                Some(q) => quantity = q,
                None => return usage("--quantity needs a whole number"),
            },
            "--compact" => compact = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            s if s.starts_with('-') => return usage(&format!("unknown option {s}")),
            s => file = Some(s.to_string()),
        }
    }
    let (Some(file), Some(material)) = (file, material) else {
        return usage("a STEP file and --material are required");
    };
    let bytes = match std::fs::read(&file) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("anvil-partcheck: {file}: {e}");
            return ExitCode::from(1);
        }
    };
    let file_name = std::path::Path::new(&file).file_name().map(|n| n.to_string_lossy().into_owned());
    match check_step(&bytes, &Request { material, quantity, file_name }) {
        Ok(r) => match serde_json::to_value(&r) {
            Ok(v) => print(&v, compact),
            Err(e) => {
                eprintln!("anvil-partcheck: {e}");
                ExitCode::from(1)
            }
        },
        Err(e) => {
            eprintln!("anvil-partcheck: {e}");
            ExitCode::from(1)
        }
    }
}

fn print(v: &serde_json::Value, compact: bool) -> ExitCode {
    let s = if compact { serde_json::to_string(v) } else { serde_json::to_string_pretty(v) };
    match s {
        Ok(s) => {
            println!("{s}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("anvil-partcheck: {e}");
            ExitCode::from(1)
        }
    }
}

fn usage(why: &str) -> ExitCode {
    eprintln!("anvil-partcheck: {why}\n{USAGE}");
    ExitCode::from(2)
}
