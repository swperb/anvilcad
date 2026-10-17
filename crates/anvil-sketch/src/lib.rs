//! AnvilCAD 2D sketcher.
//!
//! A [`Sketch`] holds points, entities (lines, circles, arcs) and constraints. Calling
//! [`Sketch::solve`] moves the geometry to satisfy the constraints, and
//! [`Sketch::profiles`] extracts closed regions for the kernel to extrude or revolve.

pub mod dimension;
pub mod regions;
pub mod sketch;
pub mod solver;

pub use dimension::Pick;
pub use regions::ProfileError;
pub use sketch::{Constraint, DimAnnotation, Entity, EntityId, Geom, Point, PointId, Sketch, ORIGIN};
pub use solver::SolveResult;

#[cfg(test)]
mod tests {
    use super::*;
    use anvil_kernel::v2;
    use std::f64::consts::PI;

    #[test]
    fn rectangle_fully_defined() {
        let mut s = Sketch::new();
        let l = s.add_rectangle(v2(1.0, 1.0), v2(30.0, 18.0));
        // Pin the lower-left corner to the origin and dimension both sides.
        let corner = match s.entities[l[0]].geom {
            Geom::Line { a, .. } => a,
            _ => unreachable!(),
        };
        s.add_constraint(Constraint::Coincident { p: corner, q: ORIGIN });
        s.add_constraint(Constraint::Length { line: l[0], value: 40.0 });
        s.add_constraint(Constraint::Length { line: l[1], value: 25.0 });
        let r = s.solve();
        assert!(r.converged, "{r:?}");
        assert_eq!(r.dof, 0, "{r:?}");
        assert!((s.line_length(l[0]) - 40.0).abs() < 1e-7);
        assert!((s.line_length(l[1]) - 25.0).abs() < 1e-7);
        let profs = s.profiles().unwrap();
        assert_eq!(profs.len(), 1);
        assert!((profs[0].area() - 1000.0).abs() < 1e-5);
    }

    #[test]
    fn under_defined_dof_count() {
        let mut s = Sketch::new();
        let a = s.add_point(v2(0.0, 5.0));
        let b = s.add_point(v2(10.0, 6.0));
        s.add_line(a, b);
        let r = s.solve();
        assert!(r.converged);
        assert_eq!(r.dof, 4);
        s.add_constraint(Constraint::Horizontal { line: 0 });
        assert_eq!(s.solve().dof, 3);
    }

    #[test]
    fn conflicting_constraints_reported() {
        let mut s = Sketch::new();
        let a = s.add_point(v2(0.0, 0.0));
        let b = s.add_point(v2(10.0, 0.0));
        let l = s.add_line(a, b);
        s.add_constraint(Constraint::Fix { p: a, x: 0.0, y: 0.0 });
        s.add_constraint(Constraint::Fix { p: b, x: 10.0, y: 0.0 });
        s.add_constraint(Constraint::Length { line: l, value: 20.0 });
        let r = s.solve();
        assert!(!r.converged);
        assert!(!r.unsatisfied.is_empty());
    }

