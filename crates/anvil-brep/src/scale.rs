//! Scaling a solid about a point.
//!
//! A uniform scale keeps every kind of surface and curve (planes stay planes, circles
//! circles, cones keep their angle), so any solid can be scaled. Scaling by different
//! factors along the axes of a frame turns circles into ellipses and cylinders into
//! elliptic cylinders, which the kernel does not hold, so a non-uniform scale is only
//! done for solids of flat faces and straight edges (and B-splines, whose control nets
//! scale exactly). See `docs/re/multibody.md` ("Scale").

use crate::geom::{Curve, Frame, Surface};
use crate::topo::{Edge, Face, Solid, Vertex};
use crate::{KernelError, Result, check};
use glam::DVec3;
use std::sync::Arc;

/// `solid` scaled about `center` by `factors` along the axes of `axes` (the world axes
/// for [`Frame::world`]). The returned solid has passed [`check::validate`].
pub fn scaled(solid: &Solid, center: DVec3, axes: &Frame, factors: [f64; 3]) -> Result<Solid> {
    if factors.iter().any(|f| !f.is_finite() || *f <= 0.0) {
        return Err(KernelError::InvalidInput("scale factors must be greater than zero".into()));
    }
    let uniform = (factors[0] - factors[1]).abs() < 1e-12 && (factors[1] - factors[2]).abs() < 1e-12;
    let map = |p: DVec3| {
        let d = p - center;
        let l =
            DVec3::new(d.dot(axes.x) * factors[0], d.dot(axes.y) * factors[1], d.dot(axes.z) * factors[2]);
        center + axes.x * l.x + axes.y * l.y + axes.z * l.z
    };
    let dir = |v: DVec3| map(center + v) - center;
    let k = factors[0];
    let frame = |f: &Frame| Frame { origin: map(f.origin), ..*f };
    let curved = || KernelError::Unsupported("a non-uniform scale would make round faces elliptic".into());

    let surfaces = solid
        .faces
        .iter()
        .map(|f| {
            Ok(match f.surface {
                Surface::Plane { frame: fr } => {
                    let (x, y) = (dir(fr.x), dir(fr.y));
                    Surface::Plane { frame: Frame::new(map(fr.origin), x.cross(y), x)? }
                }
                Surface::BSpline(ref s) => Surface::BSpline(Arc::new(s.mapped(map, frame(&s.frame)))),
                _ if !uniform => return Err(curved()),
                Surface::Cylinder { frame: fr, radius } => {
                    Surface::Cylinder { frame: frame(&fr), radius: radius * k }
                }
                Surface::Cone { frame: fr, radius, half_angle, apex } => {
                    Surface::Cone { frame: frame(&fr), radius: radius * k, half_angle, apex }
                }
                Surface::Torus { frame: fr, major, minor, v_mid } => {
                    Surface::Torus { frame: frame(&fr), major: major * k, minor: minor * k, v_mid }
                }
                Surface::Sphere { frame: fr, radius } => {
                    Surface::Sphere { frame: frame(&fr), radius: radius * k }
                }
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let edges = solid
        .edges
        .iter()
        .map(|e| {
            Ok(match e.curve {
                Curve::Line { origin, dir: d } => {
                    let o = map(origin);
                    let nd = dir(d);
                    let len = nd.length();
                    Edge {
                        curve: Curve::Line { origin: o, dir: nd / len },
                        t0: e.t0 * len,
                        t1: e.t1 * len,
                        ..e.clone()
                    }
                }
                Curve::Spline(ref s) => Edge { curve: Curve::Spline(Arc::new(s.mapped(map))), ..e.clone() },
                _ if !uniform => return Err(curved()),
                Curve::Circle { frame: fr, radius } => {
                    Edge { curve: Curve::Circle { frame: frame(&fr), radius: radius * k }, ..e.clone() }
                }
                Curve::Ellipse { frame: fr, a, b } => {
                    Edge { curve: Curve::Ellipse { frame: frame(&fr), a: a * k, b: b * k }, ..e.clone() }
                }
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let out = Solid {
        vertices: solid.vertices.iter().map(|v| Vertex { point: map(v.point) }).collect(),
        edges,
        faces: solid.faces.iter().zip(surfaces).map(|(f, surface)| Face { surface, ..f.clone() }).collect(),
    };
    check::validate(&out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{Loop2, Segment, build_regions};
    use crate::{extrude::extrude, mass, tess};
    use glam::DVec2;

    fn volume(s: &Solid) -> f64 {
        let m = tess::tessellate(s, &tess::TessParams { chord: 1e-4, max_angle: 0.01 }).expect("tess");
        assert!(m.is_watertight());
        mass::properties(&m).volume
    }

    fn plate() -> Solid {
        let sq = Loop2::new(vec![
            Segment::Line { a: DVec2::new(0.0, 0.0), b: DVec2::new(10.0, 0.0) },
            Segment::Line { a: DVec2::new(10.0, 0.0), b: DVec2::new(10.0, 10.0) },
            Segment::Line { a: DVec2::new(10.0, 10.0), b: DVec2::new(0.0, 10.0) },
            Segment::Line { a: DVec2::new(0.0, 10.0), b: DVec2::new(0.0, 0.0) },
        ]);
        let hole = Loop2::new(vec![Segment::circle(DVec2::new(5.0, 5.0), 2.0)]);
        extrude(&Frame::world(), &build_regions(&[sq, hole]).expect("r")[0], 0.0, 4.0).expect("plate")
    }

    #[test]
    fn uniform_scale_keeps_round_faces() {
        let s = plate();
        let v = volume(&s);
        let t = scaled(&s, DVec3::ZERO, &Frame::world(), [2.0, 2.0, 2.0]).expect("scaled");
        assert!((volume(&t) - 8.0 * v).abs() < 1e-6 * v);
    }

    #[test]
    fn non_uniform_scale_of_a_box_and_refusal_of_round_faces() {
        let sq = Loop2::new(vec![
            Segment::Line { a: DVec2::new(0.0, 0.0), b: DVec2::new(10.0, 0.0) },
            Segment::Line { a: DVec2::new(10.0, 0.0), b: DVec2::new(10.0, 10.0) },
            Segment::Line { a: DVec2::new(10.0, 10.0), b: DVec2::new(0.0, 10.0) },
            Segment::Line { a: DVec2::new(0.0, 10.0), b: DVec2::new(0.0, 0.0) },
        ]);
        let b = extrude(&Frame::world(), &build_regions(&[sq]).expect("r")[0], 0.0, 10.0).expect("box");
        let t = scaled(&b, DVec3::splat(5.0), &Frame::world(), [1.0, 2.0, 3.0]).expect("scaled");
        assert!((volume(&t) - 6000.0).abs() < 1e-9);
        assert!(scaled(&plate(), DVec3::ZERO, &Frame::world(), [1.0, 2.0, 1.0]).is_err());
        assert!(scaled(&b, DVec3::ZERO, &Frame::world(), [0.0, 1.0, 1.0]).is_err());
    }
}
