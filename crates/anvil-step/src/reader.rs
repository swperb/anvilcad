//! Reads B-rep bodies out of a parsed exchange file.
//!
//! Supports MANIFOLD_SOLID_BREP, BREP_WITH_VOIDS, FACETED_BREP and
//! SHELL_BASED_SURFACE_MODEL bodies made of ADVANCED_FACE, FACE_SURFACE and FACE
//! entities, with EDGE_LOOP, VERTEX_LOOP and POLY_LOOP bounds. Lengths are converted
//! to millimetres from the file's units. Geometry that cannot be read is recorded as a
//! warning and left out rather than failing the whole file.

use crate::p21::{Entity, Exchange, Param};
use crate::shapes::{BSplineCurve, BSplineSurface, Basis, Curve, Surface};
use anvil_brep::Frame;
use glam::DVec3;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct OrientedEdge {
    pub edge: u64,
    /// True when the loop runs along the edge from its start to its end vertex.
    pub forward: bool,
}

#[derive(Debug, Clone)]
pub enum LoopKind {
    Edges(Vec<OrientedEdge>),
    Vertex(u64),
    Poly(Vec<DVec3>),
}

#[derive(Debug, Clone)]
pub struct Bound {
    pub outer: bool,
    /// False flips the loop's direction.
    pub orientation: bool,
    pub kind: LoopKind,
}

#[derive(Debug, Clone)]
pub struct Face {
    pub id: u64,
    /// `None` when the surface type is not supported.
    pub surface: Option<Surface>,
    pub surface_type: String,
    /// True when the face normal agrees with the surface normal.
    pub same_sense: bool,
    pub bounds: Vec<Bound>,
}

#[derive(Debug, Clone)]
pub struct EdgeCurve {
    pub start: u64,
    pub end: u64,
    /// `None` when the curve type is not supported.
    pub curve: Option<Curve>,
    pub curve_type: String,
    /// True when the curve's direction runs from `start` to `end`.
    pub same_sense: bool,
}

#[derive(Debug, Clone)]
pub struct Body {
    pub id: u64,
    pub name: String,
    pub faces: Vec<Face>,
    /// Closed solids (as opposed to surface models).
    pub solid: bool,
}

/// A body the file stores as triangles (AP242 tessellated geometry).
#[derive(Debug, Clone, Default)]
pub struct TessBody {
    pub name: String,
    pub vertices: Vec<DVec3>,
    /// Per face: whether it lies on a plane, and its triangles.
    pub faces: Vec<(bool, Vec<[u32; 3]>)>,
    pub solid: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Model {
    pub bodies: Vec<Body>,
    pub tessellated: Vec<TessBody>,
    pub edges: HashMap<u64, EdgeCurve>,
    pub vertices: HashMap<u64, DVec3>,
    pub warnings: Vec<String>,
    /// Millimetres per file length unit.
    pub length_scale: f64,
    pub length_unit: String,
    pub assembly: bool,
    /// The distance (mm) the file says it is accurate to, if it says.
    pub uncertainty: Option<f64>,
}

struct Reader<'a> {
    ex: &'a Exchange,
    scale: f64,
    angle: f64,
    model: Model,
    curves: HashMap<u64, Option<Curve>>,
}

fn name_of(e: &Entity) -> String {
    e.params().first().and_then(Param::as_str).unwrap_or("").to_string()
}

impl<'a> Reader<'a> {
    fn warn(&mut self, msg: String) {
        if self.model.warnings.len() < 200 && !self.model.warnings.contains(&msg) {
            self.model.warnings.push(msg);
        }
    }

