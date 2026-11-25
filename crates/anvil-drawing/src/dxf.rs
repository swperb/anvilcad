//! DXF output of a laid-out sheet, and a reader for the same subset (see
//! `docs/re/drawings.md`, "DXF export").
//!
//! The file is AutoCAD R12 (`AC1009`) ASCII DXF, which every DXF reader opens, at sheet
//! size: one drawing unit is one sheet millimetre. Lines are `LINE`; circles and circular
//! arcs are exact `CIRCLE` and `ARC`; ellipses and traced curves are 2D `POLYLINE`s within
//! 0.01 mm, because R12 has no ellipse. Each pen has its own layer, and hidden lines use a
//! `DASHED` linetype. Dimensions are written as plain lines, filled `SOLID` arrowheads and
//! `TEXT`, so every reader shows them as they are on the sheet. Group codes follow
//! Autodesk's published DXF reference.

use crate::hlr::Path2;
use crate::layout::{Align, Note, Pen, SheetPicture};
use glam::DVec2;
use std::f64::consts::TAU;
use std::fmt::Write as _;

/// How far a polyline standing in for a curve may stray from it, in millimetres.
const CURVE_TOL: f64 = 0.01;

/// The layers a sheet is written on, with their linetypes.
pub const LAYERS: [(&str, &str); 6] = [
    ("VISIBLE", "CONTINUOUS"),
    ("HIDDEN", "DASHED"),
    ("THIN", "CONTINUOUS"),
    ("FRAME", "CONTINUOUS"),
    ("TITLE", "CONTINUOUS"),
    ("DIMENSIONS", "CONTINUOUS"),
];

/// The layer a pen's lines go on.
pub fn layer(pen: Pen) -> &'static str {
    match pen {
        Pen::Visible => "VISIBLE",
        Pen::Hidden => "HIDDEN",
        Pen::Thin => "THIN",
        Pen::Frame => "FRAME",
        Pen::Title => "TITLE",
        Pen::Dim => "DIMENSIONS",
    }
}

