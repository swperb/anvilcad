//! For one STEP file, lists which faces of the first solid have open mesh edges.
//!
//! cargo run --release -p anvil-step --example open_faces -- <file.stp>

#![allow(clippy::expect_used)]

use anvil_step::{p21, reader};
use std::collections::{BTreeMap, HashMap};

fn main() {
    let path = std::env::args().nth(1).expect("file");
    let text = String::from_utf8_lossy(&std::fs::read(&path).expect("read")).into_owned();
    let imp = anvil_step::import(&text).expect("import");
    let ex = p21::parse(&text).expect("parse");
    let model = reader::read(&ex);
    let body = imp.bodies.iter().find(|b| b.solid).expect("solid");
    let src = model.bodies.iter().find(|b| b.solid).expect("source");
    let m = &body.mesh;
    let mut count: HashMap<(u32, u32), (i32, usize)> = HashMap::new();
    for (fi, f) in m.faces.iter().enumerate() {
        for t in &f.triangles {
            for k in 0..3 {
                count.entry((t[k], t[(k + 1) % 3])).or_insert((0, fi)).0 += 1;
            }
        }
    }
    let mut per_face: BTreeMap<usize, usize> = BTreeMap::new();
    let mut shown = 0;
    for (&(a, b), &(n, fi)) in &count {
        if n != 1 || count.get(&(b, a)).map(|x| x.0) != Some(1) {
            *per_face.entry(fi).or_default() += 1;
            if shown < 8 && std::env::var("SHOW_EDGES").is_ok() {
                shown += 1;
                println!("open x{n} face {fi}: {:?} -> {:?}", m.vertices[a as usize], m.vertices[b as usize]);
            }
        }
    }
    let mut by_type: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for (fi, f) in src.faces.iter().enumerate() {
        let e = by_type.entry(f.surface_type.clone()).or_default();
        e.0 += 1;
        if per_face.contains_key(&fi) {
            e.1 += 1;
        }
    }
    println!("faces with open edges, by surface type (open / total):");
    for (t, (n, o)) in by_type {
        println!("  {t:32} {o:4} / {n}");
    }
    let empty = m.faces.iter().filter(|f| f.triangles.is_empty()).count();
    println!("empty faces: {empty}");
    for (fi, n) in per_face.iter().take(12) {
        let f = &src.faces[*fi];
        let loops: Vec<usize> = f
            .bounds
            .iter()
            .map(|b| match &b.kind {
                reader::LoopKind::Edges(e) => e.len(),
                _ => 0,
            })
            .collect();
        let curves: Vec<String> = f
            .bounds
            .iter()
            .flat_map(|b| match &b.kind {
                reader::LoopKind::Edges(e) => {
                    e.iter().map(|oe| model.edges[&oe.edge].curve_type.clone()).collect()
                }
                _ => vec![],
            })
            .collect();
        println!(
            "  face {fi} #{} {} sense {} open {n} tris {} loops {loops:?} curves {curves:?}",
            f.id,
            f.surface_type,
            f.same_sense,
            m.faces[*fi].triangles.len()
        );
    }
}