    fn entity(&self, id: u64) -> Result<&'a Entity, String> {
        self.ex.get(id).ok_or_else(|| format!("#{id} is referenced but not defined"))
    }

    fn reference(&self, p: Option<&Param>) -> Result<u64, String> {
        p.and_then(Param::as_ref).ok_or_else(|| "expected an entity reference".to_string())
    }

    fn point(&self, id: u64) -> Result<DVec3, String> {
        let e = self.entity(id)?;
        if e.name() != "CARTESIAN_POINT" {
            return Err(format!("#{id} is {} where a point was expected", e.name()));
        }
        let c = e.params().get(1).and_then(Param::as_list).ok_or("point without coordinates")?;
        let v = |i: usize| c.get(i).and_then(Param::as_real).unwrap_or(0.0);
        let p = DVec3::new(v(0), v(1), v(2)) * self.scale;
        if !p.is_finite() {
            return Err(format!("#{id} has a non-finite coordinate"));
        }
        Ok(p)
    }

    fn direction(&self, id: u64) -> Result<DVec3, String> {
        let e = self.entity(id)?;
        let c = e.params().get(1).and_then(Param::as_list).ok_or("direction without ratios")?;
        let v = |i: usize| c.get(i).and_then(Param::as_real).unwrap_or(0.0);
        DVec3::new(v(0), v(1), v(2)).try_normalize().ok_or_else(|| format!("#{id} is a zero direction"))
    }

    fn placement(&self, id: u64) -> Result<Frame, String> {
        let e = self.entity(id)?;
        let p = e.params();
        let origin = self.point(self.reference(p.get(1))?)?;
        let z = match p.get(2).and_then(Param::as_ref) {
            Some(r) => self.direction(r)?,
            None => DVec3::Z,
        };
        let x = match p.get(3).and_then(Param::as_ref) {
            Some(r) => self.direction(r)?,
            None => z.any_orthonormal_vector(),
        };
        let x = if x.cross(z).length() < 1e-9 { z.any_orthonormal_vector() } else { x };
        Frame::new(origin, z, x).map_err(|e| format!("#{id}: {e}"))
    }

    fn length(&self, p: Option<&Param>) -> Result<f64, String> {
        let v = p.and_then(Param::as_real).ok_or("expected a length")? * self.scale;
        if v.is_finite() { Ok(v) } else { Err("non-finite length".into()) }
    }

    fn curve(&mut self, id: u64) -> Option<Curve> {
        if let Some(c) = self.curves.get(&id) {
            return c.clone();
        }
        let r = self.read_curve(id, 0);
        let c = match r {
            Ok(c) => Some(c),
            Err(msg) => {
                self.warn(format!("curve #{id}: {msg}"));
                None
            }
        };
        self.curves.insert(id, c.clone());
        c
    }

    fn read_curve(&self, id: u64, depth: usize) -> Result<Curve, String> {
        if depth > 8 {
            return Err("curve definitions nest too deeply".into());
        }
        let e = self.entity(id)?;
        let p = e.params();
        if e.has("B_SPLINE_CURVE") || e.name() == "B_SPLINE_CURVE_WITH_KNOTS" {
            return self.bspline_curve(e);
        }
        match e.name() {
            "LINE" => {
                let origin = self.point(self.reference(p.get(1))?)?;
                let vector = self.entity(self.reference(p.get(2))?)?;
                let dir = self.direction(self.reference(vector.params().get(1))?)?;
                Ok(Curve::Line { origin, dir })
            }
            "CIRCLE" => Ok(Curve::Circle {
                frame: self.placement(self.reference(p.get(1))?)?,
                radius: self.length(p.get(2))?,
            }),
            "ELLIPSE" => Ok(Curve::Ellipse {
                frame: self.placement(self.reference(p.get(1))?)?,
                a: self.length(p.get(2))?,
                b: self.length(p.get(3))?,
            }),
            "POLYLINE" => {
                let pts = p.get(1).and_then(Param::as_list).ok_or("polyline without points")?;
                let pts: Result<Vec<DVec3>, String> =
                    pts.iter().map(|q| self.point(self.reference(Some(q))?)).collect();
                Ok(Curve::Polyline(pts?))
            }
            "TRIMMED_CURVE" => self.read_curve(self.reference(p.get(1))?, depth + 1),
            "SURFACE_CURVE" | "SEAM_CURVE" | "INTERSECTION_CURVE" | "BOUNDED_SURFACE_CURVE" => {
                self.read_curve(self.reference(p.get(1))?, depth + 1)
            }
            other => Err(format!("{other} curves are not supported yet")),
        }
    }

    fn bspline_curve(&self, e: &Entity) -> Result<Curve, String> {
        // Simple instance: (name, degree, points, form, closed, self, mults, knots, spec).
        // Complex instance: B_SPLINE_CURVE(degree, points, ...) plus
        // B_SPLINE_CURVE_WITH_KNOTS(mults, knots, spec) and RATIONAL_B_SPLINE_CURVE(weights).
        let (head, knot_part): (&[Param], &[Param]) = if e.is_complex() {
            (
                e.part("B_SPLINE_CURVE").ok_or("rational curve without B_SPLINE_CURVE")?,
                e.part("B_SPLINE_CURVE_WITH_KNOTS").ok_or("B-spline curve without knots")?,
            )
        } else {
            let p = e.params();
            (p.get(1..).unwrap_or(&[]), p.get(6..).unwrap_or(&[]))
        };
        let degree = head.first().and_then(Param::as_real).ok_or("B-spline without degree")? as usize;
        let pts = head.get(1).and_then(Param::as_list).ok_or("B-spline without control points")?;
        let points: Result<Vec<DVec3>, String> =
            pts.iter().map(|q| self.point(self.reference(Some(q))?)).collect();
        let mults: Vec<i64> = knot_part
            .first()
            .and_then(Param::as_list)
            .ok_or("missing knot multiplicities")?
            .iter()
            .filter_map(|m| m.as_real().map(|v| v as i64))
            .collect();
        let knots: Vec<f64> = knot_part
            .get(1)
            .and_then(Param::as_list)
            .ok_or("missing knots")?
            .iter()
            .filter_map(Param::as_real)
            .collect();
        let basis = Basis::new(degree, &mults, &knots).ok_or("inconsistent B-spline knots")?;
        let points = points?;
        if basis.count() != points.len() {
            return Err("B-spline knot count does not match its control points".into());
        }
        let weights = match e.part("RATIONAL_B_SPLINE_CURVE") {
            Some(w) => {
                w.first().and_then(Param::as_list).unwrap_or(&[]).iter().filter_map(Param::as_real).collect()
            }
            None => Vec::new(),
        };
        Ok(Curve::BSpline(BSplineCurve { basis, points, weights }))
    }

    fn surface(&mut self, id: u64) -> (Option<Surface>, String) {
        let ty = self.ex.get(id).map(|e| e.name().to_string()).unwrap_or_default();
        match self.read_surface(id) {
            Ok(s) => (Some(s), ty),
            Err(msg) => {
                self.warn(format!("surface #{id}: {msg}"));
                (None, ty)
            }
        }
    }

    fn read_surface(&self, id: u64) -> Result<Surface, String> {
        let e = self.entity(id)?;
        let p = e.params();
        if e.has("B_SPLINE_SURFACE") || e.name() == "B_SPLINE_SURFACE_WITH_KNOTS" {
            return self.bspline_surface(e);
        }
        match e.name() {
            "PLANE" => Ok(Surface::Plane { frame: self.placement(self.reference(p.get(1))?)? }),
            "CYLINDRICAL_SURFACE" => Ok(Surface::Cylinder {
                frame: self.placement(self.reference(p.get(1))?)?,
                radius: self.length(p.get(2))?,
            }),
            "CONICAL_SURFACE" => Ok(Surface::Cone {
                frame: self.placement(self.reference(p.get(1))?)?,
                radius: self.length(p.get(2))?,
                semi_angle: p.get(3).and_then(Param::as_real).ok_or("cone without angle")? * self.angle,
            }),
            "SPHERICAL_SURFACE" => Ok(Surface::Sphere {
                frame: self.placement(self.reference(p.get(1))?)?,
                radius: self.length(p.get(2))?,
            }),
            "TOROIDAL_SURFACE" | "DEGENERATE_TOROIDAL_SURFACE" => Ok(Surface::Torus {
                frame: self.placement(self.reference(p.get(1))?)?,
                major: self.length(p.get(2))?,
                minor: self.length(p.get(3))?,
            }),
            other => Err(format!("{other} surfaces are not supported yet")),
        }
    }

    fn bspline_surface(&self, e: &Entity) -> Result<Surface, String> {
        // Simple: (name, ud, vd, points, form, uc, vc, self, umults, vmults, uknots, vknots, spec).
        let (head, knot_part): (&[Param], &[Param]) = if e.is_complex() {
            (
                e.part("B_SPLINE_SURFACE").ok_or("rational surface without B_SPLINE_SURFACE")?,
                e.part("B_SPLINE_SURFACE_WITH_KNOTS").ok_or("B-spline surface without knots")?,
            )
        } else {
            let p = e.params();
            (p.get(1..).unwrap_or(&[]), p.get(8..).unwrap_or(&[]))
        };
        let ud = head.first().and_then(Param::as_real).ok_or("missing u degree")? as usize;
        let vd = head.get(1).and_then(Param::as_real).ok_or("missing v degree")? as usize;
        let rows = head.get(2).and_then(Param::as_list).ok_or("missing control net")?;
        let mut points = Vec::with_capacity(rows.len());
        for r in rows {
            let r = r.as_list().ok_or("control net row is not a list")?;
            let row: Result<Vec<DVec3>, String> =
                r.iter().map(|q| self.point(self.reference(Some(q))?)).collect();
            points.push(row?);
        }
        let ints = |i: usize| -> Vec<i64> {
            knot_part
                .get(i)
                .and_then(Param::as_list)
                .unwrap_or(&[])
                .iter()
                .filter_map(|m| m.as_real().map(|v| v as i64))
                .collect()
        };
        let reals = |i: usize| -> Vec<f64> {
            knot_part
                .get(i)
                .and_then(Param::as_list)
                .unwrap_or(&[])
                .iter()
                .filter_map(Param::as_real)
                .collect()
        };
        let u = Basis::new(ud, &ints(0), &reals(2)).ok_or("inconsistent u knots")?;
        let v = Basis::new(vd, &ints(1), &reals(3)).ok_or("inconsistent v knots")?;
        if u.count() != points.len() || points.iter().any(|r| r.len() != v.count()) {
            return Err("B-spline surface knots do not match its control net".into());
        }
        let weights = match e.part("RATIONAL_B_SPLINE_SURFACE") {
            Some(w) => w
                .first()
                .and_then(Param::as_list)
                .unwrap_or(&[])
                .iter()
                .map(|r| r.as_list().unwrap_or(&[]).iter().filter_map(Param::as_real).collect())
                .collect(),
            None => Vec::new(),
        };
        Ok(Surface::BSpline(BSplineSurface { u, v, points, weights }))
    }

    fn vertex(&mut self, id: u64) -> Result<u64, String> {
        if !self.model.vertices.contains_key(&id) {
            let e = self.entity(id)?;
            let p = self.point(self.reference(e.params().get(1))?)?;
            self.model.vertices.insert(id, p);
        }
        Ok(id)
    }

    fn edge(&mut self, id: u64) -> Result<u64, String> {
        if self.model.edges.contains_key(&id) {
            return Ok(id);
        }
        let e = self.entity(id)?;
        if e.name() != "EDGE_CURVE" {
            return Err(format!("{} edges are not supported", e.name()));
        }
        let p = e.params();
        let start = self.vertex(self.reference(p.get(1))?)?;
        let end = self.vertex(self.reference(p.get(2))?)?;
        let cid = self.reference(p.get(3))?;
        let curve_type = self.ex.get(cid).map(|c| c.name().to_string()).unwrap_or_default();
        let curve = self.curve(cid);
        let same_sense = p.get(4).and_then(Param::as_bool).unwrap_or(true);
        self.model.edges.insert(id, EdgeCurve { start, end, curve, curve_type, same_sense });
        Ok(id)
    }

    fn bound(&mut self, id: u64) -> Result<Bound, String> {
        let e = self.entity(id)?;
        let outer = e.name() == "FACE_OUTER_BOUND";
        let p = e.params();
        let lp = self.entity(self.reference(p.get(1))?)?;
        let orientation = p.get(2).and_then(Param::as_bool).unwrap_or(true);
        let kind = match lp.name() {
            "EDGE_LOOP" => {
                let list = lp.params().get(1).and_then(Param::as_list).ok_or("edge loop without edges")?;
                let mut edges = Vec::with_capacity(list.len());
                for item in list {
                    let oe = self.entity(self.reference(Some(item))?)?;
                    if oe.name() != "ORIENTED_EDGE" {
                        return Err(format!("{} in an edge loop", oe.name()));
                    }
                    let edge = self.edge(self.reference(oe.params().get(3))?)?;
                    let forward = oe.params().get(4).and_then(Param::as_bool).unwrap_or(true);
                    edges.push(OrientedEdge { edge, forward });
                }
                LoopKind::Edges(edges)
            }
            "VERTEX_LOOP" => LoopKind::Vertex(self.vertex(self.reference(lp.params().get(1))?)?),
            "POLY_LOOP" => {
                let list = lp.params().get(1).and_then(Param::as_list).ok_or("poly loop without points")?;
                let pts: Result<Vec<DVec3>, String> =
                    list.iter().map(|q| self.point(self.reference(Some(q))?)).collect();
                LoopKind::Poly(pts?)
            }
            other => return Err(format!("{other} loops are not supported")),
        };
        Ok(Bound { outer, orientation, kind })
    }

    fn face(&mut self, id: u64) -> Result<Face, String> {
        let e = self.entity(id)?;
        let (e, flip) = if e.name() == "ORIENTED_FACE" {
            let inner = self.entity(self.reference(e.params().get(2))?)?;
            (inner, !e.params().get(3).and_then(Param::as_bool).unwrap_or(true))
        } else {
            (e, false)
        };
        let p = e.params();
        let list = p.get(1).and_then(Param::as_list).ok_or("face without bounds")?;
        let mut bounds = Vec::with_capacity(list.len());
        for b in list {
            bounds.push(self.bound(self.reference(Some(b))?)?);
        }
        let (surface, surface_type, same_sense) = match e.name() {
            "ADVANCED_FACE" | "FACE_SURFACE" => {
                let (s, ty) = self.surface(self.reference(p.get(2))?);
                (s, ty, p.get(3).and_then(Param::as_bool).unwrap_or(true))
            }
            "FACE" => (None, "POLY".into(), true),
            other => return Err(format!("{other} faces are not supported")),
        };
        Ok(Face { id, surface, surface_type, same_sense: same_sense != flip, bounds })
    }

    fn shell_faces(&mut self, id: u64) -> Result<Vec<Face>, String> {
        let e = self.entity(id)?;
        let (e, flipped) = if e.name() == "ORIENTED_CLOSED_SHELL" {
            let inner = self.entity(self.reference(e.params().get(2))?)?;
            (inner, !e.params().get(3).and_then(Param::as_bool).unwrap_or(true))
        } else {
            (e, false)
        };
        let list = e.params().get(1).and_then(Param::as_list).ok_or("shell without faces")?;
        let mut faces = Vec::with_capacity(list.len());
        for f in list {
            let fid = self.reference(Some(f))?;
            match self.face(fid) {
                Ok(mut face) => {
                    if flipped {
                        face.same_sense = !face.same_sense;
                        for b in &mut face.bounds {
                            b.orientation = !b.orientation;
                        }
                    }
                    faces.push(face)
                }
                Err(msg) => self.warn(format!("face #{fid} skipped: {msg}")),
            }
        }
        Ok(faces)
    }

    fn body(&mut self, id: u64) -> Result<Body, String> {
        let e = self.entity(id)?;
        let p = e.params();
        let name = name_of(e);
        match e.name() {
            "MANIFOLD_SOLID_BREP" | "FACETED_BREP" => {
                Ok(Body { id, name, faces: self.shell_faces(self.reference(p.get(1))?)?, solid: true })
            }
            "BREP_WITH_VOIDS" => {
                let mut faces = self.shell_faces(self.reference(p.get(1))?)?;
                for v in p.get(2).and_then(Param::as_list).unwrap_or(&[]) {
                    faces.extend(self.shell_faces(self.reference(Some(v))?)?);
                }
                Ok(Body { id, name, faces, solid: true })
            }
            "SHELL_BASED_SURFACE_MODEL" => {
                let mut faces = Vec::new();
                for s in p.get(1).and_then(Param::as_list).unwrap_or(&[]) {
                    faces.extend(self.shell_faces(self.reference(Some(s))?)?);
                }
                Ok(Body { id, name, faces, solid: false })
            }
            other => Err(format!("{other} is not a body")),
        }
    }
}

