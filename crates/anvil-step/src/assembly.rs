//! Assembly structure: which bodies each product occurrence uses, and where it sits.
//!
//! The product tree comes from NEXT_ASSEMBLY_USAGE_OCCURRENCE (NAUO): each one says a
//! parent PRODUCT_DEFINITION uses a child PRODUCT_DEFINITION once. A product's shape is
//! the representation named by a SHAPE_DEFINITION_REPRESENTATION on its
//! PRODUCT_DEFINITION_SHAPE, together with every representation joined to it by a
//! SHAPE_REPRESENTATION_RELATIONSHIP without a transformation (the same coordinate
//! space). Geometry bodies are items of those representations.
//!
//! An occurrence is placed in one of two ways (ISO 10303-42/-43, AP203/AP214):
//!
//! - A CONTEXT_DEPENDENT_SHAPE_REPRESENTATION on the NAUO's PRODUCT_DEFINITION_SHAPE,
//!   whose relationship (a complex SHAPE_REPRESENTATION_RELATIONSHIP with
//!   REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION) carries an
//!   ITEM_DEFINED_TRANSFORMATION from one AXIS2_PLACEMENT_3D to another. The
//!   transformation takes the frame that lives in the child's representation onto the
//!   frame that lives in the parent's: a child point `p` lands at `to(from⁻¹(p))`.
//!   `rep_1` is normally the child and `transform_item_1` the child's frame, but some
//!   exporters swap them, so the roles are decided by which representation and item
//!   belong to the child's shape.
//! - A MAPPED_ITEM in the parent's representation, whose REPRESENTATION_MAP names the
//!   child's representation and a mapping origin, and whose mapping target is the frame
//!   in the parent. The k-th NAUO of a child under a parent takes the k-th such item.
//!
//! Placements compose down nested sub-assemblies to the root. Anything that cannot be
//! read is reported as a warning and placed at the identity instead.

use crate::p21::{Entity, Exchange, Param};
use crate::reader;
use anvil_brep::Frame;
use glam::DVec3;
use std::collections::{HashMap, HashSet};

/// Sub-assemblies nested deeper than this are assumed to be a cycle.
const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone)]
pub struct Node {
    /// The instance name.
    pub name: String,
    pub product: String,
    pub parent: Option<usize>,
    /// Where the occurrence sits in the root's coordinates (mm).
    pub place: Frame,
}

/// One use of a geometry body (by entity id) by an occurrence, already placed.
#[derive(Debug, Clone)]
pub struct Instance {
    pub node: usize,
    pub item: u64,
    pub place: Frame,
}

#[derive(Debug, Clone, Default)]
pub struct Structure {
    pub nodes: Vec<Node>,
    pub instances: Vec<Instance>,
    pub warnings: Vec<String>,
}

/// The inverse of a rigid motion given as an orthonormal frame.
fn inverse(f: &Frame) -> Frame {
    let x = DVec3::new(f.x.x, f.y.x, f.z.x);
    let y = DVec3::new(f.x.y, f.y.y, f.z.y);
    let z = DVec3::new(f.x.z, f.y.z, f.z.z);
    let origin = -(x * f.origin.x + y * f.origin.y + z * f.origin.z);
    Frame { origin, x, y, z }
}

/// The motion taking frame `from` onto frame `to`.
fn between(from: &Frame, to: &Frame) -> Frame {
    inverse(from).moved(to)
}

fn product_definition(e: &Entity) -> bool {
    e.has("PRODUCT_DEFINITION") || e.name().starts_with("PRODUCT_DEFINITION_WITH")
}

/// The (rep_1, rep_2) of a representation relationship, simple or complex.
fn related_reps(e: &Entity) -> Option<(u64, u64)> {
    let p = e.part("REPRESENTATION_RELATIONSHIP").unwrap_or_else(|| e.params());
    Some((p.get(2)?.as_ref()?, p.get(3)?.as_ref()?))
}

fn rep_items(e: &Entity) -> &[Param] {
    let p = e.part("REPRESENTATION").unwrap_or_else(|| e.params());
    p.get(1).and_then(Param::as_list).unwrap_or(&[])
}

struct Walker<'a> {
    ex: &'a Exchange,
    scale: f64,
    /// Geometry body entity ids the importer reads.
    geometry: &'a HashSet<u64>,
    /// Product definition -> its shape representations.
    pd_reps: HashMap<u64, Vec<u64>>,
    /// Representations sharing one coordinate space.
    same_space: HashMap<u64, Vec<u64>>,
    /// Parent product definition -> its NAUOs, in id order.
    children: HashMap<u64, Vec<u64>>,
    /// NAUO -> the representation relationship that places it.
    placement_of: HashMap<u64, u64>,
    product_uses: HashMap<String, usize>,
    out: Structure,
}

