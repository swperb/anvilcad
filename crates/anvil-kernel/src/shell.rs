//! Shell: hollow a solid to a uniform wall thickness, optionally opening selected faces.
//!
//! The cavity is the solid offset inward by the wall thickness. Every face plane moves
//! inward by `thickness`, except removed faces, which move outward so the cavity breaks
//! through them. Each vertex moves to where its (offset) face planes meet, found as the
//! smallest displacement satisfying all of them, which is exact wherever up to three
//! distinct planes meet and a least-squares fit on finely faceted curved regions. The
//! shell is the solid minus that cavity.

use crate::math::Vec3;
use crate::solid::{FaceId, FaceTag, Polygon, Solid, SurfaceKind};
use std::collections::{BTreeSet, HashMap};

#[derive(Clone, Debug, PartialEq)]
pub enum ShellError {
    Invalid(String),
    /// The wall is too thick for the part: the cavity would turn inside out.
    TooThick,
    /// The wall is thicker than the radius of a curved region, so its offset folds over.
    FoldsOver,
}

impl std::fmt::Display for ShellError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShellError::Invalid(s) => write!(f, "{s}"),
            ShellError::TooThick => write!(f, "the wall thickness is too large for this part"),
            ShellError::FoldsOver => {
                write!(f, "the wall is thicker than the radius of a curved region (such as a small fillet)")
            }
        }
    }
}

impl std::error::Error for ShellError {}

type Key = (i64, i64, i64);

fn key(v: Vec3) -> Key {
    let s = 1e6;
    ((v.x * s).round() as i64, (v.y * s).round() as i64, (v.z * s).round() as i64)
}

/// Minimum-norm displacement d with n_i . d = delta_i for every plane (least squares when
/// the planes are inconsistent).
fn displacement(planes: &[(Vec3, f64)]) -> Vec3 {
    let k = planes.len();
    // Solve (N N^T + eps I) lambda = delta, then d = N^T lambda.
    let mut g = vec![0.0; k * k];
    let mut rhs: Vec<f64> = planes.iter().map(|p| p.1).collect();
    for i in 0..k {
        for j in 0..k {
            g[i * k + j] = planes[i].0.dot(planes[j].0);
        }
        g[i * k + i] += 1e-9;
    }
    // Gaussian elimination with partial pivoting.
    for col in 0..k {
        let piv =
            (col..k).max_by(|&a, &b| g[a * k + col].abs().total_cmp(&g[b * k + col].abs())).unwrap_or(col);
        if piv != col {
            for c in 0..k {
                g.swap(piv * k + c, col * k + c);
            }
            rhs.swap(piv, col);
        }
        let d = g[col * k + col];
        if d.abs() < 1e-300 {
            continue;
        }
        for r in col + 1..k {
            let f = g[r * k + col] / d;
            if f != 0.0 {
                for c in col..k {
                    g[r * k + c] -= f * g[col * k + c];
                }
                rhs[r] -= f * rhs[col];
            }
        }
    }
    let mut lambda = vec![0.0; k];
    for i in (0..k).rev() {
        let mut s = rhs[i];
        for c in i + 1..k {
            s -= g[i * k + c] * lambda[c];
        }
        lambda[i] = if g[i * k + i].abs() > 1e-300 { s / g[i * k + i] } else { 0.0 };
    }
    planes.iter().zip(&lambda).fold(Vec3::ZERO, |acc, (p, l)| acc + p.0 * *l)
}