impl Reader<'_> {
    /// Reads a TESSELLATED_SOLID or TESSELLATED_SHELL.
    fn tess_body(&mut self, id: u64) -> Result<TessBody, String> {
        let e = self.entity(id)?;
        let mut body =
            TessBody { name: name_of(e), solid: e.name() == "TESSELLATED_SOLID", ..TessBody::default() };
        let items = e.params().get(1).and_then(Param::as_list).ok_or("tessellated body without items")?;
        let mut index: HashMap<(u64, usize), u32> = HashMap::new();
        for item in items {
            let fid = self.reference(Some(item))?;
            match self.tess_face(fid, &mut body, &mut index) {
                Ok(()) => {}
                Err(msg) => self.warn(format!("tessellated face #{fid} skipped: {msg}")),
            }
        }
        Ok(body)
    }

    fn tess_face(
        &mut self,
        id: u64,
        body: &mut TessBody,
        index: &mut HashMap<(u64, usize), u32>,
    ) -> Result<(), String> {
        let e = self.entity(id)?;
        let p = e.params();
        // Faces carry a geometric link (parameter 4) that sets do not.
        let (link, rest) = match e.name() {
            "TRIANGULATED_FACE" | "COMPLEX_TRIANGULATED_FACE" => (p.get(4), 5),
            "TRIANGULATED_SURFACE_SET" | "COMPLEX_TRIANGULATED_SURFACE_SET" => (None, 4),
            other => return Err(format!("{other} is not a triangulated face")),
        };
        let cid = self.reference(p.get(1))?;
        let coords = self.entity(cid)?;
        let pts = coords.params().get(2).and_then(Param::as_list).ok_or("coordinates list without points")?;
        let pnindex: Vec<usize> = p
            .get(rest)
            .and_then(Param::as_list)
            .unwrap_or(&[])
            .iter()
            .filter_map(|q| q.as_real().map(|v| v as usize))
            .collect();
        let lists = |i: usize| -> Vec<Vec<usize>> {
            p.get(i)
                .and_then(Param::as_list)
                .unwrap_or(&[])
                .iter()
                .map(|l| {
                    l.as_list()
                        .unwrap_or(&[])
                        .iter()
                        .filter_map(|q| q.as_real().map(|v| v as usize))
                        .collect()
                })
                .collect()
        };
        let mut local: Vec<[usize; 3]> = Vec::new();
        if e.name().starts_with("COMPLEX") {
            for strip in lists(rest + 1) {
                for i in 0..strip.len().saturating_sub(2) {
                    let (a, b, c) = (strip[i], strip[i + 1], strip[i + 2]);
                    local.push(if i % 2 == 0 { [a, b, c] } else { [b, a, c] });
                }
            }
            for fan in lists(rest + 2) {
                for i in 1..fan.len().saturating_sub(1) {
                    local.push([fan[0], fan[i], fan[i + 1]]);
                }
            }
        } else {
            for t in lists(rest + 1) {
                if let [a, b, c] = t[..] {
                    local.push([a, b, c]);
                }
            }
        }
        let scale = self.scale;
        let mut vertex = |k: usize| -> Result<u32, String> {
            // Indices are 1-based, into pnindex when present, else into the coordinates.
            let ci = if pnindex.is_empty() {
                k
            } else {
                *pnindex.get(k.wrapping_sub(1)).ok_or("bad point index")?
            };
            if let Some(&v) = index.get(&(cid, ci)) {
                return Ok(v);
            }
            let q = pts.get(ci.wrapping_sub(1)).and_then(Param::as_list).ok_or("bad coordinate index")?;
            let c = |i: usize| q.get(i).and_then(Param::as_real).unwrap_or(0.0);
            let v = body.vertices.len() as u32;
            body.vertices.push(DVec3::new(c(0), c(1), c(2)) * scale);
            index.insert((cid, ci), v);
            Ok(v)
        };
        let mut tris = Vec::with_capacity(local.len());
        for [a, b, c] in local {
            let t = [vertex(a)?, vertex(b)?, vertex(c)?];
            if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
                tris.push(t);
            }
        }
        let planar = link.and_then(Param::as_ref).and_then(|r| self.ex.get(r)).is_some_and(|l| {
            l.name() == "PLANE"
                || (l.name() == "ADVANCED_FACE"
                    && l.params()
                        .get(2)
                        .and_then(Param::as_ref)
                        .and_then(|s| self.ex.get(s))
                        .is_some_and(|s| s.name() == "PLANE"))
        });
        body.faces.push((planar, tris));
        Ok(())
    }
}

