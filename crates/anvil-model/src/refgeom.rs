//! Reference geometry: planes of many kinds, axes, points and coordinate systems. The
//! behaviour is specified in `docs/re/reference-geometry.md`.
//!
//! Each reference feature rebuilds to a place in model space (a frame for planes and
//! coordinate systems, a line for axes, a position for points) that later features use:
//! sketches and mirrors through [`PlaneRef::Feature`], patterns through
//! [`Axis::Feature`], end conditions through [`PointRef::Feature`].

use crate::document::*;
use crate::rebuild::BuildState;
use anvil_kernel::{EdgeRef, FaceId, Frame, SurfaceKind, Vec3};
use serde::{Deserialize, Serialize};

/// A straight line a feature refers to.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum LineRef {
    /// A straight model edge, named by its two faces.
    Edge(EdgeRef),
    /// A line of a sketch.
    SketchLine { sketch: u32, entity: usize },
    /// A reference axis or a fixed axis.
    Axis(Axis),
}

impl LineRef {
    fn dependencies(&self) -> Vec<u32> {
        match *self {
            LineRef::Edge(e) => vec![e.a.feature, e.b.feature],
            LineRef::SketchLine { sketch, .. } => vec![sketch],
            LineRef::Axis(Axis::Feature(id)) => vec![id],
            LineRef::Axis(_) => vec![],
        }
    }
}

/// How a reference plane is placed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum PlaneKind {
    /// Parallel to the base plane, `offset` from it.
    #[default]
    Offset,
    /// Parallel to the base plane, through a point.
    ParallelThrough { point: PointRef },
    /// Through three points: origin the first, normal (P2 - P1) x (P3 - P1).
    ThreePoints { points: [PointRef; 3] },
    /// Through a line and a point off it.
    LinePoint { line: LineRef, point: PointRef },
    /// Through a line parallel to the base plane, turned `angle` degrees from it.
    AtAngle { line: LineRef, angle: Dim },
    /// Halfway between the base plane and another (bisecting them if they meet).
    MidPlane { other: PlaneRef },
    /// Square to a model edge or sketch line, through a point.
    NormalToCurve {
        curve: LineRef,
        point: PointRef,
        #[serde(default)]
        origin_on_curve: bool,
    },
}

impl PlaneKind {
    pub const LABELS: [&'static str; 7] = [
        "Offset",
        "Parallel through point",
        "Three points",
        "Line and point",
        "At angle",
        "Mid plane",
        "Normal to curve",
    ];

    pub fn label(&self) -> &'static str {
        Self::LABELS[match self {
            PlaneKind::Offset => 0,
            PlaneKind::ParallelThrough { .. } => 1,
            PlaneKind::ThreePoints { .. } => 2,
            PlaneKind::LinePoint { .. } => 3,
            PlaneKind::AtAngle { .. } => 4,
            PlaneKind::MidPlane { .. } => 5,
            PlaneKind::NormalToCurve { .. } => 6,
        }]
    }

    pub(crate) fn dependencies(&self) -> Vec<u32> {
        let mut v = Vec::new();
        match self {
            PlaneKind::Offset => {}
            PlaneKind::ParallelThrough { point } => v.extend(point.dependency()),
            PlaneKind::ThreePoints { points } => v.extend(points.iter().filter_map(|p| p.dependency())),
            PlaneKind::LinePoint { line, point } => {
                v.extend(line.dependencies());
                v.extend(point.dependency());
            }
            PlaneKind::AtAngle { line, .. } => v.extend(line.dependencies()),
            PlaneKind::MidPlane { other } => v.extend(other.dependency()),
            PlaneKind::NormalToCurve { curve, point, .. } => {
                v.extend(curve.dependencies());
                v.extend(point.dependency());
            }
        }
        v
    }
}

/// A reference axis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AxisFeature {
    pub kind: AxisKind,
    #[serde(default)]
    pub reverse: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AxisKind {
    /// Along a straight edge or sketch line.
    Line(LineRef),
    /// Where two planes meet, directed n1 x n2.
    TwoPlanes(PlaneRef, PlaneRef),
    /// From one point towards another.
    TwoPoints(PointRef, PointRef),
    /// The axis of a cylindrical or conical face.
    Face(FaceId),
    /// Through a point, square to a plane or planar face.
    PointPlane(PointRef, PlaneRef),
}

impl AxisKind {
    pub const LABELS: [&'static str; 5] = [
        "One line or edge",
        "Two planes",
        "Two points",
        "Cylindrical or conical face",
        "Point and face or plane",
    ];

    pub fn label(&self) -> &'static str {
        Self::LABELS[match self {
            AxisKind::Line(_) => 0,
            AxisKind::TwoPlanes(..) => 1,
            AxisKind::TwoPoints(..) => 2,
            AxisKind::Face(_) => 3,
            AxisKind::PointPlane(..) => 4,
        }]
    }

    pub(crate) fn dependencies(&self) -> Vec<u32> {
        match self {
            AxisKind::Line(l) => l.dependencies(),
            AxisKind::TwoPlanes(a, b) => a.dependency().into_iter().chain(b.dependency()).collect(),
            AxisKind::TwoPoints(a, b) => a.dependency().into_iter().chain(b.dependency()).collect(),
            AxisKind::Face(f) => vec![f.feature],
            AxisKind::PointPlane(p, q) => p.dependency().into_iter().chain(q.dependency()).collect(),
        }
    }
}

/// How points are spaced along a curve.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlongMode {
    /// The k-th point is k times the value (mm) from the start.
    Distance,
    /// The k-th point is k times the value (per cent of the length) from the start.
    Percent,
    /// N points spread evenly, neither on an end: the k-th at k/(N+1) of the length.
    Even,
}

