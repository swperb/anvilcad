//! Measure: the size of one picked entity, and the distances and angles between two.
//!
//! This is the Measure core of the specification in `docs/re/measure-and-section.md`
//! (sections "What each selection reports", "Arc/circle options" and "Projected On",
//! testable statements 1 to 19, and item 1 of "Recommended scope"). It is a pure
//! function of a rebuilt part: nothing is added to the feature tree and nothing is saved.
//!
//! Picks are vertices or other points (model coordinates), edges (named by the two faces
//! they separate), faces, and reference planes. Every quantity in the result is an
//! `Option`, never a sentinel, and lengths are in millimetres. Angles are in degrees.
//!
//! Where the part was built on the exact kernel the measurement uses the exact curves and
//! surfaces: line lengths, circle radii, centres, arc lengths and chords come straight from
//! the curves, and a face's area integrates its surface's area element over a fine
//! tessellation of its parameter region, with each boundary chord corrected by the exact
//! (or, on curved faces, parabolic) area between the chord and its curve. A face's
//! perimeter is the length of every boundary loop, outer and inner, leaving out seam edges
//! (edges a face uses twice). Otherwise the faceted body is used: polygons grouped by face
//! name, edges as the segments between two named faces, and circles recovered from the
//! axis of an adjacent cylindrical or revolved face.
//!
//! The minimum distance between two entities is found between their tessellations
//! (bounding-volume search over points, segments and triangles, which is exact for planar
//! faces and straight edges) and then refined by alternating closest-point projections onto
//! the exact curves and surfaces, which makes the point, line, plane, circle, cylinder and
//! sphere cases exact.
//!
//! Decisions where the specification is silent (see its "Unknown" list): for two circles
//! that are not coplanar, or two cylinders whose axes are not parallel, the centre condition
//! uses the circle's centre or the cylinder's axis (bounded by the face), and the distance
//! is the smallest distance between the chosen sets, or the largest when either condition
//! is *Maximum*.

use crate::exact::face_id;
use crate::rebuild::BuildState;
use anvil_brep as brep;
use anvil_brep::{Curve, Surface};
use anvil_kernel::{EdgeRef, FaceId, Solid, SurfaceKind, Vec3};
use glam::{DVec2, DVec3};
use std::collections::{BTreeMap, HashMap};
use std::f64::consts::{PI, TAU};

/// One selected entity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick {
    /// A vertex, sketch point or any other point, in model coordinates.
    Point(Vec3),
    /// A model edge, named by the two faces it separates.
    Edge(EdgeRef),
    /// A model face.
    Face(FaceId),
    /// A reference plane (unbounded).
    Plane { origin: Vec3, normal: Vec3 },
}

/// Which point of an arc, circle, cylinder or sphere a distance is measured to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArcCondition {
    /// The centre (or the axis of a cylinder).
    #[default]
    Center,
    /// The near side.
    Minimum,
    /// The far side.
    Maximum,
}

/// Settings of the Measure tool.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MeasureOptions {
    /// Conditions for the first and the second selection when arcs, circles, cylinders or
    /// spheres are measured. Both `Center` is *Center to Center*, both `Minimum` is
    /// *Minimum Distance*, both `Maximum` is *Maximum Distance*, anything else is *Custom*.
    pub arc: (ArcCondition, ArcCondition),
    /// *Projected On*: a plane given by a point and a normal. With a plane, the distance
    /// between the two measured points is split into its part along the normal and its
    /// part in the plane.
    pub project_on: Option<(Vec3, Vec3)>,
}

/// What a measurement reports. A quantity that does not apply to the selection is `None`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Measurement {
    /// Coordinates of a single selected point.
    pub point: Option<Vec3>,
    /// Length of a single edge (the circumference of a full circle, the arc length of an arc).
    pub length: Option<f64>,
    /// Length of a circular arc that is not a full circle.
    pub arc_length: Option<f64>,
    /// Straight distance between the ends of a circular arc.
    pub chord_length: Option<f64>,
    /// Radius of a circular edge, cylindrical face or spherical face.
    pub radius: Option<f64>,
    pub diameter: Option<f64>,
    /// Centre of a circular edge or spherical face.
    pub center: Option<Vec3>,
    /// Area of a single face.
    pub area: Option<f64>,
    /// Total length of a single face's boundary loops.
    pub perimeter: Option<f64>,
    /// Sum of the lengths of the selected edges (two or more edges).
    pub total_length: Option<f64>,
    /// Sum of the areas of the selected faces (two or more faces).
    pub total_area: Option<f64>,
    /// The distance between two selections: the minimum distance, or the distance per the
    /// arc/circle conditions where those apply (see `arc_conditions`).
    pub distance: Option<f64>,
    /// The two points `distance` is measured between, on the first and second selection.
    pub points: Option<(Vec3, Vec3)>,
    /// Second point minus first point (signed; the dialog shows absolute values).
    pub delta: Option<Vec3>,
    /// Perpendicular distance from a planar selection to a point, or to a parallel straight
    /// edge, planar face or cylinder axis.
    pub normal_distance: Option<f64>,
    /// Distance between centres of circles and spheres or axes of cylinders, or from a
    /// cylinder's axis to a parallel plane.
    pub center_distance: Option<f64>,
    /// The arc/circle conditions `distance` was measured with, when they applied.
    pub arc_conditions: Option<(ArcCondition, ArcCondition)>,
    /// Angle between two straight edges, an edge and a plane, or two planes: 0 to 90 degrees.
    pub angle_deg: Option<f64>,
    pub parallel: Option<bool>,
    pub perpendicular: Option<bool>,
    /// With *Projected On* a plane: the distance's part within the plane.
    pub projection: Option<f64>,
    /// With *Projected On* a plane: the distance's part along the plane's normal.
    pub normal: Option<f64>,
    /// True when every selection was measured on exact geometry.
    pub exact: bool,
}

/// Measures the picked entities of a rebuilt part.
///
/// One pick reports its size; two picks report the distance (and angle, normal distance,
/// centre distance and totals where they apply); more picks report the total length of the
/// edges and the total area of the faces, and no distance.
pub fn measure(st: &BuildState, picks: &[Pick], opts: &MeasureOptions) -> Result<Measurement, String> {
    if picks.is_empty() {
        return Err("select an entity to measure".into());
    }
    let projection = match opts.project_on {
        Some((o, n)) => {
            let n = d3(n)
                .try_normalize()
                .filter(|_| finite(d3(o)))
                .ok_or("the projection plane needs a normal")?;
            Some(n)
        }
        None => None,
    };
    let mut resolver = Resolver::new(st);
    let mut ents = Vec::with_capacity(picks.len());
    for p in picks {
        ents.push(resolver.resolve(p)?);
    }
    let mut m = Measurement { exact: ents.iter().all(|e| e.exact), ..Measurement::default() };
    match ents.as_slice() {
        [e] => single(e, &mut m),
        [a, b] => pair(a, b, opts.arc, &mut m)?,
        many => totals(many, &mut m)?,
    }
    if let (Some(n), Some((pa, pb))) = (projection, m.points) {
        let v = d3(pb) - d3(pa);
        let along = v.dot(n);
        m.normal = Some(along.abs());
        m.projection = Some((v - n * along).length());
    }
    Ok(m)
}

/// The measurement as label and value pairs for display: lengths in millimetres to three
/// decimal places, angles in degrees to two, deltas as absolute values.
pub fn summary_lines(m: &Measurement) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut push = |label: &str, value: String| out.push((label.to_string(), value));
    let len = |v: f64| format!("{:.3} mm", round(v, 3));
    let area = |v: f64| format!("{:.3} mm²", round(v, 3));
    if let Some(p) = m.point {
        push("X", len(p.x));
        push("Y", len(p.y));
        push("Z", len(p.z));
    }
    if let Some(v) = m.length {
        push("Length", len(v));
    }
    if let Some(v) = m.arc_length {
        push("Arc length", len(v));
    }
    if let Some(v) = m.chord_length {
        push("Chord length", len(v));
    }
    if let Some(v) = m.radius {
        push("Radius", len(v));
    }
    if let Some(v) = m.diameter {
        push("Diameter", len(v));
    }
    if let Some(c) = m.center {
        push("Center X", len(c.x));
        push("Center Y", len(c.y));
        push("Center Z", len(c.z));
    }
    if let Some(v) = m.area {
        push("Area", area(v));
    }
    if let Some(v) = m.perimeter {
        push("Perimeter", len(v));
    }
    if let Some(v) = m.total_length {
        push("Total length", len(v));
    }
    if let Some(v) = m.total_area {
        push("Total area", area(v));
    }
    if let Some(v) = m.distance {
        let label = match m.arc_conditions {
            Some((ArcCondition::Center, ArcCondition::Center)) => "Center distance",
            Some((ArcCondition::Minimum, ArcCondition::Minimum)) => "Minimum distance",
            Some((ArcCondition::Maximum, ArcCondition::Maximum)) => "Maximum distance",
            Some(_) => "Custom distance",
            None => "Distance",
        };
        push(label, len(v));
    }
    if let Some(v) = m.center_distance {
        if !matches!(m.arc_conditions, Some((ArcCondition::Center, ArcCondition::Center))) {
            push("Center distance", len(v));
        }
    }
    if let Some(v) = m.normal_distance {
        push("Normal distance", len(v));
    }
    if let Some(d) = m.delta {
        push("dX", len(d.x.abs()));
        push("dY", len(d.y.abs()));
        push("dZ", len(d.z.abs()));
    }
    if let Some(a) = m.angle_deg {
        push("Angle", format!("{:.2}°", round(a, 2)));
    }
    if m.parallel == Some(true) {
        push("Parallel", "Yes".into());
    }
    if m.perpendicular == Some(true) {
        push("Perpendicular", "Yes".into());
    }
    if let Some(v) = m.projection {
        push("Projection", len(v));
    }
    if let Some(v) = m.normal {
        push("Normal", len(v));
    }
    out
}

fn round(v: f64, places: i32) -> f64 {
    let s = 10f64.powi(places);
    let r = (v * s).round() / s;
    if r == 0.0 {
        0.0
    } else {
        r
    }
}

// ---------------------------------------------------------------------------------------
// Conversions and small helpers.

fn d3(v: Vec3) -> DVec3 {
    DVec3::new(v.x, v.y, v.z)
}

fn v3(d: DVec3) -> Vec3 {
    Vec3 { x: d.x, y: d.y, z: d.z }
}

