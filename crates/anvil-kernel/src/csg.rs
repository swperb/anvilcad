//! Boolean operations on polygonal solids using BSP trees.
//!
//! The algorithm follows the classic csg.js approach, written iteratively with an arena so
//! deep trees cannot overflow the stack. Face tags travel with every split fragment, so the
//! result still knows which model face each piece came from.

use crate::math::Vec3;
use crate::solid::{Polygon, Solid};

const EPS: f64 = 1e-6;

#[derive(Clone, Copy)]
struct Plane {
    n: Vec3,
    w: f64,
}

impl Plane {
    fn of(p: &Polygon) -> Plane {
        Plane { n: p.normal, w: p.w }
    }
    fn flip(&mut self) {
        self.n = -self.n;
        self.w = -self.w;
    }
}

const COPLANAR: u8 = 0;
const FRONT: u8 = 1;
const BACK: u8 = 2;
const SPANNING: u8 = 3;

/// Splits `poly` by `plane` into the four output buckets.
fn split(
    plane: Plane,
    poly: Polygon,
    coplanar_front: &mut Vec<Polygon>,
    coplanar_back: &mut Vec<Polygon>,
    front: &mut Vec<Polygon>,
    back: &mut Vec<Polygon>,
) {
    let mut kind = 0u8;
    let types: Vec<u8> = poly
        .verts
        .iter()
        .map(|&v| {
            let t = plane.n.dot(v) - plane.w;
            let ty = if t < -EPS {
                BACK
            } else if t > EPS {
                FRONT
            } else {
                COPLANAR
            };
            kind |= ty;
            ty
        })
        .collect();
    match kind {
        COPLANAR => {
            if plane.n.dot(poly.normal) > 0.0 {
                coplanar_front.push(poly)
            } else {
                coplanar_back.push(poly)
            }
        }
        FRONT => front.push(poly),
        BACK => back.push(poly),
        _ => {
            let k = poly.verts.len();
            let mut f = Vec::with_capacity(k + 1);
            let mut b = Vec::with_capacity(k + 1);
            for i in 0..k {
                let j = (i + 1) % k;
                let (ti, tj) = (types[i], types[j]);
                let (vi, vj) = (poly.verts[i], poly.verts[j]);
                if ti != BACK {
                    f.push(vi);
                }
                if ti != FRONT {
                    b.push(vi);
                }
                if (ti | tj) == SPANNING {
                    let t = (plane.w - plane.n.dot(vi)) / plane.n.dot(vj - vi);
                    let v = vi.lerp(vj, t);
                    f.push(v);
                    b.push(v);
                }
            }
            // Fragments keep the parent's exact plane to avoid drift.
            if f.len() >= 3 {
                front.push(Polygon { verts: f, normal: poly.normal, w: poly.w, tag: poly.tag });
            }
            if b.len() >= 3 {
                back.push(Polygon { verts: b, normal: poly.normal, w: poly.w, tag: poly.tag });
            }
        }
    }
}

#[derive(Default)]
struct Node {
    plane: Option<Plane>,
    front: Option<usize>,
    back: Option<usize>,
    polygons: Vec<Polygon>,
}

#[derive(Default)]
struct Bsp {
    nodes: Vec<Node>,
}

impl Bsp {
    fn new(polys: Vec<Polygon>) -> Bsp {
        let mut b = Bsp { nodes: vec![Node::default()] };
        b.build(polys);
        b
    }

    fn build(&mut self, polys: Vec<Polygon>) {
        if polys.is_empty() {
            return;
        }
        let mut work = vec![(0usize, polys)];
        while let Some((ni, polys)) = work.pop() {
            if polys.is_empty() {
                continue;
            }
            let plane = *self.nodes[ni].plane.get_or_insert_with(|| Plane::of(&polys[0]));
            let (mut cf, mut cb, mut f, mut b) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            for p in polys {
                split(plane, p, &mut cf, &mut cb, &mut f, &mut b);
            }
            self.nodes[ni].polygons.extend(cf);
            self.nodes[ni].polygons.extend(cb);
            if !f.is_empty() {
                let child = self.child(ni, true);
                work.push((child, f));
            }
            if !b.is_empty() {
                let child = self.child(ni, false);
                work.push((child, b));
            }
        }
    }

