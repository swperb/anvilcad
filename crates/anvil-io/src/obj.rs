//! Wavefront OBJ export. Each model face becomes an OBJ group so downstream tools can
//! still tell faces apart.

use anvil_kernel::Solid;
use std::fmt::Write;

pub fn write(solid: &Solid, name: &str) -> String {
    let mut s = format!("# AnvilCAD OBJ export\no {name}\n");
    let mut next = 1usize;
    let mut last_face = None;
    for (pi, p) in solid.polygons.iter().enumerate() {
        if last_face != Some(p.tag.id) {
            let _ = writeln!(s, "g face_{}_{}", p.tag.id.feature, p.tag.id.index);
            last_face = Some(p.tag.id);
        }
        for v in &p.verts {
            let _ = writeln!(s, "v {} {} {}", v.x, v.y, v.z);
        }
        let _ = writeln!(s, "vn {} {} {}", p.normal.x, p.normal.y, p.normal.z);
        // One normal per polygon, so the normal index is the polygon index.
        s.push('f');
        for k in 0..p.verts.len() {
            let _ = write!(s, " {}//{}", next + k, pi + 1);
        }
        s.push('\n');
        next += p.verts.len();
    }
    s
}

#[cfg(test)]
mod tests {
    use anvil_kernel::{build::block, v3};

    #[test]
    fn counts() {
        let b = block(v3(0.0, 0.0, 0.0), v3(1.0, 1.0, 1.0), 1);
        let s = super::write(&b, "cube");
        assert_eq!(s.lines().filter(|l| l.starts_with("f ")).count(), b.polygons.len());
        assert_eq!(s.lines().filter(|l| l.starts_with("g ")).count(), 6);
    }
}
