//! Dome: a curved cap raised on (or sunk into) a flat round face.
//!
//! The behaviour is specified in `docs/re/rib-draft-dome.md` (section "Dome" and
//! testable statements 19 to 23). A plain dome of height `h` on a flat face bounded by
//! a circle of radius `r` is the spherical cap through that circle, of sphere radius
//! `(r^2 + h^2) / (2h)` with its centre on the face's axis; it is added to the body, or
//! cut into it when reversed. A height of exactly 0 on the end of a cylinder of that
//! radius means the dome tangent to the cylinder, a hemisphere (`h = r`). Heights above
//! `r` overhang the face, which the specification allows.
//!
//! The cap is built as its own solid, the circle's disc turned with its arc about the
//! face's axis by [`crate::revolve::revolve`] (an exact sphere), and combined with the
//! body by [`crate::boolean::boolean`].
//!
//! The elliptical dome (half a spheroid) is not built: the kernel has no ellipsoid
//! surface and the 2D profile has no elliptical segment to revolve, so it is refused.
//! Domes on faces that are not round and flat are unknown in the specification and are
//! refused too.

use crate::boolean::{BoolOp, boolean};
use crate::geom::{Curve, Frame, Surface};
use crate::profile::{Loop2, Segment, build_regions};
use crate::revolve::revolve;
use crate::topo::Solid;
use crate::{KernelError, Result};
use glam::{DVec2, DVec3};
use std::f64::consts::{FRAC_PI_2, TAU};

fn refuse<T>(msg: impl Into<String>) -> Result<T> {
    Err(KernelError::Unsupported(msg.into()))
}

fn invalid<T>(msg: impl Into<String>) -> Result<T> {
    Err(KernelError::InvalidInput(msg.into()))
}

/// The solid with a dome of `height` on face `face`, a flat face bounded by one circle.
/// `reverse` cuts the dome into the face instead of raising it; `elliptical` asks for
/// half a spheroid, which is not supported yet. A height of 0 means the dome tangent to
/// the cylinder the face ends. The result may hold several bodies, as a boolean does.
pub fn dome(solid: &Solid, face: usize, height: f64, elliptical: bool, reverse: bool) -> Result<Vec<Solid>> {
    let Some(f) = solid.faces.get(face) else {
        return invalid(format!("face {face} is not in the solid"));
    };
    if !height.is_finite() || height < 0.0 {
        return invalid("a dome's height must be positive");
    }
    let Surface::Plane { frame } = f.surface else {
        return refuse("domes on faces that are not flat");
    };
    if !f.inners.is_empty() {
        return refuse("domes on faces with holes");
    }
    let normal = if f.reversed { -frame.z } else { frame.z };

    // The boundary: one circle (perhaps in several arcs) lying in the face's plane.
    let mut circle: Option<(DVec3, f64, DVec3)> = None;
    for c in &f.outer.coedges {
        let Some(e) = solid.edge(c.edge) else {
            return Err(KernelError::Topology("a face names a missing edge".into()));
        };
        let Curve::Circle { frame: cf, radius } = e.curve else {
            return refuse("domes on faces that are not bounded by a circle");
        };
        match circle {
            None => circle = Some((cf.origin, radius, cf.x)),
            Some((centre, r, _)) => {
                let tol = 1e-9 * (1.0 + r);
                if (cf.origin - centre).length() > tol || (radius - r).abs() > tol {
                    return refuse("domes on faces that are not bounded by a single circle");
                }
            }
        }
        if cf.z.cross(normal).length() > 1e-9 {
            return Err(KernelError::Topology("a flat face's boundary circle leaves its plane".into()));
        }
    }
    let Some((centre, r, x_hint)) = circle else {
        return Err(KernelError::Topology("a face has no boundary".into()));
    };

    let h = if height == 0.0 {
        if !ends_cylinder(solid, face, centre, r, normal) {
            return invalid("a dome of height 0 needs the face to end a cylinder of its radius");
        }
        r
    } else {
        height
    };
    if h < 1e-9 * (1.0 + r) {
        return invalid("a dome's height must be positive");
    }
    if elliptical {
        return refuse("elliptical domes (the kernel has no ellipsoid surface yet)");
    }

    // The cap's profile in the half-plane through the axis: x out from the axis, y along
    // the dome (out of the body, or into it when reversed). The arc is centred on the
    // axis, `h - R` along it.
    let up = if reverse { -normal } else { normal };
    let x = (x_hint - normal * x_hint.dot(normal)).try_normalize().ok_or(KernelError::InvalidFrame)?;
    let profile_frame = Frame { origin: centre, x, y: up, z: x.cross(up) };
    let big = (r * r + h * h) / (2.0 * h);
    let c = DVec2::new(0.0, h - big);
    let start = (-c.y).atan2(r);
    let profile = Loop2::new(vec![
        Segment::Line { a: DVec2::ZERO, b: DVec2::new(r, 0.0) },
        Segment::Arc { center: c, radius: big, start, sweep: FRAC_PI_2 - start },
        Segment::Line { a: DVec2::new(0.0, h), b: DVec2::ZERO },
    ]);
    let region = build_regions(&[profile])?
        .into_iter()
        .next()
        .ok_or_else(|| KernelError::InvalidInput("the dome's profile is empty".into()))?;
    let cap = revolve(&profile_frame, &region, DVec2::ZERO, DVec2::Y, TAU)?;
    boolean(solid, &cap, if reverse { BoolOp::Subtract } else { BoolOp::Union })
}