/// A reference point.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PointFeature {
    /// The centre of a round model edge (or of a sketch arc or circle).
    ArcCenter(ArcRef),
    /// The area centroid of a face.
    FaceCenter(FaceId),
    /// Where two lines meet, or a line meets a plane.
    Intersection(LineRef, IntersectWith),
    /// A point dropped square onto a plane or planar face.
    Projection(PointRef, PlaneRef),
    /// On another point (a sketch point, say).
    OnPoint(PointRef),
    /// The `index`-th (1-based) of `count` points along a straight edge or sketch line.
    AlongCurve {
        curve: LineRef,
        mode: AlongMode,
        value: Dim,
        count: u32,
        index: u32,
        #[serde(default)]
        reverse: bool,
    },
    /// The part's centre of mass as built up to this point in the tree.
    CenterOfMass,
}

/// An arc or circle a feature refers to.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ArcRef {
    Edge(EdgeRef),
    Sketch { sketch: u32, entity: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum IntersectWith {
    Line(LineRef),
    Plane(PlaneRef),
}

impl PointFeature {
    pub fn label(&self) -> &'static str {
        match self {
            PointFeature::ArcCenter(_) => "Arc center",
            PointFeature::FaceCenter(_) => "Center of face",
            PointFeature::Intersection(..) => "Intersection",
            PointFeature::Projection(..) => "Projection",
            PointFeature::OnPoint(_) => "On point",
            PointFeature::AlongCurve { .. } => "Along curve",
            PointFeature::CenterOfMass => "Center of mass",
        }
    }

    pub(crate) fn dependencies(&self) -> Vec<u32> {
        match self {
            PointFeature::ArcCenter(ArcRef::Edge(e)) => vec![e.a.feature, e.b.feature],
            PointFeature::ArcCenter(ArcRef::Sketch { sketch, .. }) => vec![*sketch],
            PointFeature::FaceCenter(f) => vec![f.feature],
            PointFeature::Intersection(l, w) => {
                let mut v = l.dependencies();
                match w {
                    IntersectWith::Line(m) => v.extend(m.dependencies()),
                    IntersectWith::Plane(p) => v.extend(p.dependency()),
                }
                v
            }
            PointFeature::Projection(p, q) => p.dependency().into_iter().chain(q.dependency()).collect(),
            PointFeature::OnPoint(p) => p.dependency().into_iter().collect(),
            PointFeature::AlongCurve { curve, .. } => curve.dependencies(),
            PointFeature::CenterOfMass => vec![],
        }
    }
}

/// A direction for one axis of a coordinate system.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum DirRef {
    /// Along a line.
    Line(LineRef),
    /// From the origin towards a point.
    Point(PointRef),
    /// Along a plane's normal.
    Normal(PlaneRef),
}

impl DirRef {
    fn dependencies(&self) -> Vec<u32> {
        match self {
            DirRef::Line(l) => l.dependencies(),
            DirRef::Point(p) => p.dependency().into_iter().collect(),
            DirRef::Normal(p) => p.dependency().into_iter().collect(),
        }
    }
}

/// A coordinate system. Its XY plane is what sketches and mirrors use when they refer
/// to it, and its frame is what measurements and exports can be made in.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CoordSysFeature {
    /// The origin; the model origin when `None`.
    #[serde(default)]
    pub origin: Option<PointRef>,
    /// An offset (mm) added to the origin.
    #[serde(default)]
    pub position: [f64; 3],
    #[serde(default)]
    pub axes: [Option<DirRef>; 3],
    #[serde(default)]
    pub reverse: [bool; 3],
    /// Turns (degrees) about the world X, then Y, then Z axes, used when no axis is given.
    #[serde(default)]
    pub rotation: [f64; 3],
}

impl CoordSysFeature {
    pub(crate) fn dependencies(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.origin.iter().filter_map(|p| p.dependency()).collect();
        for a in self.axes.iter().flatten() {
            v.extend(a.dependencies());
        }
        v
    }
}

fn err<T>(m: &str) -> Result<T, String> {
    Err(m.to_string())
}

/// `v` turned `angle` radians about the unit vector `k`.
fn turn(v: Vec3, k: Vec3, angle: f64) -> Vec3 {
    let (s, c) = angle.sin_cos();
    v * c + k.cross(v) * s + k * (k.dot(v) * (1.0 - c))
}

/// A frame with origin `o` and normal `n`, its x along `x_hint` squared to `n`.
fn frame(o: Vec3, n: Vec3, x_hint: Vec3) -> Frame {
    Frame::from_normal(o, n, x_hint)
}

impl Document {
    /// The line a reference names: a point on it and its unit direction.
    pub(crate) fn resolve_line(&self, l: &LineRef, st: &BuildState) -> Result<(Vec3, Vec3), String> {
        match *l {
            LineRef::Axis(a) => self.resolve_axis(&a, st),
            LineRef::SketchLine { .. } | LineRef::Edge(_) => {
                let (p, q) = self.resolve_segment(l, st)?;
                if p.distance(q) < 1e-9 {
                    return err("the line has no length");
                }
                Ok((p, (q - p).normalized()))
            }
        }
    }

    /// The two ends of a straight edge or sketch line.
    pub(crate) fn resolve_segment(&self, l: &LineRef, st: &BuildState) -> Result<(Vec3, Vec3), String> {
        match *l {
            LineRef::Edge(e) => edge_ends(st, e),
            LineRef::SketchLine { sketch, entity } => {
                let f = st.frames.get(&sketch).ok_or("the sketch holding the line failed or is missing")?;
                let sk = &self.sketch(sketch).ok_or("the sketch holding the line is missing")?.sketch;
                match sk.entities.get(entity).map(|e| e.geom) {
                    Some(anvil_sketch::Geom::Line { a, b }) => {
                        Ok((f.to_world(sk.pos(a)), f.to_world(sk.pos(b))))
                    }
                    _ => err("the sketch entity is not a line"),
                }
            }
            LineRef::Axis(_) => err("this needs an edge or sketch line, which have ends"),
        }
    }

    /// A pattern or revolve axis as a point and unit direction.
    pub(crate) fn resolve_axis(&self, a: &Axis, st: &BuildState) -> Result<(Vec3, Vec3), String> {
        match a {
            Axis::Feature(id) => {
                st.axes.get(id).copied().ok_or_else(|| "the reference axis failed or is missing".into())
            }
            other => Ok((other.origin(), other.dir())),
        }
    }

