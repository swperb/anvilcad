//! Vector PDF output of a laid-out sheet (see `docs/re/drawings.md`, "PDF export").
//!
//! One page the size of the sheet, black lines on white: straight lines as line segments,
//! circles and ellipses as cubic Bezier curves (four per full turn, within 0.03% of the
//! radius), with the pens' widths and dashes. Text uses the standard Helvetica font, which
//! every PDF reader has, so nothing is embedded. Nothing is drawn as an image.

use crate::hlr::Path2;
use crate::layout::{Align, Note, Pen, SheetPicture, text_width};
use glam::DVec2;
use std::f64::consts::FRAC_PI_2;
use std::fmt::Write as _;

/// PDF points per millimetre.
const PT: f64 = 72.0 / 25.4;

fn num(out: &mut String, v: f64) {
    // Three decimals of a point is under a micrometre.
    let v = if v.abs() < 5e-4 { 0.0 } else { v };
    let _ = write!(out, "{:.3}", v);
    while out.ends_with('0') {
        out.pop();
    }
    if out.ends_with('.') {
        out.pop();
    }
    out.push(' ');
}

fn point(out: &mut String, p: DVec2) {
    num(out, p.x * PT);
    num(out, p.y * PT);
}

/// Appends the path's drawing operators (without the stroke).
fn path_ops(out: &mut String, path: &Path2) {
    match path {
        Path2::Line { a, b } => {
            point(out, *a);
            out.push_str("m\n");
            point(out, *b);
            out.push_str("l\n");
        }
        Path2::Poly(p) => {
            let Some(first) = p.first() else { return };
            point(out, *first);
            out.push_str("m\n");
            for q in &p[1..] {
                point(out, *q);
                out.push_str("l\n");
            }
        }
        Path2::Conic { c, u, v, t0, t1 } => {
            let at = |t: f64| *c + *u * t.cos() + *v * t.sin();
            let tangent = |t: f64| -*u * t.sin() + *v * t.cos();
            let n = ((t1 - t0).abs() / FRAC_PI_2).ceil().max(1.0) as usize;
            let h = (t1 - t0) / n as f64;
            // The affine image of a circular arc's Bezier is the image arc's Bezier.
            let k = 4.0 / 3.0 * (h / 4.0).tan();
            point(out, at(*t0));
            out.push_str("m\n");
            for i in 0..n {
                let (a, b) = (t0 + h * i as f64, t0 + h * (i + 1) as f64);
                point(out, at(a) + tangent(a) * k);
                point(out, at(b) - tangent(b) * k);
                point(out, at(b));
                out.push_str("c\n");
            }
        }
    }
}

/// `text` as a PDF string in WinAnsi encoding: brackets and backslashes escaped, Latin-1
/// letters and signs (such as Ø and °, which WinAnsi places where Latin-1 does) as octal
/// escapes, anything else shown as `?`.
fn pdf_string(text: &str) -> String {
    let mut s = String::from("(");
    for ch in text.chars() {
        match ch {
            '(' | ')' | '\\' => {
                s.push('\\');
                s.push(ch);
            }
            ' '..='~' => s.push(ch),
            '\u{a0}'..='\u{ff}' => {
                let _ = write!(s, "\\{:03o}", ch as u32);
            }
            _ => s.push('?'),
        }
    }
    s.push(')');
    s
}

fn set_pen(out: &mut String, pen: Pen) {
    num(out, pen.width() * PT);
    out.push_str("w\n");
    match pen.dash() {
        Some(dash) => {
            out.push('[');
            for d in dash {
                num(out, d * PT);
            }
            out.push_str("] 0 d\n");
        }
        None => out.push_str("[] 0 d\n"),
    }
}

fn note(out: &mut String, n: &Note) {
    // Font size from the capital height: Helvetica capitals are 0.718 em high.
    let size = n.height / 0.718 * PT;
    let width = text_width(&n.text) * n.height * PT;
    let x = n.pos.x * PT
        - match n.align {
            Align::Left => 0.0,
            Align::Center => width * 0.5,
            Align::Right => width,
        };
    out.push_str("BT /F1 ");
    num(out, size);
    out.push_str("Tf ");
    num(out, x);
    num(out, n.pos.y * PT);
    let _ = writeln!(out, "Td {} Tj ET", pdf_string(&n.text));
}

