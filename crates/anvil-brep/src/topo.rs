//! Boundary-representation topology.
//!
//! Entities live in flat arenas indexed by typed ids. A face is bounded by
//! loops of coedges; each coedge uses an edge forwards or backwards. Loops are
//! oriented counter-clockwise when viewed from outside the solid (against the
//! face's outward normal), so for a closed manifold solid every edge is used
//! exactly once in each direction.

use crate::geom::{Curve, Surface};
use glam::DVec3;
use serde::{Deserialize, Serialize};

macro_rules! id_type {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub struct $name(pub u32);

        impl $name {
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

id_type!(VertexId);
id_type!(EdgeId);
id_type!(FaceId);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Vertex {
    pub point: DVec3,
}

/// An edge runs along `curve` from parameter `t0` (at `start`) to `t1` (at
/// `end`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub curve: Curve,
    pub start: VertexId,
    pub end: VertexId,
    pub t0: f64,
    pub t1: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coedge {
    pub edge: EdgeId,
    pub reversed: bool,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Loop {
    pub coedges: Vec<Coedge>,
}

/// Where a face came from. This is the seed of persistent naming: later
/// features refer to faces by tag, not by index, so they survive rebuilds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FaceTag {
    /// The face lying on the sketch plane side of an extrusion.
    StartCap,
    /// The face at the far end of an extrusion.
    EndCap,
    /// The side face swept from a profile segment: `contour` is the loop's index in
    /// its region and `segment` is the segment's tag (its index unless the caller
    /// tagged it, for example with a sketch entity id).
    Side { contour: u32, segment: u32 },
    /// A face read from another system (for example a STEP file), numbered in the
    /// order the source listed its faces.
    Imported { index: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Face {
    pub surface: Surface,
    /// True when the outward normal is opposite the surface's natural normal.
    pub reversed: bool,
    pub outer: Loop,
    pub inners: Vec<Loop>,
    pub tag: FaceTag,
    /// The feature that created this face. Together with `tag` it names the
    /// face persistently, and boolean operations carry it over to the pieces
    /// a face is split into.
    #[serde(default)]
    pub owner: u32,
}

impl Face {
    pub fn loops(&self) -> impl Iterator<Item = &Loop> {
        std::iter::once(&self.outer).chain(self.inners.iter())
    }

    pub fn normal_at(&self, p: DVec3) -> DVec3 {
        let n = self.surface.normal_at(p);
        if self.reversed { -n } else { n }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Solid {
    pub vertices: Vec<Vertex>,
    pub edges: Vec<Edge>,
    pub faces: Vec<Face>,
}

impl Solid {
    pub fn add_vertex(&mut self, point: DVec3) -> VertexId {
        self.vertices.push(Vertex { point });
        VertexId((self.vertices.len() - 1) as u32)
    }

    pub fn add_edge(&mut self, edge: Edge) -> EdgeId {
        self.edges.push(edge);
        EdgeId((self.edges.len() - 1) as u32)
    }

    pub fn add_face(&mut self, face: Face) -> FaceId {
        self.faces.push(face);
        FaceId((self.faces.len() - 1) as u32)
    }

    pub fn vertex(&self, id: VertexId) -> Option<&Vertex> {
        self.vertices.get(id.index())
    }

    pub fn edge(&self, id: EdgeId) -> Option<&Edge> {
        self.edges.get(id.index())
    }

    pub fn face(&self, id: FaceId) -> Option<&Face> {
        self.faces.get(id.index())
    }

    /// (start, end) vertices of a coedge in its direction of use.
    pub fn coedge_vertices(&self, c: Coedge) -> Option<(VertexId, VertexId)> {
        let e = self.edge(c.edge)?;
        Some(if c.reversed { (e.end, e.start) } else { (e.start, e.end) })
    }

    /// The solid moved by the rigid motion `m` (an orthonormal frame read as a
    /// rotation plus translation). Parameters are unchanged, so topology and
    /// edge ranges carry over exactly.
    pub fn transformed(&self, m: &crate::Frame) -> Solid {
        Solid {
            vertices: self.vertices.iter().map(|v| Vertex { point: m.apply_point(v.point) }).collect(),
            edges: self.edges.iter().map(|e| Edge { curve: e.curve.moved(m), ..e.clone() }).collect(),
            faces: self.faces.iter().map(|f| Face { surface: f.surface.moved(m), ..f.clone() }).collect(),
        }
    }

    /// The faces that use an edge.
    pub fn edge_faces(&self, edge: EdgeId) -> Vec<FaceId> {
        self.faces
            .iter()
            .enumerate()
            .filter(|(_, f)| f.loops().any(|l| l.coedges.iter().any(|c| c.edge == edge)))
            .map(|(i, _)| FaceId(i as u32))
            .collect()
    }

    pub fn set_owner(&mut self, owner: u32) {
        for f in &mut self.faces {
            f.owner = owner;
        }
    }

    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        let first = self.vertices.first()?.point;
        let mut lo = first;
        let mut hi = first;
        for v in &self.vertices {
            lo = lo.min(v.point);
            hi = hi.max(v.point);
        }
        // Curved edges can bulge past their end points.
        for e in &self.edges {
            if e.curve.is_periodic() {
                for p in e.curve.sample(e.t0, e.t1, 0.0, 0.2) {
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
            }
        }
        // A torus face bulges past its edges by up to its tube radius.
        let pad = self
            .faces
            .iter()
            .filter_map(|f| match f.surface {
                Surface::Torus { minor, .. } => Some(minor),
                _ => None,
            })
            .fold(0.0, f64::max);
        Some((lo - DVec3::splat(pad), hi + DVec3::splat(pad)))
    }

    pub fn face_by_tag(&self, tag: FaceTag) -> Option<FaceId> {
        self.faces.iter().position(|f| f.tag == tag).map(|i| FaceId(i as u32))
    }
}

#[cfg(test)]
mod tests {
    use crate::extrude::extrude;
    use crate::profile::{Loop2, Segment, build_regions};
    use crate::{Frame, check};
    use glam::{DVec2, DVec3};

    #[test]
    fn transformed_solid_stays_valid_and_keeps_tags() {
        let l = Loop2::with_tags(vec![Segment::circle(DVec2::new(2.0, 0.0), 1.0)], vec![42]);
        let r = build_regions(&[l]).expect("region");
        let s = extrude(&Frame::world(), &r[0], 0.0, 3.0).expect("extrude");
        let angle = 0.7f64;
        let m = Frame {
            origin: DVec3::new(5.0, -1.0, 2.0),
            x: DVec3::new(angle.cos(), angle.sin(), 0.0),
            y: DVec3::new(-angle.sin(), angle.cos(), 0.0),
            z: DVec3::Z,
        };
        let t = s.transformed(&m);
        check::validate(&t).expect("valid");
        assert!(t.faces.iter().any(|f| f.tag == super::FaceTag::Side { contour: 0, segment: 42 }));
        let c = t.vertices.iter().map(|v| v.point.z).fold(f64::INFINITY, f64::min);
        assert!((c - 2.0).abs() < 1e-12);
    }
}