impl Walker<'_> {
    fn warn(&mut self, msg: String) {
        if self.out.warnings.len() < 200 && !self.out.warnings.contains(&msg) {
            self.out.warnings.push(msg);
        }
    }

    fn frame(&self, id: u64) -> Result<Frame, String> {
        let e = self.ex.get(id).ok_or_else(|| format!("#{id} is referenced but not defined"))?;
        if e.name() != "AXIS2_PLACEMENT_3D" {
            return Err(format!("#{id} is {} where an AXIS2_PLACEMENT_3D was expected", e.name()));
        }
        reader::frame(self.ex, self.scale, id)
    }

    /// Every representation in the same coordinate space as `seeds`, in id order.
    fn closure(&self, seeds: &[u64]) -> Vec<u64> {
        let mut seen: HashSet<u64> = seeds.iter().copied().collect();
        let mut stack: Vec<u64> = seeds.to_vec();
        while let Some(r) = stack.pop() {
            for &n in self.same_space.get(&r).map(Vec::as_slice).unwrap_or(&[]) {
                if seen.insert(n) {
                    stack.push(n);
                }
            }
        }
        let mut out: Vec<u64> = seen.into_iter().collect();
        out.sort_unstable();
        out
    }

    fn items(&self, reps: &[u64]) -> Vec<u64> {
        reps.iter()
            .filter_map(|&r| self.ex.get(r))
            .flat_map(|e| rep_items(e).iter().filter_map(Param::as_ref))
            .collect()
    }

    fn product_name(&self, pd: u64) -> String {
        let product = self
            .ex
            .get(pd)
            .and_then(|e| e.params().get(2)?.as_ref())
            .and_then(|f| self.ex.get(f)?.params().get(2)?.as_ref())
            .and_then(|p| self.ex.get(p));
        let text = |i: usize| product.and_then(|p| p.params().get(i)?.as_str()).unwrap_or("").trim();
        if !text(1).is_empty() {
            text(1).to_string()
        } else if !text(0).is_empty() {
            text(0).to_string()
        } else {
            format!("Product #{pd}")
        }
    }

    /// The motion a representation relationship applies to the child whose shape is
    /// `child` (a closure).
    fn relationship_motion(&self, rel: u64, child: &[u64]) -> Result<Frame, String> {
        let e = self.ex.get(rel).ok_or_else(|| format!("#{rel} is referenced but not defined"))?;
        let (r1, r2) = related_reps(e).ok_or_else(|| format!("#{rel} names no representations"))?;
        let Some(op) =
            e.part("REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION").and_then(|p| p.first()?.as_ref())
        else {
            // A relationship without a transformation shares the parent's coordinates.
            return Ok(Frame::world());
        };
        let t = self.ex.get(op).ok_or_else(|| format!("#{op} is referenced but not defined"))?;
        if t.name() != "ITEM_DEFINED_TRANSFORMATION" {
            return Err(format!("{} transformations are not supported", t.name()));
        }
        let p = t.params();
        let i1 = p.get(2).and_then(Param::as_ref).ok_or("transformation without transform_item_1")?;
        let i2 = p.get(3).and_then(Param::as_ref).ok_or("transformation without transform_item_2")?;
        let (f1, f2) = (self.frame(i1)?, self.frame(i2)?);
        let child_items = self.items(child);
        let (c1, c2) = (child_items.contains(&i1), child_items.contains(&i2));
        let item_1_is_child = if c1 != c2 { c1 } else { child.contains(&r1) || !child.contains(&r2) };
        Ok(if item_1_is_child { between(&f1, &f2) } else { between(&f2, &f1) })
    }

    /// The motion of a MAPPED_ITEM and the representation it maps.
    fn mapped_item(&self, id: u64) -> Result<(u64, Frame), String> {
        let e = self.ex.get(id).ok_or_else(|| format!("#{id} is referenced but not defined"))?;
        let p = e.params();
        let map_id = p.get(1).and_then(Param::as_ref).ok_or("mapped item without a mapping source")?;
        let target = p.get(2).and_then(Param::as_ref).ok_or("mapped item without a mapping target")?;
        let map = self.ex.get(map_id).ok_or_else(|| format!("#{map_id} is referenced but not defined"))?;
        let origin =
            map.params().first().and_then(Param::as_ref).ok_or("representation map without an origin")?;
        let rep = map
            .params()
            .get(1)
            .and_then(Param::as_ref)
            .ok_or("representation map without a representation")?;
        Ok((rep, between(&self.frame(origin)?, &self.frame(target)?)))
    }

    fn add_node(&mut self, name: String, product: String, parent: Option<usize>, place: Frame) -> usize {
        self.out.nodes.push(Node { name, product, parent, place });
        self.out.nodes.len() - 1
    }

    /// The name of the next instance of `product`.
    fn instance_name(&mut self, product: &str) -> String {
        let n = self.product_uses.entry(product.to_string()).or_insert(0);
        *n += 1;
        format!("{product} ({n})")
    }

    /// Places the bodies of `shape` (a closure) and walks the children of `pd`.
    fn visit(&mut self, node: usize, pd: Option<u64>, shape: &[u64], world: Frame, path: &mut Vec<u64>) {
        if path.len() > MAX_DEPTH {
            self.warn(
                "the assembly nests too deeply (or refers to itself); deeper levels are left out".into(),
            );
            return;
        }
        let items = self.items(shape);
        for &item in &items {
            if self.geometry.contains(&item) {
                self.out.instances.push(Instance { node, item, place: world });
            }
        }
        let mut mapped: Vec<(u64, u64, Frame)> = Vec::new();
        for &item in &items {
            if self.ex.get(item).is_some_and(|e| e.name() == "MAPPED_ITEM") {
                match self.mapped_item(item) {
                    Ok((rep, motion)) => mapped.push((item, rep, motion)),
                    Err(why) => self.warn(format!("mapped item #{item} skipped: {why}")),
                }
            }
        }
        let mut used = vec![false; mapped.len()];
        let nauos = pd.and_then(|pd| self.children.get(&pd)).cloned().unwrap_or_default();
        let mut child_shapes: Vec<u64> = Vec::new();
        for nauo in nauos {
            let Some(e) = self.ex.get(nauo) else { continue };
            let Some(child) = e.params().get(4).and_then(Param::as_ref) else {
                self.warn(format!("assembly usage #{nauo} names no child"));
                continue;
            };
            if path.contains(&child) {
                self.warn(format!("assembly usage #{nauo} makes a product contain itself; it is left out"));
                continue;
            }
            let seeds = self.pd_reps.get(&child).cloned().unwrap_or_default();
            let child_shape = self.closure(&seeds);
            child_shapes.extend(&child_shape);
            let product = self.product_name(child);
            let given = e.params().get(1).and_then(Param::as_str).unwrap_or("").trim().to_string();
            let name = if given.is_empty() { self.instance_name(&product) } else { given };
            let motion = if let Some(&rel) = self.placement_of.get(&nauo) {
                self.relationship_motion(rel, &child_shape)
            } else if let Some(k) =
                (0..mapped.len()).find(|&k| !used[k] && child_shape.contains(&mapped[k].1))
            {
                used[k] = true;
                Ok(mapped[k].2)
            } else {
                Err("the file gives no placement for it".to_string())
            };
            let motion = motion.unwrap_or_else(|why| {
                self.warn(format!(
                    "occurrence '{name}': its placement could not be read ({why}); it is imported in its part's own coordinates"
                ));
                Frame::world()
            });
            let place = motion.moved(&world);
            let id = self.add_node(name, product, Some(node), place);
            path.push(child);
            self.visit(id, Some(child), &child_shape, place, path);
            path.pop();
        }
        // Mapped items no NAUO accounts for are occurrences of their own, unless they
        // map a shape a NAUO already placed (some files carry both forms).
        for (k, (item, rep, motion)) in mapped.into_iter().enumerate() {
            if used[k] || child_shapes.contains(&rep) {
                continue;
            }
            let shape = self.closure(&[rep]);
            let child = self
                .pd_reps
                .iter()
                .filter(|(_, reps)| reps.iter().any(|r| shape.contains(r)))
                .map(|(pd, _)| *pd)
                .min();
            if child.is_some_and(|c| path.contains(&c)) || shape.iter().any(|r| path.contains(r)) {
                self.warn(format!("mapped item #{item} makes a shape contain itself; it is left out"));
                continue;
            }
            let product = match child {
                Some(c) => self.product_name(c),
                None => self
                    .ex
                    .get(rep)
                    .map(reader_name)
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| format!("Shape #{rep}")),
            };
            let given = self.ex.get(item).map(reader_name).unwrap_or_default();
            let name = if given.is_empty() { self.instance_name(&product) } else { given };
            let place = motion.moved(&world);
            let id = self.add_node(name, product, Some(node), place);
            // Representations go on the path too, so mapped items cannot loop.
            path.push(rep);
            self.visit(id, child, &shape, place, path);
            path.pop();
        }
    }
}

