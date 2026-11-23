//! Mass properties: from a closed triangle mesh ([`properties`]), or integrated exactly on a
//! solid's own surfaces ([`exact_properties`]).

use crate::tess::TriMesh;
use glam::DVec3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MassProperties {
    pub volume: f64,
    pub area: f64,
    pub centroid: DVec3,
}

pub fn properties(mesh: &TriMesh) -> MassProperties {
    let mut volume = 0.0;
    let mut area = 0.0;
    let mut moment = DVec3::ZERO;
    for t in &mesh.triangles {
        let [a, b, c] = t.map(|i| mesh.positions[i as usize]);
        let v = a.dot(b.cross(c)) / 6.0;
        volume += v;
        moment += (a + b + c) * (v / 4.0);
        area += 0.5 * (b - a).cross(c - a).length();
    }
    let centroid = if volume.abs() > 0.0 { moment / volume } else { DVec3::ZERO };
    MassProperties { volume, area, centroid }
}

/// Gauss–Legendre nodes and weights on [-1, 1], eight points: exact for polynomials up to
/// degree 15.
const GAUSS8: [(f64, f64); 8] = [
    (-0.960_289_856_497_536_3, 0.101_228_536_290_376_26),
    (-0.796_666_477_413_626_7, 0.222_381_034_453_374_47),
    (-0.525_532_409_916_329, 0.313_706_645_877_887_3),
    (-0.183_434_642_495_649_8, 0.362_683_783_378_362),
    (0.183_434_642_495_649_8, 0.362_683_783_378_362),
    (0.525_532_409_916_329, 0.313_706_645_877_887_3),
    (0.796_666_477_413_626_7, 0.222_381_034_453_374_47),
    (0.960_289_856_497_536_3, 0.101_228_536_290_376_26),
];

/// `f` integrated from `a` to `b` by eight-point Gauss–Legendre on `panels` equal panels.
fn gauss<T>(a: f64, b: f64, panels: usize, mut f: impl FnMut(f64) -> T) -> T
where
    T: std::ops::Add<Output = T> + std::ops::Mul<f64, Output = T> + Default,
{
    let h = (b - a) / panels as f64;
    let mut sum = T::default();
    for k in 0..panels {
        let mid = a + h * (k as f64 + 0.5);
        for (x, w) in GAUSS8 {
            sum = sum + f(mid + 0.5 * h * x) * (w * 0.5 * h);
        }
    }
    sum
}

/// What is integrated over each face, per unit `du dv`: the flux of `x / 3` (volume),
/// of `(x^2 / 2, y^2 / 2, z^2 / 2)` component by component (the first moments), and the
/// area element.
#[derive(Clone, Copy, Default)]
struct Densities {
    volume: f64,
    moment: DVec3,
    area: f64,
}

impl std::ops::Add for Densities {
    type Output = Densities;
    fn add(self, o: Densities) -> Densities {
        Densities { volume: self.volume + o.volume, moment: self.moment + o.moment, area: self.area + o.area }
    }
}

impl std::ops::Mul<f64> for Densities {
    type Output = Densities;
    fn mul(self, k: f64) -> Densities {
        Densities { volume: self.volume * k, moment: self.moment * k, area: self.area * k }
    }
}

/// Mass properties of `solid` integrated on its own surfaces, with no mesh: each face's
/// integral over its trimmed parameter domain is turned by Green's theorem into one
/// around the face's boundary edges, `∬ g du dv = ∮ G dv` with `G(u, v)` the integral of
/// `g` along `u` from a fixed `u0`. The boundary integral follows the exact edge curves,
/// and both integrals use Gauss–Legendre quadrature, so the result is exact to about
/// 1e-9 of the size, not to the tessellation's chord.
///
/// A closed loop shifted by a whole period of a wrapping surface adds `∮ H(v) dv = 0`,
/// so each loop is followed continuously from wherever it starts.
pub fn exact_properties(solid: &crate::topo::Solid) -> crate::Result<MassProperties> {
    let mut total = Densities::default();
    let scale = solid.bounds().map_or(1.0, |(lo, hi)| (hi - lo).length().max(1e-9));
    for face in &solid.faces {
        total = total + face_integral(solid, face, scale)?;
    }
    let centroid = if total.volume.abs() > 0.0 { total.moment / total.volume } else { DVec3::ZERO };
    Ok(MassProperties { volume: total.volume, area: total.area, centroid })
}