fn finite(v: DVec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

/// Tolerance for "parallel" and "perpendicular" tests on unit vectors.
const ANGLE_TOL: f64 = 1e-9;

/// Sampling of curves for the discrete distance search.
const SAMPLE_CHORD: f64 = 0.01;
const SAMPLE_ANGLE: f64 = PI / 36.0;

fn frame(origin: DVec3, normal: DVec3, x_hint: DVec3) -> Option<brep::Frame> {
    brep::Frame::new(origin, normal, x_hint)
        .or_else(|_| brep::Frame::new(origin, normal, normal.any_orthonormal_vector()))
        .ok()
}

/// A point on the line through `o` along unit `dir` nearest to `p`.
fn foot_on_line(p: DVec3, o: DVec3, dir: DVec3) -> DVec3 {
    o + dir * (p - o).dot(dir)
}

// ---------------------------------------------------------------------------------------
// Curves: pieces of exact curves between two parameters.

#[derive(Clone, Debug)]
struct Piece {
    curve: Curve,
    lo: f64,
    hi: f64,
}

impl Piece {
    fn new(curve: Curve, t0: f64, t1: f64) -> Piece {
        Piece { curve, lo: t0.min(t1), hi: t0.max(t1) }
    }

    fn line(a: DVec3, b: DVec3) -> Option<Piece> {
        let d = b - a;
        let len = d.length();
        (len > 1e-12).then(|| Piece { curve: Curve::Line { origin: a, dir: d / len }, lo: 0.0, hi: len })
    }

    fn start(&self) -> DVec3 {
        self.curve.eval(self.lo)
    }

    fn end(&self) -> DVec3 {
        self.curve.eval(self.hi)
    }

    fn length(&self) -> f64 {
        match &self.curve {
            Curve::Line { dir, .. } => (self.hi - self.lo) * dir.length(),
            Curve::Circle { radius, .. } => radius * (self.hi - self.lo),
            _ => {
                // Polyline lengths with n and 2n steps, extrapolated (Richardson).
                let poly = |n: usize| {
                    let mut l = 0.0;
                    let mut prev = self.curve.eval(self.lo);
                    for i in 1..=n {
                        let p = self.curve.eval(self.lo + (self.hi - self.lo) * i as f64 / n as f64);
                        l += (p - prev).length();
                        prev = p;
                    }
                    l
                };
                let (a, b) = (poly(512), poly(1024));
                (4.0 * b - a) / 3.0
            }
        }
    }

    fn samples(&self, chord: f64, angle: f64) -> Vec<DVec3> {
        self.curve.sample(self.lo, self.hi, chord, angle)
    }

    /// The point of the piece nearest to `p`.
    fn closest(&self, p: DVec3) -> DVec3 {
        let (lo, hi) = (self.lo, self.hi);
        match &self.curve {
            Curve::Line { origin, dir } => {
                let l2 = dir.length_squared().max(1e-300);
                self.curve.eval(((p - *origin).dot(*dir) / l2).clamp(lo, hi))
            }
            Curve::Circle { .. } => {
                let t = self.curve.param_in_range(p, lo, hi);
                if t >= lo && t <= hi {
                    self.curve.eval(t)
                } else {
                    nearer(p, self.start(), self.end())
                }
            }
            Curve::Spline(s) => s.eval(s.closest(p, lo, hi)),
            Curve::Ellipse { .. } => {
                let n = 128;
                let mut best_t = lo;
                let mut best = f64::INFINITY;
                for i in 0..=n {
                    let t = lo + (hi - lo) * i as f64 / n as f64;
                    let d = (self.curve.eval(t) - p).length_squared();
                    if d < best {
                        best = d;
                        best_t = t;
                    }
                }
                let step = (hi - lo) / n as f64;
                let (mut a, mut b) = ((best_t - step).max(lo), (best_t + step).min(hi));
                for _ in 0..80 {
                    let m1 = a + (b - a) * 0.381_966_011_250_105;
                    let m2 = a + (b - a) * 0.618_033_988_749_895;
                    if (self.curve.eval(m1) - p).length_squared() < (self.curve.eval(m2) - p).length_squared()
                    {
                        b = m2;
                    } else {
                        a = m1;
                    }
                }
                nearer(p, self.curve.eval(0.5 * (a + b)), self.curve.eval(best_t))
            }
        }
    }
}

fn nearer(p: DVec3, a: DVec3, b: DVec3) -> DVec3 {
    if (a - p).length_squared() <= (b - p).length_squared() {
        a
    } else {
        b
    }
}

// ---------------------------------------------------------------------------------------
// Surfaces.

/// The point of the (untrimmed) surface nearest to `p`, where that is well defined.
fn surface_foot(s: &Surface, p: DVec3) -> Option<DVec3> {
    match s {
        Surface::Plane { frame } => Some(p - frame.z * (p - frame.origin).dot(frame.z)),
        Surface::Cylinder { frame, radius } => {
            let l = frame.to_local(p);
            let radial = frame.x * l.x + frame.y * l.y;
            let r = radial.length();
            (r > 1e-12).then(|| frame.origin + frame.z * l.z + radial * (*radius / r))
        }
        Surface::Sphere { frame, radius } => {
            let d = p - frame.origin;
            let r = d.length();
            (r > 1e-12).then(|| frame.origin + d * (*radius / r))
        }
        Surface::Cone { frame, radius, half_angle, .. } => {
            let l = frame.to_local(p);
            let radial = frame.x * l.x + frame.y * l.y;
            let rho = radial.length();
            let dir = if rho > 1e-12 { radial / rho } else { frame.x };
            // In the half-plane through the axis the cone is the line from (radius, 0)
            // along (sin, cos) of the half angle.
            let (s, c) = half_angle.sin_cos();
            let t = (rho - radius) * s + l.z * c;
            let (qr, qz) = (radius + t * s, t * c);
            // Past the apex the nearest point is the apex.
            let (qr, qz) = if qr < 0.0 { (0.0, -radius / half_angle.tan()) } else { (qr, qz) };
            Some(frame.origin + dir * qr + frame.z * qz)
        }
        Surface::Torus { frame, major, minor, .. } => {
            let l = frame.to_local(p);
            let radial = frame.x * l.x + frame.y * l.y;
            let rho = radial.length();
            let dir = if rho > 1e-12 { radial / rho } else { frame.x };
            let centre = frame.origin + dir * *major;
            let d = p - centre;
            let r = d.length();
            (r > 1e-12).then(|| centre + d * (*minor / r))
        }
        Surface::BSpline(s) => Some(s.eval_st(s.closest(p))),
    }
}

/// Parameter-space coordinates of a triangle, unwrapped across a periodic seam.
fn tri_uv(s: &Surface, t: &[DVec3; 3]) -> [DVec2; 3] {
    let mut uv = t.map(|p| s.uv(p));
    if let Some(per) = s.u_period().filter(|p| *p > 0.0) {
        for k in 1..3 {
            uv[k].x += per * ((uv[0].x - uv[k].x) / per).round();
        }
    }
    uv
}

/// The area element of the surface's scaled parameters.
fn area_element(s: &Surface, uv: DVec2) -> f64 {
    match s {
        Surface::Plane { .. } | Surface::Cylinder { .. } => 1.0,
        _ => {
            let h = 1e-5 * (1.0 + uv.length());
            let du = DVec2::new(h, 0.0);
            let dv = DVec2::new(0.0, h);
            let su = (s.eval_uv(uv + du) - s.eval_uv(uv - du)) / (2.0 * h);
            let sv = (s.eval_uv(uv + dv) - s.eval_uv(uv - dv)) / (2.0 * h);
            su.cross(sv).length()
        }
    }
}

fn tri_area(t: &[DVec3; 3]) -> f64 {
    0.5 * (t[1] - t[0]).cross(t[2] - t[0]).length()
}

fn cross2(a: DVec2, b: DVec2) -> f64 {
    a.x * b.y - a.y * b.x
}

/// A seven-point rule exact for polynomials of degree five over a triangle: weights and
/// barycentric coordinates.
const TRI_RULE: [(f64, [f64; 3]); 7] = {
    const C: f64 = 1.0 / 3.0;
    const W1: f64 = 0.132_394_152_788_506;
    const A1: f64 = 0.059_715_871_789_770;
    const B1: f64 = 0.470_142_064_105_115;
    const W2: f64 = 0.125_939_180_544_827;
    const A2: f64 = 0.797_426_985_353_087;
    const B2: f64 = 0.101_286_507_323_456;
    [
        (0.225, [C, C, C]),
        (W1, [A1, B1, B1]),
        (W1, [B1, A1, B1]),
        (W1, [B1, B1, A1]),
        (W2, [A2, B2, B2]),
        (W2, [B2, A2, B2]),
        (W2, [B2, B2, A2]),
    ]
};

/// Eight-point Gauss-Legendre nodes and weights on [-1, 1].
const GAUSS_8: [(f64, f64); 8] = [
    (-0.960_289_856_497_536_3, 0.101_228_536_290_376_26),
    (-0.796_666_477_413_626_7, 0.222_381_034_453_374_47),
    (-0.525_532_409_916_329, 0.313_706_645_877_887_3),
    (-0.183_434_642_495_65, 0.362_683_783_378_362),
    (0.183_434_642_495_65, 0.362_683_783_378_362),
    (0.525_532_409_916_329, 0.313_706_645_877_887_3),
    (0.796_666_477_413_626_7, 0.222_381_034_453_374_47),
    (0.960_289_856_497_536_3, 0.101_228_536_290_376_26),
];

/// Area of a face's triangles, integrated over the surface's parameters where it has
/// any, so that a cylinder bounded by circles and straight lines comes out exact.
fn mesh_area(surf: Option<&Surface>, tris: &[[DVec3; 3]]) -> f64 {
    match surf {
        Some(
            s @ (Surface::Cylinder { .. }
            | Surface::Cone { .. }
            | Surface::Torus { .. }
            | Surface::Sphere { .. }),
        ) => tris
            .iter()
            .map(|t| {
                let uv = tri_uv(s, t);
                let a = 0.5 * cross2(uv[1] - uv[0], uv[2] - uv[0]).abs();
                if matches!(s, Surface::Cylinder { .. }) {
                    return a;
                }
                a * TRI_RULE
                    .iter()
                    .map(|(w, l)| w * area_element(s, uv[0] * l[0] + uv[1] * l[1] + uv[2] * l[2]))
                    .sum::<f64>()
            })
            .sum(),
        _ => tris.iter().map(tri_area).sum(),
    }
}

/// The signed area between a boundary chord `a`-`b` of a face and the curve it stands for:
/// positive when the curve bulges away from the face's interior (the side of `inside`).
fn chord_correction(surf: Option<&Surface>, piece: &Piece, a: DVec3, b: DVec3, inside: DVec3) -> f64 {
    let Some(s) = surf else { return 0.0 };
    if matches!(piece.curve, Curve::Line { .. }) || matches!(s, Surface::BSpline(_)) {
        return 0.0;
    }
    let ta = piece.curve.param_in_range(a, piece.lo, piece.hi);
    let mut tb = piece.curve.param_of(b);
    if let Some(per) = piece.curve.period() {
        tb += per * ((ta - tb) / per).round();
    }
    let m = piece.curve.eval(0.5 * (ta + tb));
    let uv = tri_uv(s, &[a, b, m]);
    let (ua, ub, um) = (uv[0], uv[1], uv[2]);
    let mut uc = s.uv(inside);
    if let Some(per) = s.u_period().filter(|p| *p > 0.0) {
        uc.x += per * ((ua.x - uc.x) / per).round();
    }
    let side_m = cross2(ub - ua, um - ua);
    let side_c = cross2(ub - ua, uc - ua);
    if side_m == 0.0 || side_c == 0.0 {
        return 0.0;
    }
    let phi = (tb - ta).abs();
    let mag = match (s, &piece.curve) {
        (Surface::Plane { .. }, Curve::Circle { radius, .. }) => 0.5 * radius * radius * (phi - phi.sin()),
        (Surface::Plane { .. }, Curve::Ellipse { a, b, .. }) => 0.5 * a * b * (phi - phi.sin()),
        _ => {
            // The sliver between the chord and the curve in parameter space, integrated
            // along the chord (Gauss-Legendre over the curve's parameter): at each node,
            // the curve's height above the chord times the area element half way up.
            let at = |t: f64| {
                let mut q = s.uv(piece.curve.eval(t));
                if let Some(per) = s.u_period().filter(|p| *p > 0.0) {
                    q.x += per * ((ua.x - q.x) / per).round();
                }
                q
            };
            let Some(dir) = (ub - ua).try_normalize() else { return 0.0 };
            let across = DVec2::new(-dir.y, dir.x);
            let (centre, half) = (0.5 * (ta + tb), 0.5 * (tb - ta));
            let step = 1e-4 * half.abs();
            let mut sum = 0.0;
            for (x, w) in GAUSS_8 {
                let t = centre + half * x;
                let c = at(t);
                let height = across.dot(c - ua);
                let foot = ua + dir * dir.dot(c - ua);
                let ds = dir.dot(at(t + step) - at(t - step)) / (2.0 * step);
                sum += w * height * area_element(s, foot + across * (0.5 * height)) * ds * half;
            }
            sum.abs()
        }
    };
    if side_m * side_c < 0.0 {
        mag
    } else {
        -mag
    }
}

/// One trimmed surface of a face (a face name can cover several after booleans).
struct Patch {
    surf: Option<Surface>,
    tris: Vec<[DVec3; 3]>,
    bvh: Bvh,
    /// How far the true surface may be from the triangles.
    tol: f64,
    boundary: Vec<Piece>,
}

impl Patch {
    fn new(surf: Option<Surface>, tris: Vec<[DVec3; 3]>, boundary: Vec<Piece>) -> Patch {
        let sag = match &surf {
            Some(s) if !matches!(s, Surface::Plane { .. }) => tris
                .iter()
                .map(|t| s.distance((t[0] + t[1] + t[2]) / 3.0))
                .filter(|d| d.is_finite())
                .fold(0.0, f64::max),
            _ => 0.0,
        };
        let scale = tris.iter().flatten().fold(1.0f64, |m, p| m.max(p.abs().max_element()));
        let bvh = Bvh::new(tris.iter().map(|t| Prim::T(t[0], t[1], t[2])).collect());
        Patch { surf, tris, bvh, tol: 2.0 * sag + 1e-9 * scale, boundary }
    }

    /// The point of the patch nearest to `p`: the foot on the surface when it lies within
    /// the face, else the nearest point of its boundary.
    fn closest(&self, p: DVec3) -> Option<DVec3> {
        let mut best: Option<DVec3> = None;
        let consider = |best: &mut Option<DVec3>, q: DVec3| {
            if best.is_none_or(|b| (q - p).length_squared() < (b - p).length_squared()) {
                *best = Some(q);
            }
        };
        if let Some(f) = self.surf.as_ref().and_then(|s| surface_foot(s, p)).filter(|f| finite(*f)) {
            if self.bvh.nearest(&Prim::P(f)).is_some_and(|(d, _, _)| d <= self.tol) {
                consider(&mut best, f);
            }
        }
        for b in &self.boundary {
            consider(&mut best, b.closest(p));
        }
        if best.is_none() {
            if let Some((_, _, q)) = self.bvh.nearest(&Prim::P(p)) {
                consider(&mut best, q);
            }
        }
        best
    }
}

// ---------------------------------------------------------------------------------------
// Resolved selections.

enum Geo {
    Point(DVec3),
    Curve(Vec<Piece>),
    Face(Vec<Patch>),
    Plane { origin: DVec3, normal: DVec3 },
}

impl Geo {
    fn closest(&self, p: DVec3) -> Option<DVec3> {
        match self {
            Geo::Point(q) => Some(*q),
            Geo::Curve(pieces) => pieces.iter().map(|c| c.closest(p)).reduce(|a, b| nearer(p, a, b)),
            Geo::Face(patches) => patches.iter().filter_map(|f| f.closest(p)).reduce(|a, b| nearer(p, a, b)),
            Geo::Plane { origin, normal } => Some(p - *normal * (p - *origin).dot(*normal)),
        }
    }

    fn prims(&self) -> Vec<Prim> {
        match self {
            Geo::Point(p) => vec![Prim::P(*p)],
            Geo::Curve(pieces) => pieces
                .iter()
                .flat_map(|c| {
                    let s = c.samples(SAMPLE_CHORD, SAMPLE_ANGLE);
                    s.windows(2).map(|w| Prim::S(w[0], w[1])).collect::<Vec<_>>()
                })
                .collect(),
            Geo::Face(patches) => {
                patches.iter().flat_map(|f| f.tris.iter().map(|t| Prim::T(t[0], t[1], t[2]))).collect()
            }
            Geo::Plane { .. } => Vec::new(),
        }
    }

    /// Points spread over the entity: curve samples or triangle corners.
    fn vertices(&self, chord: f64, angle: f64) -> Vec<DVec3> {
        match self {
            Geo::Point(p) => vec![*p],
            Geo::Curve(pieces) => pieces.iter().flat_map(|c| c.samples(chord, angle)).collect(),
            Geo::Face(patches) => {
                let mut v: Vec<DVec3> =
                    patches.iter().flat_map(|f| f.tris.iter().flatten().copied()).collect();
                v.sort_by(|a, b| {
                    a.to_array().partial_cmp(&b.to_array()).unwrap_or(std::cmp::Ordering::Equal)
                });
                v.dedup();
                v
            }
            Geo::Plane { .. } => Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum EdgeKind {
    Line { a: DVec3, b: DVec3 },
    Circle { center: DVec3, normal: DVec3, radius: f64, chord: Option<f64> },
    Other,
}

#[derive(Clone, Copy, Debug)]
enum FaceKind {
    Plane {
        origin: DVec3,
        normal: DVec3,
    },
    /// A cylinder: its axis, radius, a point on the axis level with the face's middle,
    /// and the ends of the axis over the face's extent.
    Cylinder {
        origin: DVec3,
        axis: DVec3,
        radius: f64,
        mid: DVec3,
        ends: (DVec3, DVec3),
    },
    Sphere {
        center: DVec3,
        radius: f64,
    },
    Other,
}

struct Ent {
    geo: Geo,
    edge: Option<(EdgeKind, f64)>,
    face: Option<(FaceKind, f64, f64)>,
    exact: bool,
}

impl Ent {
    fn plane(&self) -> Option<(DVec3, DVec3)> {
        match (&self.geo, &self.face) {
            (Geo::Plane { origin, normal }, _) => Some((*origin, *normal)),
            (_, Some((FaceKind::Plane { origin, normal }, _, _))) => Some((*origin, *normal)),
            _ => None,
        }
    }

    /// A straight edge: a point on it and its unit direction.
    fn line(&self) -> Option<(DVec3, DVec3)> {
        match self.edge {
            Some((EdgeKind::Line { a, b }, _)) => Some((a, (b - a).try_normalize()?)),
            _ => None,
        }
    }

    fn round(&self) -> Option<Round> {
        match (self.edge, self.face) {
            (Some((EdgeKind::Circle { center, normal, radius, .. }, _)), _) => {
                Some(Round::Circle { c: center, n: normal, r: radius })
            }
            (_, Some((FaceKind::Cylinder { origin, axis, radius, mid, ends }, _, _))) => {
                Some(Round::Cyl { o: origin, dir: axis, r: radius, mid, ends })
            }
            (_, Some((FaceKind::Sphere { center, radius }, _, _))) => {
                Some(Round::Sphere { c: center, r: radius })
            }
            _ => None,
        }
    }
}

fn classify_edge(pieces: &[Piece]) -> EdgeKind {
    let Some(first) = pieces.first() else { return EdgeKind::Other };
    let scale = pieces
        .iter()
        .fold(1.0f64, |m, p| m.max(p.start().abs().max_element()).max(p.end().abs().max_element()));
    let tol = 1e-7 * scale;
    match &first.curve {
        Curve::Line { origin, dir } => {
            let Some(dir) = dir.try_normalize() else { return EdgeKind::Other };
            let collinear = pieces.iter().all(|p| match &p.curve {
                Curve::Line { origin: o, dir: d } => {
                    d.cross(dir).length() <= ANGLE_TOL && (*o - *origin).cross(dir).length() <= tol
                }
                _ => false,
            });
            if !collinear {
                return EdgeKind::Other;
            }
            let ts = pieces.iter().flat_map(|p| [p.start(), p.end()]).map(|q| (q - *origin).dot(dir));
            let (lo, hi) = ts.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), t| (lo.min(t), hi.max(t)));
            EdgeKind::Line { a: *origin + dir * lo, b: *origin + dir * hi }
        }
        Curve::Circle { frame, radius } => {
            let same = pieces.iter().all(|p| match &p.curve {
                Curve::Circle { frame: f, radius: r } => {
                    (f.origin - frame.origin).length() <= tol
                        && (r - radius).abs() <= tol
                        && f.z.cross(frame.z).length() <= ANGLE_TOL
                }
                _ => false,
            });
            if !same {
                return EdgeKind::Other;
            }
            let sweep: f64 = pieces.iter().map(|p| p.hi - p.lo).sum();
            let chord = if sweep >= TAU - 1e-9 {
                None
            } else {
                let ends = chain_ends(pieces.iter().flat_map(|p| [p.start(), p.end()]), tol);
                match ends.as_slice() {
                    [a, b] => Some((*a - *b).length()),
                    _ => None,
                }
            };
            EdgeKind::Circle { center: frame.origin, normal: frame.z, radius: *radius, chord }
        }
        _ => EdgeKind::Other,
    }
}

/// Points that appear an odd number of times: the ends of a chain of pieces.
fn chain_ends(points: impl Iterator<Item = DVec3>, tol: f64) -> Vec<DVec3> {
    let mut groups: Vec<(DVec3, usize)> = Vec::new();
    for p in points {
        match groups.iter_mut().find(|(q, _)| (*q - p).length() <= tol.max(1e-9)) {
            Some(g) => g.1 += 1,
            None => groups.push((p, 1)),
        }
    }
    groups.into_iter().filter(|(_, n)| n % 2 == 1).map(|(p, _)| p).collect()
}

fn classify_face(patches: &[Patch]) -> FaceKind {
    let Some(first) = patches.first().and_then(|p| p.surf.as_ref()) else { return FaceKind::Other };
    let all_same = patches.iter().all(|p| p.surf.as_ref().is_some_and(|s| s.coincident(first, 1e-6)));
    if !all_same {
        return FaceKind::Other;
    }
    match first {
        Surface::Plane { frame } => FaceKind::Plane { origin: frame.origin, normal: frame.z },
        Surface::Cylinder { frame, radius } => {
            let (o, axis) = (frame.origin, frame.z);
            let mut weight = 0.0;
            let mut sum = DVec3::ZERO;
            let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
            for t in patches.iter().flat_map(|p| p.tris.iter()) {
                let a = tri_area(t);
                weight += a;
                sum += (t[0] + t[1] + t[2]) / 3.0 * a;
                for q in t {
                    let h = (*q - o).dot(axis);
                    lo = lo.min(h);
                    hi = hi.max(h);
                }
            }
            if weight <= 0.0 || !lo.is_finite() {
                return FaceKind::Other;
            }
            let mid = foot_on_line(sum / weight, o, axis);
            FaceKind::Cylinder { origin: o, axis, radius: *radius, mid, ends: (o + axis * lo, o + axis * hi) }
        }
        Surface::Sphere { frame, radius } => FaceKind::Sphere { center: frame.origin, radius: *radius },
        _ => FaceKind::Other,
    }
}

// ---------------------------------------------------------------------------------------
// Finding the geometry of a pick.

type Key = (i64, i64, i64);

fn qkey(v: DVec3) -> Key {
    let s = 1e5;
    ((v.x * s).round() as i64, (v.y * s).round() as i64, (v.z * s).round() as i64)
}

fn seg_key(a: DVec3, b: DVec3) -> (Key, Key) {
    let (ka, kb) = (qkey(a), qkey(b));
    if ka <= kb {
        (ka, kb)
    } else {
        (kb, ka)
    }
}

fn bits(p: DVec3) -> [u64; 3] {
    [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]
}

fn bits_pair(a: DVec3, b: DVec3) -> ([u64; 3], [u64; 3]) {
    let (ka, kb) = (bits(a), bits(b));
    if ka <= kb {
        (ka, kb)
    } else {
        (kb, ka)
    }
}

/// The faceted body with every polygon edge indexed by its end points.
struct Faceted {
    body: Solid,
    /// For each polygon edge: its end points and the polygons that use it.
    users: HashMap<(Key, Key), (DVec3, DVec3, Vec<usize>)>,
}

impl Faceted {
    fn new(body: &Solid) -> Faceted {
        let mut body = body.clone();
        body.repair_t_junctions();
        let mut users: HashMap<(Key, Key), (DVec3, DVec3, Vec<usize>)> = HashMap::new();
        for (pi, p) in body.polygons.iter().enumerate() {
            let k = p.verts.len();
            for i in 0..k {
                let (a, b) = (d3(p.verts[i]), d3(p.verts[(i + 1) % k]));
                let key = seg_key(a, b);
                if key.0 == key.1 {
                    continue;
                }
                users.entry(key).or_insert_with(|| (a, b, Vec::new())).2.push(pi);
            }
        }
        Faceted { body, users }
    }

    fn face_of(&self, pi: usize) -> Option<FaceId> {
        self.body.polygons.get(pi).map(|p| p.tag.id)
    }

    fn kind_of(&self, id: FaceId) -> Option<SurfaceKind> {
        self.body.polygons.iter().find(|p| p.tag.id == id).map(|p| p.tag.surface)
    }
}

/// Axes of the cylindrical or revolved faces among `kinds`.
fn axes_of(kinds: &[Option<SurfaceKind>]) -> Vec<(DVec3, DVec3)> {
    kinds
        .iter()
        .filter_map(|k| match k {
            Some(SurfaceKind::Cylinder { axis_origin, axis_dir, .. })
            | Some(SurfaceKind::Revolved { axis_origin, axis_dir }) => {
                Some((d3(*axis_origin), d3(*axis_dir)))
            }
            _ => None,
        })
        .collect()
}

/// Recovers the curve behind a set of faceted edge segments: one straight line, one
/// circular arc or circle about one of `axes`, or else the segments themselves.
fn fit_pieces(segs: &[(DVec3, DVec3)], axes: &[(DVec3, DVec3)]) -> Vec<Piece> {
    let pts: Vec<DVec3> = segs.iter().flat_map(|s| [s.0, s.1]).collect();
    let Some(&p0) = pts.first() else { return Vec::new() };
    let scale = pts.iter().fold(1.0f64, |m, p| m.max(p.abs().max_element()));
    let tol = 1e-6 * scale;
    let total: f64 = segs.iter().map(|s| (s.1 - s.0).length()).sum();

    // A straight line.
    let far = pts.iter().copied().fold(p0, |f, p| if (p - p0).length() > (f - p0).length() { p } else { f });
    if let Some(dir) = (far - p0).try_normalize() {
        if pts.iter().all(|p| (*p - p0).cross(dir).length() <= tol) {
            let ts = pts.iter().map(|p| (*p - p0).dot(dir));
            let (lo, hi) = ts.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), t| (lo.min(t), hi.max(t)));
            if (total - (hi - lo)).abs() <= tol * (1 + segs.len()) as f64 {
                return vec![Piece { curve: Curve::Line { origin: p0, dir }, lo, hi }];
            }
        }
    }

    // A circle about an adjacent face's axis.
    for &(o, dir) in axes {
        let Some(dir) = dir.try_normalize() else { continue };
        let h0 = (p0 - o).dot(dir);
        let centre = o + dir * h0;
        let rho = |p: DVec3| ((p - o) - dir * (p - o).dot(dir)).length();
        let r = rho(p0);
        if r <= tol
            || !pts.iter().all(|p| ((*p - o).dot(dir) - h0).abs() <= tol && (rho(*p) - r).abs() <= tol)
        {
            continue;
        }
        let sweep: f64 = segs
            .iter()
            .map(|(a, b)| {
                let (ua, ub) = (*a - centre, *b - centre);
                ua.cross(ub).length().atan2(ua.dot(ub))
            })
            .sum();
        let ends = chain_ends(pts.iter().copied(), tol);
        let piece = match ends.as_slice() {
            [] if (sweep - TAU).abs() <= 1e-6 => frame(centre, dir, p0 - centre)
                .map(|f| Piece::new(Curve::Circle { frame: f, radius: r }, 0.0, TAU)),
            [start, _] => {
                // Orient the frame so the arc runs counter-clockwise from its start.
                let next = segs.iter().find_map(|(a, b)| {
                    if (*a - *start).length() <= tol {
                        Some(*b)
                    } else if (*b - *start).length() <= tol {
                        Some(*a)
                    } else {
                        None
                    }
                });
                frame(centre, dir, *start - centre).and_then(|f| {
                    let turn = next.map(|q| f.to_local(q).y).unwrap_or(0.0);
                    let f = if turn < 0.0 { frame(centre, -dir, *start - centre)? } else { f };
                    Some(Piece::new(Curve::Circle { frame: f, radius: r }, 0.0, sweep))
                })
            }
            _ => None,
        };
        if let Some(piece) = piece {
            return vec![piece];
        }
    }
    segs.iter().filter_map(|(a, b)| Piece::line(*a, *b)).collect()
}

