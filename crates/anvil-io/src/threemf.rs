//! 3MF export, to the 3MF Consortium's Core Specification (version 1.3,
//! <https://github.com/3MFConsortium/spec_core>), an open format.
//!
//! A 3MF file is a ZIP package holding three parts: `[Content_Types].xml`,
//! `_rels/.rels` and the model, `3D/3dmodel.model`. The model lists each body as a mesh
//! object and places every object once on the build plate. Unlike STL it records the
//! unit (always millimetres here), keeps bodies apart and names them, and can carry a
//! material name and display colour for each.
//!
//! AnvilCAD is Y-up: the Top plane faces +Y. The 3MF build direction is +Z, so the
//! writer turns the model a quarter turn about X, taking AnvilCAD's (x, y, z) to the
//! file's (x, -z, y). A part then lands on the build plate the way it stands in
//! AnvilCAD, with its top up, and AnvilCAD's front faces the front of the printer (-Y).
//!
//! The core specification asks for meshes whose triangles share vertices by index, with
//! every edge used by exactly two triangles and every triangle wound counter-clockwise
//! seen from outside. Faceted solids come from booleans with T-junctions and from convex
//! polygons, so [`indexed_mesh`] repairs the T-junctions, welds coincident vertices and
//! triangulates each polygon without zero-area triangles.

use anvil_kernel::{Solid, Vec3};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{self, Write as _};

/// One body of the package.
pub struct Object {
    /// Shown by slicers in their object list.
    pub name: String,
    /// The body, in millimetres, in AnvilCAD's (Y-up) coordinates, where it should be placed.
    pub solid: Solid,
    /// Written as a base material, which slicers show by name and colour.
    pub material: Option<Material>,
}

/// A base material: a name and an sRGB display colour.
#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: String,
    pub color: [u8; 3],
}

/// What a 3MF file holds.
pub struct Package {
    /// The `Title` metadata: the part or assembly name.
    pub title: String,
    pub objects: Vec<Object>,
}

/// A triangle mesh with shared vertices.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IndexedMesh {
    pub vertices: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
}

impl IndexedMesh {
    /// Volume enclosed by the triangles, by the divergence theorem.
    pub fn volume(&self) -> f64 {
        self.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| self.vertices[i as usize]);
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }

    /// Edges not matched by exactly one edge running the other way in another triangle.
    /// Zero for a closed, consistently wound, edge-manifold mesh.
    pub fn bad_edges(&self) -> usize {
        let mut count: HashMap<(u32, u32), i32> = HashMap::new();
        for t in &self.triangles {
            for i in 0..3 {
                *count.entry((t[i], t[(i + 1) % 3])).or_default() += 1;
            }
        }
        count.iter().filter(|&(&(a, b), &n)| n != 1 || count.get(&(b, a)).copied().unwrap_or(0) != 1).count()
    }
}

/// Vertices closer than this (in mm, per axis) are the same vertex. Matches the
/// tolerance [`Solid::repair_t_junctions`] works to.
const WELD: f64 = 1e-7;

/// The solid as a mesh with shared vertices: T-junctions repaired, coincident vertices
/// welded, each polygon triangulated, and triangles that collapse when welded dropped.
pub fn indexed_mesh(solid: &Solid) -> IndexedMesh {
    let mut solid = solid.clone();
    solid.repair_t_junctions();
    let key =
        |v: Vec3| ((v.x / WELD).round() as i64, (v.y / WELD).round() as i64, (v.z / WELD).round() as i64);
    let mut mesh = IndexedMesh::default();
    let mut ids: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let mut ring: Vec<(u32, Vec3)> = Vec::new();
    for p in &solid.polygons {
        ring.clear();
        for &v in &p.verts {
            let next = mesh.vertices.len() as u32;
            let id = *ids.entry(key(v)).or_insert_with(|| {
                mesh.vertices.push(v);
                next
            });
            if ring.last().map(|r| r.0) != Some(id) {
                ring.push((id, v));
            }
        }
        while ring.len() > 1 && ring.first().map(|r| r.0) == ring.last().map(|r| r.0) {
            ring.pop();
        }
        let n = ring.len();
        if n < 3 {
            continue;
        }
        // A strictly convex polygon fans from its first corner. Repairing T-junctions puts
        // extra points on straight edges, and a fan from a corner next to such an edge has
        // zero-area triangles; those polygons fan from their centre instead, which lies
        // strictly inside a convex polygon and so is never in line with an edge.
        let straight = (0..n).any(|i| {
            let (a, b, c) = (ring[(i + n - 1) % n].1, ring[i].1, ring[(i + 1) % n].1);
            let (u, w) = (b - a, c - b);
            u.cross(w).length() <= 1e-9 * u.length() * w.length()
        });
        if n == 3 || !straight {
            for i in 1..n - 1 {
                mesh.triangles.push([ring[0].0, ring[i].0, ring[i + 1].0]);
            }
        } else {
            let centre = ring.iter().fold(Vec3::ZERO, |s, r| s + r.1) / n as f64;
            let c = mesh.vertices.len() as u32;
            mesh.vertices.push(centre);
            for i in 0..n {
                mesh.triangles.push([c, ring[i].0, ring[(i + 1) % n].0]);
            }
        }
    }
    mesh
}