    pub(crate) fn rebuild_plane(
        &self,
        id: u32,
        pf: &PlaneFeature,
        st: &mut BuildState,
    ) -> Result<(), String> {
        let num = |d: &Dim, what: &str| d.eval(&st.variables).map_err(|e| format!("{what}: {e}"));
        let base = self.resolve_plane(&pf.base, st);
        let point = |p: &PointRef| self.resolve_point(p, st);
        let mut f = match &pf.kind {
            PlaneKind::Offset => {
                let off = num(&pf.offset, "offset")?;
                base?.offset(if pf.flip { -off } else { off })
            }
            PlaneKind::ParallelThrough { point: p } => Frame { origin: point(p)?, ..base? },
            PlaneKind::ThreePoints { points } => {
                let (a, b, c) = (point(&points[0])?, point(&points[1])?, point(&points[2])?);
                let n = (b - a).cross(c - a);
                if n.length() < 1e-9 * (b - a).length().max(1.0) * (c - a).length().max(1.0) {
                    return err("the three points lie on one line");
                }
                frame(a, n, b - a)
            }
            PlaneKind::LinePoint { line, point: p } => {
                let (o, u) = self.resolve_line(line, st)?;
                let n = u.cross(point(p)? - o);
                if n.length() < 1e-9 {
                    return err("the point lies on the line");
                }
                frame(o, n, u)
            }
            PlaneKind::AtAngle { line, angle } => {
                let b = base?;
                let (o, u) = self.resolve_line(line, st)?;
                if u.dot(b.z).abs() > 1e-9 {
                    return err("the line must be parallel to the plane it is turned from");
                }
                let a = num(angle, "angle")?.to_radians();
                let n = turn(b.z, u, if pf.flip { -a } else { a });
                frame(o, n, u)
            }
            PlaneKind::MidPlane { other } => {
                let (p, q) = (base?, self.resolve_plane(other, st)?);
                let n2 = if p.z.dot(q.z) >= 0.0 { q.z } else { -q.z };
                if p.z.cross(n2).length() < 1e-9 {
                    let d = (q.origin - p.origin).dot(p.z);
                    p.offset(0.5 * d)
                } else {
                    // The bisector through the planes' line of intersection.
                    let n = (p.z + n2).normalized();
                    let dir = p.z.cross(q.z).normalized();
                    let mid = (p.origin + q.origin) * 0.5;
                    // The point of the meeting line nearest the midpoint of the origins.
                    let line_pt = plane_plane_point(&p, &q).ok_or("the planes do not meet")?;
                    let o = line_pt + dir * (mid - line_pt).dot(dir);
                    frame(o, n, dir)
                }
            }
            PlaneKind::NormalToCurve { curve, point: p, origin_on_curve } => {
                let (o, u) = self.resolve_line(curve, st)?;
                let pt = point(p)?;
                let foot = o + u * (pt - o).dot(u);
                frame(if *origin_on_curve { foot } else { pt }, u, u.any_perpendicular())
            }
        };
        if pf.flip_normal ^ (pf.flip && pf.kind == PlaneKind::Offset) {
            f = Frame { z: -f.z, x: -f.x, ..f };
        }
        st.frames.insert(id, f);
        Ok(())
    }

    pub(crate) fn rebuild_axis(&self, id: u32, a: &AxisFeature, st: &mut BuildState) -> Result<(), String> {
        let (o, d) = match &a.kind {
            AxisKind::Line(l) => self.resolve_line(l, st)?,
            AxisKind::TwoPlanes(p, q) => {
                let (p, q) = (self.resolve_plane(p, st)?, self.resolve_plane(q, st)?);
                let dir = p.z.cross(q.z);
                if dir.length() < 1e-9 {
                    return err("the two planes are parallel");
                }
                let dir = dir.normalized();
                let pt = plane_plane_point(&p, &q).ok_or("the planes do not meet")?;
                // Through the point of the line nearest the first plane's origin.
                (pt + dir * (p.origin - pt).dot(dir), dir)
            }
            AxisKind::TwoPoints(p, q) => {
                let (p, q) = (self.resolve_point(p, st)?, self.resolve_point(q, st)?);
                if (q - p).length() < 1e-9 {
                    return err("the two points coincide");
                }
                (p, (q - p).normalized())
            }
            AxisKind::Face(f) => match st.body.face_polygons(*f).next().map(|p| p.tag.surface) {
                Some(SurfaceKind::Cylinder { axis_origin, axis_dir, .. })
                | Some(SurfaceKind::Revolved { axis_origin, axis_dir }) => {
                    (axis_origin, axis_dir.normalized())
                }
                Some(_) => return err("the face is not cylindrical or conical"),
                None => return err("the face no longer exists"),
            },
            AxisKind::PointPlane(p, q) => (self.resolve_point(p, st)?, self.resolve_plane(q, st)?.z),
        };
        st.axes.insert(id, (o, if a.reverse { -d } else { d }));
        Ok(())
    }