    fn child(&mut self, ni: usize, front: bool) -> usize {
        let existing = if front { self.nodes[ni].front } else { self.nodes[ni].back };
        existing.unwrap_or_else(|| {
            self.nodes.push(Node::default());
            let c = self.nodes.len() - 1;
            if front {
                self.nodes[ni].front = Some(c);
            } else {
                self.nodes[ni].back = Some(c);
            }
            c
        })
    }

    fn invert(&mut self) {
        for n in &mut self.nodes {
            for p in &mut n.polygons {
                p.flip();
            }
            if let Some(pl) = &mut n.plane {
                pl.flip();
            }
            std::mem::swap(&mut n.front, &mut n.back);
        }
    }

    /// Removes the parts of `polys` that lie inside this BSP's solid.
    fn clip_polygons(&self, polys: Vec<Polygon>) -> Vec<Polygon> {
        let mut out = Vec::new();
        let mut work = vec![(0usize, polys)];
        while let Some((ni, polys)) = work.pop() {
            let node = &self.nodes[ni];
            let Some(plane) = node.plane else {
                out.extend(polys);
                continue;
            };
            let (mut f, mut b) = (Vec::new(), Vec::new());
            let (mut cf, mut cb) = (Vec::new(), Vec::new());
            for p in polys {
                split(plane, p, &mut cf, &mut cb, &mut f, &mut b);
            }
            f.extend(cf);
            b.extend(cb);
            match node.front {
                Some(c) => work.push((c, f)),
                None => out.extend(f),
            }
            if let Some(c) = node.back {
                work.push((c, b));
            }
            // Polygons behind a leaf are inside the solid and are discarded.
        }
        out
    }

    fn clip_to(&mut self, other: &Bsp) {
        for i in 0..self.nodes.len() {
            let polys = std::mem::take(&mut self.nodes[i].polygons);
            self.nodes[i].polygons = other.clip_polygons(polys);
        }
    }

    fn all_polygons(&self) -> Vec<Polygon> {
        self.nodes.iter().flat_map(|n| n.polygons.iter().cloned()).collect()
    }
}

impl Solid {
    pub fn union(&self, other: &Solid) -> Solid {
        if self.is_empty() {
            return other.clone();
        }
        if other.is_empty() || !overlaps(self, other) {
            let mut polys = self.polygons.clone();
            polys.extend(other.polygons.iter().cloned());
            return Solid::new(polys);
        }
        let mut a = Bsp::new(self.polygons.clone());
        let mut b = Bsp::new(other.polygons.clone());
        a.clip_to(&b);
        b.clip_to(&a);
        b.invert();
        b.clip_to(&a);
        b.invert();
        a.build(b.all_polygons());
        finish(a.all_polygons())
    }

    pub fn subtract(&self, other: &Solid) -> Solid {
        if self.is_empty() || other.is_empty() || !overlaps(self, other) {
            return self.clone();
        }
        let mut a = Bsp::new(self.polygons.clone());
        let mut b = Bsp::new(other.polygons.clone());
        a.invert();
        a.clip_to(&b);
        b.clip_to(&a);
        b.invert();
        b.clip_to(&a);
        b.invert();
        a.build(b.all_polygons());
        a.invert();
        finish(a.all_polygons())
    }

    pub fn intersect(&self, other: &Solid) -> Solid {
        if self.is_empty() || other.is_empty() || !overlaps(self, other) {
            return Solid::default();
        }
        let mut a = Bsp::new(self.polygons.clone());
        let mut b = Bsp::new(other.polygons.clone());
        a.invert();
        b.clip_to(&a);
        b.invert();
        a.clip_to(&b);
        b.clip_to(&a);
        a.build(b.all_polygons());
        a.invert();
        finish(a.all_polygons())
    }
}

fn overlaps(a: &Solid, b: &Solid) -> bool {
    let (ba, bb) = (a.bounds(), b.bounds());
    ba.min.x <= bb.max.x + EPS
        && bb.min.x <= ba.max.x + EPS
        && ba.min.y <= bb.max.y + EPS
        && bb.min.y <= ba.max.y + EPS
        && ba.min.z <= bb.max.z + EPS
        && bb.min.z <= ba.max.z + EPS
}

fn finish(polys: Vec<Polygon>) -> Solid {
    let mut s = Solid::new(polys);
    s.repair_t_junctions();
    s
}