/// Writes the package as the bytes of a `.3mf` file.
pub fn write(pkg: &Package) -> io::Result<Vec<u8>> {
    let mut zip = Zip::default();
    zip.add("[Content_Types].xml", CONTENT_TYPES.as_bytes())?;
    zip.add("_rels/.rels", RELS.as_bytes())?;
    zip.add("3D/3dmodel.model", model_xml(pkg).as_bytes())?;
    zip.finish()
}

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
 <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
 <Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>
</Types>
"#;

const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
 <Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>
"#;

/// The model part: metadata, a base material group when any object has a material,
/// one mesh object per body and a build item for each.
pub fn model_xml(pkg: &Package) -> String {
    let mut s = String::from(concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?>"#,
        "\n",
        r#"<model unit="millimeter" xml:lang="en-US" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">"#,
        "\n"
    ));
    let _ = writeln!(s, r#" <metadata name="Title">{}</metadata>"#, escape(&pkg.title));
    s.push_str(" <metadata name=\"Application\">AnvilCAD</metadata>\n <resources>\n");

    let mut materials: Vec<&Material> = Vec::new();
    for m in pkg.objects.iter().filter_map(|o| o.material.as_ref()) {
        if !materials.contains(&m) {
            materials.push(m);
        }
    }
    // Resource ids: 1 is the material group, objects follow from 2.
    if !materials.is_empty() {
        s.push_str("  <basematerials id=\"1\">\n");
        for m in &materials {
            let [r, g, b] = m.color;
            let name = escape(&m.name);
            let _ = writeln!(s, r##"   <base name="{name}" displaycolor="#{r:02X}{g:02X}{b:02X}"/>"##);
        }
        s.push_str("  </basematerials>\n");
    }
    let mut placed = Vec::new();
    for (k, o) in pkg.objects.iter().enumerate() {
        let mesh = indexed_mesh(&o.solid);
        if mesh.triangles.is_empty() {
            continue;
        }
        let id = k + 2;
        placed.push(id);
        let _ = write!(s, r#"  <object id="{id}" type="model" name="{}""#, escape(&o.name));
        if let Some(i) = o.material.as_ref().and_then(|m| materials.iter().position(|x| *x == m)) {
            let _ = write!(s, r#" pid="1" pindex="{i}""#);
        }
        s.push_str(">\n   <mesh>\n    <vertices>\n");
        for v in &mesh.vertices {
            let [x, y, z] = to_build(*v);
            let _ = writeln!(s, r#"     <vertex x="{}" y="{}" z="{}"/>"#, num(x), num(y), num(z));
        }
        s.push_str("    </vertices>\n    <triangles>\n");
        for [a, b, c] in &mesh.triangles {
            let _ = writeln!(s, r#"     <triangle v1="{a}" v2="{b}" v3="{c}"/>"#);
        }
        s.push_str("    </triangles>\n   </mesh>\n  </object>\n");
    }
    s.push_str(" </resources>\n <build>\n");
    for id in placed {
        let _ = writeln!(s, r#"  <item objectid="{id}"/>"#);
    }
    s.push_str(" </build>\n</model>\n");
    s
}

/// AnvilCAD's Y-up coordinates in the file's Z-up ones: a proper rotation, so the
/// triangles keep their outward winding.
pub fn to_build(v: Vec3) -> [f64; 3] {
    [v.x, -v.z, v.y]
}

/// A coordinate to the nearest nanometre, in the shortest form that reads back exactly.
fn num(x: f64) -> String {
    let r = (x * 1e6).round() / 1e6;
    if r == 0.0 {
        "0".into()
    } else {
        format!("{r}")
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Control characters other than tab and newlines are not allowed in XML 1.0.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

/// A minimal ZIP writer: deflated entries, a central directory, no ZIP64. 3MF packages
/// are Open Packaging Convention ZIP files, which allow only stored and deflated entries.
#[derive(Default)]
struct Zip {
    out: Vec<u8>,
    central: Vec<u8>,
    entries: u16,
}

impl Zip {
    fn add(&mut self, name: &str, data: &[u8]) -> io::Result<()> {
        let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(data)?;
        let packed = enc.finish()?;
        let too_big = || {
            io::Error::new(io::ErrorKind::InvalidData, "the model is too large for a 3MF file (over 4 GB)")
        };
        let size = u32::try_from(data.len()).map_err(|_| too_big())?;
        let csize = u32::try_from(packed.len()).map_err(|_| too_big())?;
        let offset = u32::try_from(self.out.len()).map_err(|_| too_big())?;
        let crc = crc32fast::hash(data);
        let name_len = u16::try_from(name.len()).map_err(|_| too_big())?;
        // Version 2.0, no flags, deflate, 1980-01-01 00:00 so output is reproducible.
        let common = |b: &mut Vec<u8>| {
            for v in [20u16, 0, 8, 0, 0x21] {
                b.extend_from_slice(&v.to_le_bytes());
            }
            for v in [crc, csize, size] {
                b.extend_from_slice(&v.to_le_bytes());
            }
            b.extend_from_slice(&name_len.to_le_bytes());
            b.extend_from_slice(&0u16.to_le_bytes()); // extra field length
        };
        self.out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        common(&mut self.out);
        self.out.extend_from_slice(name.as_bytes());
        self.out.extend_from_slice(&packed);

        self.central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        self.central.extend_from_slice(&20u16.to_le_bytes()); // made by
        common(&mut self.central);
        for v in [0u16, 0, 0] {
            // comment length, disk number, internal attributes
            self.central.extend_from_slice(&v.to_le_bytes());
        }
        self.central.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        self.central.extend_from_slice(&offset.to_le_bytes());
        self.central.extend_from_slice(name.as_bytes());
        self.entries += 1;
        Ok(())
    }

    fn finish(mut self) -> io::Result<Vec<u8>> {
        let too_big = || {
            io::Error::new(io::ErrorKind::InvalidData, "the model is too large for a 3MF file (over 4 GB)")
        };
        let at = u32::try_from(self.out.len()).map_err(|_| too_big())?;
        let len = u32::try_from(self.central.len()).map_err(|_| too_big())?;
        self.out.extend_from_slice(&self.central);
        self.out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        for v in [0u16, 0, self.entries, self.entries] {
            self.out.extend_from_slice(&v.to_le_bytes());
        }
        self.out.extend_from_slice(&len.to_le_bytes());
        self.out.extend_from_slice(&at.to_le_bytes());
        self.out.extend_from_slice(&0u16.to_le_bytes()); // comment length
        Ok(self.out)
    }
}

/// The parts of a ZIP package and their contents, read through its central directory.
/// Reads stored and deflated entries and checks each one's CRC; it does not read ZIP64,
/// encrypted or spanned archives.
pub fn read_parts(bytes: &[u8]) -> io::Result<Vec<(String, Vec<u8>)>> {
    use std::io::Read as _;
    let bad =
        |why: &str| io::Error::new(io::ErrorKind::InvalidData, format!("not a readable 3MF package: {why}"));
    let u16_at = |i: usize| -> io::Result<usize> {
        bytes.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize).ok_or_else(|| bad("truncated"))
    };
    let u32_at = |i: usize| -> io::Result<usize> {
        bytes
            .get(i..i + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .ok_or_else(|| bad("truncated"))
    };
    // The end record is the last 22 bytes, plus a comment of up to 64 KB.
    let lowest = bytes.len().saturating_sub(22 + 0xFFFF);
    let eocd = (lowest..=bytes.len().saturating_sub(22))
        .rev()
        .find(|&i| u32_at(i).ok() == Some(0x0605_4b50))
        .ok_or_else(|| bad("no end of central directory"))?;
    let (count, mut at) = (u16_at(eocd + 10)?, u32_at(eocd + 16)?);
    let mut parts = Vec::with_capacity(count);
    for _ in 0..count {
        if u32_at(at)? != 0x0201_4b50 {
            return Err(bad("damaged central directory"));
        }
        let (flags, method) = (u16_at(at + 8)?, u16_at(at + 10)?);
        let (crc, csize, size) = (u32_at(at + 16)?, u32_at(at + 20)?, u32_at(at + 24)?);
        let (name_len, extra_len, comment_len) = (u16_at(at + 28)?, u16_at(at + 30)?, u16_at(at + 32)?);
        let name = bytes.get(at + 46..at + 46 + name_len).ok_or_else(|| bad("truncated"))?;
        let name = String::from_utf8_lossy(name).into_owned();
        if flags & 1 != 0 {
            return Err(bad("encrypted entry"));
        }
        let local = u32_at(at + 42)?;
        if u32_at(local)? != 0x0403_4b50 {
            return Err(bad("damaged entry"));
        }
        let start = local + 30 + u16_at(local + 26)? + u16_at(local + 28)?;
        let packed = bytes.get(start..start + csize).ok_or_else(|| bad("truncated"))?;
        let data = match method {
            0 => packed.to_vec(),
            8 => {
                let mut data = Vec::with_capacity(size);
                flate2::read::DeflateDecoder::new(packed).read_to_end(&mut data)?;
                data
            }
            _ => return Err(bad("unsupported compression")),
        };
        if data.len() != size || crc32fast::hash(&data) as usize != crc {
            return Err(bad("checksum mismatch"));
        }
        parts.push((name, data));
        at += 46 + name_len + extra_len + comment_len;
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_kernel::build::{block, cylinder};
    use anvil_kernel::{v3, Frame};

    fn cube() -> Solid {
        block(v3(0.0, 0.0, 0.0), v3(10.0, 20.0, 30.0), 1).unwrap()
    }

    #[test]
    fn y_up_becomes_z_up_keeping_the_winding() {
        let to = |v: Vec3| {
            let [x, y, z] = to_build(v);
            v3(x, y, z)
        };
        // Top goes up; the viewer's side in the Front view goes to the printer's front.
        assert_eq!(to(Vec3::Y), Vec3::Z);
        assert_eq!(to(Vec3::Z), -Vec3::Y);
        assert_eq!(to(Vec3::X), Vec3::X);
        // Turned, the mesh still encloses the same positive volume: outward winding kept.
        let mut m = indexed_mesh(&cube());
        m.vertices = m.vertices.into_iter().map(to).collect();
        assert!((m.volume() - 6000.0).abs() < 1e-9);
    }

    #[test]
    fn cube_mesh_shares_its_eight_corners() {
        let m = indexed_mesh(&cube());
        assert_eq!(m.vertices.len(), 8);
        assert_eq!(m.triangles.len(), 12);
        assert_eq!(m.bad_edges(), 0);
        assert!((m.volume() - 6000.0).abs() < 1e-9);
    }

    #[test]
    fn boolean_result_is_closed_without_slivers() {
        // A post sunk into a slab: the union splits the slab's top around the post.
        let base = block(v3(0.0, 0.0, 0.0), v3(30.0, 10.0, 10.0), 1).unwrap();
        let top = block(v3(10.0, 2.0, 5.0), v3(20.0, 8.0, 20.0), 2).unwrap();
        let s = base.union(&top);
        // Booleans repair T-junctions by putting extra points on straight polygon edges;
        // fanning those polygons from a corner would make zero-area triangles.
        let in_line = |p: &anvil_kernel::Polygon| {
            let n = p.verts.len();
            (0..n).any(|i| {
                let (a, b, c) = (p.verts[(i + n - 1) % n], p.verts[i], p.verts[(i + 1) % n]);
                (b - a).cross(c - b).length() < 1e-9
            })
        };
        assert!(s.polygons.iter().any(in_line), "the union should have points along straight edges");
        let m = indexed_mesh(&s);
        assert_eq!(m.bad_edges(), 0);
        assert!((m.volume() - (3000.0 + 600.0)).abs() < 1e-6, "volume {}", m.volume());
        for t in &m.triangles {
            let [a, b, c] = t.map(|i| m.vertices[i as usize]);
            assert!((b - a).cross(c - a).length() > 1e-9, "zero-area triangle");
        }
    }

    #[test]
    fn cylinder_mesh_is_closed() {
        let c = cylinder(v3(0.0, 0.0, 0.0), 5.0, 12.0, 1).unwrap();
        let m = indexed_mesh(&c);
        assert_eq!(m.bad_edges(), 0);
        assert!((m.volume() - c.mass_properties().volume).abs() < 1e-6);
    }

    #[test]
    fn package_holds_the_three_parts() {
        let abs = Material { name: "ABS".into(), color: [184, 191, 204] };
        let pkg = Package {
            title: "Bracket & <Co>".into(),
            objects: vec![
                Object { name: "Body 1".into(), solid: cube(), material: Some(abs.clone()) },
                Object {
                    name: "Body 2".into(),
                    solid: cube().transformed(&Frame { origin: v3(50.0, 0.0, 0.0), ..Frame::WORLD }),
                    material: Some(abs),
                },
            ],
        };
        let entries = read_parts(&write(&pkg).unwrap()).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.0.as_str()).collect();
        assert_eq!(names, ["[Content_Types].xml", "_rels/.rels", "3D/3dmodel.model"]);
        let model = String::from_utf8(entries[2].1.clone()).unwrap();
        assert!(model.contains(r#"unit="millimeter""#));
        assert!(model.contains("<metadata name=\"Title\">Bracket &amp; &lt;Co&gt;</metadata>"));
        assert_eq!(model.matches("<base ").count(), 1, "one material shared by both bodies");
        assert!(model.contains(r##"<base name="ABS" displaycolor="#B8BFCC"/>"##));
        assert!(model.contains(r#"<object id="2" type="model" name="Body 1" pid="1" pindex="0">"#));
        assert!(model.contains(r#"<object id="3" type="model" name="Body 2" pid="1" pindex="0">"#));
        assert_eq!(model.matches("<vertex ").count(), 16);
        assert_eq!(model.matches("<triangle ").count(), 24);
        // The far corner (60, 20, 30) turned so +Y is up.
        assert!(model.contains(r#"<vertex x="60" y="-30" z="20"/>"#));
        assert!(model.contains("<item objectid=\"2\"/>\n  <item objectid=\"3\"/>"));
    }

    #[test]
    fn package_without_material_has_no_material_group() {
        let pkg = Package {
            title: "Part1".into(),
            objects: vec![Object { name: "Part1".into(), solid: cube(), material: None }],
        };
        let model = model_xml(&pkg);
        assert!(!model.contains("basematerials"));
        assert!(model.contains(r#"<object id="2" type="model" name="Part1">"#));
    }

    #[test]
    fn output_is_reproducible() {
        let pkg = Package {
            title: "Part1".into(),
            objects: vec![Object { name: "Part1".into(), solid: cube(), material: None }],
        };
        assert_eq!(write(&pkg).unwrap(), write(&pkg).unwrap());
    }

    #[test]
    fn damaged_packages_are_errors() {
        let pkg = Package {
            title: "Part1".into(),
            objects: vec![Object { name: "Part1".into(), solid: cube(), material: None }],
        };
        let good = write(&pkg).unwrap();
        assert!(read_parts(&[]).is_err());
        assert!(read_parts(&good[..good.len() / 2]).is_err());
        let mut flipped = good.clone();
        flipped[60] ^= 0xFF; // inside the first entry's compressed data
        assert!(read_parts(&flipped).is_err());
        assert_eq!(read_parts(&good).unwrap().len(), 3);
    }
}
