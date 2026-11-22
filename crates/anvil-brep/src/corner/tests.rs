use super::*;
use crate::blend::{Blend, blend_chain, edge_near};
use crate::{check, extrude, mass, tess};

/// Volume and area, tessellated finely (the chords fall short of the round faces by a
/// few parts in a million).
fn measure(s: &Solid) -> (f64, f64) {
    check::validate(s).expect("valid");
    let m = tess::tessellate(s, &tess::TessParams { chord: 1e-4, max_angle: 0.01 }).expect("tess");
    assert!(m.is_watertight(), "not watertight");
    let p = mass::properties(&m);
    (p.volume, p.area)
}

fn close_all(mut s: Solid, corners: &[Corner]) -> Solid {
    for (i, c) in corners.iter().enumerate() {
        let mut out = close_corner(&s, c, 1, &[100 + 10 * i as u32]).expect("close");
        assert_eq!(out.len(), 1);
        s = out.remove(0);
    }
    s
}

fn near(got: f64, want: f64, rel: f64) -> bool {
    (got - want).abs() <= rel * want
}

fn prism(pts: &[(f64, f64)], z0: f64, z1: f64) -> Solid {
    let k = pts.len();
    let l = Loop2::new(
        (0..k)
            .map(|i| Segment::Line {
                a: DVec2::new(pts[i].0, pts[i].1),
                b: DVec2::new(pts[(i + 1) % k].0, pts[(i + 1) % k].1),
            })
            .collect(),
    );
    let r = build_regions(&[l]).expect("region");
    extrude::extrude(&Frame::world(), &r[0], z0, z1).expect("prism")
}

/// Rounds the edges through `points` one after another, as the blend feature does, and
/// returns the rounded solid with the corners found on the original.
fn round_one_by_one(solid: &Solid, points: &[DVec3], r: f64) -> (Solid, Vec<Corner>) {
    let ids: Vec<EdgeId> = points.iter().map(|p| edge_near(solid, *p, 1e-6).expect("edge")).collect();
    let corners = ball_corners(solid, &ids, r).expect("corners");
    let mut s = solid.clone();
    for p in points {
        let e = edge_near(&s, *p, 1e-6).expect("edge again");
        let mut out = blend_chain(&s, &[(e, 0)], Blend::Fillet(r), 1).expect("round");
        assert_eq!(out.len(), 1);
        s = out.remove(0);
    }
    (s, corners)
}

fn cube_edge_midpoints(s: f64) -> Vec<DVec3> {
    let h = s * 0.5;
    let mut out = Vec::new();
    for a in [0.0, s] {
        for b in [0.0, s] {
            out.push(DVec3::new(h, a, b));
            out.push(DVec3::new(a, h, b));
            out.push(DVec3::new(a, b, h));
        }
    }
    out
}

#[test]
fn box_corners_are_eighths_of_the_ball() {
    // brep-bench's fillet/box-all-edges (#114): a cube of 10, all twelve edges rounded to
    // 1. Volume 8^3 + 6 8^2 r + (pi/4) 96 r^2 + 4 pi r^3 / 3, area 6 8^2 + (pi/2) 96 r
    // + 4 pi r^2. Rounded one edge after another and left at that, it measured 977.68.
    let cube = prism(&[(0., 0.), (10., 0.), (10., 10.), (0., 10.)], 0.0, 10.0);
    let (s, corners) = round_one_by_one(&cube, &cube_edge_midpoints(10.0), 1.0);
    assert_eq!(corners.len(), 8);
    assert!(corners.iter().all(|c| c.convex && c.patches.len() == 1 && c.patches[0].normals.len() == 3));
    let (v0, _) = measure(&s);
    assert!(v0 > 977.6, "{v0}");
    let s = close_all(s, &corners);
    let (v, a) = measure(&s);
    let want_v = 512.0 + 6.0 * 64.0 + PI / 4.0 * 96.0 + 4.0 * PI / 3.0;
    let want_a = 6.0 * 64.0 + PI / 2.0 * 96.0 + 4.0 * PI;
    assert!(near(v, want_v, 5e-6), "{v} vs {want_v}");
    assert!(near(a, want_a, 5e-6), "{a} vs {want_a}");
    assert_eq!(s.faces.iter().filter(|f| matches!(f.surface, Surface::Sphere { .. })).count(), 8);
}