    pub(crate) fn rebuild_point(&self, id: u32, p: &PointFeature, st: &mut BuildState) -> Result<(), String> {
        let num = |d: &Dim, what: &str| d.eval(&st.variables).map_err(|e| format!("{what}: {e}"));
        let at = match p {
            PointFeature::ArcCenter(ArcRef::Sketch { sketch, entity }) => {
                let f = st.frames.get(sketch).ok_or("the sketch failed or is missing")?;
                let sk = &self.sketch(*sketch).ok_or("the sketch is missing")?.sketch;
                match sk.entities.get(*entity).map(|e| e.geom) {
                    Some(anvil_sketch::Geom::Arc { c, .. } | anvil_sketch::Geom::Circle { c, .. }) => {
                        f.to_world(sk.pos(c))
                    }
                    _ => return err("the sketch entity is not an arc or circle"),
                }
            }
            PointFeature::ArcCenter(ArcRef::Edge(e)) => edge_circle_center(st, *e)?,
            PointFeature::FaceCenter(f) => face_centroid(st, *f)?,
            PointFeature::Intersection(l, with) => {
                let (o, u) = self.resolve_line(l, st)?;
                match with {
                    IntersectWith::Plane(pl) => {
                        let pl = self.resolve_plane(pl, st)?;
                        let den = u.dot(pl.z);
                        if den.abs() < 1e-12 {
                            return err("the line runs parallel to the plane");
                        }
                        o + u * ((pl.origin - o).dot(pl.z) / den)
                    }
                    IntersectWith::Line(m) => {
                        let (q, w) = self.resolve_line(m, st)?;
                        let n = u.cross(w);
                        if n.length() < 1e-12 {
                            return err("the lines are parallel");
                        }
                        // Closest points of the two lines; they must meet.
                        let t = (q - o).cross(w).dot(n) / n.length_sq();
                        let s = (q - o).cross(u).dot(n) / n.length_sq();
                        let (a, b) = (o + u * t, q + w * s);
                        if a.distance(b) > 1e-6 {
                            return err("the lines do not meet");
                        }
                        a
                    }
                }
            }
            PointFeature::Projection(pt, pl) => {
                let (pt, pl) = (self.resolve_point(pt, st)?, self.resolve_plane(pl, st)?);
                pt - pl.z * (pt - pl.origin).dot(pl.z)
            }
            PointFeature::OnPoint(pt) => self.resolve_point(pt, st)?,
            PointFeature::AlongCurve { curve, mode, value, count, index, reverse } => {
                let (a, b) = self.resolve_segment(curve, st)?;
                let (a, b) = if *reverse { (b, a) } else { (a, b) };
                let len = a.distance(b);
                let (k, n) = (*index as f64, (*count).max(1) as f64);
                let v = num(value, "value")?;
                let d = match mode {
                    AlongMode::Distance => {
                        if v * n > len + 1e-9 {
                            return Err(format!(
                                "{} points {v} apart do not fit; the most is {}",
                                n,
                                len / n
                            ));
                        }
                        k * v
                    }
                    AlongMode::Percent => {
                        if v * n > 100.0 + 1e-9 {
                            return Err(format!(
                                "{} points at {v}% do not fit; the most is {}%",
                                n,
                                100.0 / n
                            ));
                        }
                        k * v / 100.0 * len
                    }
                    AlongMode::Even => k * len / (n + 1.0),
                };
                a + (b - a).normalized() * d
            }
            PointFeature::CenterOfMass => {
                if st.body.is_empty() {
                    return err("there is no body yet");
                }
                exact_centroid(st).unwrap_or(st.body.mass_properties().centroid)
            }
        };
        st.points.insert(id, at);
        Ok(())
    }

    pub(crate) fn rebuild_coordsys(
        &self,
        id: u32,
        c: &CoordSysFeature,
        st: &mut BuildState,
    ) -> Result<(), String> {
        let origin = match &c.origin {
            Some(p) => self.resolve_point(p, st)?,
            None => Vec3::ZERO,
        } + Vec3 { x: c.position[0], y: c.position[1], z: c.position[2] };
        let mut dirs: [Option<Vec3>; 3] = [None; 3];
        for (i, d) in c.axes.iter().enumerate() {
            let Some(d) = d else { continue };
            let v = match d {
                DirRef::Line(l) => self.resolve_line(l, st)?.1,
                DirRef::Point(p) => {
                    let v = self.resolve_point(p, st)? - origin;
                    if v.length() < 1e-9 {
                        return err("an axis points at the origin itself");
                    }
                    v.normalized()
                }
                DirRef::Normal(p) => self.resolve_plane(p, st)?.z,
            };
            dirs[i] = Some(if c.reverse[i] { -v } else { v });
        }
        let axes = complete_axes(dirs, c.rotation)?;
        st.frames.insert(id, Frame { origin, x: axes[0], y: axes[1], z: axes[2] });
        Ok(())
    }
}

/// A point on the line where two planes meet, or `None` if they are parallel.
fn plane_plane_point(p: &Frame, q: &Frame) -> Option<Vec3> {
    let (n1, n2) = (p.z, q.z);
    let (d1, d2) = (n1.dot(p.origin), n2.dot(q.origin));
    let n = n1.cross(n2);
    if n.length_sq() < 1e-18 {
        return None;
    }
    Some((n2.cross(n) * d1 + n.cross(n1) * d2) * (1.0 / n.length_sq()))
}

/// The axes of a coordinate system from the directions given (see the specification's
/// completion rules), or from the rotations when none is given.
pub(crate) fn complete_axes(d: [Option<Vec3>; 3], rotation: [f64; 3]) -> Result<[Vec3; 3], String> {
    let world = [Vec3::X, Vec3::Y, Vec3::Z];
    let given: Vec<usize> = (0..3).filter(|&i| d[i].is_some()).collect();
    let mut a = world;
    match given.as_slice() {
        [] => {
            for (i, k) in [Vec3::X, Vec3::Y, Vec3::Z].into_iter().enumerate() {
                let ang = rotation[i].to_radians();
                if ang != 0.0 {
                    for v in &mut a {
                        *v = turn(*v, k, ang);
                    }
                }
            }
            return Ok(a);
        }
        [i] => {
            let v = d[*i].unwrap_or(world[*i]).normalized();
            // The next axis in the cycle: the first world axis not near parallel, squared.
            let next = (i + 1) % 3;
            let pick = world
                .iter()
                .copied()
                .filter(|w| w.cross(v).length() > 1f64.to_radians().sin())
                .find(|w| *w == world[next])
                .or_else(|| world.iter().copied().find(|w| w.cross(v).length() > 1f64.to_radians().sin()))
                .ok_or("the axis has no direction")?;
            let w = (pick - v * pick.dot(v)).normalized();
            a[*i] = v;
            a[next] = w;
            a[(i + 2) % 3] = v.cross(w);
        }
        [i, j] => {
            let (u, w) = (d[*i].unwrap_or(world[*i]).normalized(), d[*j].unwrap_or(world[*j]));
            let w = w - u * w.dot(u);
            if w.length() < 1e-9 {
                return err("two axes are given the same direction");
            }
            let w = w.normalized();
            a[*i] = u;
            a[*j] = w;
            let k = 3 - i - j;
            // Right-handed: z = x × y, x = y × z, y = z × x.
            a[k] = a[(k + 1) % 3].cross(a[(k + 2) % 3]);
        }
        _ => {
            let x = d[0].unwrap_or(Vec3::X).normalized();
            let y = d[1].unwrap_or(Vec3::Y);
            let y = y - x * y.dot(x);
            if y.length() < 1e-9 {
                return err("two axes are given the same direction");
            }
            let y = y.normalized();
            a = [x, y, x.cross(y)];
        }
    }
    Ok(a)
}

