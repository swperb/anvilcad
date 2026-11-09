//! Visual evaluation: the display mesh is rendered as a depth and normal image from
//! several directions and compared, pixel by pixel, with the same view ray-cast through
//! the analytic reference. A missing or extra face, a crack, a face bulging off its
//! surface or a shading normal pointing the wrong way all show up as bad pixels, on any
//! machine, without a GPU and without golden images that drift between drivers.

use crate::csg::Tree;
use anvil_brep::tess::TriMesh;
use glam::DVec3;
use serde::Serialize;
use std::path::Path;

/// An orthographic view looking along `dir`.
#[derive(Debug, Clone, Copy)]
pub struct View {
    pub name: &'static str,
    pub dir: DVec3,
    pub up: DVec3,
}

pub const VIEWS: [View; 4] = [
    View { name: "top", dir: DVec3::new(0.0, 0.0, -1.0), up: DVec3::Y },
    View { name: "front", dir: DVec3::new(0.0, 1.0, 0.0), up: DVec3::Z },
    View { name: "right", dir: DVec3::new(-1.0, 0.0, 0.0), up: DVec3::Z },
    View {
        name: "iso",
        dir: DVec3::new(-0.577_350_269_189_625_8, 0.577_350_269_189_625_8, -0.577_350_269_189_625_8),
        up: DVec3::Z,
    },
];

/// One rendered image: per pixel, the depth along the view (infinite where nothing is
/// hit) and the unit normal there.
#[derive(Debug, Clone)]
pub struct Image {
    pub n: usize,
    pub depth: Vec<f64>,
    pub normal: Vec<DVec3>,
}

/// The camera for a view of a box: pixel centres, the direction and the pixel size.
struct Camera {
    origin: DVec3,
    u: DVec3,
    v: DVec3,
    w: DVec3,
    pixel: f64,
}

impl Camera {
    fn new(view: &View, lo: DVec3, hi: DVec3, n: usize) -> Camera {
        let w = view.dir.normalize();
        let u = view.up.cross(w).normalize();
        let v = w.cross(u);
        let center = (lo + hi) * 0.5;
        let radius = (hi - lo).length() * 0.5 * 1.04 + 1e-9;
        Camera {
            origin: center - w * (radius * 2.0) - u * radius - v * radius,
            u,
            v,
            w,
            pixel: 2.0 * radius / n as f64,
        }
    }

    fn ray(&self, i: usize, j: usize) -> DVec3 {
        self.origin + self.u * ((i as f64 + 0.5) * self.pixel) + self.v * ((j as f64 + 0.5) * self.pixel)
    }
}

pub fn render_reference(tree: &Tree, view: &View, lo: DVec3, hi: DVec3, n: usize) -> Image {
    let cam = Camera::new(view, lo, hi, n);
    let mut img = Image { n, depth: vec![f64::INFINITY; n * n], normal: vec![DVec3::ZERO; n * n] };
    for j in 0..n {
        for i in 0..n {
            let o = cam.ray(i, j);
            if let Some(s) = tree.spans(o, cam.w).into_iter().find(|s| s.t0 >= 0.0 && s.t0.is_finite()) {
                img.depth[j * n + i] = s.t0;
                img.normal[j * n + i] = s.n0;
            }
        }
    }
    img
}