fn reader_name(e: &Entity) -> String {
    e.params().first().and_then(Param::as_str).unwrap_or("").trim().to_string()
}

/// Reads the assembly structure, or `None` when the file has none (no NAUO and no
/// MAPPED_ITEM). `geometry` holds the entity ids of the bodies the importer reads.
pub fn read(ex: &Exchange, scale: f64, geometry: &HashSet<u64>) -> Option<Structure> {
    let has_nauo = ex.of_type("NEXT_ASSEMBLY_USAGE_OCCURRENCE").next().is_some();
    if !has_nauo && ex.of_type("MAPPED_ITEM").next().is_none() {
        return None;
    }
    let mut w = Walker {
        ex,
        scale,
        geometry,
        pd_reps: HashMap::new(),
        same_space: HashMap::new(),
        children: HashMap::new(),
        placement_of: HashMap::new(),
        product_uses: HashMap::new(),
        out: Structure::default(),
    };
    // The PRODUCT_DEFINITION_SHAPE -> what it describes.
    let shape_of = |pds: u64| -> Option<u64> {
        let e = ex.get(pds)?;
        e.params().get(2)?.as_ref()
    };
    for id in ex.of_type("SHAPE_DEFINITION_REPRESENTATION") {
        let Some(p) = ex.get(id).map(Entity::params) else { continue };
        let (Some(def), Some(rep)) = (p.first().and_then(Param::as_ref), p.get(1).and_then(Param::as_ref))
        else {
            continue;
        };
        if let Some(pd) = shape_of(def).filter(|pd| ex.get(*pd).is_some_and(product_definition)) {
            w.pd_reps.entry(pd).or_default().push(rep);
        }
    }
    let mut placing: HashSet<u64> = HashSet::new();
    for id in ex.of_type("CONTEXT_DEPENDENT_SHAPE_REPRESENTATION") {
        let Some(p) = ex.get(id).map(Entity::params) else { continue };
        let (Some(rel), Some(pds)) = (p.first().and_then(Param::as_ref), p.get(1).and_then(Param::as_ref))
        else {
            continue;
        };
        placing.insert(rel);
        if let Some(nauo) = shape_of(pds) {
            w.placement_of.entry(nauo).or_insert(rel);
        }
    }
    let mut rels: Vec<u64> = ex
        .of_type("SHAPE_REPRESENTATION_RELATIONSHIP")
        .chain(ex.of_type("REPRESENTATION_RELATIONSHIP"))
        .collect();
    rels.sort_unstable();
    rels.dedup();
    for id in rels {
        let Some(e) = ex.get(id) else { continue };
        if placing.contains(&id) || e.has("REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION") {
            continue;
        }
        if let Some((a, b)) = related_reps(e) {
            w.same_space.entry(a).or_default().push(b);
            w.same_space.entry(b).or_default().push(a);
        }
    }
    let mut used: HashSet<u64> = HashSet::new();
    for id in ex.of_type("NEXT_ASSEMBLY_USAGE_OCCURRENCE") {
        let Some(p) = ex.get(id).map(Entity::params) else { continue };
        let (Some(parent), Some(child)) =
            (p.get(3).and_then(Param::as_ref), p.get(4).and_then(Param::as_ref))
        else {
            continue;
        };
        w.children.entry(parent).or_default().push(id);
        used.insert(child);
    }
    let mut roots: Vec<u64> = ex
        .entities
        .iter()
        .filter(|(id, e)| product_definition(e) && !used.contains(id))
        .map(|(id, _)| *id)
        .filter(|id| w.pd_reps.contains_key(id) || w.children.contains_key(id))
        .collect();
    roots.sort_unstable();
    for pd in roots {
        let seeds = w.pd_reps.get(&pd).cloned().unwrap_or_default();
        let shape = w.closure(&seeds);
        let product = w.product_name(pd);
        let node = w.add_node(product.clone(), product, None, Frame::world());
        w.visit(node, Some(pd), &shape, Frame::world(), &mut vec![pd]);
    }
    if !has_nauo && w.out.nodes.iter().all(|n| n.parent.is_none()) {
        // MAPPED_ITEMs that place no geometry (e.g. annotation): not an assembly.
        return None;
    }
    Some(w.out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_undoes_a_motion() {
        let f = Frame::new(DVec3::new(3.0, -2.0, 7.0), DVec3::new(1.0, 1.0, 0.0), DVec3::Z).expect("frame");
        let p = DVec3::new(0.5, 4.0, -1.0);
        let back = inverse(&f).apply_point(f.apply_point(p));
        assert!((back - p).length() < 1e-12);
        let id = between(&f, &f);
        assert!((id.apply_point(p) - p).length() < 1e-12);
    }
}