/// The two ends of a straight model edge.
pub(crate) fn edge_ends(st: &BuildState, e: EdgeRef) -> Result<(Vec3, Vec3), String> {
    if let Some(bodies) = &st.exact {
        for b in bodies {
            for (i, edge) in b.edges.iter().enumerate() {
                let faces = b.edge_faces(anvil_brep::EdgeId(i as u32));
                let names: Vec<FaceId> =
                    faces.iter().filter_map(|f| b.face(*f)).map(crate::exact::face_id).collect();
                if names.len() == 2 && EdgeRef::new(names[0], names[1]) == e {
                    if let anvil_brep::Curve::Line { .. } = edge.curve {
                        let (p, q) = (edge.curve.eval(edge.t0), edge.curve.eval(edge.t1));
                        return Ok((Vec3 { x: p.x, y: p.y, z: p.z }, Vec3 { x: q.x, y: q.y, z: q.z }));
                    }
                    return err("the edge is not straight");
                }
            }
        }
    }
    let segs: Vec<[Vec3; 2]> =
        st.body.model_edges().into_iter().filter(|m| m.faces == e).map(|m| m.seg).collect();
    let first = segs.first().ok_or("the edge no longer exists")?;
    let u = (first[1] - first[0]).normalized();
    let o = first[0];
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for s in &segs {
        for p in s {
            if (*p - o).cross(u).length() > 1e-6 {
                return err("the edge is not straight");
            }
            let t = (*p - o).dot(u);
            lo = lo.min(t);
            hi = hi.max(t);
        }
    }
    Ok((o + u * lo, o + u * hi))
}

/// The centre of a round model edge.
fn edge_circle_center(st: &BuildState, e: EdgeRef) -> Result<Vec3, String> {
    if let Some(bodies) = &st.exact {
        for b in bodies {
            for (i, edge) in b.edges.iter().enumerate() {
                let faces = b.edge_faces(anvil_brep::EdgeId(i as u32));
                let names: Vec<FaceId> =
                    faces.iter().filter_map(|f| b.face(*f)).map(crate::exact::face_id).collect();
                if names.len() == 2 && EdgeRef::new(names[0], names[1]) == e {
                    if let anvil_brep::Curve::Circle { frame, .. } = edge.curve {
                        let o = frame.origin;
                        return Ok(Vec3 { x: o.x, y: o.y, z: o.z });
                    }
                    return err("the edge is not round");
                }
            }
        }
    }
    // Faceted: the centre of the circle through three of the edge's points.
    let pts: Vec<Vec3> =
        st.body.model_edges().into_iter().filter(|m| m.faces == e).flat_map(|m| m.seg).collect();
    if pts.len() < 3 {
        return err("the edge no longer exists or is not round");
    }
    let (a, b, c) = (pts[0], pts[pts.len() / 3], pts[2 * pts.len() / 3]);
    let (ab, ac) = (b - a, c - a);
    let n = ab.cross(ac);
    if n.length_sq() < 1e-18 {
        return err("the edge is not round");
    }
    let o = a + (n.cross(ab) * ac.length_sq() + ac.cross(n) * ab.length_sq()) * (0.5 / n.length_sq());
    Ok(o)
}

/// The volume centroid of the exact bodies, finely tessellated, when the part is exact.
fn exact_centroid(st: &BuildState) -> Option<Vec3> {
    let params = anvil_brep::tess::TessParams { chord: 1e-5, max_angle: 0.005 };
    let (mut vol, mut c) = (0.0, glam::DVec3::ZERO);
    for b in st.exact.as_ref()? {
        let m = anvil_brep::mass::properties(&anvil_brep::tess::tessellate(b, &params).ok()?);
        vol += m.volume;
        c += m.centroid * m.volume;
    }
    (vol > 0.0).then(|| {
        let c = c / vol;
        Vec3 { x: c.x, y: c.y, z: c.z }
    })
}

/// The area centroid of a face: of the exact face, finely tessellated, when the part
/// is exact.
fn face_centroid(st: &BuildState, f: FaceId) -> Result<Vec3, String> {
    if let Some(bodies) = &st.exact {
        let params = anvil_brep::tess::TessParams { chord: 1e-5, max_angle: 0.005 };
        let (mut area, mut c) = (0.0, glam::DVec3::ZERO);
        for b in bodies {
            if !b.faces.iter().any(|x| crate::exact::face_id(x) == f) {
                continue;
            }
            let mesh = anvil_brep::tess::tessellate(b, &params).map_err(|e| e.to_string())?;
            for (t, fi) in mesh.triangles.iter().zip(&mesh.triangle_face) {
                if b.faces.get(*fi as usize).map(crate::exact::face_id) != Some(f) {
                    continue;
                }
                let [p, q, r] = t.map(|i| mesh.positions[i as usize]);
                let w = (q - p).cross(r - p).length() * 0.5;
                area += w;
                c += (p + q + r) * (w / 3.0);
            }
        }
        if area > 0.0 {
            let c = c / area;
            return Ok(Vec3 { x: c.x, y: c.y, z: c.z });
        }
    }
    let (mut area, mut c) = (0.0, Vec3::ZERO);
    for p in st.body.face_polygons(f) {
        let v0 = p.verts[0];
        for i in 1..p.verts.len() - 1 {
            let (a, b) = (p.verts[i], p.verts[i + 1]);
            let w = (a - v0).cross(b - v0).length() * 0.5;
            area += w;
            c += (v0 + a + b) * (w / 3.0);
        }
    }
    if area <= 0.0 {
        return err("the face no longer exists");
    }
    Ok(c * (1.0 / area))
}