struct Resolver<'a> {
    exact: Option<&'a [brep::Solid]>,
    meshes: HashMap<usize, brep::tess::TriMesh>,
    body: &'a Solid,
    faceted: Option<Faceted>,
}

impl<'a> Resolver<'a> {
    fn new(st: &'a BuildState) -> Resolver<'a> {
        Resolver {
            exact: st.exact.as_deref().filter(|b| !b.is_empty()),
            meshes: HashMap::new(),
            body: &st.body,
            faceted: None,
        }
    }

    fn resolve(&mut self, pick: &Pick) -> Result<Ent, String> {
        match *pick {
            Pick::Point(p) => {
                let p = d3(p);
                if !finite(p) {
                    return Err("a picked point is not a finite position".into());
                }
                Ok(Ent { geo: Geo::Point(p), edge: None, face: None, exact: true })
            }
            Pick::Plane { origin, normal } => {
                let origin = d3(origin);
                let normal =
                    d3(normal).try_normalize().filter(|_| finite(origin)).ok_or("a plane needs a normal")?;
                Ok(Ent { geo: Geo::Plane { origin, normal }, edge: None, face: None, exact: true })
            }
            Pick::Face(id) => self
                .exact_face(id)
                .or_else(|| self.faceted_face(id))
                .ok_or_else(|| format!("face <{}:{}> is not in the part", id.feature, id.index)),
            Pick::Edge(e) => self.exact_edge(&e).or_else(|| self.faceted_edge(&e)).ok_or_else(|| {
                format!(
                    "the edge between faces <{}:{}> and <{}:{}> is not in the part",
                    e.a.feature, e.a.index, e.b.feature, e.b.index
                )
            }),
        }
    }

    fn exact_mesh(&mut self, bi: usize) -> Option<&brep::tess::TriMesh> {
        let bodies = self.exact?;
        if let std::collections::hash_map::Entry::Vacant(slot) = self.meshes.entry(bi) {
            slot.insert(brep::tess::tessellate(bodies.get(bi)?, &brep::tess::TessParams::default()).ok()?);
        }
        self.meshes.get(&bi)
    }

    fn exact_face(&mut self, id: FaceId) -> Option<Ent> {
        let bodies = self.exact?;
        let mut patches = Vec::new();
        let mut area = 0.0;
        // Boundary edges of every patch: an edge two patches of the face share lies
        // inside the face, not on its perimeter.
        let mut edges: Vec<((usize, brep::EdgeId), f64, usize)> = Vec::new();
        for (bi, body) in bodies.iter().enumerate() {
            for (fi, face) in body.faces.iter().enumerate() {
                if face_id(face) != id {
                    continue;
                }
                let mesh = self.exact_mesh(bi)?;
                let (patch, a, bounds) = exact_patch(body, fi, mesh)?;
                patches.push(patch);
                area += a;
                for (e, length) in bounds {
                    match edges.iter_mut().find(|x| x.0 == (bi, e)) {
                        Some(x) => x.2 += 1,
                        None => edges.push(((bi, e), length, 1)),
                    }
                }
            }
        }
        if patches.is_empty() {
            return None;
        }
        let perimeter = edges.iter().filter(|x| x.2 == 1).map(|x| x.1).sum();
        let kind = classify_face(&patches);
        Some(Ent { geo: Geo::Face(patches), edge: None, face: Some((kind, area, perimeter)), exact: true })
    }

    fn exact_edge(&mut self, e: &EdgeRef) -> Option<Ent> {
        let bodies = self.exact?;
        let mut pieces = Vec::new();
        for solid in bodies {
            for (ei, edge) in solid.edges.iter().enumerate() {
                let names: Vec<FaceId> = solid
                    .edge_faces(brep::EdgeId(ei as u32))
                    .iter()
                    .filter_map(|f| solid.face(*f))
                    .map(face_id)
                    .collect();
                if names.len() == 2 && EdgeRef::new(names[0], names[1]) == *e {
                    pieces.push(Piece::new(edge.curve.clone(), edge.t0, edge.t1));
                }
            }
        }
        if pieces.is_empty() {
            return None;
        }
        let length = pieces.iter().map(Piece::length).sum();
        let kind = classify_edge(&pieces);
        Some(Ent { geo: Geo::Curve(pieces), edge: Some((kind, length)), face: None, exact: true })
    }

    fn faceted(&mut self) -> &Faceted {
        let body = self.body;
        self.faceted.get_or_insert_with(|| Faceted::new(body))
    }

    fn faceted_face(&mut self, id: FaceId) -> Option<Ent> {
        let fc = self.faceted();
        let polys: Vec<usize> =
            fc.body.polygons.iter().enumerate().filter(|(_, p)| p.tag.id == id).map(|(i, _)| i).collect();
        let first = fc.body.polygons.get(*polys.first()?)?;
        let surf = match first.tag.surface {
            SurfaceKind::Plane => {
                let n = d3(first.normal);
                frame(n * first.w, n, n.any_orthonormal_vector()).map(|frame| Surface::Plane { frame })
            }
            SurfaceKind::Cylinder { axis_origin, axis_dir, radius } => {
                let dir = d3(axis_dir);
                frame(d3(axis_origin), dir, dir.any_orthonormal_vector())
                    .map(|frame| Surface::Cylinder { frame, radius })
            }
            _ => None,
        };
        let mut tris = Vec::new();
        // Boundary segments grouped by the face on their other side.
        let mut groups: BTreeMap<Option<FaceId>, Vec<(DVec3, DVec3, DVec3)>> = BTreeMap::new();
        for &pi in &polys {
            let Some(poly) = fc.body.polygons.get(pi) else { continue };
            let verts: Vec<DVec3> = poly.verts.iter().map(|v| d3(*v)).collect();
            let Some(&v0) = verts.first() else { continue };
            for w in verts.windows(2).skip(1) {
                tris.push([v0, w[0], w[1]]);
            }
            let centroid = verts.iter().copied().sum::<DVec3>() / verts.len() as f64;
            let k = verts.len();
            for i in 0..k {
                let (a, b) = (verts[i], verts[(i + 1) % k]);
                let key = seg_key(a, b);
                if key.0 == key.1 {
                    continue;
                }
                let users = fc.users.get(&key).map(|u| u.2.as_slice()).unwrap_or(&[]);
                if users.iter().filter(|u| fc.face_of(**u) == Some(id)).count() >= 2 {
                    continue;
                }
                let other = users.iter().filter_map(|u| fc.face_of(*u)).find(|f| *f != id);
                groups.entry(other).or_default().push((a, b, centroid));
            }
        }
        let own = Some(first.tag.surface);
        let mut boundary = Vec::new();
        let (mut perimeter, mut correction) = (0.0, 0.0);
        for (other, segs) in &groups {
            let axes = axes_of(&[own, other.and_then(|o| fc.kind_of(o))]);
            let pairs: Vec<(DVec3, DVec3)> = segs.iter().map(|s| (s.0, s.1)).collect();
            let pieces = fit_pieces(&pairs, &axes);
            perimeter += pieces.iter().map(Piece::length).sum::<f64>();
            if let [piece] = pieces.as_slice() {
                for (a, b, inside) in segs {
                    correction += chord_correction(surf.as_ref(), piece, *a, *b, *inside);
                }
            }
            boundary.extend(pieces);
        }
        let area = mesh_area(surf.as_ref(), &tris) + correction;
        let patch = Patch::new(surf, tris, boundary);
        let patches = vec![patch];
        let kind = match first.tag.surface {
            SurfaceKind::Plane => {
                let n = d3(first.normal);
                FaceKind::Plane { origin: n * first.w, normal: n }
            }
            SurfaceKind::Cylinder { .. } => classify_face(&patches),
            _ => FaceKind::Other,
        };
        Some(Ent { geo: Geo::Face(patches), edge: None, face: Some((kind, area, perimeter)), exact: false })
    }

    fn faceted_edge(&mut self, e: &EdgeRef) -> Option<Ent> {
        let fc = self.faceted();
        let mut segs: Vec<(DVec3, DVec3)> = fc
            .users
            .values()
            .filter_map(|(a, b, users)| {
                let [u, v] = users.as_slice() else { return None };
                let (fu, fv) = (fc.face_of(*u)?, fc.face_of(*v)?);
                (fu != fv && EdgeRef::new(fu, fv) == *e).then_some((*a, *b))
            })
            .collect();
        if segs.is_empty() {
            return None;
        }
        segs.sort_by(|x, y| {
            (x.0.to_array(), x.1.to_array())
                .partial_cmp(&(y.0.to_array(), y.1.to_array()))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let axes = axes_of(&[fc.kind_of(e.a), fc.kind_of(e.b)]);
        let pieces = fit_pieces(&segs, &axes);
        let length = pieces.iter().map(Piece::length).sum();
        let kind = classify_edge(&pieces);
        Some(Ent { geo: Geo::Curve(pieces), edge: Some((kind, length)), face: None, exact: false })
    }
}

/// Boundary edges of a patch with their lengths.
type Bounds = Vec<(brep::EdgeId, f64)>;

/// One exact face: its patch, its area, and its boundary edges (seams left out).
fn exact_patch(body: &brep::Solid, fi: usize, mesh: &brep::tess::TriMesh) -> Option<(Patch, f64, Bounds)> {
    let face = body.faces.get(fi)?;
    let pos = |i: u32| mesh.positions.get(i as usize).copied();
    let tris: Vec<[DVec3; 3]> = mesh
        .triangles
        .iter()
        .zip(&mesh.triangle_face)
        .filter(|(_, f)| **f as usize == fi)
        .filter_map(|(t, _)| Some([pos(t[0])?, pos(t[1])?, pos(t[2])?]))
        .collect();
    // The third corner of the triangle on each triangle edge, for telling the face's side.
    let mut third: HashMap<([u64; 3], [u64; 3]), DVec3> = HashMap::new();
    for t in &tris {
        for k in 0..3 {
            third.insert(bits_pair(t[k], t[(k + 1) % 3]), t[(k + 2) % 3]);
        }
    }
    // Edges a face uses twice are seams of a closed surface, not boundaries.
    let mut uses: Vec<(brep::EdgeId, usize)> = Vec::new();
    for c in face.loops().flat_map(|l| l.coedges.iter()) {
        match uses.iter_mut().find(|u| u.0 == c.edge) {
            Some(u) => u.1 += 1,
            None => uses.push((c.edge, 1)),
        }
    }
    let mut boundary = Vec::new();
    let mut bounds = Vec::new();
    let mut correction = 0.0;
    for (id, n) in uses {
        if n != 1 {
            continue;
        }
        let Some(edge) = body.edge(id) else { continue };
        let piece = Piece::new(edge.curve.clone(), edge.t0, edge.t1);
        bounds.push((id, piece.length()));
        if let Some(samples) = mesh.edges.get(id.index()) {
            for w in samples.windows(2) {
                if let Some(c) = third.get(&bits_pair(w[0], w[1])) {
                    correction += chord_correction(Some(&face.surface), &piece, w[0], w[1], *c);
                }
            }
        }
        boundary.push(piece);
    }
    let area = mesh_area(Some(&face.surface), &tris) + correction;
    Some((Patch::new(Some(face.surface.clone()), tris, boundary), area, bounds))
}

// ---------------------------------------------------------------------------------------
// What the selections report.

fn single(e: &Ent, m: &mut Measurement) {
    if let Geo::Point(p) = e.geo {
        m.point = Some(v3(p));
    }
    if let Some((kind, length)) = e.edge {
        m.length = Some(length);
        if let EdgeKind::Circle { center, radius, chord, .. } = kind {
            m.radius = Some(radius);
            m.diameter = Some(2.0 * radius);
            m.center = Some(v3(center));
            if let Some(chord) = chord {
                m.arc_length = Some(length);
                m.chord_length = Some(chord);
            }
        }
    }
    if let Some((kind, area, perimeter)) = e.face {
        m.area = Some(area);
        m.perimeter = Some(perimeter);
        match kind {
            FaceKind::Cylinder { radius, .. } => {
                m.radius = Some(radius);
                m.diameter = Some(2.0 * radius);
            }
            FaceKind::Sphere { center, radius } => {
                m.radius = Some(radius);
                m.diameter = Some(2.0 * radius);
                m.center = Some(v3(center));
            }
            _ => {}
        }
    }
}

fn pair(a: &Ent, b: &Ent, arc: (ArcCondition, ArcCondition), m: &mut Measurement) -> Result<(), String> {
    if let (Some((_, la)), Some((_, lb))) = (a.edge, b.edge) {
        m.total_length = Some(la + lb);
    }
    if let (Some((_, aa, _)), Some((_, ab, _))) = (a.face, b.face) {
        m.total_area = Some(aa + ab);
    }
    let (d, pa, pb) = match arc_distance(a, b, arc) {
        Some(r) => {
            m.center_distance = r.center;
            m.arc_conditions = Some(arc);
            (r.distance, r.pa, r.pb)
        }
        None => {
            min_distance(&a.geo, &b.geo).ok_or("the distance between the selections could not be found")?
        }
    };
    m.distance = Some(d);
    m.points = Some((v3(pa), v3(pb)));
    m.delta = Some(v3(pb - pa));
    m.normal_distance = normal_distance(a, b).or_else(|| normal_distance(b, a));
    if let Some(angle) = angle_between(a, b) {
        m.angle_deg = Some(angle.to_degrees());
        m.parallel = Some(angle <= ANGLE_TOL);
        m.perpendicular = Some((angle - PI / 2.0).abs() <= ANGLE_TOL);
    }
    Ok(())
}

fn totals(ents: &[Ent], m: &mut Measurement) -> Result<(), String> {
    if ents.iter().any(|e| e.edge.is_none() && e.face.is_none()) {
        return Err("with more than two selections, select only edges and faces".into());
    }
    let lengths: Vec<f64> = ents.iter().filter_map(|e| e.edge.map(|x| x.1)).collect();
    let areas: Vec<f64> = ents.iter().filter_map(|e| e.face.map(|x| x.1)).collect();
    if !lengths.is_empty() {
        m.total_length = Some(lengths.iter().sum());
    }
    if !areas.is_empty() {
        m.total_area = Some(areas.iter().sum());
    }
    Ok(())
}

/// Perpendicular distance from `plane`'s plane to a point, or to a parallel straight edge,
/// planar face or cylinder axis.
fn normal_distance(plane: &Ent, other: &Ent) -> Option<f64> {
    let (o, n) = plane.plane()?;
    let gap = |q: DVec3| (q - o).dot(n).abs();
    if let Geo::Point(p) = other.geo {
        return Some(gap(p));
    }
    if let Some((q, dir)) = other.line() {
        return (dir.dot(n).abs() <= ANGLE_TOL).then(|| gap(q));
    }
    if let Some((q, n2)) = other.plane() {
        return (n.cross(n2).length() <= ANGLE_TOL).then(|| gap(q));
    }
    if let Some((FaceKind::Cylinder { origin, axis, .. }, _, _)) = other.face {
        return (axis.dot(n).abs() <= ANGLE_TOL).then(|| gap(origin));
    }
    None
}

/// Angle between two straight edges, a straight edge and a plane, or two planes, from 0
/// to a right angle, in radians.
fn angle_between(a: &Ent, b: &Ent) -> Option<f64> {
    let between_lines = |u: DVec3, v: DVec3| u.cross(v).length().atan2(u.dot(v).abs());
    match (a.line(), a.plane(), b.line(), b.plane()) {
        (Some((_, u)), _, Some((_, v)), _) => Some(between_lines(u, v)),
        (Some((_, u)), _, _, Some((_, n))) | (_, Some((_, n)), Some((_, u)), _) => {
            Some(u.dot(n).abs().atan2(u.cross(n).length()))
        }
        (_, Some((_, n1)), _, Some((_, n2))) => Some(between_lines(n1, n2)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------
// Arc/circle conditions.

#[derive(Clone, Copy, Debug)]
enum Round {
    Circle { c: DVec3, n: DVec3, r: f64 },
    Cyl { o: DVec3, dir: DVec3, r: f64, mid: DVec3, ends: (DVec3, DVec3) },
    Sphere { c: DVec3, r: f64 },
}

impl Round {
    fn radius(&self) -> f64 {
        match *self {
            Round::Circle { r, .. } | Round::Cyl { r, .. } | Round::Sphere { r, .. } => r,
        }
    }

    /// The centre as a set: a point, or the axis over the face's extent.
    fn centre_geo(&self) -> Geo {
        match *self {
            Round::Circle { c, .. } | Round::Sphere { c, .. } => Geo::Point(c),
            Round::Cyl { mid, ends, .. } => match Piece::line(ends.0, ends.1) {
                Some(p) => Geo::Curve(vec![p]),
                None => Geo::Point(mid),
            },
        }
    }
}

struct ArcResult {
    distance: f64,
    pa: DVec3,
    pb: DVec3,
    center: Option<f64>,
}

fn sign(c: ArcCondition) -> f64 {
    match c {
        ArcCondition::Center => 0.0,
        ArcCondition::Minimum => -1.0,
        ArcCondition::Maximum => 1.0,
    }
}

fn arc_distance(a: &Ent, b: &Ent, conds: (ArcCondition, ArcCondition)) -> Option<ArcResult> {
    match (a.round(), b.round()) {
        (Some(ra), Some(rb)) => Some(round_round(ra, rb, a, b, conds)),
        (Some(ra), None) => round_plane(ra, b.plane()?, conds.0).map(|r| r.swapped(false)),
        (None, Some(rb)) => round_plane(rb, a.plane()?, conds.1).map(|r| r.swapped(true)),
        (None, None) => None,
    }
}

impl ArcResult {
    fn swapped(self, swap: bool) -> ArcResult {
        if swap {
            ArcResult { pa: self.pb, pb: self.pa, ..self }
        } else {
            self
        }
    }
}

/// The two centres of a pair of round entities when they lie in one plane across their
/// axes (coplanar circles, parallel cylinders, spheres), with that plane's normal.
fn coplanar_centres(ra: &Round, rb: &Round) -> Option<(DVec3, DVec3, Option<DVec3>)> {
    let scale = 1.0 + [ra, rb].iter().map(|r| r.radius()).fold(0.0, f64::max);
    let tol = 1e-9 * scale;
    let parallel = |u: DVec3, v: DVec3| u.cross(v).length() <= ANGLE_TOL;
    match (*ra, *rb) {
        (Round::Circle { c: c1, n: n1, .. }, Round::Circle { c: c2, n: n2, .. }) => (parallel(n1, n2)
            && (c2 - c1).dot(n1).abs() <= tol * (1.0 + (c2 - c1).length()))
        .then_some((c1, c2, Some(n1))),
        (Round::Cyl { dir: d1, mid, .. }, Round::Cyl { o: o2, dir: d2, .. }) => {
            parallel(d1, d2).then(|| (mid, foot_on_line(mid, o2, d2), Some(d1)))
        }
        (Round::Circle { c, n, .. }, Round::Cyl { o, dir, .. }) => {
            parallel(n, dir).then(|| (c, foot_on_line(c, o, dir), Some(dir)))
        }
        (Round::Cyl { o, dir, .. }, Round::Circle { c, n, .. }) => {
            parallel(n, dir).then(|| (foot_on_line(c, o, dir), c, Some(dir)))
        }
        (Round::Sphere { c: c1, .. }, Round::Sphere { c: c2, .. }) => Some((c1, c2, None)),
        (Round::Sphere { c, .. }, Round::Cyl { o, dir, .. }) => Some((c, foot_on_line(c, o, dir), Some(dir))),
        (Round::Cyl { o, dir, .. }, Round::Sphere { c, .. }) => Some((foot_on_line(c, o, dir), c, Some(dir))),
        (Round::Sphere { c: cs, .. }, Round::Circle { c, n, .. }) => {
            ((cs - c).dot(n).abs() <= tol * (1.0 + (cs - c).length())).then_some((cs, c, Some(n)))
        }
        (Round::Circle { c, n, .. }, Round::Sphere { c: cs, .. }) => {
            ((cs - c).dot(n).abs() <= tol * (1.0 + (cs - c).length())).then_some((c, cs, Some(n)))
        }
    }
}

fn round_round(ra: Round, rb: Round, a: &Ent, b: &Ent, conds: (ArcCondition, ArcCondition)) -> ArcResult {
    let (r1, r2) = (ra.radius(), rb.radius());
    if let Some((c1, c2, axis)) = coplanar_centres(&ra, &rb) {
        // Measured along the line joining the centres: each condition moves its point by
        // its radius towards (minimum) or away from (maximum) the other entity.
        let v = c2 - c1;
        let d = v.length();
        let u = match v.try_normalize() {
            Some(u) if d > 1e-12 => u,
            _ => axis.map(|n| n.any_orthonormal_vector()).unwrap_or(DVec3::X),
        };
        let w = match axis {
            Some(n) => n.cross(u).try_normalize().unwrap_or_else(|| u.any_orthonormal_vector()),
            None => u.any_orthonormal_vector(),
        };
        let (pa, pb) = if conds == (ArcCondition::Minimum, ArcCondition::Minimum) {
            if d >= r1 + r2 {
                (c1 + u * r1, c2 - u * r2)
            } else if d <= (r1 - r2).abs() {
                // One inside the other.
                if r1 >= r2 {
                    (c1 + u * r1, c2 + u * r2)
                } else {
                    (c1 - u * r1, c2 - u * r2)
                }
            } else {
                // Overlapping: they cross.
                let x = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
                let h = (r1 * r1 - x * x).max(0.0).sqrt();
                let p = c1 + u * x + w * h;
                (p, p)
            }
        } else {
            (c1 - u * (sign(conds.0) * r1), c2 + u * (sign(conds.1) * r2))
        };
        return ArcResult { distance: (pb - pa).length(), pa, pb, center: Some(d) };
    }
    // Not in one plane: centre to centre between the centres (or the axes' closest points),
    // otherwise between the chosen sets, found numerically.
    let (ca, cb) = (ra.centre_geo(), rb.centre_geo());
    let centre = centre_pair(&ra, &rb);
    let center = centre.map(|(p, q)| (q - p).length());
    let found = match conds {
        (ArcCondition::Center, ArcCondition::Center) => centre.map(|(p, q)| ((q - p).length(), p, q)),
        _ => {
            let sa = if conds.0 == ArcCondition::Center { &ca } else { &a.geo };
            let sb = if conds.1 == ArcCondition::Center { &cb } else { &b.geo };
            if conds.0 == ArcCondition::Maximum || conds.1 == ArcCondition::Maximum {
                max_distance(sa, sb)
            } else {
                min_distance(sa, sb)
            }
        }
    };
    match found {
        Some((distance, pa, pb)) => ArcResult { distance, pa, pb, center },
        None => {
            let (pa, pb) = centre.unwrap_or((DVec3::ZERO, DVec3::ZERO));
            ArcResult { distance: center.unwrap_or(0.0), pa, pb, center }
        }
    }
}

/// Closest points of two centres: points, or unbounded axis lines.
fn centre_pair(ra: &Round, rb: &Round) -> Option<(DVec3, DVec3)> {
    let as_line = |r: &Round| match *r {
        Round::Cyl { o, dir, .. } => Some((o, dir)),
        _ => None,
    };
    let as_point = |r: &Round| match *r {
        Round::Circle { c, .. } | Round::Sphere { c, .. } => Some(c),
        Round::Cyl { .. } => None,
    };
    match (as_point(ra), as_point(rb), as_line(ra), as_line(rb)) {
        (Some(p), Some(q), _, _) => Some((p, q)),
        (Some(p), _, _, Some((o, d))) => Some((p, foot_on_line(p, o, d))),
        (_, Some(q), Some((o, d)), _) => Some((foot_on_line(q, o, d), q)),
        (_, _, Some((o1, d1)), Some((o2, d2))) => {
            // Closest points of two unbounded lines.
            let r = o1 - o2;
            let (b, c, f) = (d1.dot(d2), d1.dot(r), d2.dot(r));
            let denom = 1.0 - b * b;
            if denom <= 1e-18 {
                Some((o1, foot_on_line(o1, o2, d2)))
            } else {
                let s = (b * f - c) / denom;
                let t = (f + b * s).clamp(f64::MIN, f64::MAX);
                Some((o1 + d1 * s, o2 + d2 * t))
            }
        }
        _ => None,
    }
}

/// A cylinder whose axis is parallel to a plane, or a sphere, against that plane: only the
/// round entity carries a condition. The first point is on the round entity.
fn round_plane(r: Round, (o, n): (DVec3, DVec3), cond: ArcCondition) -> Option<ArcResult> {
    let (c, w) = match r {
        Round::Cyl { dir, mid, .. } if dir.dot(n).abs() <= ANGLE_TOL => {
            (mid, dir.cross(n).normalize_or_zero())
        }
        Round::Sphere { c, .. } => (c, n.any_orthonormal_vector()),
        _ => return None,
    };
    let radius = r.radius();
    let sd = (c - o).dot(n);
    let d = sd.abs();
    let u = if sd > 0.0 { -n } else { n };
    let foot = c + u * d;
    let (pr, pp) = match cond {
        ArcCondition::Center => (c, foot),
        ArcCondition::Maximum => (c - u * radius, foot),
        ArcCondition::Minimum if d >= radius => (c + u * radius, foot),
        ArcCondition::Minimum => {
            let p = foot + w * (radius * radius - d * d).max(0.0).sqrt();
            (p, p)
        }
    };
    Some(ArcResult { distance: (pp - pr).length(), pa: pr, pb: pp, center: Some(d) })
}

// ---------------------------------------------------------------------------------------
// Distances between entities.

/// The smallest distance between two entities and the points where it is found, on `a`
/// and on `b`.
fn min_distance(a: &Geo, b: &Geo) -> Option<(f64, DVec3, DVec3)> {
    match (a, b) {
        (Geo::Plane { origin: o1, normal: n1 }, Geo::Plane { origin: o2, normal: n2 }) => {
            if n1.cross(*n2).length() <= ANGLE_TOL {
                let q = *o1 - *n2 * (*o1 - *o2).dot(*n2);
                Some(((q - *o1).length(), *o1, q))
            } else {
                // They cross: a point on both.
                let (d1, d2) = (n1.dot(*o1), n2.dot(*o2));
                let k = n1.dot(*n2);
                let det = 1.0 - k * k;
                let p = (*n1 * (d1 - d2 * k) + *n2 * (d2 - d1 * k)) / det;
                Some((0.0, p, p))
            }
        }
        (Geo::Plane { origin, normal }, other) => {
            plane_distance(other, *origin, *normal).map(|(d, p, q)| (d, q, p))
        }
        (other, Geo::Plane { origin, normal }) => plane_distance(other, *origin, *normal),
        _ => {
            let bvh = Bvh::new(b.prims());
            let mut best: Option<(f64, DVec3, DVec3)> = None;
            for p in a.prims() {
                if let Some(found) = bvh.nearest_within(&p, best.map(|b| b.0)) {
                    if best.is_none_or(|b| found.0 < b.0) {
                        best = Some(found);
                    }
                }
            }
            let (_, pa, pb) = best?;
            Some(refine(a, b, pa, pb))
        }
    }
}

/// Alternating closest-point projections onto the exact entities, from a start found on
/// their tessellations. Each step can only shorten the distance.
fn refine(a: &Geo, b: &Geo, mut pa: DVec3, mut pb: DVec3) -> (f64, DVec3, DVec3) {
    let scale = 1.0 + pa.abs().max_element().max(pb.abs().max_element());
    for _ in 0..500 {
        let Some(na) = a.closest(pb) else { break };
        let Some(nb) = b.closest(na) else { break };
        let moved = (na - pa).length() + (nb - pb).length();
        pa = na;
        pb = nb;
        if moved <= 1e-14 * scale {
            break;
        }
    }
    ((pb - pa).length(), pa, pb)
}

/// Distance from a bounded entity to an unbounded plane: the point on the entity first,
/// then the point on the plane.
fn plane_distance(g: &Geo, o: DVec3, n: DVec3) -> Option<(f64, DVec3, DVec3)> {
    let verts = g.vertices(SAMPLE_CHORD, SAMPLE_ANGLE);
    let side = |p: DVec3| (p - o).dot(n);
    let scale = 1.0 + verts.iter().fold(0.0f64, |m, p| m.max(p.abs().max_element()));
    let eps = 1e-12 * scale;
    let below = verts.iter().any(|p| side(*p) < -eps);
    let above = verts.iter().any(|p| side(*p) > eps);
    if below && above {
        // The entity crosses the plane: find a crossing point on one of its pieces.
        for prim in g.prims() {
            let corners = prim.corners();
            for (i, p) in corners.iter().enumerate() {
                for q in corners.iter().skip(i + 1) {
                    let (sp, sq) = (side(*p), side(*q));
                    if sp * sq < 0.0 {
                        let x = *p + (*q - *p) * (sp / (sp - sq));
                        return Some((0.0, x, x));
                    }
                }
            }
        }
    }
    let start = verts.iter().copied().min_by(|p, q| side(*p).abs().total_cmp(&side(*q).abs()))?;
    let plane = Geo::Plane { origin: o, normal: n };
    Some(refine(g, &plane, start, start - n * side(start)))
}

/// The largest distance between two bounded entities, from their vertices and then by
/// alternating farthest-point steps on finer samples.
fn max_distance(a: &Geo, b: &Geo) -> Option<(f64, DVec3, DVec3)> {
    let (va, vb) = (a.vertices(SAMPLE_CHORD, SAMPLE_ANGLE), b.vertices(SAMPLE_CHORD, SAMPLE_ANGLE));
    let mut best: Option<(f64, DVec3, DVec3)> = None;
    for p in &va {
        for q in &vb {
            let d = (*q - *p).length();
            if best.is_none_or(|b| d > b.0) {
                best = Some((d, *p, *q));
            }
        }
    }
    let (_, mut pa, mut pb) = best?;
    let (fa, fb) = (a.vertices(1e-6, 1e-3), b.vertices(1e-6, 1e-3));
    let farthest = |set: &[DVec3], p: DVec3| {
        set.iter().copied().max_by(|x, y| (*x - p).length_squared().total_cmp(&(*y - p).length_squared()))
    };
    for _ in 0..20 {
        let Some(na) = farthest(&fa, pb) else { break };
        let Some(nb) = farthest(&fb, na) else { break };
        let moved = (na - pa).length() + (nb - pb).length();
        pa = na;
        pb = nb;
        if moved <= 1e-12 {
            break;
        }
    }
    Some(((pb - pa).length(), pa, pb))
}

// ---------------------------------------------------------------------------------------
// Primitives and a bounding-volume hierarchy for the discrete distance search.

#[derive(Clone, Copy, Debug)]
enum Prim {
    P(DVec3),
    S(DVec3, DVec3),
    T(DVec3, DVec3, DVec3),
}

impl Prim {
    fn corners(&self) -> Vec<DVec3> {
        match *self {
            Prim::P(p) => vec![p],
            Prim::S(a, b) => vec![a, b],
            Prim::T(a, b, c) => vec![a, b, c],
        }
    }

    fn bounds(&self) -> (DVec3, DVec3) {
        match *self {
            Prim::P(p) => (p, p),
            Prim::S(a, b) => (a.min(b), a.max(b)),
            Prim::T(a, b, c) => (a.min(b).min(c), a.max(b).max(c)),
        }
    }
}

fn cp_seg(p: DVec3, a: DVec3, b: DVec3) -> DVec3 {
    let ab = b - a;
    let l2 = ab.length_squared();
    if l2 <= 1e-300 {
        return a;
    }
    a + ab * ((p - a).dot(ab) / l2).clamp(0.0, 1.0)
}

/// The point of triangle `abc` nearest to `p` (after Ericson, Real-Time Collision
/// Detection, 5.1.5).
fn cp_tri(p: DVec3, a: DVec3, b: DVec3, c: DVec3) -> DVec3 {
    let ab = b - a;
    let ac = c - a;
    if ab.cross(ac).length_squared() <= 1e-24 * (ab.length_squared() * ac.length_squared()).max(1e-300) {
        let q1 = cp_seg(p, a, b);
        let q2 = cp_seg(p, b, c);
        let q3 = cp_seg(p, c, a);
        return nearer(p, nearer(p, q1, q2), q3);
    }
    let ap = p - a;
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// Closest points of segments `p1q1` and `p2q2` (after Ericson, 5.1.9).
fn seg_seg(p1: DVec3, q1: DVec3, p2: DVec3, q2: DVec3) -> (DVec3, DVec3) {
    let (d1, d2, r) = (q1 - p1, q2 - p2, p1 - p2);
    let (a, e, f) = (d1.dot(d1), d2.dot(d2), d2.dot(r));
    let eps = 1e-300;
    if a <= eps && e <= eps {
        return (p1, p2);
    }
    let (s, t) = if a <= eps {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = d1.dot(r);
        if e <= eps {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = d1.dot(d2);
            let denom = a * e - b * b;
            let mut s = if denom > 1e-14 * a * e { ((b * f - c * e) / denom).clamp(0.0, 1.0) } else { 0.0 };
            let mut t = (b * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            }
            (s, t)
        }
    };
    (p1 + d1 * s, p2 + d2 * t)
}

fn closer(best: &mut (f64, DVec3, DVec3), p: DVec3, q: DVec3) {
    let d = (q - p).length();
    if d < best.0 {
        *best = (d, p, q);
    }
}

/// Closest points of segment `ab` and triangle `t`.
fn seg_tri(a: DVec3, b: DVec3, t: [DVec3; 3]) -> (f64, DVec3, DVec3) {
    let n = (t[1] - t[0]).cross(t[2] - t[0]);
    if n.length_squared() > 1e-300 {
        let (da, db) = ((a - t[0]).dot(n), (b - t[0]).dot(n));
        if (da < 0.0 && db > 0.0) || (da > 0.0 && db < 0.0) {
            let x = a + (b - a) * (da / (da - db));
            let inside = (0..3).all(|k| (t[(k + 1) % 3] - t[k]).cross(x - t[k]).dot(n) >= 0.0);
            if inside {
                return (0.0, x, x);
            }
        }
    }
    let mut best = (f64::INFINITY, a, a);
    closer(&mut best, a, cp_tri(a, t[0], t[1], t[2]));
    closer(&mut best, b, cp_tri(b, t[0], t[1], t[2]));
    for k in 0..3 {
        let (p, q) = seg_seg(a, b, t[k], t[(k + 1) % 3]);
        closer(&mut best, p, q);
    }
    best
}

fn prim_distance(x: &Prim, y: &Prim) -> (f64, DVec3, DVec3) {
    let flip = |(d, p, q): (f64, DVec3, DVec3)| (d, q, p);
    match (*x, *y) {
        (Prim::P(p), Prim::P(q)) => ((q - p).length(), p, q),
        (Prim::P(p), Prim::S(a, b)) => {
            let q = cp_seg(p, a, b);
            ((q - p).length(), p, q)
        }
        (Prim::P(p), Prim::T(a, b, c)) => {
            let q = cp_tri(p, a, b, c);
            ((q - p).length(), p, q)
        }
        (Prim::S(..) | Prim::T(..), Prim::P(_)) => flip(prim_distance(y, x)),
        (Prim::S(a, b), Prim::S(c, d)) => {
            let (p, q) = seg_seg(a, b, c, d);
            ((q - p).length(), p, q)
        }
        (Prim::S(a, b), Prim::T(p, q, r)) => seg_tri(a, b, [p, q, r]),
        (Prim::T(..), Prim::S(..)) => flip(prim_distance(y, x)),
        (Prim::T(a, b, c), Prim::T(p, q, r)) => {
            let (s, t) = ([a, b, c], [p, q, r]);
            let mut best = (f64::INFINITY, a, p);
            for k in 0..3 {
                let found = seg_tri(s[k], s[(k + 1) % 3], t);
                if found.0 < best.0 {
                    best = found;
                }
                let found = flip(seg_tri(t[k], t[(k + 1) % 3], s));
                if found.0 < best.0 {
                    best = found;
                }
            }
            best
        }
    }
}

fn box_gap(alo: DVec3, ahi: DVec3, blo: DVec3, bhi: DVec3) -> f64 {
    let gap = (blo - ahi).max(alo - bhi).max(DVec3::ZERO);
    gap.length()
}

struct Node {
    lo: DVec3,
    hi: DVec3,
    /// Children, or `None` for a leaf holding `prims[start..end]`.
    children: Option<(usize, usize)>,
    start: usize,
    end: usize,
}

struct Bvh {
    prims: Vec<Prim>,
    nodes: Vec<Node>,
}

impl Bvh {
    fn new(prims: Vec<Prim>) -> Bvh {
        let bounds: Vec<(DVec3, DVec3)> = prims.iter().map(Prim::bounds).collect();
        let mut order: Vec<usize> = (0..prims.len()).collect();
        let mut nodes = Vec::new();
        if !prims.is_empty() {
            build(&mut nodes, &mut order, 0, &bounds);
        }
        let prims = order.iter().filter_map(|&i| prims.get(i).copied()).collect();
        Bvh { prims, nodes }
    }

    fn nearest(&self, q: &Prim) -> Option<(f64, DVec3, DVec3)> {
        self.nearest_within(q, None)
    }

    /// The nearest primitive to `q`, if any is nearer than `limit`.
    fn nearest_within(&self, q: &Prim, limit: Option<f64>) -> Option<(f64, DVec3, DVec3)> {
        let (qlo, qhi) = q.bounds();
        let mut best: Option<(f64, DVec3, DVec3)> = None;
        let bound = |best: &Option<(f64, DVec3, DVec3)>| match (best, limit) {
            (Some(b), Some(l)) => b.0.min(l),
            (Some(b), None) => b.0,
            (None, Some(l)) => l,
            (None, None) => f64::INFINITY,
        };
        let mut stack = vec![0usize];
        while let Some(i) = stack.pop() {
            let Some(node) = self.nodes.get(i) else { continue };
            if box_gap(qlo, qhi, node.lo, node.hi) >= bound(&best) {
                continue;
            }
            match node.children {
                Some((l, r)) => {
                    let gap = |j: usize| {
                        self.nodes.get(j).map(|n| box_gap(qlo, qhi, n.lo, n.hi)).unwrap_or(f64::INFINITY)
                    };
                    // Visit the nearer child first.
                    if gap(l) <= gap(r) {
                        stack.push(r);
                        stack.push(l);
                    } else {
                        stack.push(l);
                        stack.push(r);
                    }
                }
                None => {
                    for prim in self.prims.get(node.start..node.end).unwrap_or(&[]) {
                        let found = prim_distance(q, prim);
                        if found.0 < bound(&best) {
                            best = Some(found);
                        }
                    }
                }
            }
        }
        best
    }
}

fn build(nodes: &mut Vec<Node>, idx: &mut [usize], start: usize, bounds: &[(DVec3, DVec3)]) -> usize {
    let (mut lo, mut hi) = (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY));
    for &i in idx.iter() {
        if let Some(b) = bounds.get(i) {
            lo = lo.min(b.0);
            hi = hi.max(b.1);
        }
    }
    let me = nodes.len();
    nodes.push(Node { lo, hi, children: None, start, end: start + idx.len() });
    if idx.len() <= 4 {
        return me;
    }
    let ext = (hi - lo).to_array();
    let axis = if ext[0] >= ext[1] && ext[0] >= ext[2] {
        0
    } else if ext[1] >= ext[2] {
        1
    } else {
        2
    };
    let centre = |i: usize| bounds.get(i).map(|b| (b.0 + b.1).to_array()[axis]).unwrap_or(0.0);
    idx.sort_by(|a, b| centre(*a).total_cmp(&centre(*b)));
    let mid = idx.len() / 2;
    let (l, r) = idx.split_at_mut(mid);
    let left = build(nodes, l, start, bounds);
    let right = build(nodes, r, start + mid, bounds);
    if let Some(node) = nodes.get_mut(me) {
        node.children = Some((left, right));
    }
    me
}

#[cfg(test)]
mod tests {
    //! Testable statements 1 to 19 of `docs/re/measure-and-section.md`, on parts built
    //! with the modelling API, on the exact path and (where marked) the faceted fallback.

    use super::*;
    use crate::document::*;
    use anvil_kernel::{v2, v3 as kv3};
    use anvil_sketch::Sketch;

    const EPS: f64 = 1e-6;

    fn close(got: Option<f64>, want: f64, tol: f64) {
        let got = got.unwrap_or(f64::NAN);
        assert!((got - want).abs() <= tol, "got {got}, want {want}");
    }

    fn close_v(got: Option<Vec3>, want: [f64; 3], tol: f64) {
        let got = got.map(d3).unwrap_or(DVec3::NAN);
        assert!((got - DVec3::from_array(want)).length() <= tol, "got {got}, want {want:?}");
    }

    fn sketch(doc: &mut Document, plane: PlaneRef, draw: impl FnOnce(&mut Sketch)) -> u32 {
        let mut sf = SketchFeature::new(plane);
        draw(&mut sf.sketch);
        doc.add_feature(FeatureKind::Sketch(sf))
    }

    fn extrude(doc: &mut Document, sketch: u32, depth: f64) -> u32 {
        doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch,
            end: EndCondition::Blind,
            depth: Dim::Value(depth),
            reverse: false,
            op: BodyOp::Boss,
            ..Default::default()
        }))
    }

    fn rebuild(doc: &mut Document) -> BuildState {
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        assert!(st.exact.as_ref().is_some_and(|b| !b.is_empty()), "the part should be exact");
        st
    }

    /// The same part measured on its faceted body only.
    fn faceted(st: &BuildState) -> BuildState {
        let mut f = st.clone();
        f.exact = None;
        f
    }

    /// A box [x0, x1] x [y0, y1] x [0, depth]: its feature id and the four side lines
    /// (bottom y = y0, right x = x1, top y = y1, left x = x0).
    fn block(doc: &mut Document, x0: f64, y0: f64, x1: f64, y1: f64, depth: f64) -> (u32, [usize; 4]) {
        let mut lines = [0; 4];
        let sk = sketch(doc, PlaneRef::Front, |s| lines = s.add_rectangle(v2(x0, y0), v2(x1, y1)));
        (extrude(doc, sk, depth), lines)
    }

    fn start(f: u32) -> FaceId {
        FaceId { feature: f, index: FaceId::START_CAP }
    }

    fn end(f: u32) -> FaceId {
        FaceId { feature: f, index: FaceId::END_CAP }
    }

    fn side(f: u32, entity: usize) -> FaceId {
        FaceId::side(f, entity as u32)
    }

    fn edge(a: FaceId, b: FaceId) -> Pick {
        Pick::Edge(EdgeRef::new(a, b))
    }

    fn run(st: &BuildState, picks: &[Pick]) -> Measurement {
        measure(st, picks, &MeasureOptions::default()).expect("measure")
    }

    fn run_arc(st: &BuildState, picks: &[Pick], arc: (ArcCondition, ArcCondition)) -> Measurement {
        measure(st, picks, &MeasureOptions { arc, project_on: None }).expect("measure")
    }

    fn the_box() -> (BuildState, u32, [usize; 4]) {
        let mut doc = Document::new();
        let (ex, l) = block(&mut doc, 0.0, 0.0, 100.0, 50.0, 20.0);
        (rebuild(&mut doc), ex, l)
    }

    // Statement 1.
    #[test]
    fn a_vertex_reports_its_coordinates() {
        let (st, _, _) = the_box();
        let m = run(&st, &[Pick::Point(kv3(10.0, 20.0, 30.0))]);
        close_v(m.point, [10.0, 20.0, 30.0], 0.0);
        assert_eq!(m.distance, None);
        let lines = summary_lines(&m);
        assert_eq!(lines[0], ("X".to_string(), "10.000 mm".to_string()));
        assert_eq!(lines[2], ("Z".to_string(), "30.000 mm".to_string()));
    }

    // Statement 2, exact and faceted.
    #[test]
    fn box_edge_length_and_face_area_and_perimeter() {
        let (st, ex, l) = the_box();
        for st in [st.clone(), faceted(&st)] {
            let m = run(&st, &[edge(start(ex), side(ex, l[0]))]);
            close(m.length, 100.0, EPS);
            assert_eq!(m.radius, None);
            let m = run(&st, &[Pick::Face(end(ex))]);
            close(m.area, 5000.0, EPS);
            close(m.perimeter, 300.0, EPS);
        }
        assert!(run(&st, &[Pick::Face(end(ex))]).exact);
        assert!(!run(&faceted(&st), &[Pick::Face(end(ex))]).exact);
    }

    fn disc(r: f64, depth: f64) -> (BuildState, u32, usize) {
        let mut doc = Document::new();
        let mut c = 0;
        let sk = sketch(&mut doc, PlaneRef::Front, |s| {
            let p = s.add_point(v2(0.0, 0.0));
            c = s.add_circle(p, r);
        });
        let ex = extrude(&mut doc, sk, depth);
        (rebuild(&mut doc), ex, c)
    }

    // Statement 3, exact and faceted.
    #[test]
    fn circular_edge_reports_diameter_radius_length_and_centre() {
        let (st, ex, c) = disc(10.0, 5.0);
        for st in [st.clone(), faceted(&st)] {
            let m = run(&st, &[edge(end(ex), side(ex, c))]);
            close(m.diameter, 20.0, EPS);
            close(m.radius, 10.0, EPS);
            close(m.length, 20.0 * PI, EPS);
            close_v(m.center, [0.0, 0.0, 5.0], EPS);
            assert_eq!(m.arc_length, None);
            assert_eq!(m.chord_length, None);
        }
        let m = run(&st, &[edge(end(ex), side(ex, c))]);
        let lines = summary_lines(&m);
        assert!(lines.contains(&("Length".to_string(), "62.832 mm".to_string())), "{lines:?}");
    }

    // Statement 4, exact and faceted.
    #[test]
    fn quarter_arc_reports_arc_length_chord_and_radius() {
        let mut doc = Document::new();
        let mut arc = 0;
        let sk = sketch(&mut doc, PlaneRef::Front, |s| {
            let (o, p, q) =
                (s.add_point(v2(0.0, 0.0)), s.add_point(v2(10.0, 0.0)), s.add_point(v2(0.0, 10.0)));
            s.add_line(o, p);
            arc = s.add_arc(o, p, q);
            s.add_line(q, o);
        });
        let ex = extrude(&mut doc, sk, 5.0);
        let st = rebuild(&mut doc);
        for st in [st.clone(), faceted(&st)] {
            let m = run(&st, &[edge(end(ex), side(ex, arc))]);
            close(m.arc_length, 5.0 * PI, EPS);
            close(m.length, 5.0 * PI, EPS);
            close(m.chord_length, 200f64.sqrt(), EPS);
            close(m.radius, 10.0, EPS);
            close_v(m.center, [0.0, 0.0, 5.0], EPS);
        }
    }

    // Statement 5, exact and faceted.
    #[test]
    fn cylinder_side_face_reports_area_radius_and_perimeter_without_the_seam() {
        let (st, ex, c) = disc(5.0, 20.0);
        for st in [st.clone(), faceted(&st)] {
            let m = run(&st, &[Pick::Face(side(ex, c))]);
            close(m.area, 200.0 * PI, EPS);
            close(m.radius, 5.0, EPS);
            close(m.diameter, 10.0, EPS);
            close(m.perimeter, 20.0 * PI, EPS);
        }
    }

    // Statement 6, exact and faceted.
    #[test]
    fn plate_with_a_hole_counts_the_hole() {
        let mut doc = Document::new();
        let sk = sketch(&mut doc, PlaneRef::Front, |s| {
            s.add_rectangle(v2(-20.0, -20.0), v2(20.0, 20.0));
            let p = s.add_point(v2(0.0, 0.0));
            s.add_circle(p, 5.0);
        });
        let ex = extrude(&mut doc, sk, 5.0);
        let st = rebuild(&mut doc);
        for st in [st.clone(), faceted(&st)] {
            let m = run(&st, &[Pick::Face(end(ex))]);
            close(m.area, 1600.0 - 25.0 * PI, EPS);
            close(m.perimeter, 160.0 + 10.0 * PI, EPS);
        }
        let lines = summary_lines(&run(&st, &[Pick::Face(end(ex))]));
        assert!(lines.contains(&("Area".to_string(), "1521.460 mm²".to_string())), "{lines:?}");
        assert!(lines.contains(&("Perimeter".to_string(), "191.416 mm".to_string())), "{lines:?}");
    }

    // Statement 7.
    #[test]
    fn two_vertices_report_distance_and_deltas() {
        let (st, _, _) = the_box();
        let m = run(&st, &[Pick::Point(kv3(0.0, 0.0, 0.0)), Pick::Point(kv3(30.0, 40.0, 0.0))]);
        close(m.distance, 50.0, EPS);
        close_v(m.delta, [30.0, 40.0, 0.0], EPS);
        assert_eq!(m.angle_deg, None);
    }

    // Statements 8 and 9, exact and faceted.
    #[test]
    fn box_faces_report_distance_normal_distance_and_angle() {
        let (st, ex, l) = the_box();
        for st in [st.clone(), faceted(&st)] {
            let m = run(&st, &[Pick::Face(end(ex)), Pick::Face(start(ex))]);
            close(m.distance, 20.0, EPS);
            close(m.normal_distance, 20.0, EPS);
            close(m.angle_deg, 0.0, EPS);
            assert_eq!(m.parallel, Some(true));
            close(m.total_area, 10000.0, EPS);
            let m = run(&st, &[Pick::Face(end(ex)), Pick::Face(side(ex, l[0]))]);
            close(m.distance, 0.0, EPS);
            close(m.angle_deg, 90.0, EPS);
            assert_eq!(m.perpendicular, Some(true));
            assert_eq!(m.normal_distance, None);
        }
    }

    // Statement 10, exact and faceted: the top of [0, 10]^2 x [0, 5] and the bottom of
    // [20, 30] x [0, 10] x [0, 20] (the statement's configuration, mirrored).
    #[test]
    fn parallel_faces_that_do_not_overlap() {
        let mut doc = Document::new();
        let (a, _) = block(&mut doc, 0.0, 0.0, 10.0, 10.0, 5.0);
        let (b, _) = block(&mut doc, 20.0, 0.0, 30.0, 10.0, 20.0);
        let st = rebuild(&mut doc);
        for st in [st.clone(), faceted(&st)] {
            let m = run(&st, &[Pick::Face(end(a)), Pick::Face(start(b))]);
            close(m.normal_distance, 5.0, EPS);
            close(m.distance, 125f64.sqrt(), EPS);
            let (p, q) = m.points.expect("points");
            close(Some(p.x), 10.0, EPS);
            close(Some(q.x), 20.0, EPS);
        }
    }

    // Statements 11 and 12.
    #[test]
    fn vertex_and_face() {
        let mut doc = Document::new();
        let (a, _) = block(&mut doc, -10.0, -10.0, 10.0, 10.0, 5.0);
        let st = rebuild(&mut doc);
        let m = run(&st, &[Pick::Point(kv3(0.0, 0.0, 30.0)), Pick::Face(start(a))]);
        close(m.distance, 30.0, EPS);
        close(m.normal_distance, 30.0, EPS);

        let mut doc = Document::new();
        let (a, _) = block(&mut doc, 0.0, 0.0, 10.0, 10.0, 5.0);
        let st = rebuild(&mut doc);
        for st in [st.clone(), faceted(&st)] {
            let m = run(&st, &[Pick::Point(kv3(20.0, 0.0, 10.0)), Pick::Face(start(a))]);
            close(m.distance, 200f64.sqrt(), EPS);
            close(m.normal_distance, 10.0, EPS);
            close_v(m.points.map(|p| p.1), [10.0, 0.0, 0.0], EPS);
        }
    }

    // Statement 13: a wedge whose sloping edge runs from (0, 0, 0) to (10, 0, 10).
    #[test]
    fn sloping_edge_and_face_make_45_degrees() {
        let mut doc = Document::new();
        let mut l = [0; 3];
        // The Top plane maps sketch (x, y) to (x, 0, -y) and extrudes along +Y.
        let sk = sketch(&mut doc, PlaneRef::Top, |s| {
            let p = [s.add_point(v2(0.0, 0.0)), s.add_point(v2(10.0, 0.0)), s.add_point(v2(10.0, -10.0))];
            l = [s.add_line(p[0], p[1]), s.add_line(p[1], p[2]), s.add_line(p[2], p[0])];
        });
        let ex = extrude(&mut doc, sk, 10.0);
        let st = rebuild(&mut doc);
        let slope = edge(start(ex), side(ex, l[2]));
        let m = run(&st, &[slope]);
        close(m.length, 200f64.sqrt(), EPS);
        let m = run(&st, &[slope, Pick::Face(side(ex, l[0]))]);
        close(m.angle_deg, 45.0, EPS);
        close(m.distance, 0.0, EPS);
        assert_eq!(m.normal_distance, None);
    }

    // Statement 14 (on a box 10 tall, so the edges are 10 apart).
    #[test]
    fn skew_edges() {
        let mut doc = Document::new();
        let (ex, l) = block(&mut doc, 0.0, 0.0, 100.0, 50.0, 10.0);
        let st = rebuild(&mut doc);
        for st in [st.clone(), faceted(&st)] {
            let along_x = edge(start(ex), side(ex, l[0]));
            let along_y = edge(end(ex), side(ex, l[1]));
            let m = run(&st, &[along_x, along_y]);
            close(m.distance, 10.0, EPS);
            close(m.angle_deg, 90.0, EPS);
            close(m.total_length, 150.0, EPS);
            close_v(m.delta, [0.0, 0.0, 10.0], EPS);
        }
    }

    /// Two cylinders, radii 5 and 3, with centres 50 apart, and heights `h1` and `h2`:
    /// their top rims and their side faces.
    fn two_cylinders(h1: f64, h2: f64) -> (BuildState, [Pick; 2], [Pick; 2]) {
        let mut doc = Document::new();
        let mut c = [0; 2];
        let s1 = sketch(&mut doc, PlaneRef::Front, |s| {
            let p = s.add_point(v2(0.0, 0.0));
            c[0] = s.add_circle(p, 5.0);
        });
        let e1 = extrude(&mut doc, s1, h1);
        let s2 = sketch(&mut doc, PlaneRef::Front, |s| {
            let p = s.add_point(v2(50.0, 0.0));
            c[1] = s.add_circle(p, 3.0);
        });
        let e2 = extrude(&mut doc, s2, h2);
        let st = rebuild(&mut doc);
        let rims = [edge(end(e1), side(e1, c[0])), edge(end(e2), side(e2, c[1]))];
        let faces = [Pick::Face(side(e1, c[0])), Pick::Face(side(e2, c[1]))];
        (st, rims, faces)
    }

    // Statement 15, exact and faceted, for the circles and for the cylinders they bound.
    #[test]
    fn coplanar_circles_with_every_arc_condition() {
        use ArcCondition::{Center as C, Maximum as X, Minimum as N};
        let (st, rims, faces) = two_cylinders(10.0, 10.0);
        let cases = [
            ((C, C), 50.0),
            ((N, N), 42.0),
            ((X, X), 58.0),
            ((N, C), 45.0),
            ((C, X), 53.0),
            ((N, X), 48.0),
            ((X, N), 52.0),
        ];
        for st in [st.clone(), faceted(&st)] {
            for picks in [rims, faces] {
                for (arc, want) in cases {
                    let m = run_arc(&st, &picks, arc);
                    close(m.distance, want, EPS);
                    close(m.center_distance, 50.0, EPS);
                    assert_eq!(m.arc_conditions, Some(arc));
                    let (p, q) = m.points.expect("points");
                    close(Some((d3(q) - d3(p)).length()), want, EPS);
                }
            }
        }
        let m = run(&st, &rims);
        close(m.total_length, 16.0 * PI, EPS);
        let lines = summary_lines(&m);
        assert_eq!(lines.iter().find(|l| l.0 == "Center distance").map(|l| l.1.as_str()), Some("50.000 mm"));
    }

    // Circles that are not coplanar: centre distance between centres, minimum and maximum
    // between the curves themselves (the specification's Decision for this case).
    #[test]
    fn circles_at_different_heights() {
        use ArcCondition::{Center as C, Maximum as X, Minimum as N};
        let (st, rims, _) = two_cylinders(10.0, 20.0);
        close(run_arc(&st, &rims, (C, C)).distance, 2600f64.sqrt(), EPS);
        close(run_arc(&st, &rims, (N, N)).distance, (42.0f64 * 42.0 + 100.0).sqrt(), EPS);
        close(run_arc(&st, &rims, (X, X)).distance, (58.0f64 * 58.0 + 100.0).sqrt(), 1e-5);
    }

    // Statement 16, against a planar face and against a reference plane.
    #[test]
    fn cylinder_and_a_parallel_plane() {
        use ArcCondition::{Center as C, Maximum as X, Minimum as N};
        let mut doc = Document::new();
        let mut c = 0;
        let s1 = sketch(&mut doc, PlaneRef::Front, |s| {
            let p = s.add_point(v2(0.0, 0.0));
            c = s.add_circle(p, 5.0);
        });
        let cyl = extrude(&mut doc, s1, 20.0);
        let (b, l) = block(&mut doc, 30.0, -10.0, 40.0, 10.0, 20.0);
        let st = rebuild(&mut doc);
        let wall = Pick::Face(side(b, l[3]));
        let plane = Pick::Plane { origin: kv3(30.0, 0.0, 0.0), normal: kv3(-1.0, 0.0, 0.0) };
        for st in [st.clone(), faceted(&st)] {
            for other in [wall, plane] {
                for (arc, want) in [((C, C), 30.0), ((N, N), 25.0), ((X, X), 35.0)] {
                    let m = run_arc(&st, &[Pick::Face(side(cyl, c)), other], arc);
                    close(m.distance, want, EPS);
                    close(m.center_distance, 30.0, EPS);
                    close(m.normal_distance, 30.0, EPS);
                    // The plane first: the condition applies to the cylinder, second.
                    let m = run_arc(&st, &[other, Pick::Face(side(cyl, c))], (arc.1, arc.0));
                    close(m.distance, want, EPS);
                }
            }
        }
    }

    // Statement 17, exact and faceted.
    #[test]
    fn concentric_circles() {
        use ArcCondition::{Center as C, Maximum as X, Minimum as N};
        let mut doc = Document::new();
        let mut c = [0; 2];
        let sk = sketch(&mut doc, PlaneRef::Front, |s| {
            let p = s.add_point(v2(0.0, 0.0));
            c = [s.add_circle(p, 10.0), s.add_circle(p, 4.0)];
        });
        let ex = extrude(&mut doc, sk, 5.0);
        let st = rebuild(&mut doc);
        let picks = [edge(end(ex), side(ex, c[0])), edge(end(ex), side(ex, c[1]))];
        for st in [st.clone(), faceted(&st)] {
            for (arc, want) in [((C, C), 0.0), ((N, N), 6.0), ((X, X), 14.0)] {
                close(run_arc(&st, &picks, arc).distance, want, EPS);
            }
        }
    }

    // Statement 18.
    #[test]
    fn projected_on_a_plane() {
        let (st, _, _) = the_box();
        let picks = [Pick::Point(kv3(0.0, 0.0, 0.0)), Pick::Point(kv3(30.0, 40.0, 50.0))];
        let m = run(&st, &picks);
        close(m.distance, 5000f64.sqrt(), EPS);
        assert_eq!(m.projection, None);
        let opts = MeasureOptions {
            project_on: Some((kv3(0.0, 0.0, 0.0), kv3(0.0, 0.0, 2.0))),
            ..Default::default()
        };
        let m = measure(&st, &picks, &opts).expect("measure");
        close(m.projection, 50.0, EPS);
        close(m.normal, 50.0, EPS);
        let lines = summary_lines(&m);
        assert!(lines.contains(&("Distance".to_string(), "70.711 mm".to_string())), "{lines:?}");
        let bad = MeasureOptions {
            project_on: Some((kv3(0.0, 0.0, 0.0), kv3(0.0, 0.0, 0.0))),
            ..Default::default()
        };
        assert!(measure(&st, &picks, &bad).is_err());
    }

    // Statement 19.
    #[test]
    fn three_faces_report_total_area_and_no_distance() {
        let (st, ex, l) = the_box();
        let m = run(&st, &[Pick::Face(end(ex)), Pick::Face(start(ex)), Pick::Face(side(ex, l[0]))]);
        close(m.total_area, 12000.0, EPS);
        assert_eq!(m.distance, None);
        assert_eq!(m.area, None);
    }

    #[test]
    fn a_sphere_face_reports_its_radius_area_and_centre() {
        let mut doc = Document::new();
        let (mut arc, mut axis) = (0, 0);
        let sk = sketch(&mut doc, PlaneRef::Front, |s| {
            let (c, a, b) =
                (s.add_point(v2(0.0, 0.0)), s.add_point(v2(0.0, -10.0)), s.add_point(v2(0.0, 10.0)));
            arc = s.add_arc(c, a, b);
            axis = s.add_line(b, a);
        });
        doc.add_feature(FeatureKind::Revolve(RevolveFeature {
            sketch: sk,
            axis,
            angle: Dim::Value(360.0),
            op: BodyOp::Boss,
            ..Default::default()
        }));
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        let ids = st.body.face_ids();
        let ball = ids.iter().copied().find(|f| f.index == 2 + arc as u32);
        let ball = ball.unwrap_or_else(|| panic!("no sphere face in {ids:?}"));
        let m = run(&st, &[Pick::Face(ball)]);
        close(m.radius, 10.0, EPS);
        close_v(m.center, [0.0, 0.0, 0.0], EPS);
        close(m.area, 400.0 * PI, EPS);
        close(m.perimeter, 0.0, EPS);
        let m = run(&st, &[Pick::Point(kv3(0.0, 0.0, 30.0)), Pick::Face(ball)]);
        close(m.distance, 20.0, EPS);
    }

    #[test]
    fn bad_selections_are_errors() {
        let (st, ex, _) = the_box();
        let opts = MeasureOptions::default();
        assert!(measure(&st, &[], &opts).is_err());
        let missing = FaceId { feature: ex + 7, index: 0 };
        assert!(measure(&st, &[Pick::Face(missing)], &opts).is_err());
        assert!(measure(&faceted(&st), &[Pick::Face(missing)], &opts).is_err());
        let three = [Pick::Point(kv3(0.0, 0.0, 0.0)), Pick::Face(end(ex)), Pick::Face(start(ex))];
        assert!(measure(&st, &three, &opts).is_err());
        assert!(measure(&st, &[Pick::Point(kv3(f64::NAN, 0.0, 0.0))], &opts).is_err());
    }
}