#[test]
fn concave_corner_is_filled_to_the_ball() {
    // brep-bench's fillet/notched-cube-inside-corner: a cube of 20 less the cube from
    // (10, 10, 10) to (20, 20, 20), its three inside edges rounded to 3. Volume
    // 7216 - 51.75 pi, area 2274 + 29.25 pi.
    let cube = prism(&[(0., 0.), (20., 0.), (20., 20.), (0., 20.)], 0.0, 20.0);
    let notch = prism(&[(10., 10.), (30., 10.), (30., 30.), (10., 30.)], 10.0, 30.0);
    let part = boolean::boolean(&cube, &notch, BoolOp::Subtract).expect("notch").remove(0);
    let pts = [DVec3::new(15.0, 10.0, 10.0), DVec3::new(10.0, 15.0, 10.0), DVec3::new(10.0, 10.0, 15.0)];
    let (s, corners) = round_one_by_one(&part, &pts, 3.0);
    assert_eq!(corners.len(), 1, "only the inside corner has all its edges rounded");
    assert!(!corners[0].convex);
    assert!((corners[0].patches[0].centre - DVec3::splat(13.0)).length() < 1e-9);
    let (v, a) = measure(&close_all(s, &corners));
    let (want_v, want_a) = (7216.0 - 51.75 * PI, 2274.0 + 29.25 * PI);
    assert!(near(v, want_v, 5e-6), "{v} vs {want_v}");
    assert!(near(a, want_a, 5e-6), "{a} vs {want_a}");
}

fn octahedron(r: f64) -> Solid {
    let sq = |z: f64| {
        let frame = Frame::new(DVec3::new(0.0, 0.0, z), DVec3::Z, DVec3::X).expect("frame");
        let p = [(r, 0.0), (0.0, r), (-r, 0.0), (0.0, -r)];
        let outline = Loop2::new(
            (0..4)
                .map(|i| Segment::Line {
                    a: DVec2::new(p[i].0, p[i].1),
                    b: DVec2::new(p[(i + 1) % 4].0, p[(i + 1) % 4].1),
                })
                .collect(),
        );
        Section::Loop { frame, outline }
    };
    let up = loft_ruled(&sq(0.0), &Section::Point(DVec3::new(0.0, 0.0, r)), [None, None]).expect("up");
    let down = loft_ruled(&sq(0.0), &Section::Point(DVec3::new(0.0, 0.0, -r)), [None, None]).expect("down");
    boolean::boolean(&up, &down, BoolOp::Union).expect("octahedron").remove(0)
}

#[test]
fn octahedron_corners_are_square_patches() {
    // brep-bench's fillet/octahedron-all-edges: |x| + |y| + |z| <= 10, all twelve edges
    // rounded to 2, four rounds at each corner. The octahedron shrunk by 2 grown by 2
    // (Steiner): 4 s^3 / 3 + 8 sqrt3 s^2 + 24 sqrt2 s acos(1/3) + 32 pi / 3, area
    // 4 sqrt3 s^2 + 24 sqrt2 s acos(1/3) + 16 pi, s = 10 - 2 sqrt3.
    let o = octahedron(10.0);
    let pts: Vec<DVec3> = o.edges.iter().map(|e| e.curve.eval(0.5 * (e.t0 + e.t1))).collect();
    assert_eq!(pts.len(), 12);
    let (s, corners) = round_one_by_one(&o, &pts, 2.0);
    assert_eq!(corners.len(), 6);
    assert!(corners.iter().all(|c| c.patches.len() == 1 && c.patches[0].normals.len() == 4));
    let (v, a) = measure(&close_all(s, &corners));
    let sp = 10.0 - 2.0 * 3f64.sqrt();
    let turn = (1.0f64 / 3.0).acos();
    let want_v = 4.0 * sp.powi(3) / 3.0
        + 8.0 * 3f64.sqrt() * sp * sp
        + 24.0 * 2f64.sqrt() * sp * turn
        + 32.0 * PI / 3.0;
    let want_a = 4.0 * 3f64.sqrt() * sp * sp + 24.0 * 2f64.sqrt() * sp * turn + 16.0 * PI;
    assert!(near(v, want_v, 1e-5), "{v} vs {want_v}");
    assert!(near(a, want_a, 1e-5), "{a} vs {want_a}");
}

fn rect_pyramid() -> Solid {
    let p = [(-10.0, -6.0), (10.0, -6.0), (10.0, 6.0), (-10.0, 6.0)];
    let outline = Loop2::new(
        (0..4)
            .map(|i| Segment::Line {
                a: DVec2::new(p[i].0, p[i].1),
                b: DVec2::new(p[(i + 1) % 4].0, p[(i + 1) % 4].1),
            })
            .collect(),
    );
    loft_ruled(
        &Section::Loop { frame: Frame::world(), outline },
        &Section::Point(DVec3::new(0.0, 0.0, 12.0)),
        [None, None],
    )
    .expect("pyramid")
}