/// The integrals of one face (see [`exact_properties`]).
fn face_integral(
    solid: &crate::topo::Solid,
    face: &crate::topo::Face,
    scale: f64,
) -> crate::Result<Densities> {
    use crate::geom::Surface;
    use glam::DVec2;
    let s = &face.surface;
    let h = 1e-6 * scale;
    // The parameter steps that keep the surface smooth over a panel.
    let size = match s {
        Surface::Cylinder { radius, .. } | Surface::Cone { radius, .. } | Surface::Sphere { radius, .. } => {
            *radius
        }
        Surface::Torus { minor, .. } => *minor,
        _ => 0.05 * scale,
    };
    let periods = (
        s.u_period(),
        match s {
            Surface::Torus { minor, .. } => Some(std::f64::consts::TAU * minor),
            _ => None,
        },
    );
    let unwrap = |uv: DVec2, near: DVec2| {
        let shift = |x: f64, r: f64, p: Option<f64>| p.map_or(x, |p| x + ((r - x) / p).round() * p);
        DVec2::new(shift(uv.x, near.x, periods.0), shift(uv.y, near.y, periods.1))
    };
    // The integrands at a parameter point, from the surface and its central differences.
    let dens = |uv: DVec2| -> Densities {
        let p = s.eval_uv(uv);
        let du = (s.eval_uv(uv + DVec2::X * h) - s.eval_uv(uv - DVec2::X * h)) / (2.0 * h);
        let dv = (s.eval_uv(uv + DVec2::Y * h) - s.eval_uv(uv - DVec2::Y * h)) / (2.0 * h);
        let n = du.cross(dv);
        Densities { volume: p.dot(n) / 3.0, moment: p * p * 0.5 * n, area: n.length() }
    };
    // The chart's orientation against the outward normal, for the area (the fluxes take
    // their sign from the loops, which run counter-clockwise about the outward normal).
    let mut orient = 0.0;
    let mut loops: Vec<Vec<(DVec2, DVec2)>> = Vec::new();
    for l in face.loops() {
        // Points along the loop's edges at Gauss nodes, with dv/dt times the weight.
        let mut pts: Vec<(DVec2, DVec2)> = Vec::new();
        let mut prev: Option<DVec2> = None;
        for c in &l.coedges {
            let e =
                solid.edge(c.edge).ok_or_else(|| crate::KernelError::InvalidInput("missing edge".into()))?;
            let (ta, tb) = if c.reversed { (e.t1, e.t0) } else { (e.t0, e.t1) };
            // Enough panels for the edge's length both on the model and in the layout,
            // which can stretch it many times over (a band of a sphere reaching near the
            // point its layout is projected from).
            let mut len3 = 0.0;
            let mut len2 = 0.0;
            let mut last: Option<(DVec3, DVec2)> = None;
            for k in 0..=32 {
                let p = e.curve.eval(ta + (tb - ta) * k as f64 / 32.0);
                let q = s.uv(p);
                if let Some((p0, q0)) = last {
                    len3 += p.distance(p0);
                    len2 += unwrap(q, q0).distance(q0);
                }
                last = Some((p, last.map_or(q, |(_, q0)| unwrap(q, q0))));
            }
            let panels = ((len3.max(len2) / size * 8.0).ceil() as usize).clamp(4, 2000);
            // Panels end at a spline edge's knots too, where it may bend sharply.
            let mut cuts: Vec<f64> =
                (0..=panels).map(|k| ta + (tb - ta) * k as f64 / panels as f64).collect();
            if let crate::geom::Curve::Spline(sp) = &e.curve {
                let (lo, hi) = (ta.min(tb), ta.max(tb));
                let room = 1e-9 * (hi - lo);
                cuts.extend(sp.knots.iter().filter(|k| **k > lo + room && **k < hi - room));
                if tb >= ta {
                    cuts.sort_by(f64::total_cmp)
                } else {
                    cuts.sort_by(|a, b| b.total_cmp(a))
                }
                cuts.dedup_by(|a, b| (*a - *b).abs() <= room);
            }
            let dt = 1e-6 * ((tb - ta) / panels as f64).abs().max(1e-12);
            for c in cuts.windows(2) {
                let (mid, step) = (0.5 * (c[0] + c[1]), c[1] - c[0]);
                for (x, w) in GAUSS8 {
                    let t = mid + 0.5 * step * x;
                    let raw = s.uv(e.curve.eval(t));
                    let uv = prev.map_or(raw, |q| unwrap(raw, q));
                    // The layout's velocity along the edge, from the curve's tangent split
                    // along the surface's two tangents (differencing the layout itself
                    // would amplify the rounding in finding a point's parameters).
                    let tangent = (e.curve.eval(t + dt) - e.curve.eval(t - dt)) / (2.0 * dt);
                    let su = (s.eval_uv(uv + DVec2::X * h) - s.eval_uv(uv - DVec2::X * h)) / (2.0 * h);
                    let sv = (s.eval_uv(uv + DVec2::Y * h) - s.eval_uv(uv - DVec2::Y * h)) / (2.0 * h);
                    let (a11, a12, a22) = (su.dot(su), su.dot(sv), sv.dot(sv));
                    let det = a11 * a22 - a12 * a12;
                    let (b1, b2) = (su.dot(tangent), sv.dot(tangent));
                    let d = if det.abs() > 1e-300 {
                        DVec2::new((a22 * b1 - a12 * b2) / det, (a11 * b2 - a12 * b1) / det)
                    } else {
                        let a = unwrap(s.uv(e.curve.eval(t + dt)), uv);
                        let b = unwrap(s.uv(e.curve.eval(t - dt)), uv);
                        (a - b) / (2.0 * dt)
                    };
                    pts.push((uv, d * (w * 0.5 * step)));
                    prev = Some(uv);
                    if orient == 0.0 {
                        let n = {
                            let su =
                                (s.eval_uv(uv + DVec2::X * h) - s.eval_uv(uv - DVec2::X * h)) / (2.0 * h);
                            let sv =
                                (s.eval_uv(uv + DVec2::Y * h) - s.eval_uv(uv - DVec2::Y * h)) / (2.0 * h);
                            su.cross(sv)
                        };
                        let out = face.normal_at(e.curve.eval(t));
                        if n.length() > 0.0 {
                            orient = n.dot(out).signum();
                        }
                    }
                }
            }
        }
        loops.push(pts);
    }
    // A spline surface may bend sharply at its knots: the inner integrals stop there. They
    // run along whichever parameter crosses fewer knots (along a rolling round's length
    // there can be thousands), by either form of Green's theorem:
    //   the double integral of g = the loop integral of G dv, G the integral of g along u,
    //                            = minus the loop integral of H du, H that along v.
    let (breaks, along_v) = match s {
        Surface::BSpline(sp) => {
            let (bu, bv) = (sp.u_breaks(), sp.v_breaks());
            if bv.len() < bu.len() { (bv, true) } else { (bu, false) }
        }
        _ => (Vec::new(), false),
    };
    // The parameter integrated along, the other one, and the period of the first.
    let along = |q: DVec2| if along_v { q.y } else { q.x };
    let across = |q: DVec2| if along_v { q.x } else { q.y };
    let period = if along_v { None } else { s.u_period() };
    let start = loops.iter().flatten().map(|(uv, _)| along(*uv)).fold(f64::INFINITY, f64::min);
    let at = |x: f64, other: f64| if along_v { DVec2::new(other, x) } else { DVec2::new(x, other) };
    let mut sum = Densities::default();
    for pts in &loops {
        for (uv, dw) in pts {
            // Along a stretch where the other parameter stays put (a rolling round's
            // contact curves, a cylinder's circles) the loop integral gains nothing: skip
            // the work.
            let dw_across = across(*dw);
            if dw_across == 0.0 || dw_across.abs() <= 1e-14 * size {
                continue;
            }
            // The densities along the parameter from `start`, piece by piece between the
            // breaks (repeated every period on a wrapping surface).
            let x = along(*uv);
            let (a, b) = (start.min(x), start.max(x));
            let mut cuts = vec![a];
            for k in &breaks {
                match period {
                    Some(p) => {
                        let mut c = k + ((a - k) / p).ceil() * p;
                        while c < b {
                            cuts.push(c);
                            c += p;
                        }
                    }
                    None if *k > a && *k < b => cuts.push(*k),
                    None => {}
                }
            }
            cuts.push(b);
            cuts.sort_by(f64::total_cmp);
            let mut g = Densities::default();
            for w in cuts.windows(2) {
                let panels = (((w[1] - w[0]) / size * 4.0).ceil() as usize).clamp(1, 2000);
                g = g + gauss(w[0], w[1], panels, |t| dens(at(t, across(*uv))));
            }
            let sign = if x >= start { 1.0 } else { -1.0 };
            let form = if along_v { -1.0 } else { 1.0 };
            sum = sum + g * (dw_across * sign * form);
        }
    }
    if !(sum.volume.is_finite() && sum.area.is_finite()) {
        return Err(crate::KernelError::NonFinite);
    }
    sum.area *= orient;
    Ok(sum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blend::{Blend, blend_chain, edge_near};
    use crate::boolean::{BoolOp, boolean};
    use crate::profile::{Loop2, Segment, build_regions};
    use crate::topo::{EdgeId, Solid};
    use crate::{Frame, extrude, revolve};
    use glam::DVec2;
    use std::f64::consts::PI;

    fn line(a: (f64, f64), b: (f64, f64)) -> Segment {
        Segment::Line { a: DVec2::new(a.0, a.1), b: DVec2::new(b.0, b.1) }
    }

    fn prism(l: Loop2, z0: f64, z1: f64) -> Solid {
        let r = build_regions(&[l]).expect("region");
        extrude::extrude(&Frame::world(), &r[0], z0, z1).expect("prism")
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop2 {
        Loop2::new(vec![
            line((x0, y0), (x1, y0)),
            line((x1, y0), (x1, y1)),
            line((x1, y1), (x0, y1)),
            line((x0, y1), (x0, y0)),
        ])
    }

    /// `profile` (x the radius, y the height) turned a whole turn about the z axis.
    fn turned(profile: Loop2) -> Solid {
        let plane = Frame { origin: DVec3::ZERO, x: DVec3::X, y: DVec3::Z, z: DVec3::NEG_Y };
        let r = build_regions(&[profile]).expect("region");
        revolve::revolve(&plane, &r[0], DVec2::ZERO, DVec2::Y, 2.0 * PI).expect("revolve")
    }

    fn arc(c: (f64, f64), r: f64, from: f64, to: f64) -> Segment {
        Segment::Arc { center: DVec2::new(c.0, c.1), radius: r, start: from, sweep: to - from }
    }

    fn near(got: f64, want: f64, rel: f64) -> bool {
        (got - want).abs() <= rel * want.abs().max(1.0)
    }

    fn check(s: &Solid, volume: f64, area: f64) -> MassProperties {
        let m = exact_properties(s).expect("exact");
        assert!(near(m.volume, volume, 1e-8), "volume {} vs {volume}", m.volume);
        assert!(near(m.area, area, 1e-8), "area {} vs {area}", m.area);
        m
    }

    #[test]
    fn analytic_solids_measure_exactly() {
        let m = check(&prism(rect(0.0, 0.0, 10.0, 10.0), 0.0, 10.0), 1000.0, 600.0);
        assert!((m.centroid - DVec3::splat(5.0)).length() < 1e-7);
        check(
            &prism(Loop2::new(vec![Segment::circle(DVec2::ZERO, 3.0)]), 0.0, 10.0),
            90.0 * PI,
            18.0 * PI + 60.0 * PI,
        );
        let ball = turned(Loop2::new(vec![
            arc((0.0, 0.0), 5.0, -0.5 * PI, 0.5 * PI),
            line((0.0, 5.0), (0.0, -5.0)),
        ]));
        check(&ball, 4.0 * PI * 125.0 / 3.0, 100.0 * PI);
        let torus = turned(Loop2::new(vec![Segment::circle(DVec2::new(5.0, 0.0), 2.0)]));
        check(&torus, 2.0 * PI * PI * 5.0 * 4.0, 4.0 * PI * PI * 10.0);
        let cone = turned(Loop2::new(vec![
            line((0.0, 0.0), (3.0, 0.0)),
            line((3.0, 0.0), (0.0, 4.0)),
            line((0.0, 4.0), (0.0, 0.0)),
        ]));
        check(&cone, 12.0 * PI, 9.0 * PI + 15.0 * PI);
    }

    #[test]
    fn a_band_of_a_sphere_measures_exactly() {
        // A ball of 5 bored through by 2: its band of sphere stretches its layout to many
        // times the radius, which no grid follows but the boundary integral does.
        let h = 21f64.sqrt();
        let b = (h / 5.0).asin();
        let bored = turned(Loop2::new(vec![arc((0.0, 0.0), 5.0, -b, b), line((2.0, h), (2.0, -h))]));
        let v = 4.0 * PI * 125.0 / 3.0 - 8.0 * PI * h - 2.0 * PI * (5.0 - h).powi(2) * (10.0 + h) / 3.0;
        check(&bored, v, 20.0 * PI * h + 8.0 * PI * h);
    }

    #[test]
    fn trimmed_and_blended_faces_measure_exactly() {
        // A plate with a hole: the top and bottom faces are trimmed by the hole's circles.
        let rod = prism(Loop2::new(vec![Segment::circle(DVec2::new(20.0, 10.0), 4.0)]), -1.0, 6.0);
        let plate = boolean(&prism(rect(0.0, 0.0, 40.0, 20.0), 0.0, 5.0), &rod, BoolOp::Subtract)
            .expect("cut")
            .remove(0);
        let m = check(&plate, 4000.0 - 80.0 * PI, 2.0 * (800.0 - 16.0 * PI) + 600.0 + 40.0 * PI);
        assert!((m.centroid - DVec3::new(20.0, 10.0, 2.5)).length() < 1e-7, "{}", m.centroid);
        // A cube's edge filleted to 2: the round's quarter cylinder.
        let cube = prism(rect(0.0, 0.0, 10.0, 10.0), 0.0, 10.0);
        let e = edge_near(&cube, DVec3::new(5.0, 0.0, 10.0), 1e-6).expect("edge");
        let f = blend_chain(&cube, &[(e, 0)], Blend::Fillet(2.0), 1).expect("fillet").remove(0);
        // The round replaces two 2 x 10 strips; each end face loses its corner, 4 - pi.
        check(&f, 1000.0 - (4.0 - PI) * 10.0, 600.0 - 40.0 + 10.0 * PI - 2.0 * (4.0 - PI));
        // A hemisphere's rim chamfered 2 on the flat and 1 on the ball (brep-bench's
        // chamfer/hemisphere-rim-two-distances): a rolling bevel, a B-spline face fitted
        // to 1e-7, so exact to that.
        let hemi = turned(Loop2::new(vec![
            line((0.0, 0.0), (5.0, 0.0)),
            arc((0.0, 0.0), 5.0, 0.0, 0.5 * PI),
            line((0.0, 5.0), (0.0, 0.0)),
        ]));
        let rim: Vec<(EdgeId, u32)> = (0..hemi.edges.len() as u32)
            .map(EdgeId)
            .filter(|id| {
                hemi.edge(*id).is_some_and(|e| {
                    let p = e.curve.eval(0.5 * (e.t0 + e.t1));
                    p.z.abs() < 1e-9 && (p.truncate().length() - 5.0).abs() < 1e-9
                })
            })
            .map(|id| (id, 0))
            .collect();
        let flat = (0..hemi.faces.len())
            .find(|&f| crate::boolean::face_contains(&hemi, f, DVec3::new(2.5, 0.0, 0.0)))
            .expect("flat");
        let c =
            blend_chain(&hemi, &rim, Blend::ChamferDistances { distance: 2.0, other: 1.0, along: flat }, 1)
                .expect("chamfer");
        let m = exact_properties(&c[0]).expect("exact");
        assert!(near(m.volume, 234.396_144_679_021_4, 1e-6), "{}", m.volume);
        assert!(near(m.area, 207.325_442_466_954_4, 1e-6), "{}", m.area);
    }
}
