use super::*;
use crate::blend::{Blend, blend_edge, edge_near};
use crate::boolean::{BoolOp, boolean};
use crate::extrude::extrude;
use crate::profile::{Loop2, Segment, build_regions};
use crate::shell::shell;
use crate::topo::FaceTag;
use crate::{mass, tess};
use glam::DVec2;
use std::f64::consts::PI;

fn prism(loops: &[Loop2], z0: f64, z1: f64) -> Solid {
    let r = build_regions(loops).expect("region");
    extrude(&Frame::world(), &r[0], z0, z1).expect("prism")
}

fn polygon(pts: &[(f64, f64)]) -> Loop2 {
    let p: Vec<DVec2> = pts.iter().map(|&(x, y)| DVec2::new(x, y)).collect();
    Loop2::new((0..p.len()).map(|i| Segment::Line { a: p[i], b: p[(i + 1) % p.len()] }).collect())
}

fn square(s: f64) -> Loop2 {
    polygon(&[(0.0, 0.0), (s, 0.0), (s, s), (0.0, s)])
}

fn volume(solids: &[Solid]) -> f64 {
    solids
        .iter()
        .map(|s| {
            check::validate(s).expect("valid");
            let m = tess::tessellate(s, &tess::TessParams { chord: 1e-4, max_angle: 0.01 }).expect("tess");
            assert!(m.is_watertight(), "not watertight");
            mass::properties(&m).volume
        })
        .sum()
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1.0)
}

fn top(s: &Solid) -> usize {
    s.faces.iter().position(|f| f.tag == FaceTag::EndCap).expect("top")
}

#[test]
fn open_box() {
    // A 10 mm cube, open at the top, walls 1 thick: the cavity is 8 x 8 x 9.
    let cube = prism(&[square(10.0)], 0.0, 10.0);
    let s = shell(&cube, &[top(&cube)], 1.0).expect("shell");
    assert_eq!(s.len(), 1);
    assert!(close(volume(&s), 1000.0 - 576.0, 1e-9), "{}", volume(&s));
    // Open at the top and one side.
    let side =
        cube.faces.iter().position(|f| matches!(f.tag, FaceTag::Side { segment: 0, .. })).expect("side");
    let s = shell(&cube, &[top(&cube), side], 1.0).expect("shell");
    assert!(close(volume(&s), 1000.0 - 8.0 * 9.0 * 9.0, 1e-9), "{}", volume(&s));
}

#[test]
fn open_cup_and_tube_with_a_hole() {
    // A cylinder of radius 5, 10 tall, open at the top: walls of 1 leave 250 pi - 144 pi.
    let cyl = prism(&[Loop2::new(vec![Segment::circle(DVec2::ZERO, 5.0)])], 0.0, 10.0);
    let s = shell(&cyl, &[top(&cyl)], 1.0).expect("shell");
    assert!(close(volume(&s), 106.0 * PI, 5e-5), "{}", volume(&s));
    // A block with a hole of radius 2 through it: the hole's wall thickens outwards.
    let block = prism(&[square(10.0)], 0.0, 10.0);
    let hole = prism(&[Loop2::new(vec![Segment::circle(DVec2::new(5.0, 5.0), 2.0)])], -1.0, 11.0);
    let holed = boolean(&block, &hole, BoolOp::Subtract).expect("hole").remove(0);
    let s = shell(&holed, &[top(&holed)], 1.0).expect("shell");
    let want = (100.0 - 4.0 * PI) * 10.0 - (64.0 - 9.0 * PI) * 9.0;
    assert!(close(volume(&s), want, 1e-5), "{} vs {want}", volume(&s));
}

#[test]
fn rounds_stay_concentric() {
    // A square prism with one upright edge rounded (radius 2): the inside of the shell
    // has a round of radius 1 at that corner, tangent to its neighbours like the outside.
    let p = prism(&[square(10.0)], 0.0, 10.0);
    let e = edge_near(&p, DVec3::new(10.0, 10.0, 5.0), 1e-6).expect("edge");
    let r = blend_edge(&p, e, Blend::Fillet(2.0), 1, 0).expect("fillet").remove(0);
    let s = shell(&r, &[top(&r)], 1.0).expect("shell");
    let want = (100.0 - (4.0 - PI)) * 10.0 - (64.0 - (1.0 - PI / 4.0)) * 9.0;
    assert!(close(volume(&s), want, 1e-5), "{} vs {want}", volume(&s));
}

