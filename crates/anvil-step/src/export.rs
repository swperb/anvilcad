//! Exact STEP export: AP214 ADVANCED_BREP with analytic curves and surfaces.

use anvil_brep::{Curve, Frame, Solid, Surface};
use std::fmt::Write;

fn real(v: f64) -> String {
    let s = format!("{v:.10}");
    let s = s.trim_end_matches('0');
    if s == "-0." { "0.".into() } else { s.into() }
}

fn escape(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\'' => "''".to_string(),
            '\\' => "\\\\".to_string(),
            c if c.is_ascii() && !c.is_ascii_control() => c.to_string(),
            c => format!("\\X2\\{:04X}\\X0\\", c as u32 & 0xFFFF),
        })
        .collect()
}

struct Writer {
    out: String,
    next: usize,
}

impl Writer {
    fn ent(&mut self, body: impl AsRef<str>) -> usize {
        self.next += 1;
        let _ = writeln!(self.out, "#{}={};", self.next, body.as_ref());
        self.next
    }

    fn point(&mut self, p: glam::DVec3) -> usize {
        self.ent(format!("CARTESIAN_POINT('',({},{},{}))", real(p.x), real(p.y), real(p.z)))
    }

    fn dir(&mut self, d: glam::DVec3) -> usize {
        self.ent(format!("DIRECTION('',({},{},{}))", real(d.x), real(d.y), real(d.z)))
    }

    /// A B-spline curve; rational ones need STEP's complex-instance form.
    fn spline(&mut self, sp: &anvil_brep::spline::Spline) -> usize {
        let pts: Vec<String> = sp.points.iter().map(|p| format!("#{}", self.point(*p))).collect();
        let mut knots: Vec<f64> = Vec::new();
        let mut mults: Vec<usize> = Vec::new();
        for k in &sp.knots {
            match knots.last() {
                Some(last) if last == k => {
                    if let Some(m) = mults.last_mut() {
                        *m += 1;
                    }
                }
                _ => {
                    knots.push(*k);
                    mults.push(1);
                }
            }
        }
        let list = |v: Vec<String>| v.join(",");
        let pts = list(pts);
        let mults = list(mults.iter().map(|m| m.to_string()).collect());
        let knots = list(knots.iter().map(|k| real(*k)).collect());
        let closed = if sp.closed { ".T." } else { ".F." };
        let deg = sp.degree;
        if sp.weights.is_empty() {
            self.ent(format!(
                "B_SPLINE_CURVE_WITH_KNOTS('',{deg},({pts}),.UNSPECIFIED.,{closed},.F.,({mults}),({knots}),.UNSPECIFIED.)"
            ))
        } else {
            let weights = list(sp.weights.iter().map(|w| real(*w)).collect());
            self.ent(format!(
                "(BOUNDED_CURVE()B_SPLINE_CURVE({deg},({pts}),.UNSPECIFIED.,{closed},.F.)B_SPLINE_CURVE_WITH_KNOTS(({mults}),({knots}),.UNSPECIFIED.)CURVE()GEOMETRIC_REPRESENTATION_ITEM()RATIONAL_B_SPLINE_CURVE(({weights}))REPRESENTATION_ITEM(''))"
            ))
        }
    }

    fn placement(&mut self, f: &Frame) -> usize {
        let o = self.point(f.origin);
        let z = self.dir(f.z);
        let x = self.dir(f.x);
        self.ent(format!("AXIS2_PLACEMENT_3D('',#{o},#{z},#{x})"))
    }
}