/// True when the face's circle is also the edge of a cylinder of radius `r` about the
/// face's axis, running away from the face (into the body).
fn ends_cylinder(solid: &Solid, face: usize, centre: DVec3, r: f64, normal: DVec3) -> bool {
    let Some(f) = solid.faces.get(face) else { return false };
    f.outer.coedges.iter().all(|c| {
        solid.edge_faces(c.edge).into_iter().filter(|g| g.index() != face).any(|g| {
            let Some(other) = solid.face(g) else { return false };
            let Surface::Cylinder { frame, radius } = other.surface else { return false };
            let d = centre - frame.origin;
            frame.z.cross(normal).length() < 1e-9
                && (d - frame.z * d.dot(frame.z)).length() < 1e-9 * (1.0 + r)
                && (radius - r).abs() < 1e-9 * (1.0 + r)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extrude::extrude;
    use crate::{check, mass, tess};
    use std::f64::consts::PI;

    fn cylinder(r: f64, h: f64) -> Solid {
        let region = build_regions(&[Loop2::new(vec![Segment::circle(DVec2::ZERO, r)])]).expect("region");
        extrude(&Frame::world(), &region[0], 0.0, h).expect("cylinder")
    }

    fn top(s: &Solid, z: f64) -> usize {
        (0..s.faces.len())
            .find(|&f| {
                matches!(s.faces[f].surface, Surface::Plane { frame } if (frame.origin.z - z).abs() < 1e-9)
            })
            .expect("top")
    }

    fn volume(solids: &[Solid]) -> f64 {
        solids
            .iter()
            .map(|s| {
                check::validate(s).expect("valid");
                let m =
                    tess::tessellate(s, &tess::TessParams { chord: 1e-4, max_angle: 0.01 }).expect("tess");
                assert!(m.is_watertight(), "not watertight");
                mass::properties(&m).volume
            })
            .sum()
    }

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * b.abs().max(1.0)
    }

    fn sphere(s: &Solid) -> (DVec3, f64) {
        s.faces
            .iter()
            .find_map(|f| match f.surface {
                Surface::Sphere { frame, radius } => Some((frame.origin, radius)),
                _ => None,
            })
            .expect("a spherical face")
    }

    #[test]
    fn spherical_cap_on_a_cylinder() {
        // Statement 19: height 5 on a cylinder of radius 10 and height 20.
        let c = cylinder(10.0, 20.0);
        let d = dome(&c, top(&c, 20.0), 5.0, false, false).expect("dome");
        assert_eq!(d.len(), 1);
        let (centre, radius) = sphere(&d[0]);
        assert!(close(radius, 12.5, 1e-12));
        assert!((centre - DVec3::new(0.0, 0.0, 12.5)).length() < 1e-9, "{centre:?}");
        let cap = PI * 5.0 * (300.0 + 25.0) / 6.0;
        assert!(close(cap, 850.848010, 1e-9));
        let want = 2000.0 * PI + cap;
        assert!(close(want, 7134.033318, 1e-9));
        assert!(close(volume(&d), want, 3e-5), "{} vs {want}", volume(&d));
        // Statement 20: reversed, the same cap is cut in.
        let d = dome(&c, top(&c, 20.0), 5.0, false, true).expect("dome");
        let (centre, _) = sphere(&d[0]);
        assert!((centre - DVec3::new(0.0, 0.0, 27.5)).length() < 1e-9, "{centre:?}");
        assert!(close(volume(&d), 5432.337297, 3e-5), "{}", volume(&d));
    }

    #[test]
    fn tangent_and_tall_domes() {
        let c = cylinder(10.0, 20.0);
        // Statement 21: height 0 is the hemisphere tangent to the cylinder.
        let d = dome(&c, top(&c, 20.0), 0.0, false, false).expect("dome");
        assert!(close(sphere(&d[0]).1, 10.0, 1e-12));
        assert!(close(volume(&d), 8377.580410, 3e-5), "{}", volume(&d));
        // Statement 23: height 15 overhangs the face, a cap of sphere radius 65 / 6.
        let d = dome(&c, top(&c, 20.0), 15.0, false, false).expect("dome");
        assert!(close(sphere(&d[0]).1, 10.833333, 1e-7));
        let cap = PI * 15.0 * (300.0 + 225.0) / 6.0;
        assert!(close(cap, 4123.340358, 1e-9));
        assert!(close(volume(&d), 2000.0 * PI + cap, 3e-5), "{}", volume(&d));
        // On the bottom face the dome goes down.
        let d = dome(&c, top(&c, 0.0), 5.0, false, false).expect("dome");
        let (centre, _) = sphere(&d[0]);
        assert!((centre - DVec3::new(0.0, 0.0, 7.5)).length() < 1e-9, "{centre:?}");
        assert!(close(volume(&d), 7134.033318, 3e-5), "{}", volume(&d));
    }

    #[test]
    fn unsupported_and_bad_domes() {
        let c = cylinder(10.0, 20.0);
        let t = top(&c, 20.0);
        // Statement 22's elliptical dome is not built yet.
        assert!(matches!(dome(&c, t, 5.0, true, false), Err(KernelError::Unsupported(_))));
        assert!(matches!(dome(&c, t, -1.0, false, false), Err(KernelError::InvalidInput(_))));
        assert!(matches!(dome(&c, t, f64::NAN, false, false), Err(KernelError::InvalidInput(_))));
        assert!(dome(&c, 99, 5.0, false, false).is_err());
        // Not flat, and not round.
        let side = (0..c.faces.len()).find(|&f| matches!(c.faces[f].surface, Surface::Cylinder { .. }));
        assert!(matches!(dome(&c, side.expect("side"), 5.0, false, false), Err(KernelError::Unsupported(_))));
        let square = Loop2::new(vec![
            Segment::Line { a: DVec2::new(0.0, 0.0), b: DVec2::new(10.0, 0.0) },
            Segment::Line { a: DVec2::new(10.0, 0.0), b: DVec2::new(10.0, 10.0) },
            Segment::Line { a: DVec2::new(10.0, 10.0), b: DVec2::new(0.0, 10.0) },
            Segment::Line { a: DVec2::new(0.0, 10.0), b: DVec2::new(0.0, 0.0) },
        ]);
        let b =
            extrude(&Frame::world(), &build_regions(&[square]).expect("region")[0], 0.0, 5.0).expect("box");
        assert!(matches!(dome(&b, top(&b, 5.0), 2.0, false, false), Err(KernelError::Unsupported(_))));
    }
}