/// Hollows `solid` with walls of `thickness`, removing (opening) the faces in `remove`.
/// Cavity faces are tagged with `feature`.
pub fn shell(solid: &Solid, remove: &[FaceId], thickness: f64, feature: u32) -> Result<Solid, ShellError> {
    if thickness.is_nan() || thickness <= 0.0 {
        return Err(ShellError::Invalid("the wall thickness must be greater than zero".into()));
    }
    if solid.is_empty() {
        return Err(ShellError::Invalid("there is no body to shell".into()));
    }
    let ids: BTreeSet<FaceId> = solid.polygons.iter().map(|p| p.tag.id).collect();
    for r in remove {
        if !ids.contains(r) {
            return Err(ShellError::Invalid(format!(
                "face <{}:{}> chosen for removal no longer exists",
                r.feature, r.index
            )));
        }
    }
    if remove.len() >= ids.len() {
        return Err(ShellError::Invalid("at least one face must remain".into()));
    }

    // Distinct offset planes meeting at each vertex: (normal, required displacement).
    let mut planes_at: HashMap<Key, Vec<(Vec3, f64)>> = HashMap::new();
    for p in &solid.polygons {
        let delta = if remove.contains(&p.tag.id) { thickness } else { -thickness };
        for &v in &p.verts {
            let list = planes_at.entry(key(v)).or_default();
            // The same plane (and offset) can appear through many polygons of one face.
            let dup = list.iter().any(|(n, d)| (*n - p.normal).length() < 1e-9 && (*d - delta).abs() < 1e-12);
            if !dup {
                list.push((p.normal, delta));
            }
        }
    }
    let moved: HashMap<Key, Vec3> = planes_at.iter().map(|(k, planes)| (*k, displacement(planes))).collect();

    // Cavity: same topology, moved vertices, split into triangles so faces stay planar.
    let index_of: HashMap<FaceId, u32> = ids.iter().enumerate().map(|(i, id)| (*id, 2 + i as u32)).collect();
    let mut cavity = Vec::new();
    for p in &solid.polygons {
        let tag = FaceTag {
            id: FaceId { feature, index: index_of[&p.tag.id] },
            surface: if p.tag.surface == SurfaceKind::Plane {
                SurfaceKind::Plane
            } else {
                SurfaceKind::Other
            },
        };
        let vs: Vec<Vec3> = p.verts.iter().map(|&v| v + moved[&key(v)]).collect();
        for i in 1..vs.len().saturating_sub(1) {
            if let Some(t) = Polygon::new(vec![vs[0], vs[i], vs[i + 1]], tag) {
                // An offset triangle that turned over means the offset surface crossed
                // itself; the result would be wrong, so refuse.
                if t.normal.dot(p.normal) < 0.0 && t.area() > 1e-9 {
                    return Err(ShellError::FoldsOver);
                }
                cavity.push(t);
            }
        }
    }
    let cavity = Solid::new(cavity);
    let (v_body, v_cav) = (solid.mass_properties().volume, cavity.mass_properties().volume);
    if v_cav.is_nan() || v_cav <= 0.0 || v_cav >= v_body + 1e-9 && remove.is_empty() {
        return Err(ShellError::TooThick);
    }
    let out = solid.subtract(&cavity);
    if out.is_empty() {
        return Err(ShellError::TooThick);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::{block, cylinder};
    use crate::math::v3;

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / b.abs().max(1e-9)
    }

    #[test]
    fn closed_box_shell() {
        let b = block(v3(0.0, 0.0, 0.0), v3(20.0, 10.0, 10.0), 1).unwrap();
        let s = shell(&b, &[], 1.0, 7).unwrap();
        let v = s.mass_properties().volume;
        assert!(rel(v, 2000.0 - 18.0 * 8.0 * 8.0) < 1e-9, "{v}");
        assert_eq!(s.open_edges(), 0);
    }

    #[test]
    fn open_top_box() {
        // Like a SOLIDWORKS shell with the top face removed.
        let b = block(v3(0.0, 0.0, 0.0), v3(20.0, 10.0, 10.0), 1).unwrap();
        let top = FaceId { feature: 1, index: FaceId::END_CAP };
        let s = shell(&b, &[top], 1.0, 7).unwrap();
        let v = s.mass_properties().volume;
        // Cavity is 18 x 8 x 9 (floor 1 thick, open at the top).
        assert!(rel(v, 2000.0 - 18.0 * 8.0 * 9.0) < 1e-9, "{v}");
        assert_eq!(s.open_edges(), 0);
    }

    #[test]
    fn cylinder_cup() {
        let c = cylinder(v3(0.0, 0.0, 0.0), 10.0, 20.0, 1).unwrap();
        let top = FaceId { feature: 1, index: FaceId::END_CAP };
        let s = shell(&c, &[top], 2.0, 7).unwrap();
        let outer = c.mass_properties().volume;
        // The faceted cavity has radius ~8 and depth 18.
        let cavity = outer * (8.0 / 10.0) * (8.0 / 10.0) * (18.0 / 20.0);
        let v = s.mass_properties().volume;
        assert!(rel(v, outer - cavity) < 0.01, "{v} vs {}", outer - cavity);
    }

    #[test]
    fn l_shape_keeps_concave_corner() {
        let plate = block(v3(0.0, 0.0, 0.0), v3(20.0, 10.0, 4.0), 1).unwrap();
        let wall = block(v3(0.0, 0.0, 0.0), v3(4.0, 10.0, 20.0), 2).unwrap();
        let l = plate.union(&wall);
        let s = shell(&l, &[], 1.0, 7).unwrap();
        // Inward offset of an L is an L: legs 18 x 2 and 2 x 18 overlapping in 2 x 2,
        // depth 8.
        let cavity = (18.0 * 2.0 + 2.0 * 18.0 - 2.0 * 2.0) * 8.0;
        let v = s.mass_properties().volume;
        assert!(rel(v, l.mass_properties().volume - cavity) < 1e-6, "{v}");
    }

    #[test]
    fn too_thick() {
        let b = block(v3(0.0, 0.0, 0.0), v3(10.0, 10.0, 2.0), 1).unwrap();
        assert!(matches!(shell(&b, &[], 1.5, 7), Err(ShellError::TooThick | ShellError::FoldsOver)));
    }
}

#[cfg(test)]
mod curved_tests {
    use super::*;
    use crate::build::revolve;
    use crate::math::{v2, Frame};
    use crate::profile::{Loop2, Profile, Seg2};

    #[test]
    fn sphere_shell() {
        // Half disc revolved into a faceted sphere of radius 10.
        let lp = Loop2::new(vec![
            Seg2::Arc {
                center: v2(0.0, 0.0),
                radius: 10.0,
                start: -std::f64::consts::FRAC_PI_2,
                sweep: std::f64::consts::PI,
                source: 0,
            },
            Seg2::Line { a: v2(0.0, 10.0), b: v2(0.0, -10.0), source: 1 },
        ]);
        let s = revolve(
            &[Profile::new(lp, vec![])],
            &Frame::WORLD,
            v2(0.0, 0.0),
            v2(0.0, 1.0),
            std::f64::consts::TAU,
            1,
        )
        .unwrap();
        let outer = s.mass_properties().volume;
        let sh = shell(&s, &[], 1.0, 7).unwrap();
        let v = sh.mass_properties().volume;
        let expected = outer * (1.0 - 0.729);
        println!("outer {outer:.1} shell {v:.1} expected {expected:.1}");
        assert!((v - expected).abs() / expected < 0.03, "{v} vs {expected}");
    }
}