#[test]
fn split_apex_is_a_ridge_between_two_patches() {
    // brep-bench's fillet/rectangular-pyramid-all-edges: a pyramid on the rectangle
    // 20 x 12 with its apex at height 12, all eight edges rounded to 3/2. No ball touches
    // the four faces at the apex: moved in by 3/2, the two long sides meet along the line
    // y = 0, z = 12 - 3 sqrt5 / 2, and the short ends cut it off at x = +-x',
    // x' = (5 sqrt5 - sqrt61) / 4. The apex is a round about that segment with a patch of
    // the ball at each end. The pyramid shrunk by 3/2 grown by 3/2 (Steiner, see the case)
    // has volume 848.690972 and area 527.660727. Rounded one edge after another and left
    // at that, it measured 863.37.
    let pyramid = rect_pyramid();
    let pts: Vec<DVec3> = pyramid.edges.iter().map(|e| e.curve.eval(0.5 * (e.t0 + e.t1))).collect();
    assert_eq!(pts.len(), 8);
    let (s, corners) = round_one_by_one(&pyramid, &pts, 1.5);
    assert_eq!(corners.len(), 5);
    let apex = corners.iter().find(|c| c.ridges.len() == 1).expect("the apex splits");
    assert_eq!(apex.patches.len(), 2);
    let xp = (5.0 * 5f64.sqrt() - 61f64.sqrt()) / 4.0;
    let zr = 12.0 - 1.5 * 5f64.sqrt();
    let ridge = &apex.ridges[0];
    for (end, want) in [(ridge.from, xp), (ridge.to, xp)] {
        assert!(
            (end.x.abs() - want).abs() < 1e-9 && end.y.abs() < 1e-9 && (end.z - zr).abs() < 1e-9,
            "{end}"
        );
    }
    assert!((ridge.from.x + ridge.to.x).abs() < 1e-9, "the ends are either side of the axis");
    assert_eq!(corners.iter().filter(|c| c.ridges.is_empty() && c.patches.len() == 1).count(), 4);
    let (v0, _) = measure(&s);
    assert!(v0 > 863.3, "{v0}");
    let s = close_all(s, &corners);
    let (v, a) = measure(&s);
    assert!(near(v, 848.690_972_160_223_9, 1e-5), "{v}");
    assert!(near(a, 527.660_726_666_457_4, 1e-5), "{a}");
    // A patch at each base corner and at each end of the ridge, each one face, and the
    // ridge's round between the apex's two patches.
    assert_eq!(s.faces.iter().filter(|f| matches!(f.surface, Surface::Sphere { .. })).count(), 6);
    let ridge_faces = s
        .faces
        .iter()
        .filter(|f| matches!(f.surface, Surface::Cylinder { frame, .. } if frame.z.cross(DVec3::X).length() < 1e-9 && frame.origin.y.abs() < 1e-9 && (frame.origin.z - zr).abs() < 1e-9))
        .count();
    assert_eq!(ridge_faces, 1);
}

#[test]
fn corners_with_an_edge_left_sharp_are_not_closed() {
    // Two of a cube corner's three edges rounded: the rounds meet as they always have, and
    // no patch is made.
    let cube = prism(&[(0., 0.), (10., 0.), (10., 10.), (0., 10.)], 0.0, 10.0);
    let ids: Vec<EdgeId> = [DVec3::new(5.0, 0.0, 10.0), DVec3::new(0.0, 5.0, 10.0)]
        .iter()
        .map(|p| edge_near(&cube, *p, 1e-6).expect("edge"))
        .collect();
    assert!(ball_corners(&cube, &ids, 1.0).expect("corners").is_empty());
}

#[test]
fn corner_patches_that_would_overlap_are_refused() {
    // A block 1 thick with all its edges rounded to 0.6: along the short edges the
    // patches at the two ends would need 1.2 of the 1.
    let block = prism(&[(0., 0.), (10., 0.), (10., 10.), (0., 10.)], 0.0, 1.0);
    let ids: Vec<EdgeId> = (0..block.edges.len() as u32).map(EdgeId).collect();
    let err = ball_corners(&block, &ids, 0.6).expect_err("too big");
    assert!(matches!(err, KernelError::InvalidInput(_)), "{err:?}");
    assert!(ball_corners(&block, &ids, 0.4).is_ok());
}

#[test]
fn smooth_joins_leave_the_corner_to_the_rounds() {
    // A plate whose outline has rounded corners, every edge selected: at each vertex a
    // line runs smoothly into an arc and the upright seam between them is no crease, so
    // there is no ball corner and nothing is refused, as before.
    let r = 2.0;
    let h = 0.5 * PI;
    let arc = |cx: f64, cy: f64, start: f64| Segment::Arc {
        center: DVec2::new(cx, cy),
        radius: r,
        start,
        sweep: h,
    };
    let line =
        |a: (f64, f64), b: (f64, f64)| Segment::Line { a: DVec2::new(a.0, a.1), b: DVec2::new(b.0, b.1) };
    let outline = Loop2::new(vec![
        line((r, 0.0), (10.0 - r, 0.0)),
        arc(10.0 - r, r, -h),
        line((10.0, r), (10.0, 10.0 - r)),
        arc(10.0 - r, 10.0 - r, 0.0),
        line((10.0 - r, 10.0), (r, 10.0)),
        arc(r, 10.0 - r, h),
        line((0.0, 10.0 - r), (0.0, r)),
        arc(r, r, PI),
    ]);
    let region = build_regions(&[outline]).expect("region");
    let plate = extrude::extrude(&Frame::world(), &region[0], 0.0, 3.0).expect("plate");
    let ids: Vec<EdgeId> = (0..plate.edges.len() as u32).map(EdgeId).collect();
    assert!(ball_corners(&plate, &ids, 0.5).expect("no refusal").is_empty());
}