/// Millimetres per unit for a length unit entity, following conversion-based units.
fn unit_scale(ex: &Exchange, id: u64, depth: usize) -> Option<(f64, String)> {
    if depth > 8 {
        return None;
    }
    let e = ex.get(id)?;
    if let Some(si) = e.part("SI_UNIT") {
        let prefix = si.first().and_then(Param::as_enum).unwrap_or("");
        let base = match si.get(1).and_then(Param::as_enum)? {
            "METRE" => 1000.0,
            "RADIAN" => 1.0,
            _ => return None,
        };
        let f = match prefix {
            "" => 1.0,
            "MILLI" => 1e-3,
            "CENTI" => 1e-2,
            "DECI" => 1e-1,
            "MICRO" => 1e-6,
            "NANO" => 1e-9,
            "KILO" => 1e3,
            _ => return None,
        };
        let name = if prefix.is_empty() {
            si[1].as_enum()?.to_lowercase()
        } else {
            format!("{}{}", prefix, si[1].as_enum()?).to_lowercase()
        };
        return Some((base * f, name));
    }
    if let Some(cb) = e.part("CONVERSION_BASED_UNIT") {
        let name = cb.first().and_then(Param::as_str).unwrap_or("unit").to_lowercase();
        let m = ex.get(cb.get(1)?.as_ref()?)?;
        let parts = m.part("MEASURE_WITH_UNIT").or_else(|| m.parts.first().map(|(_, p)| p.as_slice()))?;
        let value = parts.first()?.as_real()?;
        let (base, _) = unit_scale(ex, parts.get(1)?.as_ref()?, depth + 1)?;
        return Some((value * base, name));
    }
    None
}