/// The page's content stream.
fn content(pic: &SheetPicture) -> String {
    let mut out = String::new();
    out.push_str("1 J 1 j 0 G\n");
    let mut pen: Option<Pen> = None;
    for s in &pic.strokes {
        if pen != Some(s.pen) {
            set_pen(&mut out, s.pen);
            pen = Some(s.pen);
        }
        path_ops(&mut out, &s.path);
        out.push_str("S\n");
    }
    if !pic.dims.is_empty() {
        set_pen(&mut out, Pen::Dim);
    }
    for d in &pic.dims {
        for p in &d.lines {
            path_ops(&mut out, p);
            out.push_str("S\n");
        }
        for a in &d.arrows {
            point(&mut out, a[0]);
            out.push_str("m\n");
            point(&mut out, a[1]);
            out.push_str("l\n");
            point(&mut out, a[2]);
            out.push_str("l\nh f\n");
        }
    }
    let fill = |out: &mut String, a: &[DVec2; 3]| {
        point(out, a[0]);
        out.push_str("m\n");
        point(out, a[1]);
        out.push_str("l\n");
        point(out, a[2]);
        out.push_str("l\nh f\n");
    };
    for (_, t) in &pic.fills {
        fill(&mut out, t);
    }
    if !pic.callouts.is_empty() {
        set_pen(&mut out, Pen::Dim);
    }
    for c in &pic.callouts {
        for p in &c.lines {
            path_ops(&mut out, p);
            out.push_str("S\n");
        }
        fill(&mut out, &c.arrow);
    }
    let texts = pic
        .notes
        .iter()
        .chain(pic.dims.iter().map(|d| &d.text))
        .chain(pic.labels.iter())
        .chain(pic.callouts.iter().flat_map(|c| c.text.iter()));
    for n in texts {
        note(&mut out, n);
    }
    out
}

/// The sheet as a PDF file. `title` goes in the document information.
pub fn write_pdf(pic: &SheetPicture, title: &str) -> Vec<u8> {
    let body = content(pic);
    let mut w = String::new();
    num(&mut w, pic.width * PT);
    num(&mut w, pic.height * PT);
    let media = w.trim_end().to_string();
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {media}] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
        ),
        format!("<< /Length {} >>\nstream\n{body}endstream", body.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string(),
        format!("<< /Title {} /Producer (AnvilCAD) >>", pdf_string(title)),
    ];
    let mut out = String::from("%PDF-1.4\n");
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        let _ = write!(out, "{} 0 obj\n{o}\nendobj\n", i + 1);
    }
    let xref = out.len();
    let _ = write!(out, "xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for off in offsets {
        let _ = writeln!(out, "{off:010} 00000 n ");
    }
    let _ = write!(
        out,
        "trailer\n<< /Size {} /Root 1 0 R /Info {} 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1,
        objects.len()
    );
    out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Stroke;

    #[test]
    fn numbers_are_short() {
        let mut s = String::new();
        num(&mut s, 12.5);
        num(&mut s, 3.0);
        num(&mut s, -0.0001);
        num(&mut s, 1.23456);
        assert_eq!(s, "12.5 3 0 1.235 ");
    }

    #[test]
    fn strings_are_escaped() {
        assert_eq!(pdf_string("a(b)\\c\u{e9}\u{2014}"), "(a\\(b\\)\\\\c\\351?)");
        assert_eq!(pdf_string("\u{d8}8 30\u{b0}"), "(\\3308 30\\260)");
    }

    #[test]
    fn a_full_circle_is_four_beziers_through_its_quadrant_points() {
        let mut s = String::new();
        let r = 25.4 / 72.0 * 10.0; // 10 pt
        path_ops(
            &mut s,
            &Path2::Conic {
                c: DVec2::ZERO,
                u: DVec2::X * r,
                v: DVec2::Y * r,
                t0: 0.0,
                t1: std::f64::consts::TAU,
            },
        );
        assert_eq!(s.matches(" c\n").count(), 4);
        assert!(s.starts_with("10 0 m\n10 5.523 5.523 10 0 10 c\n"), "{s}");
    }

    #[test]
    fn the_file_has_a_valid_cross_reference_table() {
        let pic = SheetPicture {
            width: 297.0,
            height: 210.0,
            strokes: vec![Stroke {
                pen: Pen::Hidden,
                path: Path2::Line { a: DVec2::ZERO, b: DVec2::new(25.4, 0.0) },
                view: None,
            }],
            notes: vec![Note {
                pos: DVec2::new(100.0, 10.0),
                height: 3.5,
                text: "Scale 1:1".into(),
                align: Align::Left,
            }],
            ..SheetPicture::default()
        };
        let bytes = write_pdf(&pic, "test");
        let text = String::from_utf8(bytes).expect("ascii");
        assert!(text.contains("/MediaBox [0 0 841.89 595.276]"));
        assert!(text.contains("0 0 m\n72 0 l\nS\n"));
        assert!(text.contains("[8.504 2.835 ] 0 d"));
        assert!(text.contains("(Scale 1:1) Tj"));
        // Every object starts where the table says it does.
        let xref_at: usize = text
            .rsplit("startxref\n")
            .next()
            .and_then(|t| t.lines().next())
            .and_then(|n| n.parse().ok())
            .expect("startxref");
        let table = &text[xref_at..];
        for (i, line) in table.lines().skip(3).take(6).enumerate() {
            let off: usize = line[..10].parse().expect("offset");
            assert!(text[off..].starts_with(&format!("{} 0 obj", i + 1)), "object {}", i + 1);
        }
    }
}