/// A number with up to six decimals, trailing zeros dropped.
fn num(v: f64) -> String {
    let v = if v.abs() < 5e-7 { 0.0 } else { v };
    let mut s = format!("{v:.6}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    s
}

struct Out(String);

impl Out {
    fn pair(&mut self, code: i32, value: &str) {
        let _ = write!(self.0, "{code:>3}\r\n{value}\r\n");
    }
    fn f(&mut self, code: i32, v: f64) {
        self.pair(code, &num(v));
    }
    fn i(&mut self, code: i32, v: i32) {
        self.pair(code, &v.to_string());
    }
    fn point(&mut self, base: i32, p: DVec2) {
        self.f(base, p.x);
        self.f(base + 10, p.y);
        self.f(base + 20, 0.0);
    }
    fn start(&mut self, kind: &str, layer: &str) {
        self.pair(0, kind);
        self.pair(8, layer);
    }
}

/// DXF text: Ø, ° and ± as `%%c`, `%%d` and `%%p`, a percent sign doubled where it could
/// be read as one of those, other Latin-1 letters as `\U+00XX`, anything else as `?`.
pub fn dxf_text(text: &str) -> String {
    let mut s = String::new();
    for ch in text.chars() {
        match ch {
            '\u{d8}' | '\u{f8}' | '\u{2300}' => s.push_str("%%c"),
            '\u{b0}' => s.push_str("%%d"),
            '\u{b1}' => s.push_str("%%p"),
            '%' => s.push_str("%%%"),
            ' '..='~' => s.push(ch),
            '\u{a0}'..='\u{ff}' => {
                let _ = write!(s, "\\U+{:04X}", ch as u32);
            }
            _ => s.push('?'),
        }
    }
    s
}

/// A conic piece that is part of a circle: centre, radius, start angle and sweep
/// (counter-clockwise, radians).
fn circular(c: DVec2, u: DVec2, v: DVec2, t0: f64, t1: f64) -> Option<(DVec2, f64, f64, f64)> {
    let r = u.length();
    if r < 1e-12 || (v.length() - r).abs() > 1e-9 * r || u.dot(v).abs() > 1e-9 * r * r {
        return None;
    }
    let sign = u.perp_dot(v).signum();
    let au = u.y.atan2(u.x);
    let (a, b) = (au + sign * t0, au + sign * t1);
    let (start, sweep) = if b >= a { (a, b - a) } else { (b, a - b) };
    Some((c, r, start, sweep))
}

fn polyline(o: &mut Out, layer: &str, pts: &[DVec2]) {
    if pts.len() < 2 {
        return;
    }
    let closed = pts.len() > 2 && pts[0].distance(pts[pts.len() - 1]) < 1e-9;
    let pts = if closed { &pts[..pts.len() - 1] } else { pts };
    o.start("POLYLINE", layer);
    o.i(66, 1);
    o.point(10, DVec2::ZERO);
    o.i(70, if closed { 1 } else { 0 });
    for p in pts {
        o.start("VERTEX", layer);
        o.point(10, *p);
    }
    o.start("SEQEND", layer);
}

fn path(o: &mut Out, layer: &str, p: &Path2) {
    match p {
        Path2::Line { a, b } => {
            o.start("LINE", layer);
            o.point(10, *a);
            o.point(11, *b);
        }
        Path2::Conic { c, u, v, t0, t1 } => match circular(*c, *u, *v, *t0, *t1) {
            Some((c, r, _, sweep)) if sweep >= TAU - 1e-9 => {
                o.start("CIRCLE", layer);
                o.point(10, c);
                o.f(40, r);
            }
            Some((c, r, start, sweep)) => {
                o.start("ARC", layer);
                o.point(10, c);
                o.f(40, r);
                o.f(50, start.to_degrees().rem_euclid(360.0));
                o.f(51, (start + sweep).to_degrees().rem_euclid(360.0));
            }
            None => polyline(o, layer, &p.points(CURVE_TOL)),
        },
        Path2::Poly(pts) => polyline(o, layer, pts),
    }
}

fn text(o: &mut Out, layer: &str, n: &Note) {
    o.start("TEXT", layer);
    o.point(10, n.pos);
    o.f(40, n.height);
    o.pair(1, &dxf_text(&n.text));
    let h = match n.align {
        Align::Left => 0,
        Align::Center => 1,
        Align::Right => 2,
    };
    if h != 0 {
        o.i(72, h);
        o.point(11, n.pos);
    }
}

/// The sheet as R12 ASCII DXF.
pub fn write_dxf(pic: &SheetPicture) -> String {
    let mut o = Out(String::new());
    o.pair(0, "SECTION");
    o.pair(2, "HEADER");
    o.pair(9, "$ACADVER");
    o.pair(1, "AC1009");
    o.pair(9, "$INSUNITS");
    o.i(70, 4);
    o.pair(9, "$MEASUREMENT");
    o.i(70, 1);
    o.pair(9, "$EXTMIN");
    o.point(10, DVec2::ZERO);
    o.pair(9, "$EXTMAX");
    o.point(10, DVec2::new(pic.width, pic.height));
    o.pair(9, "$LIMMIN");
    o.f(10, 0.0);
    o.f(20, 0.0);
    o.pair(9, "$LIMMAX");
    o.f(10, pic.width);
    o.f(20, pic.height);
    o.pair(0, "ENDSEC");

    o.pair(0, "SECTION");
    o.pair(2, "TABLES");
    o.pair(0, "TABLE");
    o.pair(2, "LTYPE");
    o.i(70, 2);
    o.pair(0, "LTYPE");
    o.pair(2, "CONTINUOUS");
    o.i(70, 0);
    o.pair(3, "Solid line");
    o.i(72, 65);
    o.i(73, 0);
    o.f(40, 0.0);
    o.pair(0, "LTYPE");
    o.pair(2, "DASHED");
    o.i(70, 0);
    o.pair(3, "Dashed: 3 mm dashes, 1 mm gaps");
    o.i(72, 65);
    o.i(73, 2);
    o.f(40, 4.0);
    o.f(49, 3.0);
    o.f(49, -1.0);
    o.pair(0, "ENDTAB");
    o.pair(0, "TABLE");
    o.pair(2, "LAYER");
    o.i(70, LAYERS.len() as i32);
    for (name, ltype) in LAYERS {
        o.pair(0, "LAYER");
        o.pair(2, name);
        o.i(70, 0);
        // Colour 7 is black on a white background and white on a black one.
        o.i(62, 7);
        o.pair(6, ltype);
    }
    o.pair(0, "ENDTAB");
    o.pair(0, "TABLE");
    o.pair(2, "STYLE");
    o.i(70, 1);
    o.pair(0, "STYLE");
    o.pair(2, "STANDARD");
    o.i(70, 0);
    o.f(40, 0.0);
    o.f(41, 1.0);
    o.f(50, 0.0);
    o.i(71, 0);
    o.f(42, 3.5);
    o.pair(3, "txt");
    o.pair(4, "");
    o.pair(0, "ENDTAB");
    o.pair(0, "ENDSEC");

    o.pair(0, "SECTION");
    o.pair(2, "ENTITIES");
    for s in &pic.strokes {
        path(&mut o, layer(s.pen), &s.path);
    }
    for n in &pic.notes {
        text(&mut o, "TITLE", n);
    }
    for d in &pic.dims {
        let l = layer(Pen::Dim);
        for p in &d.lines {
            path(&mut o, l, p);
        }
        for a in &d.arrows {
            o.start("SOLID", l);
            o.point(10, a[0]);
            o.point(11, a[1]);
            o.point(12, a[2]);
            o.point(13, a[2]);
        }
        text(&mut o, l, &d.text);
    }
    o.pair(0, "ENDSEC");
    o.pair(0, "EOF");
    o.0
}

/// One entity read back from a DXF file, with its group codes in file order. A
/// `POLYLINE` carries its vertices.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    pub kind: String,
    pub codes: Vec<(i32, String)>,
    pub vertices: Vec<DVec2>,
}