/// Writes solids as one STEP part with a body per solid.
pub fn write(solids: &[Solid], name: &str) -> String {
    let name = escape(name);
    let mut w = Writer { out: String::new(), next: 0 };
    let app = w.ent("APPLICATION_CONTEXT('automotive design')");
    w.ent(format!(
        "APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#{app})"
    ));
    let pctx = w.ent(format!("PRODUCT_CONTEXT('',#{app},'mechanical')"));
    let prod = w.ent(format!("PRODUCT('{name}','{name}','',(#{pctx}))"));
    w.ent(format!("PRODUCT_RELATED_PRODUCT_CATEGORY('part',$,(#{prod}))"));
    let pdf = w.ent(format!("PRODUCT_DEFINITION_FORMATION('','',#{prod})"));
    let pdctx = w.ent(format!("PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')"));
    let pd = w.ent(format!("PRODUCT_DEFINITION('design','',#{pdf},#{pdctx})"));
    let pds = w.ent(format!("PRODUCT_DEFINITION_SHAPE('','',#{pd})"));
    let len = w.ent("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))");
    let ang = w.ent("(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))");
    let sol = w.ent("(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())");
    let unc = w.ent(format!(
        "UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#{len},'distance_accuracy_value','confusion accuracy')"
    ));
    let ctx = w.ent(format!(
        "(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{unc}))GLOBAL_UNIT_ASSIGNED_CONTEXT((#{len},#{ang},#{sol}))REPRESENTATION_CONTEXT('Context3D','3D Context'))"
    ));
    let origin = w.placement(&Frame::world());

    let mut breps = Vec::new();
    for (si, solid) in solids.iter().enumerate() {
        let vertices: Vec<usize> = solid
            .vertices
            .iter()
            .map(|v| {
                let p = w.point(v.point);
                w.ent(format!("VERTEX_POINT('',#{p})"))
            })
            .collect();
        let mut edges = Vec::with_capacity(solid.edges.len());
        for e in &solid.edges {
            let (geom, same) = match &e.curve {
                Curve::Spline(sp) => (w.spline(sp), e.t1 >= e.t0),
                &Curve::Line { origin, dir } => {
                    let p = w.point(origin);
                    let d = w.dir(dir);
                    let v = w.ent(format!("VECTOR('',#{d},1.)"));
                    (w.ent(format!("LINE('',#{p},#{v})")), e.t1 >= e.t0)
                }
                &Curve::Circle { frame, radius } => {
                    let a = w.placement(&frame);
                    (w.ent(format!("CIRCLE('',#{a},{})", real(radius))), e.t1 >= e.t0)
                }
                &Curve::Ellipse { frame, a, b } => {
                    let pl = w.placement(&frame);
                    (w.ent(format!("ELLIPSE('',#{pl},{},{})", real(a), real(b))), e.t1 >= e.t0)
                }
            };
            let (vs, ve) = (vertices[e.start.index()], vertices[e.end.index()]);
            edges.push(
                w.ent(format!("EDGE_CURVE('',#{vs},#{ve},#{geom},{})", if same { ".T." } else { ".F." })),
            );
        }
        let mut faces = Vec::with_capacity(solid.faces.len());
        for f in &solid.faces {
            let surf = match f.surface {
                Surface::Plane { frame } => {
                    let a = w.placement(&frame);
                    w.ent(format!("PLANE('',#{a})"))
                }
                Surface::Cylinder { frame, radius } => {
                    let a = w.placement(&frame);
                    w.ent(format!("CYLINDRICAL_SURFACE('',#{a},{})", real(radius)))
                }
                Surface::Cone { frame, radius, half_angle } => {
                    let a = w.placement(&frame);
                    w.ent(format!("CONICAL_SURFACE('',#{a},{},{})", real(radius), real(half_angle)))
                }
                Surface::Torus { frame, major, minor, .. } => {
                    let a = w.placement(&frame);
                    w.ent(format!("TOROIDAL_SURFACE('',#{a},{},{})", real(major), real(minor)))
                }
                Surface::Sphere { frame, radius } => {
                    let a = w.placement(&frame);
                    w.ent(format!("SPHERICAL_SURFACE('',#{a},{})", real(radius)))
                }
            };
            let mut bounds = Vec::new();
            for (li, l) in f.loops().enumerate() {
                let oes: Vec<String> = l
                    .coedges
                    .iter()
                    .map(|c| {
                        let e = edges[c.edge.index()];
                        format!(
                            "#{}",
                            w.ent(format!(
                                "ORIENTED_EDGE('',*,*,#{e},{})",
                                if c.reversed { ".F." } else { ".T." }
                            ))
                        )
                    })
                    .collect();
                let lp = w.ent(format!("EDGE_LOOP('',({}))", oes.join(",")));
                let kind = if li == 0 { "FACE_OUTER_BOUND" } else { "FACE_BOUND" };
                bounds.push(format!("#{}", w.ent(format!("{kind}('',#{lp},.T.)"))));
            }
            faces.push(format!(
                "#{}",
                w.ent(format!(
                    "ADVANCED_FACE('',({}),#{surf},{})",
                    bounds.join(","),
                    if f.reversed { ".F." } else { ".T." }
                ))
            ));
        }
        let shell = w.ent(format!("CLOSED_SHELL('',({}))", faces.join(",")));
        breps.push(format!("#{}", w.ent(format!("MANIFOLD_SOLID_BREP('Body{}',#{shell})", si + 1))));
    }
    let rep =
        w.ent(format!("ADVANCED_BREP_SHAPE_REPRESENTATION('{name}',(#{origin},{}),#{ctx})", breps.join(",")));
    w.ent(format!("SHAPE_DEFINITION_REPRESENTATION(#{pds},#{rep})"));

    let mut s = String::new();
    let _ = writeln!(s, "ISO-10303-21;");
    let _ = writeln!(s, "HEADER;");
    let _ = writeln!(s, "FILE_DESCRIPTION(('AnvilCAD part'),'2;1');");
    let _ = writeln!(s, "FILE_NAME('{name}','',(''),(''),'AnvilCAD','AnvilCAD','');");
    let _ = writeln!(s, "FILE_SCHEMA(('AUTOMOTIVE_DESIGN {{ 1 0 10303 214 1 1 1 1 }}'));");
    let _ = writeln!(s, "ENDSEC;");
    let _ = writeln!(s, "DATA;");
    s.push_str(&w.out);
    let _ = writeln!(s, "ENDSEC;");
    let _ = writeln!(s, "END-ISO-10303-21;");
    s
}
