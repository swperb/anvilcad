//! Imports every STEP file under a folder and prints one line per file.
//!
//! cargo run --release -p anvil-step --example report -- <folder>

use anvil_step::{Mesh, import};
use glam::DVec3;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Instant;

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            files(&p, out);
        } else if matches!(
            p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref(),
            Some("stp" | "step")
        ) {
            out.push(p);
        }
    }
}

fn stats(m: &Mesh) -> (usize, usize, f64) {
    let mut count: HashMap<(u32, u32), i32> = HashMap::new();
    let p = |i: u32| DVec3::from_array(m.vertices[i as usize]);
    let mut vol = 0.0;
    let mut tris = 0;
    for t in m.faces.iter().flat_map(|f| &f.triangles) {
        tris += 1;
        vol += p(t[0]).dot(p(t[1]).cross(p(t[2]))) / 6.0;
        for k in 0..3 {
            *count.entry((t[k], t[(k + 1) % 3])).or_default() += 1;
        }
    }
    let open = count.iter().filter(|&(&(a, b), &n)| n != 1 || count.get(&(b, a)) != Some(&1)).count();
    (tris, open, vol)
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let mut list = Vec::new();
    files(Path::new(&dir), &mut list);
    list.sort();
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    for path in list {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let text = String::from_utf8_lossy(&std::fs::read(&path).unwrap_or_default()).into_owned();
        let t = Instant::now();
        match import(&text) {
            Ok(imp) => {
                let ms = t.elapsed().as_secs_f64() * 1000.0;
                let solids: Vec<_> = imp.bodies.iter().filter(|b| b.solid).collect();
                let exact = solids.iter().filter(|b| b.exact.is_some()).count();
                let (mut tris, mut open, mut vol, mut closed) = (0, 0, 0.0, 0);
                for b in &solids {
                    let (t, o, v) = stats(&b.mesh);
                    tris += t;
                    open += o;
                    vol += v;
                    if o == 0 {
                        closed += 1;
                    }
                }
                println!(
                    "{name:40} {ms:6.0} ms  solids {:2} (exact {exact}, closed {closed})  sheets {:2}  tris {tris:7}  open {open:6}  vol {vol:12.1}  {}",
                    solids.len(),
                    imp.bodies.len() - solids.len(),
                    imp.length_unit,
                );
                for w in &imp.warnings {
                    let key = w.split(':').next_back().unwrap_or(w).trim().to_string();
                    *kinds.entry(key).or_default() += 1;
                }
                for b in imp.bodies.iter().filter(|b| b.solid) {
                    if let Some(why) = &b.not_exact {
                        *kinds.entry(format!("mesh: {why}")).or_default() += 1;
                    }
                }
            }
            Err(e) => println!("{name:44} FAILED: {e}"),
        }
    }
    println!("\nmost common warnings and mesh reasons:");
    let mut v: Vec<_> = kinds.into_iter().collect();
    v.sort_by_key(|e| std::cmp::Reverse(e.1));
    for (k, n) in v.into_iter().take(20) {
        println!("{n:5}  {k}");
    }
}