impl Entity {
    pub fn get(&self, code: i32) -> Option<&str> {
        self.codes.iter().find(|(c, _)| *c == code).map(|(_, v)| v.as_str())
    }

    pub fn num(&self, code: i32) -> Option<f64> {
        self.get(code).and_then(|v| v.trim().parse().ok())
    }

    pub fn layer(&self) -> &str {
        self.get(8).unwrap_or("0")
    }

    /// The point in group codes `base` and `base + 10`.
    pub fn point(&self, base: i32) -> Option<DVec2> {
        Some(DVec2::new(self.num(base)?, self.num(base + 10)?))
    }
}

/// What [`read_dxf`] finds in a file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DxfFile {
    pub version: Option<String>,
    /// Layer names with their linetypes.
    pub layers: Vec<(String, String)>,
    pub entities: Vec<Entity>,
}

/// Reads an ASCII DXF file's version, layers and entities. Fails on a file that is not
/// pairs of a group code and a value.
pub fn read_dxf(text: &str) -> Result<DxfFile, String> {
    let lines: Vec<&str> = text.lines().collect();
    if !lines.len().is_multiple_of(2) {
        return Err("the file ends in the middle of a group".into());
    }
    let mut pairs = Vec::with_capacity(lines.len() / 2);
    for (i, ch) in lines.chunks(2).enumerate() {
        let code: i32 = ch[0]
            .trim()
            .parse()
            .map_err(|_| format!("line {}: \"{}\" is not a group code", i * 2 + 1, ch[0].trim()))?;
        pairs.push((code, ch[1].trim_end_matches('\r').to_string()));
    }
    let mut out = DxfFile::default();
    let mut section = String::new();
    let mut i = 0;
    while i < pairs.len() {
        let (code, value) = (&pairs[i].0, pairs[i].1.trim());
        if *code == 0 && value == "SECTION" {
            section = pairs.get(i + 1).map(|p| p.1.trim().to_string()).unwrap_or_default();
            i += 2;
            continue;
        }
        if *code == 0 && (value == "ENDSEC" || value == "EOF") {
            section.clear();
            i += 1;
            continue;
        }
        if *code == 9 && value == "$ACADVER" && section == "HEADER" {
            out.version = pairs.get(i + 1).map(|p| p.1.trim().to_string());
            i += 2;
            continue;
        }
        if *code == 0 && section == "TABLES" && value == "LAYER" {
            let mut j = i + 1;
            let (mut name, mut ltype) = (String::new(), String::new());
            while j < pairs.len() && pairs[j].0 != 0 {
                match pairs[j].0 {
                    2 => name = pairs[j].1.trim().to_string(),
                    6 => ltype = pairs[j].1.trim().to_string(),
                    _ => {}
                }
                j += 1;
            }
            out.layers.push((name, ltype));
            i = j;
            continue;
        }
        if *code == 0 && section == "ENTITIES" {
            let kind = value.to_string();
            let mut j = i + 1;
            let mut codes = Vec::new();
            while j < pairs.len() && pairs[j].0 != 0 {
                codes.push((pairs[j].0, pairs[j].1.trim().to_string()));
                j += 1;
            }
            match kind.as_str() {
                "VERTEX" => {
                    let v = Entity { kind, codes, vertices: Vec::new() };
                    match (out.entities.last_mut(), v.point(10)) {
                        (Some(p), Some(q)) if p.kind == "POLYLINE" => p.vertices.push(q),
                        _ => return Err("a VERTEX outside a POLYLINE".into()),
                    }
                }
                "SEQEND" => {}
                _ => out.entities.push(Entity { kind, codes, vertices: Vec::new() }),
            }
            i = j;
            continue;
        }
        i += 1;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Stroke;

    #[test]
    fn text_codes() {
        assert_eq!(dxf_text("\u{d8}8 30\u{b0} 5%"), "%%c8 30%%d 5%%%");
        assert_eq!(dxf_text("caf\u{e9}\u{2014}"), "caf\\U+00E9?");
    }

    #[test]
    fn every_kind_of_path_round_trips() {
        let r = 5.0;
        let pic = SheetPicture {
            width: 420.0,
            height: 297.0,
            strokes: vec![
                Stroke {
                    pen: Pen::Visible,
                    path: Path2::Line { a: DVec2::new(1.0, 2.0), b: DVec2::new(3.5, 2.0) },
                    view: Some(1),
                },
                Stroke {
                    pen: Pen::Hidden,
                    path: Path2::Conic {
                        c: DVec2::new(50.0, 60.0),
                        u: DVec2::X * r,
                        v: DVec2::Y * r,
                        t0: 0.0,
                        t1: TAU,
                    },
                    view: Some(1),
                },
                // Clockwise, from 90 to 0 degrees: the arc from 0 to 90 counter-clockwise.
                Stroke {
                    pen: Pen::Thin,
                    path: Path2::Conic {
                        c: DVec2::ZERO,
                        u: DVec2::Y * r,
                        v: DVec2::X * r,
                        t0: 0.0,
                        t1: std::f64::consts::FRAC_PI_2,
                    },
                    view: Some(1),
                },
                Stroke {
                    pen: Pen::Visible,
                    path: Path2::Conic {
                        c: DVec2::ZERO,
                        u: DVec2::X * 10.0,
                        v: DVec2::Y * 4.0,
                        t0: 0.0,
                        t1: TAU,
                    },
                    view: Some(1),
                },
                Stroke {
                    pen: Pen::Frame,
                    path: Path2::Poly(vec![DVec2::ZERO, DVec2::X, DVec2::ONE, DVec2::ZERO]),
                    view: None,
                },
            ],
            notes: vec![Note {
                pos: DVec2::new(10.0, 20.0),
                height: 3.5,
                text: "Scale 1:2".into(),
                align: Align::Left,
            }],
            ..SheetPicture::default()
        };
        let text = write_dxf(&pic);
        let f = read_dxf(&text).expect("reads");
        assert_eq!(f.version.as_deref(), Some("AC1009"));
        assert!(f.layers.contains(&("HIDDEN".to_string(), "DASHED".to_string())));
        let kinds: Vec<&str> = f.entities.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(kinds, ["LINE", "CIRCLE", "ARC", "POLYLINE", "POLYLINE", "TEXT"]);
        let e = &f.entities;
        assert_eq!(
            (e[0].point(10), e[0].point(11)),
            (Some(DVec2::new(1.0, 2.0)), Some(DVec2::new(3.5, 2.0)))
        );
        assert_eq!(
            (e[1].layer(), e[1].point(10), e[1].num(40)),
            ("HIDDEN", Some(DVec2::new(50.0, 60.0)), Some(5.0))
        );
        assert_eq!((e[2].num(50), e[2].num(51)), (Some(0.0), Some(90.0)));
        // The ellipse stays within 0.01 mm of the curve.
        for p in &e[3].vertices {
            let q = DVec2::new(p.x / 10.0, p.y / 4.0);
            assert!((q.length() - 1.0).abs() < 0.01 / 4.0, "{p}");
        }
        assert_eq!(e[4].get(70), Some("1"), "closed");
        assert_eq!(e[4].vertices.len(), 3);
        assert_eq!((e[5].get(1), e[5].num(40)), (Some("Scale 1:2"), Some(3.5)));
        assert!(read_dxf("0\nSECTION\n2").is_err());
        assert!(read_dxf("x\nSECTION\n").is_err());
    }
}
