//! Reflection of a solid in a plane.
//!
//! A reflection is not a rigid motion: it turns right-handed frames left-handed and
//! turns a face's loops clockwise when seen from outside. So the mirrored solid is
//! rebuilt rather than moved:
//!
//! - every point is reflected;
//! - each frame-based surface gets the right-handed frame `(M o, -M x, M y, M z)`,
//!   which describes the reflected point set and keeps the surface's natural normal
//!   equal to the reflected normal (planes, cylinders, cones, tori and spheres all
//!   take their natural normal from the axis or from the centre);
//! - a B-spline patch's control net is reflected, which turns its natural normal
//!   (along ds x dt) the other way, so its face's `reversed` flag is toggled;
//! - circles and ellipses run the other way round in the new frame (parameter `t`
//!   becomes `pi - t`), so those edges swap their ends;
//! - every loop is reversed so it runs counter-clockwise seen from outside again.

use crate::geom::{Curve, Frame, Surface};
use crate::topo::{Coedge, Edge, Face, Loop, Solid, Vertex};
use crate::{KernelError, Result, check, tol};
use glam::DVec3;
use std::f64::consts::PI;
use std::sync::Arc;

/// A reflection in the plane through `origin` with normal `normal`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reflection {
    origin: DVec3,
    normal: DVec3,
}

impl Reflection {
    pub fn new(origin: DVec3, normal: DVec3) -> Result<Self> {
        if !tol::is_finite3(origin) || !tol::is_finite3(normal) {
            return Err(KernelError::NonFinite);
        }
        let normal = normal.try_normalize().ok_or(KernelError::InvalidFrame)?;
        Ok(Self { origin, normal })
    }

    pub fn point(&self, p: DVec3) -> DVec3 {
        p - self.normal * (2.0 * (p - self.origin).dot(self.normal))
    }

    pub fn dir(&self, d: DVec3) -> DVec3 {
        d - self.normal * (2.0 * d.dot(self.normal))
    }

    /// The right-handed frame describing the reflection of `f`'s point set, with
    /// `x` negated so that `z` is the reflected `z`.
    pub fn frame(&self, f: &Frame) -> Frame {
        Frame { origin: self.point(f.origin), x: -self.dir(f.x), y: self.dir(f.y), z: self.dir(f.z) }
    }

    fn surface(&self, s: &Surface) -> (Surface, bool) {
        match *s {
            Surface::Plane { frame } => (Surface::Plane { frame: self.frame(&frame) }, false),
            Surface::Cylinder { frame, radius } => {
                (Surface::Cylinder { frame: self.frame(&frame), radius }, false)
            }
            Surface::Cone { frame, radius, half_angle, apex } => {
                (Surface::Cone { frame: self.frame(&frame), radius, half_angle, apex }, false)
            }
            Surface::Torus { frame, major, minor, v_mid } => {
                (Surface::Torus { frame: self.frame(&frame), major, minor, v_mid }, false)
            }
            Surface::Sphere { frame, radius } => {
                (Surface::Sphere { frame: self.frame(&frame), radius }, false)
            }
            Surface::BSpline(ref p) => {
                let q = p.mapped(|c| self.point(c), self.frame(&p.frame));
                (Surface::BSpline(Arc::new(q)), true)
            }
        }
    }

    /// The reflected edge, and whether it now runs the other way.
    fn edge(&self, e: &Edge) -> (Edge, bool) {
        match e.curve {
            Curve::Line { origin, dir } => (
                Edge { curve: Curve::Line { origin: self.point(origin), dir: self.dir(dir) }, ..e.clone() },
                false,
            ),
            Curve::Circle { frame, radius } => (
                Edge {
                    curve: Curve::Circle { frame: self.frame(&frame), radius },
                    start: e.end,
                    end: e.start,
                    t0: PI - e.t1,
                    t1: PI - e.t0,
                },
                true,
            ),
            Curve::Ellipse { frame, a, b } => (
                Edge {
                    curve: Curve::Ellipse { frame: self.frame(&frame), a, b },
                    start: e.end,
                    end: e.start,
                    t0: PI - e.t1,
                    t1: PI - e.t0,
                },
                true,
            ),
            Curve::Spline(ref s) => {
                (Edge { curve: Curve::Spline(Arc::new(s.mapped(|p| self.point(p)))), ..e.clone() }, false)
            }
        }
    }
}

/// `solid` reflected in the plane through `origin` with normal `normal`. Face tags and
/// owners carry over, so a mirrored face keeps the name of the face it copies.
pub fn mirrored(solid: &Solid, origin: DVec3, normal: DVec3) -> Result<Solid> {
    let r = Reflection::new(origin, normal)?;
    mirror_with(solid, &r)
}

