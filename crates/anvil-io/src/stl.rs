//! STL export.

use anvil_kernel::Solid;

fn triangles(solid: &Solid) -> impl Iterator<Item = ([f32; 3], [[f32; 3]; 3])> + '_ {
    solid.polygons.iter().flat_map(|p| {
        let n = p.normal.to_f32();
        (1..p.verts.len() - 1)
            .map(move |i| (n, [p.verts[0].to_f32(), p.verts[i].to_f32(), p.verts[i + 1].to_f32()]))
    })
}

pub fn write_binary(solid: &Solid, name: &str) -> Vec<u8> {
    let tris: Vec<_> = triangles(solid).collect();
    let mut out = Vec::with_capacity(84 + tris.len() * 50);
    let mut header = [0u8; 80];
    let h = format!("AnvilCAD binary STL: {name}");
    let hb = h.as_bytes();
    header[..hb.len().min(80)].copy_from_slice(&hb[..hb.len().min(80)]);
    out.extend_from_slice(&header);
    out.extend_from_slice(&(tris.len() as u32).to_le_bytes());
    for (n, vs) in tris {
        for c in n {
            out.extend_from_slice(&c.to_le_bytes());
        }
        for v in vs {
            for c in v {
                out.extend_from_slice(&c.to_le_bytes());
            }
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out
}

pub fn write_ascii(solid: &Solid, name: &str) -> String {
    use std::fmt::Write;
    let mut s = format!("solid {name}\n");
    for (n, vs) in triangles(solid) {
        let _ = writeln!(s, "  facet normal {:e} {:e} {:e}", n[0], n[1], n[2]);
        s.push_str("    outer loop\n");
        for v in vs {
            let _ = writeln!(s, "      vertex {:e} {:e} {:e}", v[0], v[1], v[2]);
        }
        s.push_str("    endloop\n  endfacet\n");
    }
    let _ = writeln!(s, "endsolid {name}");
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_kernel::{build::block, v3};

    #[test]
    fn binary_layout() {
        let b = block(v3(0.0, 0.0, 0.0), v3(1.0, 1.0, 1.0), 1);
        let bytes = write_binary(&b, "cube");
        let n = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
        assert_eq!(n, 12);
        assert_eq!(bytes.len(), 84 + 50 * n);
        assert!(write_ascii(&b, "cube").matches("facet normal").count() == 12);
    }
}
