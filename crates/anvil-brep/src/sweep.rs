//! Sweeping a planar region along a path of lines and arcs.
//!
//! The region is carried along the path with its plane kept square to the path (the
//! frame turns with the path, without twisting). Along a line that sweeps a prism, along
//! an arc a solid of revolution about the arc's axis, so every face is exact. The pieces
//! meet face to face where the path's segments meet, and are joined by unions.
//!
//! The path must run smoothly from one segment to the next (no sharp corners), and an
//! arc must not be tighter than the region is wide, or the sweep would run into itself.

use crate::boolean::{BoolOp, boolean};
use crate::geom::Frame;
use crate::profile::Region;
use crate::topo::Solid;
use crate::{KernelError, Result, tol};
use glam::{DVec2, DVec3};

/// One segment of a sweep path, in model space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathSegment {
    Line {
        a: DVec3,
        b: DVec3,
    },
    /// An arc about the axis through `center` along the unit vector `axis`, from
    /// `start` turning `angle` radians (right-handed about `axis`, positive).
    Arc {
        center: DVec3,
        axis: DVec3,
        start: DVec3,
        angle: f64,
    },
}

impl PathSegment {
    pub fn start(&self) -> DVec3 {
        match *self {
            PathSegment::Line { a, .. } => a,
            PathSegment::Arc { start, .. } => start,
        }
    }

    pub fn end(&self) -> DVec3 {
        match *self {
            PathSegment::Line { b, .. } => b,
            PathSegment::Arc { center, axis, start, angle } => rotate(start, center, axis, angle),
        }
    }

    /// Unit tangent at the start or end.
    pub fn tangent(&self, at_end: bool) -> DVec3 {
        match *self {
            PathSegment::Line { a, b } => (b - a).normalize_or_zero(),
            PathSegment::Arc { center, axis, .. } => {
                let p = if at_end { self.end() } else { self.start() };
                axis.cross(p - center).normalize_or_zero()
            }
        }
    }

    pub fn length(&self) -> f64 {
        match *self {
            PathSegment::Line { a, b } => (b - a).length(),
            PathSegment::Arc { center, start, angle, .. } => (start - center).length() * angle.abs(),
        }
    }
}

/// `p` turned `angle` radians about the line through `c` along unit `axis`.
pub fn rotate(p: DVec3, c: DVec3, axis: DVec3, angle: f64) -> DVec3 {
    c + rotate_dir(p - c, axis, angle)
}

fn rotate_dir(v: DVec3, axis: DVec3, angle: f64) -> DVec3 {
    let (s, co) = angle.sin_cos();
    v * co + axis.cross(v) * s + axis * axis.dot(v) * (1.0 - co)
}