pub fn render_mesh(meshes: &[TriMesh], view: &View, lo: DVec3, hi: DVec3, n: usize) -> Image {
    let cam = Camera::new(view, lo, hi, n);
    let mut img = Image { n, depth: vec![f64::INFINITY; n * n], normal: vec![DVec3::ZERO; n * n] };
    for mesh in meshes {
        let proj: Vec<DVec3> = mesh
            .positions
            .iter()
            .map(|p| {
                let d = *p - cam.origin;
                DVec3::new(d.dot(cam.u) / cam.pixel - 0.5, d.dot(cam.v) / cam.pixel - 0.5, d.dot(cam.w))
            })
            .collect();
        for t in &mesh.triangles {
            let [a, b, c] = t.map(|k| proj[k as usize]);
            let [na, nb, nc] = t.map(|k| mesh.normals.get(k as usize).copied().unwrap_or(DVec3::ZERO));
            let area = (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x);
            if area.abs() < 1e-14 {
                continue;
            }
            let x0 = a.x.min(b.x).min(c.x).ceil().max(0.0) as usize;
            let x1 = (a.x.max(b.x).max(c.x).floor() as i64).min(n as i64 - 1);
            let y0 = a.y.min(b.y).min(c.y).ceil().max(0.0) as usize;
            let y1 = (a.y.max(b.y).max(c.y).floor() as i64).min(n as i64 - 1);
            if x1 < 0 || y1 < 0 {
                continue;
            }
            for j in y0..=(y1 as usize) {
                for i in x0..=(x1 as usize) {
                    let (px, py) = (i as f64, j as f64);
                    let w0 = ((b.x - px) * (c.y - py) - (b.y - py) * (c.x - px)) / area;
                    let w1 = ((c.x - px) * (a.y - py) - (c.y - py) * (a.x - px)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-9 || w1 < -1e-9 || w2 < -1e-9 {
                        continue;
                    }
                    let z = w0 * a.z + w1 * b.z + w2 * c.z;
                    let k = j * n + i;
                    if z < img.depth[k] {
                        img.depth[k] = z;
                        img.normal[k] = (na * w0 + nb * w1 + nc * w2).normalize_or_zero();
                    }
                }
            }
        }
    }
    img
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Diff {
    pub view: String,
    /// Pixels judged (hit by either image, away from silhouettes and creases).
    pub judged: usize,
    /// Pixels covered by one image and not the other.
    pub coverage: usize,
    /// Pixels where both hit but the depths differ by more than the tolerance.
    pub depth: usize,
    /// Pixels where the shading normals differ by more than 20 degrees.
    pub normal: usize,
}

impl Diff {
    pub fn bad_fraction(&self) -> f64 {
        (self.coverage + self.depth + self.normal) as f64 / self.judged.max(1) as f64
    }
}

/// Compares a mesh image with the reference. Pixels next to a silhouette, a depth step
/// or a crease in the reference are not judged: a pixel there legitimately lands on
/// either side.
pub fn compare(view: &View, reference: &Image, mesh: &Image, depth_tol: f64) -> (Diff, Vec<u8>) {
    let n = reference.n;
    let mut d = Diff { view: view.name.into(), ..Diff::default() };
    let mut mask = vec![0u8; n * n];
    let pixel_step = |k: usize, l: usize| {
        let (a, b) = (reference.depth[k], reference.depth[l]);
        a.is_finite() != b.is_finite()
            || (a.is_finite() && (a - b).abs() > 4.0 * depth_tol.max(1e-9) + 1e-9)
            || reference.normal[k].dot(reference.normal[l]) < 0.94
    };
    for j in 0..n {
        for i in 0..n {
            let k = j * n + i;
            let (r, m) = (reference.depth[k], mesh.depth[k]);
            if !r.is_finite() && !m.is_finite() {
                continue;
            }
            let mut edge = false;
            for (di, dj) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1), (-1, -1), (1, 1), (-1, 1), (1, -1)] {
                let (a, b) = (i as i64 + di, j as i64 + dj);
                if a < 0 || b < 0 || a >= n as i64 || b >= n as i64 {
                    continue;
                }
                if pixel_step(k, b as usize * n + a as usize) {
                    edge = true;
                    break;
                }
            }
            if edge {
                continue;
            }
            d.judged += 1;
            if r.is_finite() != m.is_finite() {
                d.coverage += 1;
                mask[k] = 1;
            } else if (r - m).abs() > depth_tol {
                d.depth += 1;
                mask[k] = 2;
            } else if reference.normal[k].dot(mesh.normal[k]) < 0.94 {
                d.normal += 1;
                mask[k] = 3;
            }
        }
    }
    (d, mask)
}

fn shade(img: &Image, k: usize, view: &View) -> u8 {
    if !img.depth[k].is_finite() {
        return 255;
    }
    let light = (-view.dir + view.up * 0.5).normalize();
    let l = img.normal[k].dot(light).clamp(-1.0, 1.0);
    (40.0 + 180.0 * (0.5 + 0.5 * l)) as u8
}

/// Writes reference, mesh and difference side by side for each view, one row per view:
/// grey shading, with bad pixels red (coverage), blue (depth) or green (normal).
pub fn write_png(path: &Path, rows: &[(View, Image, Image, Vec<u8>)]) -> std::io::Result<()> {
    let Some(first) = rows.first() else { return Ok(()) };
    let n = first.1.n;
    let (w, h) = (3 * n + 2 * 4, rows.len() * (n + 4));
    let mut px = vec![255u8; w * h * 3];
    for (r, (view, reference, mesh, mask)) in rows.iter().enumerate() {
        for j in 0..n {
            for i in 0..n {
                // Images are stored bottom-up.
                let k = j * n + i;
                let y = r * (n + 4) + (n - 1 - j);
                let put = |px: &mut Vec<u8>, x: usize, rgb: [u8; 3]| {
                    let o = (y * w + x) * 3;
                    px[o..o + 3].copy_from_slice(&rgb);
                };
                let a = shade(reference, k, view);
                let b = shade(mesh, k, view);
                put(&mut px, i, [a; 3]);
                put(&mut px, n + 4 + i, [b; 3]);
                let rgb = match mask[k] {
                    1 => [220, 30, 30],
                    2 => [30, 60, 220],
                    3 => [30, 170, 60],
                    _ => [a.saturating_add(20); 3],
                };
                put(&mut px, 2 * (n + 4) + i, rgb);
            }
        }
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(std::io::Error::other)?;
    writer.write_image_data(&px).map_err(std::io::Error::other)
}
