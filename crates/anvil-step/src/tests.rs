use super::*;
use anvil_brep::boolean::{BoolOp, boolean};
use anvil_brep::profile::{Loop2, Segment, build_regions};
use anvil_brep::{Frame, check, extrude, mass, tess};
use glam::{DVec2, DVec3};
use std::f64::consts::PI;

fn prism(pts: &[DVec2], frame: &Frame, z0: f64, z1: f64) -> anvil_brep::Solid {
    let l = Loop2::new(
        (0..pts.len()).map(|i| Segment::Line { a: pts[i], b: pts[(i + 1) % pts.len()] }).collect(),
    );
    let r = build_regions(&[l]).expect("region");
    extrude::extrude(frame, &r[0], z0, z1).expect("prism")
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<DVec2> {
    vec![DVec2::new(x0, y0), DVec2::new(x1, y0), DVec2::new(x1, y1), DVec2::new(x0, y1)]
}

fn rod(c: DVec3, axis: DVec3, r: f64, z0: f64, z1: f64) -> anvil_brep::Solid {
    let f = Frame::new(c, axis, if axis.x.abs() < 0.9 { DVec3::X } else { DVec3::Y }).expect("frame");
    let reg = build_regions(&[Loop2::new(vec![Segment::circle(DVec2::ZERO, r)])]).expect("region");
    extrude::extrude(&f, &reg[0], z0, z1).expect("rod")
}

fn exact_volume(s: &anvil_brep::Solid) -> f64 {
    let m = tess::tessellate(s, &tess::TessParams::default()).expect("tess");
    mass::properties(&m).volume
}

/// Volume of an imported mesh by the divergence theorem.
fn mesh_volume(m: &Mesh) -> f64 {
    let p = |i: u32| DVec3::from_array(m.vertices[i as usize]);
    m.faces.iter().flat_map(|f| &f.triangles).map(|t| p(t[0]).dot(p(t[1]).cross(p(t[2]))) / 6.0).sum()
}

/// True when every triangle edge is matched by an opposite one.
fn watertight(m: &Mesh) -> bool {
    let mut count: std::collections::HashMap<(u32, u32), i32> = Default::default();
    for t in m.faces.iter().flat_map(|f| &f.triangles) {
        for k in 0..3 {
            *count.entry((t[k], t[(k + 1) % 3])).or_default() += 1;
        }
    }
    count.iter().all(|(&(a, b), &n)| n == 1 && count.get(&(b, a)) == Some(&1))
}

fn round_trip(solid: &anvil_brep::Solid) -> Import {
    let text = export::write(std::slice::from_ref(solid), "test part");
    import(&text).expect("import")
}

#[test]
fn box_with_hole_round_trips_exactly() {
    let plate = prism(&rect(0.0, 0.0, 40.0, 20.0), &Frame::world(), 0.0, 5.0);
    let hole = rod(DVec3::new(20.0, 10.0, 0.0), DVec3::Z, 4.0, -1.0, 6.0);
    let part = boolean(&plate, &hole, BoolOp::Subtract).expect("cut").remove(0);
    let imp = round_trip(&part);
    assert_eq!(imp.bodies.len(), 1);
    let b = &imp.bodies[0];
    let exact = b.exact.as_ref().unwrap_or_else(|| panic!("not exact: {:?}", b.not_exact));
    check::validate(exact).expect("valid");
    assert_eq!(check::genus(exact).expect("genus"), 1);
    assert_eq!(exact.faces.len(), part.faces.len());
    assert!((exact_volume(exact) - exact_volume(&part)).abs() < 1e-6);
    assert!(watertight(&b.mesh));
    let exact_v = 4000.0 - PI * 16.0 * 5.0;
    assert!((mesh_volume(&b.mesh) - exact_v).abs() / exact_v < 0.002, "{}", mesh_volume(&b.mesh));
    assert_eq!(imp.length_unit, "millimetre");
}

#[test]
fn slanted_cut_and_fillet_round_trip() {
    // Ellipse edges from a slanted cut through a rod, and a fillet on a block.
    let r = rod(DVec3::ZERO, DVec3::Z, 2.0, 0.0, 10.0);
    let slab_frame =
        Frame::new(DVec3::new(0.0, 0.0, 5.0), DVec3::new(1.0, 0.0, 1.0), DVec3::Y).expect("frame");
    let slab = prism(&rect(-10.0, -10.0, 10.0, 10.0), &slab_frame, 0.0, 20.0);
    let cut = boolean(&r, &slab, BoolOp::Subtract).expect("cut").remove(0);
    let imp = round_trip(&cut);
    let e = imp.bodies[0].exact.as_ref().expect("exact");
    assert!((exact_volume(e) - exact_volume(&cut)).abs() / exact_volume(&cut) < 1e-6);

    let block = prism(&rect(0.0, 0.0, 10.0, 10.0), &Frame::world(), 0.0, 10.0);
    let edge = anvil_brep::blend::edge_near(&block, DVec3::new(5.0, 0.0, 10.0), 1e-6).expect("edge");
    let filleted = anvil_brep::blend::blend_edge(&block, edge, anvil_brep::blend::Blend::Fillet(3.0), 1, 0)
        .expect("fillet")
        .remove(0);
    let imp = round_trip(&filleted);
    let e = imp.bodies[0].exact.as_ref().expect("exact");
    assert!((exact_volume(e) - exact_volume(&filleted)).abs() / exact_volume(&filleted) < 1e-6);
    assert!(watertight(&imp.bodies[0].mesh));
}

#[test]
fn inch_files_are_scaled_to_millimetres() {
    let cube = prism(&rect(0.0, 0.0, 1.0, 1.0), &Frame::world(), 0.0, 1.0);
    let text = export::write(&[cube], "inch cube").replace(
        "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))",
        "(CONVERSION_BASED_UNIT('INCH',#9001)LENGTH_UNIT()NAMED_UNIT(*));\n#9000=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));\n#9001=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#9000)",
    );
    let imp = import(&text).expect("import");
    assert_eq!(imp.length_unit, "inch");
    let v = exact_volume(imp.bodies[0].exact.as_ref().expect("exact"));
    assert!((v - 25.4f64.powi(3)).abs() < 1e-6, "{v}");
}

/// A minimal STEP writer for hand-built test geometry.
struct Fixture {
    lines: Vec<String>,
}

impl Fixture {
    fn new() -> Fixture {
        Fixture { lines: Vec::new() }
    }
    fn e(&mut self, body: impl Into<String>) -> usize {
        self.lines.push(body.into());
        self.lines.len()
    }
    fn pt(&mut self, p: DVec3) -> usize {
        self.e(format!("CARTESIAN_POINT('',({:?},{:?},{:?}))", p.x, p.y, p.z))
    }
    fn dir(&mut self, d: DVec3) -> usize {
        self.e(format!("DIRECTION('',({:?},{:?},{:?}))", d.x, d.y, d.z))
    }
    fn axis(&mut self, o: DVec3, z: DVec3, x: DVec3) -> usize {
        let (o, z, x) = (self.pt(o), self.dir(z), self.dir(x));
        self.e(format!("AXIS2_PLACEMENT_3D('',#{o},#{z},#{x})"))
    }
    fn vertex(&mut self, p: DVec3) -> usize {
        let p = self.pt(p);
        self.e(format!("VERTEX_POINT('',#{p})"))
    }
    fn text(&self) -> String {
        let mut s =
            String::from("ISO-10303-21;\nHEADER;\nFILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));\nENDSEC;\nDATA;\n");
        for (i, l) in self.lines.iter().enumerate() {
            s.push_str(&format!("#{}={};\n", i + 1, l));
        }
        s.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
        s
    }
}

/// A cone frustum: bottom radius `r0` at z = 0, top radius `r1` at z = h. The side is a
/// CONICAL_SURFACE bounded by the two circles and a seam line.
fn frustum(r0: f64, r1: f64, h: f64) -> String {
    let mut f = Fixture::new();
    let (b, t) = (DVec3::new(r0, 0.0, 0.0), DVec3::new(r1, 0.0, h));
    let vb = f.vertex(b);
    let vt = f.vertex(t);
    let ab = f.axis(DVec3::ZERO, DVec3::Z, DVec3::X);
    let cb = f.e(format!("CIRCLE('',#{ab},{r0:?})"));
    let at = f.axis(DVec3::new(0.0, 0.0, h), DVec3::Z, DVec3::X);
    let ct = f.e(format!("CIRCLE('',#{at},{r1:?})"));
    let eb = f.e(format!("EDGE_CURVE('',#{vb},#{vb},#{cb},.T.)"));
    let et = f.e(format!("EDGE_CURVE('',#{vt},#{vt},#{ct},.T.)"));
    let sp = f.pt(b);
    let sd = f.dir(t - b);
    let sv = f.e(format!("VECTOR('',#{sd},1.)"));
    let sl = f.e(format!("LINE('',#{sp},#{sv})"));
    let es = f.e(format!("EDGE_CURVE('',#{vb},#{vt},#{sl},.T.)"));
    let face = |f: &mut Fixture, edges: &[(usize, bool)], surf: usize, sense: bool| {
        let oes: Vec<String> = edges
            .iter()
            .map(|(e, fw)| {
                format!("#{}", f.e(format!("ORIENTED_EDGE('',*,*,#{e},{})", if *fw { ".T." } else { ".F." })))
            })
            .collect();
        let lp = f.e(format!("EDGE_LOOP('',({}))", oes.join(",")));
        let bd = f.e(format!("FACE_OUTER_BOUND('',#{lp},.T.)"));
        f.e(format!("ADVANCED_FACE('',(#{bd}),#{surf},{})", if sense { ".T." } else { ".F." }))
    };
    let pb = f.e(format!("PLANE('',#{ab})"));
    let fb = face(&mut f, &[(eb, false)], pb, false);
    let pt = f.e(format!("PLANE('',#{at})"));
    let ft = face(&mut f, &[(et, true)], pt, true);
    let angle = ((r1 - r0) / h).atan();
    let cone = f.e(format!("CONICAL_SURFACE('',#{ab},{r0:?},{angle:?})"));
    // Side loop, counter-clockwise seen from outside: bottom circle forwards, up the
    // seam, top circle backwards, down the seam.
    let fs = face(&mut f, &[(eb, true), (es, true), (et, false), (es, false)], cone, true);
    let shell = f.e(format!("CLOSED_SHELL('',(#{fb},#{ft},#{fs}))"));
    f.e(format!("MANIFOLD_SOLID_BREP('frustum',#{shell})"));
    f.text()
}

#[test]
fn cone_frustum_is_meshed_accurately() {
    let (r0, r1, h) = (10.0, 5.0, 8.0);
    let imp = import(&frustum(r0, r1, h)).expect("import");
    let b = &imp.bodies[0];
    assert!(b.exact.is_none(), "cones are not exact yet");
    assert!(watertight(&b.mesh), "frustum mesh has gaps");
    let v = mesh_volume(&b.mesh);
    let exact = PI * h * (r0 * r0 + r0 * r1 + r1 * r1) / 3.0;
    assert!((v - exact).abs() / exact < 0.005, "{v} vs {exact}");
    assert_eq!(imp.length_unit, "millimetre (assumed)");
    assert!(b.mesh.faces[0].planar && !b.mesh.faces[2].planar);
}

#[test]
fn whole_sphere_face() {
    let mut f = Fixture::new();
    let a = f.axis(DVec3::new(1.0, 2.0, 3.0), DVec3::Z, DVec3::X);
    let s = f.e(format!("SPHERICAL_SURFACE('',#{a},5.)"));
    let face = f.e(format!("ADVANCED_FACE('',(),#{s},.T.)"));
    let shell = f.e(format!("CLOSED_SHELL('',(#{face}))"));
    f.e(format!("MANIFOLD_SOLID_BREP('ball',#{shell})"));
    let imp = import(&f.text()).expect("import");
    let v = mesh_volume(&imp.bodies[0].mesh);
    let exact = 4.0 / 3.0 * PI * 125.0;
    assert!((v - exact).abs() / exact < 0.01, "{v}");
}

/// A unit cube scaled by `s`, with the top face given as a bilinear B-spline surface
/// and its edges as degree-1 B-spline curves, or as a faceted (POLY_LOOP) brep.
fn cube(s: f64, spline_top: bool, faceted: bool) -> String {
    let mut f = Fixture::new();
    let corner = |i: usize| DVec3::new((i & 1) as f64, ((i >> 1) & 1) as f64, ((i >> 2) & 1) as f64) * s;
    // Faces as corner index loops, counter-clockwise from outside.
    let quads: [([usize; 4], DVec3); 6] = [
        ([0, 2, 3, 1], DVec3::NEG_Z),
        ([4, 5, 7, 6], DVec3::Z),
        ([0, 1, 5, 4], DVec3::NEG_Y),
        ([2, 6, 7, 3], DVec3::Y),
        ([0, 4, 6, 2], DVec3::NEG_X),
        ([1, 3, 7, 5], DVec3::X),
    ];
    if faceted {
        let pts: Vec<usize> = (0..8).map(|i| f.pt(corner(i))).collect();
        let faces: Vec<String> = quads
            .iter()
            .map(|(q, _)| {
                let lp = f.e(format!(
                    "POLY_LOOP('',({}))",
                    q.iter().map(|i| format!("#{}", pts[*i])).collect::<Vec<_>>().join(",")
                ));
                let b = f.e(format!("FACE_OUTER_BOUND('',#{lp},.T.)"));
                format!("#{}", f.e(format!("FACE('',(#{b}))")))
            })
            .collect();
        let shell = f.e(format!("CLOSED_SHELL('',({}))", faces.join(",")));
        f.e(format!("FACETED_BREP('cube',#{shell})"));
        return f.text();
    }
    let verts: Vec<usize> = (0..8).map(|i| f.vertex(corner(i))).collect();
    let mut edges: std::collections::HashMap<(usize, usize), usize> = Default::default();
    let mut faces = Vec::new();
    for (qi, (q, n)) in quads.iter().enumerate() {
        let top = qi == 1;
        let mut oes = Vec::new();
        for k in 0..4 {
            let (a, b) = (q[k], q[(k + 1) % 4]);
            let key = (a.min(b), a.max(b));
            let id = match edges.get(&key) {
                Some(id) => *id,
                None => {
                    let (pa, pb) = (corner(key.0), corner(key.1));
                    let geom = if spline_top && (key.0 >= 4 && key.1 >= 4) {
                        let (ca, cb) = (f.pt(pa), f.pt(pb));
                        f.e(format!("B_SPLINE_CURVE_WITH_KNOTS('',1,(#{ca},#{cb}),.UNSPECIFIED.,.F.,.F.,(2,2),(0.,1.),.UNSPECIFIED.)"))
                    } else {
                        let p = f.pt(pa);
                        let d = f.dir(pb - pa);
                        let v = f.e(format!("VECTOR('',#{d},1.)"));
                        f.e(format!("LINE('',#{p},#{v})"))
                    };
                    let id = f.e(format!("EDGE_CURVE('',#{},#{},#{geom},.T.)", verts[key.0], verts[key.1]));
                    edges.insert(key, id);
                    id
                }
            };
            let forward = a == key.0;
            oes.push(format!(
                "#{}",
                f.e(format!("ORIENTED_EDGE('',*,*,#{id},{})", if forward { ".T." } else { ".F." }))
            ));
        }
        let lp = f.e(format!("EDGE_LOOP('',({}))", oes.join(",")));
        let bd = f.e(format!("FACE_OUTER_BOUND('',#{lp},.T.)"));
        let surf = if top && spline_top {
            let c: Vec<usize> = [4, 5, 6, 7].iter().map(|i| f.pt(corner(*i))).collect();
            // u along x, v along y: rows are u.
            f.e(format!(
                "B_SPLINE_SURFACE_WITH_KNOTS('',1,1,((#{},#{}),(#{},#{})),.UNSPECIFIED.,.F.,.F.,.F.,(2,2),(2,2),(0.,1.),(0.,1.),.UNSPECIFIED.)",
                c[0], c[2], c[1], c[3]
            ))
        } else {
            let o = corner(q[0]);
            let x = (corner(q[1]) - o).normalize();
            let a = f.axis(o, *n, x);
            f.e(format!("PLANE('',#{a})"))
        };
        faces.push(format!("#{}", f.e(format!("ADVANCED_FACE('',(#{bd}),#{surf},.T.)"))));
    }
    let shell = f.e(format!("CLOSED_SHELL('',({}))", faces.join(",")));
    f.e(format!("MANIFOLD_SOLID_BREP('cube',#{shell})"));
    f.text()
}

#[test]
fn bspline_top_face_and_edges() {
    let imp = import(&cube(10.0, true, false)).expect("import");
    let b = &imp.bodies[0];
    assert!(b.exact.is_none());
    assert!(watertight(&b.mesh));
    assert!((mesh_volume(&b.mesh) - 1000.0).abs() < 1e-6, "{}", mesh_volume(&b.mesh));
}

#[test]
fn plain_cube_is_exact_and_faceted_cube_is_meshed() {
    let imp = import(&cube(10.0, false, false)).expect("import");
    let e = imp.bodies[0].exact.as_ref().expect("exact");
    assert!((exact_volume(e) - 1000.0).abs() < 1e-9);
    let imp = import(&cube(10.0, false, true)).expect("import");
    let b = &imp.bodies[0];
    assert!(b.exact.is_none());
    assert!(watertight(&b.mesh));
    assert!((mesh_volume(&b.mesh) - 1000.0).abs() < 1e-9);
    assert!(b.mesh.faces.iter().all(|f| f.planar));
    assert_eq!(b.mesh.faces.len(), 6);
}

#[test]
fn files_without_bodies_and_broken_files_are_errors() {
    let empty = "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n#1=CARTESIAN_POINT('',(0.,0.,0.));\nENDSEC;\nEND-ISO-10303-21;\n";
    assert!(matches!(import(empty), Err(StepError::NoBodies(_))));
    assert!(matches!(import("not a step file"), Err(StepError::Parse(_))));
    // A body whose shell references a missing face is skipped with a warning.
    let broken = "ISO-10303-21;\nDATA;\n#1=CLOSED_SHELL('',(#99));\n#2=MANIFOLD_SOLID_BREP('x',#1);\nENDSEC;\nEND-ISO-10303-21;\n";
    match import(broken) {
        Err(StepError::NoBodies(w)) => assert!(!w.is_empty()),
        other => panic!("{other:?}"),
    }
}

/// A cone with its apex: a flat disc of radius `r` at z = 0 and a CONICAL_SURFACE face
/// bounded by the rim circle and a VERTEX_LOOP at the apex (like a drill point).
fn pointed_cone(r: f64, h: f64) -> String {
    let mut f = Fixture::new();
    let vrim = f.vertex(DVec3::new(r, 0.0, 0.0));
    let vapex = f.vertex(DVec3::new(0.0, 0.0, h));
    let ab = f.axis(DVec3::ZERO, DVec3::Z, DVec3::X);
    let c = f.e(format!("CIRCLE('',#{ab},{r:?})"));
    let rim = f.e(format!("EDGE_CURVE('',#{vrim},#{vrim},#{c},.T.)"));
    let disc_oe = f.e(format!("ORIENTED_EDGE('',*,*,#{rim},.F.)"));
    let disc_loop = f.e(format!("EDGE_LOOP('',(#{disc_oe}))"));
    let disc_bound = f.e(format!("FACE_OUTER_BOUND('',#{disc_loop},.T.)"));
    let plane = f.e(format!("PLANE('',#{ab})"));
    let disc = f.e(format!("ADVANCED_FACE('',(#{disc_bound}),#{plane},.F.)"));
    let cone_oe = f.e(format!("ORIENTED_EDGE('',*,*,#{rim},.T.)"));
    let cone_loop = f.e(format!("EDGE_LOOP('',(#{cone_oe}))"));
    let cone_bound = f.e(format!("FACE_OUTER_BOUND('',#{cone_loop},.T.)"));
    let apex_loop = f.e(format!("VERTEX_LOOP('',#{vapex})"));
    let apex_bound = f.e(format!("FACE_BOUND('',#{apex_loop},.T.)"));
    let angle = (-r / h).atan();
    let cone = f.e(format!("CONICAL_SURFACE('',#{ab},{r:?},{angle:?})"));
    let side = f.e(format!("ADVANCED_FACE('',(#{cone_bound},#{apex_bound}),#{cone},.T.)"));
    let shell = f.e(format!("CLOSED_SHELL('',(#{disc},#{side}))"));
    f.e(format!("MANIFOLD_SOLID_BREP('cone',#{shell})"));
    f.text()
}

#[test]
fn cone_with_apex_is_closed() {
    let (r, h) = (6.0, 9.0);
    let imp = import(&pointed_cone(r, h)).expect("import");
    let m = &imp.bodies[0].mesh;
    assert!(watertight(m), "pointed cone mesh has gaps");
    let exact = PI * r * r * h / 3.0;
    assert!((mesh_volume(m) - exact).abs() / exact < 0.005, "{}", mesh_volume(m));
}

/// A hole drilled across a shaft: its entry and exit edges are spline curves, which
/// export as B-splines and come back exact.
#[test]
fn cross_drilled_shaft_round_trips_exactly() {
    let shaft = rod(DVec3::new(-10.0, 0.0, 0.0), DVec3::X, 5.0, 0.0, 20.0);
    let drill = rod(DVec3::new(1.0, 0.0, -8.0), DVec3::Z, 2.0, 0.0, 16.0);
    let part = boolean(&shaft, &drill, BoolOp::Subtract).expect("cut").remove(0);
    let splines = |s: &anvil_brep::Solid| {
        s.edges.iter().filter(|e| matches!(e.curve, anvil_brep::Curve::Spline(_))).count()
    };
    assert!(splines(&part) >= 2);
    let text = export::write(std::slice::from_ref(&part), "shaft");
    assert!(text.contains("B_SPLINE_CURVE_WITH_KNOTS"));
    let imp = import(&text).expect("import");
    let b = &imp.bodies[0];
    let exact = b.exact.as_ref().unwrap_or_else(|| panic!("not exact: {:?}", b.not_exact));
    check::validate(exact).expect("valid");
    assert_eq!(exact.faces.len(), part.faces.len());
    assert!(splines(exact) >= 2);
    let v = exact_volume(&part);
    assert!((exact_volume(exact) - v).abs() / v < 1e-6, "{} vs {v}", exact_volume(exact));
    assert!(watertight(&b.mesh));
    // The importer's own mesh is coarser (0.02 mm chords on 2 mm radii).
    assert!((mesh_volume(&b.mesh) - v).abs() / v < 5e-3, "{} vs {v}", mesh_volume(&b.mesh));
}

/// A ball of radius `r` about `c` above the plane z = c.z: a sphere face over a disc.
fn dome(c: DVec3, r: f64) -> anvil_brep::Solid {
    use anvil_brep::topo::{Coedge, Edge, Face, FaceTag, Loop};
    let mut s = anvil_brep::Solid::default();
    let circle = Frame { origin: c, ..Frame::world() };
    let v = s.add_vertex(c + DVec3::new(r, 0.0, 0.0));
    let e = s.add_edge(Edge {
        curve: anvil_brep::Curve::Circle { frame: circle, radius: r },
        start: v,
        end: v,
        t0: 0.0,
        t1: std::f64::consts::TAU,
    });
    let l = |reversed| Loop { coedges: vec![Coedge { edge: e, reversed }] };
    s.add_face(Face {
        surface: anvil_brep::Surface::Sphere { frame: circle, radius: r },
        reversed: false,
        outer: l(false),
        inners: vec![],
        tag: FaceTag::Imported { index: 0 },
        owner: 0,
    });
    s.add_face(Face {
        surface: anvil_brep::Surface::Plane { frame: circle },
        reversed: true,
        outer: l(true),
        inners: vec![],
        tag: FaceTag::Imported { index: 1 },
        owner: 0,
    });
    s
}

/// A dome and a block with a spherical socket cut in it both come back exact, the
/// socket's inward-facing sphere included.
#[test]
fn dome_and_socket_round_trip_exactly() {
    let ball = dome(DVec3::ZERO, 6.0);
    let block = prism(&rect(-10.0, -10.0, 10.0, 10.0), &Frame::world(), -8.0, 4.0);
    let socket = boolean(&block, &ball, BoolOp::Subtract).expect("cut").remove(0);
    for part in [ball, socket] {
        assert!(part.faces.iter().any(|f| matches!(f.surface, anvil_brep::Surface::Sphere { .. })));
        let imp = round_trip(&part);
        let b = &imp.bodies[0];
        let exact = b.exact.as_ref().unwrap_or_else(|| panic!("not exact: {:?}", b.not_exact));
        check::validate(exact).expect("valid");
        let v = exact_volume(&part);
        assert!((exact_volume(exact) - v).abs() / v < 1e-6, "{} vs {v}", exact_volume(exact));
        assert!(watertight(&b.mesh));
        // The importer's own mesh steps 10 degrees at a time; on a sphere, which curves
        // both ways, that loses about half a percent.
        assert!((mesh_volume(&b.mesh) - v).abs() / v < 1e-2, "{} vs {v}", mesh_volume(&b.mesh));
    }
}