#[test]
fn spherical_bowl() {
    // A hemisphere of radius 5 on its flat face, opened there: a bowl 1 thick.
    use crate::revolve::revolve;
    let half = Loop2::new(vec![
        Segment::Line { a: DVec2::ZERO, b: DVec2::new(5.0, 0.0) },
        Segment::Arc { center: DVec2::ZERO, radius: 5.0, start: 0.0, sweep: 0.5 * PI },
        Segment::Line { a: DVec2::new(0.0, 5.0), b: DVec2::ZERO },
    ]);
    let r = build_regions(&[half]).expect("region");
    let dome = revolve(&Frame::world(), &r[0], DVec2::ZERO, DVec2::Y, 2.0 * PI).expect("dome");
    let flat = dome.faces.iter().position(|f| matches!(f.surface, Surface::Plane { .. })).expect("flat");
    let s = shell(&dome, &[flat], 1.0).expect("shell");
    let want = 2.0 / 3.0 * PI * (125.0 - 64.0);
    assert!(close(volume(&s), want, 5e-5), "{} vs {want}", volume(&s));
}

#[test]
fn walls_too_thick_are_refused() {
    let cube = prism(&[square(10.0)], 0.0, 10.0);
    assert!(shell(&cube, &[top(&cube)], 6.0).is_err());
    let cyl = prism(&[Loop2::new(vec![Segment::circle(DVec2::ZERO, 1.0)])], 0.0, 10.0);
    assert!(shell(&cyl, &[top(&cyl)], 2.0).is_err());
    // A round smaller than the wall on its convex side.
    let p = prism(&[square(10.0)], 0.0, 10.0);
    let e = edge_near(&p, DVec3::new(10.0, 10.0, 5.0), 1e-6).expect("edge");
    let r = blend_edge(&p, e, Blend::Fillet(0.5), 1, 0).expect("fillet").remove(0);
    assert!(shell(&r, &[top(&r)], 1.0).is_err());
    // No face removed: a sealed hollow, not built exactly.
    assert!(matches!(shell(&cube, &[], 1.0), Err(KernelError::Unsupported(_))));
}

#[test]
fn revolved_cup_with_a_rounded_bottom() {
    // A cup turned from a profile 5 wide and 10 tall whose outer bottom corner is
    // rounded with radius 2 (a torus between the flat bottom and the cylinder). Opened
    // at the top with walls 1 thick, the inside has a round of radius 1 on the same
    // centre. Pappus: 2 pi times the difference of the profiles' first moments.
    use crate::revolve::revolve;
    let profile = Loop2::new(vec![
        Segment::Line { a: DVec2::ZERO, b: DVec2::new(3.0, 0.0) },
        Segment::Arc { center: DVec2::new(3.0, 2.0), radius: 2.0, start: -0.5 * PI, sweep: 0.5 * PI },
        Segment::Line { a: DVec2::new(5.0, 2.0), b: DVec2::new(5.0, 10.0) },
        Segment::Line { a: DVec2::new(5.0, 10.0), b: DVec2::new(0.0, 10.0) },
        Segment::Line { a: DVec2::new(0.0, 10.0), b: DVec2::ZERO },
    ]);
    let r = build_regions(&[profile]).expect("region");
    let cup = revolve(&Frame::world(), &r[0], DVec2::ZERO, DVec2::Y, 2.0 * PI).expect("cup");
    let rim = cup
        .faces
        .iter()
        .position(|f| matches!(f.surface, Surface::Plane { frame } if (frame.origin.y - 10.0).abs() < 1e-9))
        .expect("top");
    let s = shell(&cup, &[rim], 1.0).expect("shell");
    // A corner of a square of side f, less the quarter disc, has area f^2 (1 - pi/4) and
    // its centroid f (10 - 3 pi) / (3 (4 - pi)) in from the corner.
    let corner = |f: f64| f * f * (1.0 - PI / 4.0);
    let inset = |f: f64| f * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
    let outer = 10.0 * 25.0 / 2.0 - corner(2.0) * (5.0 - inset(2.0));
    let inner = 9.0 * 16.0 / 2.0 - corner(1.0) * (4.0 - inset(1.0));
    let want = 2.0 * PI * (outer - inner);
    assert!(s[0].faces.iter().filter(|f| matches!(f.surface, Surface::Torus { .. })).count() == 2);
    assert!(close(volume(&s), want, 5e-5), "{} vs {want}", volume(&s));
}