/// The length and angle units of the file's geometric context.
fn units(ex: &Exchange) -> (f64, String, f64) {
    let mut length = None;
    let mut angle = None;
    for id in ex.of_type("GLOBAL_UNIT_ASSIGNED_CONTEXT") {
        let Some(units) = ex
            .get(id)
            .and_then(|e| e.part("GLOBAL_UNIT_ASSIGNED_CONTEXT"))
            .and_then(|p| p.first()?.as_list())
        else {
            continue;
        };
        for u in units.iter().filter_map(Param::as_ref) {
            let Some(e) = ex.get(u) else { continue };
            if e.has("LENGTH_UNIT") && length.is_none() {
                length = unit_scale(ex, u, 0);
            }
            if e.has("PLANE_ANGLE_UNIT") && angle.is_none() {
                angle = unit_scale(ex, u, 0).map(|(f, _)| f);
            }
        }
    }
    let (scale, name) = length.unwrap_or((1.0, "millimetre (assumed)".into()));
    (scale, name, angle.unwrap_or(1.0))
}

pub fn read(ex: &Exchange) -> Model {
    let (scale, unit, angle) = units(ex);
    let mut r = Reader {
        ex,
        scale,
        angle,
        model: Model { length_scale: scale, length_unit: unit, ..Model::default() },
        curves: HashMap::new(),
    };
    r.model.assembly = ex.of_type("NEXT_ASSEMBLY_USAGE_OCCURRENCE").next().is_some();
    // The length uncertainty the file declares (in its length unit), the largest if
    // several: points it says are the same may be that far apart.
    r.model.uncertainty = ex
        .of_type("UNCERTAINTY_MEASURE_WITH_UNIT")
        .filter_map(|id| ex.get(id))
        .filter_map(|e| {
            let v = e.params().first()?;
            match v {
                Param::Typed(name, _) if name != "LENGTH_MEASURE" => None,
                _ => v.as_real(),
            }
        })
        .filter(|v| v.is_finite() && *v > 0.0)
        .map(|v| v * scale)
        .reduce(f64::max);
    let mut ids: Vec<u64> =
        ["MANIFOLD_SOLID_BREP", "BREP_WITH_VOIDS", "FACETED_BREP", "SHELL_BASED_SURFACE_MODEL"]
            .iter()
            .flat_map(|t| ex.of_type(t).collect::<Vec<_>>())
            .collect();
    ids.sort_unstable();
    ids.dedup();
    for id in ex.of_type("TESSELLATED_SOLID").chain(ex.of_type("TESSELLATED_SHELL")).collect::<Vec<_>>() {
        match r.tess_body(id) {
            Ok(b) if b.faces.iter().any(|(_, t)| !t.is_empty()) => r.model.tessellated.push(b),
            Ok(_) => {}
            Err(msg) => r.warn(format!("tessellated body #{id} skipped: {msg}")),
        }
    }
    for id in ids {
        match r.body(id) {
            Ok(b) if !b.faces.is_empty() => r.model.bodies.push(b),
            Ok(_) => r.warn(format!("body #{id} has no readable faces")),
            Err(msg) => r.warn(format!("body #{id} skipped: {msg}")),
        }
    }
    r.model
}