pub fn mirror_with(solid: &Solid, r: &Reflection) -> Result<Solid> {
    let vertices = solid.vertices.iter().map(|v| Vertex { point: r.point(v.point) }).collect();
    let mut turned = Vec::with_capacity(solid.edges.len());
    let edges = solid
        .edges
        .iter()
        .map(|e| {
            let (edge, flip) = r.edge(e);
            turned.push(flip);
            edge
        })
        .collect();
    let reverse = |l: &Loop| Loop {
        coedges: l
            .coedges
            .iter()
            .rev()
            .map(|c| Coedge { edge: c.edge, reversed: c.reversed ^ !turned[c.edge.index()] })
            .collect(),
    };
    let faces = solid
        .faces
        .iter()
        .map(|f| {
            let (surface, toggle) = r.surface(&f.surface);
            Face {
                surface,
                reversed: f.reversed ^ toggle,
                outer: reverse(&f.outer),
                inners: f.inners.iter().map(reverse).collect(),
                tag: f.tag,
                owner: f.owner,
            }
        })
        .collect();
    let out = Solid { vertices, edges, faces };
    check::validate(&out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boolean::{BoolOp, boolean};
    use crate::extrude::extrude;
    use crate::profile::{Loop2, Segment, build_regions};
    use crate::revolve::revolve;
    use crate::{mass, tess};
    use glam::DVec2;
    use std::f64::consts::TAU;

    fn volume(s: &Solid) -> f64 {
        let m = tess::tessellate(s, &tess::TessParams { chord: 1e-4, max_angle: 0.01 }).expect("tess");
        assert!(m.is_watertight(), "not watertight");
        mass::properties(&m).volume
    }

    fn centroid(s: &Solid) -> DVec3 {
        let m = tess::tessellate(s, &tess::TessParams { chord: 1e-4, max_angle: 0.01 }).expect("tess");
        mass::properties(&m).centroid
    }

    fn polygon(pts: &[(f64, f64)]) -> Loop2 {
        let p: Vec<DVec2> = pts.iter().map(|&(x, y)| DVec2::new(x, y)).collect();
        Loop2::new((0..p.len()).map(|i| Segment::Line { a: p[i], b: p[(i + 1) % p.len()] }).collect())
    }

    /// A 20 x 10 plate, 5 thick, with a round hole, sitting at x 5..25.
    fn plate() -> Solid {
        let outer = polygon(&[(5.0, 0.0), (25.0, 0.0), (25.0, 10.0), (5.0, 10.0)]);
        let hole = Loop2::new(vec![Segment::circle(DVec2::new(15.0, 5.0), 3.0)]);
        let r = build_regions(&[outer, hole]).expect("region");
        extrude(&Frame::world(), &r[0], 0.0, 5.0).expect("extrude")
    }

    #[test]
    fn plate_mirrored_in_the_yz_plane() {
        let s = plate();
        let m = mirrored(&s, DVec3::ZERO, DVec3::X).expect("mirror");
        let exact = (200.0 - PI * 9.0) * 5.0;
        assert!((volume(&m) - exact).abs() / exact < 1e-4, "{}", volume(&m));
        let c = centroid(&m);
        assert!((c - DVec3::new(-15.0, 5.0, 2.5)).length() < 1e-3, "{c}");
        // Faces keep their names.
        assert_eq!(
            m.faces.iter().map(|f| f.tag).collect::<Vec<_>>(),
            s.faces.iter().map(|f| f.tag).collect::<Vec<_>>()
        );
    }

    #[test]
    fn mirrored_copy_joins_the_original_across_the_plane() {
        let s = plate();
        // Reflect in x = 5, the plate's left face: the two halves share that face.
        let m = mirrored(&s, DVec3::new(5.0, 0.0, 0.0), DVec3::X).expect("mirror");
        let u = boolean(&s, &m, BoolOp::Union).expect("union");
        assert_eq!(u.len(), 1);
        let exact = 2.0 * (200.0 - PI * 9.0) * 5.0;
        assert!((volume(&u[0]) - exact).abs() / exact < 1e-4, "{}", volume(&u[0]));
    }

    #[test]
    fn revolved_solid_in_a_tilted_plane() {
        // A ring with a round section (torus) and flat and conical faces: radii 2..5,
        // with a chamfered corner, turned a full turn.
        let r = build_regions(&[polygon(&[(2.0, 0.0), (5.0, 0.0), (5.0, 2.0), (4.0, 3.0), (2.0, 3.0)])])
            .expect("region");
        let s = revolve(&Frame::world(), &r[0], DVec2::ZERO, DVec2::Y, TAU).expect("revolve");
        let n = DVec3::new(1.0, 2.0, -0.5);
        let m = mirrored(&s, DVec3::new(10.0, 0.0, 3.0), n).expect("mirror");
        assert!((volume(&m) - volume(&s)).abs() / volume(&s) < 1e-4);
        // Reflecting twice gives the original back.
        let back = mirrored(&m, DVec3::new(10.0, 0.0, 3.0), n).expect("mirror");
        for (a, b) in back.vertices.iter().zip(&s.vertices) {
            assert!((a.point - b.point).length() < 1e-9);
        }
        assert!((volume(&back) - volume(&s)).abs() / volume(&s) < 1e-6);
    }

    #[test]
    fn partial_revolve_with_an_arc() {
        // A half-disc section turned through 90 degrees makes part of a sphere and
        // part-circle edges, which swap their ends when reflected.
        let r = build_regions(&[Loop2::new(vec![
            Segment::Arc { center: DVec2::new(0.0, 4.0), radius: 3.0, start: -0.5 * PI, sweep: PI },
            Segment::Line { a: DVec2::new(0.0, 7.0), b: DVec2::new(0.0, 1.0) },
        ])])
        .expect("region");
        let s = revolve(&Frame::world(), &r[0], DVec2::ZERO, DVec2::Y, 0.5 * PI).expect("revolve");
        let m = mirrored(&s, DVec3::ZERO, DVec3::Z).expect("mirror");
        let exact = 4.0 / 3.0 * PI * 27.0 / 4.0;
        assert!((volume(&m) - exact).abs() / exact < 1e-3, "{}", volume(&m));
        assert!(m.vertices.iter().all(|v| v.point.z <= 1e-9));
    }

    #[test]
    fn rejects_a_zero_normal() {
        assert!(mirrored(&plate(), DVec3::ZERO, DVec3::ZERO).is_err());
    }
}
