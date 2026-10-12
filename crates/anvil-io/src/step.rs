//! STEP AP214 export as a faceted B-rep (FACETED_BREP with POLY_LOOP faces).
//!
//! This is accepted by mainstream CAD importers. Exporting exact analytic surfaces
//! (ADVANCED_BREP) will follow once the kernel keeps exact face boundaries through booleans.

use anvil_kernel::Solid;
use std::collections::HashMap;
use std::fmt::Write;

fn real(v: f64) -> String {
    // STEP reals need a decimal point; fixed-precision formatting always has one, so only
    // trailing zeros are trimmed ("10.000000000" -> "10.").
    format!("{v:.9}").trim_end_matches('0').to_string()
}

fn escape(s: &str) -> String {
    s.replace('\'', "''")
}

pub fn write(solid: &Solid, name: &str) -> String {
    let name = escape(name);
    let mut d = String::new();
    let mut id = 0usize;
    let mut ent = |d: &mut String, body: String| -> usize {
        id += 1;
        let _ = writeln!(d, "#{id}={body};");
        id
    };

    let app = ent(&mut d, "APPLICATION_CONTEXT('automotive design')".into());
    ent(
        &mut d,
        format!("APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#{app})"),
    );
    let pctx = ent(&mut d, format!("PRODUCT_CONTEXT('',#{app},'mechanical')"));
    let prod = ent(&mut d, format!("PRODUCT('{name}','{name}','',(#{pctx}))"));
    ent(&mut d, format!("PRODUCT_RELATED_PRODUCT_CATEGORY('part',$,(#{prod}))"));
    let pdf = ent(&mut d, format!("PRODUCT_DEFINITION_FORMATION('','',#{prod})"));
    let pdctx = ent(&mut d, format!("PRODUCT_DEFINITION_CONTEXT('part definition',#{app},'design')"));
    let pd = ent(&mut d, format!("PRODUCT_DEFINITION('design','',#{pdf},#{pdctx})"));
    let pds = ent(&mut d, format!("PRODUCT_DEFINITION_SHAPE('','',#{pd})"));
    let len = ent(&mut d, "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))".into());
    let ang = ent(&mut d, "(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))".into());
    let sol = ent(&mut d, "(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())".into());
    let unc = ent(
        &mut d,
        format!("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#{len},'distance_accuracy_value','confusion accuracy')"),
    );
    let ctx = ent(
        &mut d,
        format!("(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{unc}))GLOBAL_UNIT_ASSIGNED_CONTEXT((#{len},#{ang},#{sol}))REPRESENTATION_CONTEXT('Context3D','3D Context'))"),
    );
    let o = ent(&mut d, "CARTESIAN_POINT('',(0.,0.,0.))".into());
    let z = ent(&mut d, "DIRECTION('',(0.,0.,1.))".into());
    let x = ent(&mut d, "DIRECTION('',(1.,0.,0.))".into());
    let ax = ent(&mut d, format!("AXIS2_PLACEMENT_3D('',#{o},#{z},#{x})"));

    // Shared vertices.
    let mut points: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let mut faces = Vec::new();
    for p in &solid.polygons {
        let mut refs = Vec::new();
        for v in &p.verts {
            let key = ((v.x * 1e6).round() as i64, (v.y * 1e6).round() as i64, (v.z * 1e6).round() as i64);
            let pid = match points.get(&key) {
                Some(&pid) => pid,
                None => {
                    let pid = ent(
                        &mut d,
                        format!("CARTESIAN_POINT('',({},{},{}))", real(v.x), real(v.y), real(v.z)),
                    );
                    points.insert(key, pid);
                    pid
                }
            };
            if refs.last() != Some(&pid) {
                refs.push(pid);
            }
        }
        if refs.len() > 1 && refs.first() == refs.last() {
            refs.pop();
        }
        if refs.len() < 3 {
            continue;
        }
        let list: Vec<String> = refs.iter().map(|r| format!("#{r}")).collect();
        let lp = ent(&mut d, format!("POLY_LOOP('',({}))", list.join(",")));
        let bound = ent(&mut d, format!("FACE_OUTER_BOUND('',#{lp},.T.)"));
        faces.push(ent(&mut d, format!("FACE('',(#{bound}))")));
    }
    let face_list: Vec<String> = faces.iter().map(|f| format!("#{f}")).collect();
    let shell = ent(&mut d, format!("CLOSED_SHELL('',({}))", face_list.join(",")));
    let brep = ent(&mut d, format!("FACETED_BREP('{name}',#{shell})"));
    let rep = ent(&mut d, format!("FACETED_BREP_SHAPE_REPRESENTATION('{name}',(#{ax},#{brep}),#{ctx})"));
    ent(&mut d, format!("SHAPE_DEFINITION_REPRESENTATION(#{pds},#{rep})"));

    let mut s = String::new();
    s.push_str("ISO-10303-21;\nHEADER;\n");
    s.push_str("FILE_DESCRIPTION(('AnvilCAD model'),'2;1');\n");
    let _ = writeln!(
        s,
        "FILE_NAME('{name}.step','',(''),(''),'AnvilCAD {}','AnvilCAD','');",
        env!("CARGO_PKG_VERSION")
    );
    s.push_str("FILE_SCHEMA(('AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }'));\nENDSEC;\nDATA;\n");
    s.push_str(&d);
    s.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
    s
}

#[cfg(test)]
mod tests {
    use anvil_kernel::{build::block, v3};

    #[test]
    fn cube_structure() {
        let b = block(v3(0.0, 0.0, 0.0), v3(10.0, 10.0, 10.0), 1);
        let s = super::write(&b, "cube");
        assert!(s.starts_with("ISO-10303-21;"));
        assert!(s.trim_end().ends_with("END-ISO-10303-21;"));
        // 8 shared corners, one face per polygon.
        assert_eq!(s.matches("CARTESIAN_POINT('',(").count(), 9);
        assert_eq!(s.matches("=FACE('',").count(), b.polygons.len());
        assert!(s.contains("FACETED_BREP_SHAPE_REPRESENTATION"));
    }

    #[test]
    fn reals_have_points() {
        assert_eq!(super::real(10.0), "10.");
        assert_eq!(super::real(-0.5), "-0.5");
    }
}