/// Sweeps `region` (drawn in `frame`, whose origin must lie at the start of the path
/// and whose normal must run along it) along `path`. Returns the swept solid (or
/// solids, if the region had several separate parts after joining).
pub fn sweep(frame: &Frame, region: &Region, path: &[PathSegment]) -> Result<Vec<Solid>> {
    let Some(first) = path.first() else {
        return Err(KernelError::InvalidInput("the sweep path is empty".into()));
    };
    let scale = path.iter().map(PathSegment::length).sum::<f64>().max(1.0);
    if (first.start() - frame.origin).length() > tol::LINEAR * 10.0 * scale {
        return Err(KernelError::InvalidInput("the profile does not sit at the start of the path".into()));
    }
    if first.tangent(false).cross(frame.z).length() > 1e-6 || first.tangent(false).dot(frame.z) < 0.0 {
        return Err(KernelError::InvalidInput("the profile's plane is not square to the path".into()));
    }
    for w in path.windows(2) {
        if (w[0].end() - w[1].start()).length() > tol::LINEAR * 10.0 * scale {
            return Err(KernelError::InvalidInput("the sweep path is not one connected chain".into()));
        }
        if w[0].tangent(true).dot(w[1].tangent(false)) < 1.0 - 1e-9 {
            return Err(KernelError::Unsupported("the sweep path has a sharp corner".into()));
        }
    }

    let mut f = *frame;
    let mut pieces: Vec<Solid> = Vec::new();
    for seg in path {
        match *seg {
            PathSegment::Line { a, b } => {
                let len = (b - a).length();
                if len < tol::LINEAR {
                    continue;
                }
                pieces.push(crate::extrude::extrude(&f, region, 0.0, len)?);
                f = f.with_origin(f.origin + f.z * len);
            }
            PathSegment::Arc { center, axis, angle, .. } => {
                if angle.abs() < 1e-12 {
                    continue;
                }
                // The axis lies in the region's plane: as a point and direction there.
                let c = f.to_local(center);
                let d = DVec2::new(axis.dot(f.x), axis.dot(f.y));
                pieces.push(crate::revolve::revolve(&f, region, DVec2::new(c.x, c.y), d, angle.abs())?);
                let turn = |v: DVec3| rotate_dir(v, axis, angle);
                f = Frame {
                    origin: rotate(f.origin, center, axis, angle),
                    x: turn(f.x),
                    y: turn(f.y),
                    z: turn(f.z),
                };
            }
        }
    }
    let mut it = pieces.into_iter();
    let Some(mut acc) = it.next().map(|s| vec![s]) else {
        return Err(KernelError::InvalidInput("the sweep path has no length".into()));
    };
    for piece in it {
        // Each piece starts on the face where the last one ended.
        let last = acc.pop().ok_or_else(|| KernelError::Boolean("empty sweep".into()))?;
        let joined = boolean(&last, &piece, BoolOp::Union)?;
        acc.extend(joined);
    }
    Ok(acc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{Loop2, Segment, build_regions};
    use crate::{mass, tess};
    use std::f64::consts::PI;

    fn volume(s: &Solid) -> f64 {
        let m = tess::tessellate(s, &tess::TessParams { chord: 1e-4, max_angle: 0.01 }).expect("tess");
        assert!(m.is_watertight(), "not watertight");
        mass::properties(&m).volume
    }

    fn disc(r: f64) -> Region {
        build_regions(&[Loop2::new(vec![Segment::circle(DVec2::ZERO, r)])]).expect("region").remove(0)
    }

    #[test]
    fn straight_sweep_is_a_cylinder() {
        let f = Frame::new(DVec3::ZERO, DVec3::X, DVec3::Y).expect("frame");
        let s = sweep(&f, &disc(2.0), &[PathSegment::Line { a: DVec3::ZERO, b: DVec3::new(10.0, 0.0, 0.0) }])
            .expect("sweep");
        assert_eq!(s.len(), 1);
        assert!((volume(&s[0]) - PI * 40.0).abs() < 1e-3 * PI * 40.0);
    }

    #[test]
    fn line_arc_line_follows_pappus() {
        // A 4 mm round bar: 10 along x, a quarter turn of radius 20 to head along y, 10 more.
        let f = Frame::new(DVec3::ZERO, DVec3::X, DVec3::Y).expect("frame");
        let path = [
            PathSegment::Line { a: DVec3::ZERO, b: DVec3::new(10.0, 0.0, 0.0) },
            PathSegment::Arc {
                center: DVec3::new(10.0, 20.0, 0.0),
                axis: DVec3::Z,
                start: DVec3::new(10.0, 0.0, 0.0),
                angle: 0.5 * PI,
            },
            PathSegment::Line { a: DVec3::new(30.0, 20.0, 0.0), b: DVec3::new(30.0, 30.0, 0.0) },
        ];
        let s = sweep(&f, &disc(2.0), &path).expect("sweep");
        assert_eq!(s.len(), 1);
        let exact = PI * 4.0 * (10.0 + 20.0 * 0.5 * PI + 10.0);
        assert!((volume(&s[0]) - exact).abs() < 1e-3 * exact, "{} vs {exact}", volume(&s[0]));
        // It ends square to y at (30, 30).
        let m = tess::tessellate(&s[0], &tess::TessParams::default()).expect("tess");
        let (lo, hi) = m.bounds().expect("bounds");
        assert!((hi.y - 30.0).abs() < 1e-9 && (hi.x - 32.0).abs() < 1e-3 && lo.x.abs() < 1e-9, "{lo} {hi}");
    }

    #[test]
    fn refuses_a_sharp_corner() {
        let f = Frame::new(DVec3::ZERO, DVec3::X, DVec3::Y).expect("frame");
        let path = [
            PathSegment::Line { a: DVec3::ZERO, b: DVec3::new(10.0, 0.0, 0.0) },
            PathSegment::Line { a: DVec3::new(10.0, 0.0, 0.0), b: DVec3::new(10.0, 10.0, 0.0) },
        ];
        assert!(matches!(sweep(&f, &disc(1.0), &path), Err(KernelError::Unsupported(_))));
    }
}