#[cfg(test)]
mod tests {
    //! Statements from `docs/re/reference-geometry.md`, numbered as there.
    use super::*;
    use anvil_kernel::v2;

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 { x, y, z }
    }

    fn near(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-7
    }

    /// The 100 x 60 x 40 block with a corner at the origin; returns its extrude id.
    fn block() -> (Document, u32) {
        let mut doc = Document::new();
        let mut sf = SketchFeature::new(PlaneRef::Front);
        sf.sketch.add_rectangle(v2(0.0, 0.0), v2(100.0, 60.0));
        let s = doc.add_feature(FeatureKind::Sketch(sf));
        let e = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: s,
            depth: Dim::Value(40.0),
            ..Default::default()
        }));
        (doc, e)
    }

    fn plane(doc: &mut Document, base: PlaneRef, kind: PlaneKind, offset: f64) -> u32 {
        doc.add_feature(FeatureKind::Plane(PlaneFeature {
            base,
            offset: Dim::Value(offset),
            kind,
            ..Default::default()
        }))
    }

    fn model(p: Vec3) -> PointRef {
        PointRef::Model(p)
    }

    fn built(doc: &mut Document) -> BuildState {
        let st = doc.rebuild();
        assert!(st.errors.is_empty(), "{:?}", st.errors);
        st
    }

    #[test]
    fn s02_s03_offsets_and_flips() {
        let (mut doc, e) = block();
        let top = PlaneRef::Face(FaceId { feature: e, index: FaceId::END_CAP });
        let p = plane(&mut doc, top, PlaneKind::Offset, 10.0);
        let st = built(&mut doc);
        let f = st.frames[&p];
        assert!((f.origin.z - 50.0).abs() < 1e-9 && near(f.z, Vec3::Z));
        // Flip normal only.
        if let Some(FeatureKind::Plane(x)) = doc.feature_mut(p).map(|f| &mut f.kind) {
            x.flip_normal = true;
        }
        let f = built(&mut doc).frames[&p];
        assert!((f.origin.z - 50.0).abs() < 1e-9 && near(f.z, -Vec3::Z));
        // The other side, keeping the normal: flip both (old files' flip did both).
        if let Some(FeatureKind::Plane(x)) = doc.feature_mut(p).map(|f| &mut f.kind) {
            x.flip = true;
        }
        let f = built(&mut doc).frames[&p];
        assert!((f.origin.z - 30.0).abs() < 1e-9 && near(f.z, Vec3::Z), "{f:?}");
    }

    #[test]
    fn s05_s06_s07_points_and_lines() {
        let (mut doc, _) = block();
        let a = plane(
            &mut doc,
            PlaneRef::Top,
            PlaneKind::ParallelThrough { point: model(v(100.0, 60.0, 40.0)) },
            0.0,
        );
        let b = plane(
            &mut doc,
            PlaneRef::Front,
            PlaneKind::ThreePoints {
                points: [model(v(0.0, 0.0, 0.0)), model(v(100.0, 0.0, 0.0)), model(v(0.0, 60.0, 0.0))],
            },
            0.0,
        );
        let c = plane(
            &mut doc,
            PlaneRef::Front,
            PlaneKind::ThreePoints {
                points: [model(v(100.0, 0.0, 0.0)), model(v(0.0, 0.0, 0.0)), model(v(0.0, 60.0, 0.0))],
            },
            0.0,
        );
        let x_axis = LineRef::Axis(Axis::Line { origin: Vec3::ZERO, dir: Vec3::X });
        let d = plane(
            &mut doc,
            PlaneRef::Front,
            PlaneKind::LinePoint { line: x_axis, point: model(v(0.0, 0.0, 40.0)) },
            0.0,
        );
        let st = built(&mut doc);
        assert!((st.frames[&a].origin.y - 60.0).abs() < 1e-9 && near(st.frames[&a].z, Vec3::Y));
        assert!(near(st.frames[&b].z, Vec3::Z) && near(st.frames[&b].origin, Vec3::ZERO));
        assert!(near(st.frames[&c].z, -Vec3::Z));
        assert!(near(st.frames[&d].z, -Vec3::Y));
        let bad = plane(
            &mut doc,
            PlaneRef::Front,
            PlaneKind::ThreePoints {
                points: [model(v(0.0, 0.0, 0.0)), model(v(50.0, 0.0, 0.0)), model(v(100.0, 0.0, 0.0))],
            },
            0.0,
        );
        assert!(doc.rebuild().errors.contains_key(&bad));
    }

    #[test]
    fn s08_s10_s11_at_angle() {
        let (mut doc, _) = block();
        let x_axis = LineRef::Axis(Axis::Line { origin: Vec3::ZERO, dir: Vec3::X });
        let a =
            plane(&mut doc, PlaneRef::Top, PlaneKind::AtAngle { line: x_axis, angle: Dim::Value(30.0) }, 0.0);
        let st = built(&mut doc);
        let n = st.frames[&a].z;
        assert!(near(n, v(0.0, 30f64.to_radians().cos(), 30f64.to_radians().sin())), "{n:?}");
        let off = LineRef::Axis(Axis::Line { origin: v(0.0, 60.0, 40.0), dir: Vec3::X });
        let b =
            plane(&mut doc, PlaneRef::Top, PlaneKind::AtAngle { line: off, angle: Dim::Value(90.0) }, 0.0);
        let f = built(&mut doc).frames[&b];
        assert!(near(f.z, Vec3::Z) && (f.origin.z - 40.0).abs() < 1e-9, "{f:?}");
        let slant = LineRef::Axis(Axis::Line { origin: Vec3::ZERO, dir: v(0.0, 60.0, 40.0) });
        let c =
            plane(&mut doc, PlaneRef::Top, PlaneKind::AtAngle { line: slant, angle: Dim::Value(10.0) }, 0.0);
        assert!(doc.rebuild().errors.contains_key(&c));
    }

    #[test]
    fn s12_s14_s15_s16_normal_and_mid_planes() {
        let (mut doc, e) = block();
        let edge_x = LineRef::Axis(Axis::Line { origin: Vec3::ZERO, dir: Vec3::X });
        let n1 = plane(
            &mut doc,
            PlaneRef::Front,
            PlaneKind::NormalToCurve {
                curve: edge_x,
                point: model(v(30.0, 20.0, 10.0)),
                origin_on_curve: false,
            },
            0.0,
        );
        let n2 = plane(
            &mut doc,
            PlaneRef::Front,
            PlaneKind::NormalToCurve {
                curve: edge_x,
                point: model(v(30.0, 20.0, 10.0)),
                origin_on_curve: true,
            },
            0.0,
        );
        let left = PlaneRef::Face(FaceId::side(e, 3));
        let right = PlaneRef::Face(FaceId::side(e, 1));
        let m1 = plane(&mut doc, left, PlaneKind::MidPlane { other: right }, 0.0);
        let m2 = plane(&mut doc, right, PlaneKind::MidPlane { other: left }, 0.0);
        let z30 = plane(&mut doc, PlaneRef::Front, PlaneKind::Offset, 30.0);
        let m3 = plane(&mut doc, PlaneRef::Front, PlaneKind::MidPlane { other: PlaneRef::Feature(z30) }, 0.0);
        let tilted = PlaneRef::Fixed(Frame::from_normal(Vec3::ZERO, v(0.0, 0.5, 0.75f64.sqrt()), Vec3::X));
        let m4 = plane(&mut doc, PlaneRef::Top, PlaneKind::MidPlane { other: tilted }, 0.0);
        let st = built(&mut doc);
        assert!(near(st.frames[&n1].z, Vec3::X) && near(st.frames[&n1].origin, v(30.0, 20.0, 10.0)));
        assert!(near(st.frames[&n2].origin, v(30.0, 0.0, 0.0)));
        assert!((st.frames[&m1].origin.x - 50.0).abs() < 1e-9 && near(st.frames[&m1].z, -Vec3::X));
        assert!(near(st.frames[&m2].z, Vec3::X));
        assert!((st.frames[&m3].origin.z - 15.0).abs() < 1e-9 && near(st.frames[&m3].z, Vec3::Z));
        let c30 = 30f64.to_radians();
        assert!(near(st.frames[&m4].z, v(0.0, c30.cos(), c30.sin())), "{:?}", st.frames[&m4].z);
        assert!(st.frames[&m4].origin.y.abs() < 1e-9 && st.frames[&m4].origin.z.abs() < 1e-9);
    }

    #[test]
    fn s19_s20_s21_axes() {
        let (mut doc, _) = block();
        let a = doc.add_feature(FeatureKind::Axis(AxisFeature {
            kind: AxisKind::TwoPlanes(PlaneRef::Right, PlaneRef::Top),
            reverse: false,
        }));
        assert_eq!(doc.feature(a).map(|f| f.name.as_str()), Some("Axis1"));
        let b = doc.add_feature(FeatureKind::Axis(AxisFeature {
            kind: AxisKind::TwoPoints(model(Vec3::ZERO), model(v(100.0, 60.0, 40.0))),
            reverse: false,
        }));
        let st = built(&mut doc);
        assert!(near(st.axes[&a].1, Vec3::Z) && near(st.axes[&a].0, Vec3::ZERO));
        assert!(
            near(st.axes[&b].1, v(0.8111071, 0.4866643, 0.3244428))
                || st.axes[&b].1.distance(v(0.8111071, 0.4866643, 0.3244428)) < 1e-6
        );
        let off = plane(&mut doc, PlaneRef::Front, PlaneKind::Offset, 5.0);
        let c = doc.add_feature(FeatureKind::Axis(AxisFeature {
            kind: AxisKind::TwoPlanes(PlaneRef::Front, PlaneRef::Feature(off)),
            reverse: false,
        }));
        assert!(doc.rebuild().errors.contains_key(&c));
    }

    #[test]
    fn s22_axis_of_a_hole_and_s25_face_centre() {
        let (mut doc, e) = block();
        let top = PlaneRef::Face(FaceId { feature: e, index: FaceId::END_CAP });
        let mut sf = SketchFeature::new(top);
        let c = sf.sketch.add_point(v2(80.0, 30.0));
        sf.sketch.add_circle(c, 10.0);
        let s = doc.add_feature(FeatureKind::Sketch(sf));
        let hole = doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
            sketch: s,
            end: EndCondition::ThroughAll,
            op: BodyOp::Cut,
            ..Default::default()
        }));
        let ax = doc.add_feature(FeatureKind::Axis(AxisFeature {
            kind: AxisKind::Face(FaceId::side(hole, 0)),
            reverse: false,
        }));
        let pc = doc.add_feature(FeatureKind::Point(PointFeature::FaceCenter(FaceId {
            feature: e,
            index: FaceId::END_CAP,
        })));
        let cm = doc.add_feature(FeatureKind::Point(PointFeature::CenterOfMass));
        let st = built(&mut doc);
        let (o, d) = st.axes[&ax];
        assert!(d.cross(Vec3::Z).length() < 1e-9 && (o.x - 80.0).abs() < 1e-9 && (o.y - 30.0).abs() < 1e-9);
        assert!((st.points[&pc].x - 48.3424126).abs() < 1e-5, "{:?}", st.points[&pc]);
        assert!((st.points[&pc].z - 40.0).abs() < 1e-9);
        assert!(
            (st.points[&cm].x - 48.3424126).abs() < 1e-5 && (st.points[&cm].z - 20.0).abs() < 1e-6,
            "{:?}",
            st.points[&cm]
        );
    }

    #[test]
    fn s26_s27_s28_points() {
        let (mut doc, _) = block();
        let x_axis = LineRef::Axis(Axis::Line { origin: Vec3::ZERO, dir: Vec3::X });
        let cross = LineRef::Axis(Axis::Line { origin: v(20.0, -10.0, 0.0), dir: Vec3::Y });
        let i = doc
            .add_feature(FeatureKind::Point(PointFeature::Intersection(x_axis, IntersectWith::Line(cross))));
        let up = LineRef::Axis(Axis::Line { origin: v(20.0, 10.0, 5.0), dir: Vec3::Z });
        let j = doc.add_feature(FeatureKind::Point(PointFeature::Intersection(
            up,
            IntersectWith::Plane(PlaneRef::Front),
        )));
        let p = doc.add_feature(FeatureKind::Point(PointFeature::Projection(
            model(v(20.0, 10.0, 70.0)),
            PlaneRef::Fixed(Frame::WORLD.offset(40.0)),
        )));
        let along: Vec<u32> = (1..=4)
            .map(|k| {
                doc.add_feature(FeatureKind::Point(PointFeature::AlongCurve {
                    curve: LineRef::Edge(EdgeRef::new(
                        FaceId::side(2, 0),
                        FaceId { feature: 2, index: FaceId::START_CAP },
                    )),
                    mode: AlongMode::Even,
                    value: Dim::Value(0.0),
                    count: 4,
                    index: k,
                    reverse: false,
                }))
            })
            .collect();
        let st = built(&mut doc);
        assert!(near(st.points[&i], v(20.0, 0.0, 0.0)));
        assert!(near(st.points[&j], v(20.0, 10.0, 0.0)));
        assert!(near(st.points[&p], v(20.0, 10.0, 40.0)));
        let xs: Vec<f64> = along.iter().map(|k| st.points[k].x).collect();
        assert!(
            xs.iter().zip([20.0, 40.0, 60.0, 80.0]).all(|(a, b)| (a - b).abs() < 1e-9)
                || xs.iter().zip([80.0, 60.0, 40.0, 20.0]).all(|(a, b)| (a - b).abs() < 1e-9),
            "{xs:?}"
        );
        let skew = LineRef::Axis(Axis::Line { origin: v(0.0, 5.0, 3.0), dir: Vec3::Y });
        let bad = doc
            .add_feature(FeatureKind::Point(PointFeature::Intersection(x_axis, IntersectWith::Line(skew))));
        assert!(doc.rebuild().errors.contains_key(&bad));
    }

    #[test]
    fn s29_s30_s31_s32_s33_coordinate_systems() {
        let mut doc = Document::new();
        let line = |o: Vec3, d: Vec3| Some(DirRef::Line(LineRef::Axis(Axis::Line { origin: o, dir: d })));
        let a = doc.add_feature(FeatureKind::CoordSys(CoordSysFeature {
            origin: Some(model(v(100.0, 0.0, 0.0))),
            axes: [line(v(100.0, 0.0, 0.0), Vec3::Y), line(v(100.0, 0.0, 0.0), Vec3::Z), None],
            ..Default::default()
        }));
        let b = doc.add_feature(FeatureKind::CoordSys(CoordSysFeature {
            origin: Some(model(v(100.0, 0.0, 0.0))),
            axes: [line(v(100.0, 0.0, 0.0), Vec3::Y), line(v(100.0, 0.0, 0.0), Vec3::Z), None],
            reverse: [true, false, false],
            ..Default::default()
        }));
        let c = doc.add_feature(FeatureKind::CoordSys(CoordSysFeature {
            axes: [None, None, Some(DirRef::Normal(PlaneRef::Front))],
            ..Default::default()
        }));
        let d = doc.add_feature(FeatureKind::CoordSys(CoordSysFeature {
            position: [200.0, 200.0, 200.0],
            rotation: [0.0, 0.0, 90.0],
            ..Default::default()
        }));
        let st = built(&mut doc);
        let f = st.frames[&a];
        assert!(near(f.x, Vec3::Y) && near(f.y, Vec3::Z) && near(f.z, Vec3::X));
        let local = f.to_local3(v(100.0, 60.0, 40.0));
        assert!(near(local, v(60.0, 40.0, 0.0)));
        let f = st.frames[&b];
        assert!(near(f.x, -Vec3::Y) && near(f.y, Vec3::Z) && near(f.z, -Vec3::X));
        let f = st.frames[&c];
        assert!(near(f.x, Vec3::X) && near(f.y, Vec3::Y) && near(f.z, Vec3::Z));
        let f = st.frames[&d];
        assert!(near(f.origin, v(200.0, 200.0, 200.0)) && near(f.x, Vec3::Y) && near(f.y, -Vec3::X));
        let bad = doc.add_feature(FeatureKind::CoordSys(CoordSysFeature {
            axes: [line(Vec3::ZERO, Vec3::X), line(Vec3::ZERO, Vec3::X), None],
            ..Default::default()
        }));
        assert!(doc.rebuild().errors.contains_key(&bad));
    }

    #[test]
    fn pattern_about_a_reference_axis() {
        let (mut doc, e) = block();
        let ax = doc.add_feature(FeatureKind::Axis(AxisFeature {
            kind: AxisKind::TwoPlanes(PlaneRef::Right, PlaneRef::Top),
            reverse: false,
        }));
        doc.add_feature(FeatureKind::CircularPattern(CircularPatternFeature {
            features: vec![e],
            axis: Axis::Feature(ax),
            total_angle: Dim::Value(360.0),
            count: Dim::Value(2.0),
            ..Default::default()
        }));
        let st = built(&mut doc);
        assert!((st.body.mass_properties().volume - 2.0 * 240000.0).abs() < 1e-6);
        assert!((st.body.bounds().min.x + 100.0).abs() < 1e-9);
    }
}