    #[test]
    fn plate_with_hole_profile_and_dimension_change() {
        let mut s = Sketch::new();
        let l = s.add_rectangle(v2(-20.0, -10.0), v2(20.0, 10.0));
        let c = s.add_point(v2(0.0, 0.0));
        let circle = s.add_circle(c, 4.0);
        s.add_constraint(Constraint::Coincident { p: c, q: ORIGIN });
        let d = s.add_constraint(Constraint::Diameter { entity: circle, value: 8.0 });
        s.add_constraint(Constraint::Length { line: l[0], value: 40.0 });
        assert!(s.solve().converged);
        let p = s.profiles().unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].holes.len(), 1);
        let a0 = p[0].area();
        assert!((a0 - (800.0 - PI * 16.0)).abs() < 1e-5, "{a0}");

        s.constraints[d].set_value(10.0);
        assert!(s.solve().converged);
        assert!((s.radius(circle) - 5.0).abs() < 1e-7);
    }

    #[test]
    fn slot_with_arcs() {
        // Obround: two lines joined by two half arcs.
        let mut s = Sketch::new();
        let c1 = s.add_point(v2(0.0, 0.0));
        let c2 = s.add_point(v2(20.0, 0.0));
        let p1 = s.add_point(v2(0.0, -5.0));
        let p2 = s.add_point(v2(20.0, -5.0));
        let p3 = s.add_point(v2(20.0, 5.0));
        let p4 = s.add_point(v2(0.0, 5.0));
        s.add_line(p1, p2);
        s.add_arc(c2, p2, p3);
        s.add_line(p3, p4);
        s.add_arc(c1, p4, p1);
        assert!(s.solve().converged);
        let prof = s.profiles().unwrap();
        assert_eq!(prof.len(), 1);
        let expected = 20.0 * 10.0 + PI * 25.0;
        assert!((prof[0].area() - expected).abs() < 1e-6, "{}", prof[0].area());
    }

    #[test]
    fn open_contour_error() {
        let mut s = Sketch::new();
        let a = s.add_point(v2(0.0, 0.0));
        let b = s.add_point(v2(10.0, 0.0));
        let c = s.add_point(v2(10.0, 10.0));
        s.add_line(a, b);
        s.add_line(b, c);
        assert!(matches!(s.profiles(), Err(ProfileError::OpenContour { .. })));
    }

    #[test]
    fn construction_geometry_ignored() {
        let mut s = Sketch::new();
        s.add_rectangle(v2(0.0, 0.0), v2(10.0, 10.0));
        let a = s.add_point(v2(0.0, 0.0));
        let b = s.add_point(v2(10.0, 10.0));
        let diag = s.add_line(a, b);
        s.entities[diag].construction = true;
        assert_eq!(s.profiles().unwrap().len(), 1);
    }

    #[test]
    fn tangent_and_perpendicular() {
        let mut s = Sketch::new();
        let a = s.add_point(v2(-10.0, 3.0));
        let b = s.add_point(v2(10.0, 4.0));
        let line = s.add_line(a, b);
        let c = s.add_point(v2(0.0, 0.0));
        let circ = s.add_circle(c, 2.0);
        s.add_constraint(Constraint::Coincident { p: c, q: ORIGIN });
        s.add_constraint(Constraint::Tangent { line, entity: circ });
        s.add_constraint(Constraint::Radius { entity: circ, value: 5.0 });
        let r = s.solve();
        assert!(r.converged, "{r:?}");
        let (pa, pb) = (s.pos(a), s.pos(b));
        let d = (pb - pa).cross(s.pos(c) - pa).abs() / pa.distance(pb);
        assert!((d - 5.0).abs() < 1e-7);
    }

    #[test]
    fn drag_keeps_constraints() {
        let mut s = Sketch::new();
        let l = s.add_rectangle(v2(0.0, 0.0), v2(10.0, 5.0));
        let corner = match s.entities[l[1]].geom {
            Geom::Line { b, .. } => b,
            _ => unreachable!(),
        };
        let r = s.drag_point(corner, v2(20.0, 8.0));
        assert!(r.converged);
        // Still a rectangle after dragging the top-right corner.
        for (e, horiz) in [(l[0], true), (l[1], false), (l[2], true), (l[3], false)] {
            let Geom::Line { a, b } = s.entities[e].geom else { unreachable!() };
            let d = s.pos(b) - s.pos(a);
            assert!(if horiz { d.y.abs() } else { d.x.abs() } < 1e-7);
        }
    }

    #[test]
    fn delete_entity_remaps() {
        let mut s = Sketch::new();
        let l = s.add_rectangle(v2(0.0, 0.0), v2(10.0, 5.0));
        s.add_constraint(Constraint::Length { line: l[2], value: 10.0 });
        s.delete_entities(&[l[0]]);
        assert_eq!(s.entities.len(), 3);
        // Horizontal on the old bottom line is gone; the length constraint now targets index 1.
        assert!(s.constraints.contains(&Constraint::Length { line: 1, value: 10.0 }));
        assert!(s.solve().converged);
    }

    #[test]
    fn serde_roundtrip() {
        let mut s = Sketch::new();
        s.add_rectangle(v2(0.0, 0.0), v2(10.0, 5.0));
        let json = serde_json::to_string(&s).unwrap();
        let back: Sketch = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }
}
