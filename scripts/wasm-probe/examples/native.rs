//! Runs the probe natively, so scripts/wasm-check.sh can compare its output and timing
//! with the WebAssembly build. Usage: native <file.anvprt | file.step> ...
use std::time::Instant;

fn main() {
    let mut failed = false;
    for path in std::env::args().skip(1) {
        let name = std::path::Path::new(&path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                println!("{name}: ERROR {e}");
                failed = true;
                continue;
            }
        };
        let step =
            path.to_ascii_lowercase().ends_with(".step") || path.to_ascii_lowercase().ends_with(".stp");
        let t0 = Instant::now();
        let r = if step {
            anvil_wasm_probe::import_step_text(&text)
        } else {
            anvil_wasm_probe::rebuild_part_text(&text)
        };
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        match r {
            Ok(s) => println!("{name}: {s} ms={ms:.1}"),
            Err(e) => {
                println!("{name}: ERROR {e} ms={ms:.1}");
                failed = true;
            }
        }
    }
    std::process::exit(i32::from(failed));
}
