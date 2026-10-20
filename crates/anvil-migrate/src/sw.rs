//! A SOLIDWORKS session and helpers for reading documents through its API.

use crate::com::{self, Arg, ComError, ComResult, Disp, Value};
use anvil_kernel::{v3, Frame, Vec3};

/// SOLIDWORKS stores lengths in metres; AnvilCAD uses millimetres.
pub const M_TO_MM: f64 = 1000.0;

// Enum values read from SolidWorks.Interop.swconst (SOLIDWORKS 2024).
pub const SW_DOC_PART: i32 = 1;
pub const SW_OPEN_SILENT: i32 = 1;
pub const SW_OPEN_READ_ONLY: i32 = 2;
pub const SW_ROLLBACK_TO_END: i32 = 1;
pub const SW_ROLLBACK_AFTER_FEATURE: i32 = 4;
pub const SW_SOLID_BODY: i32 = 0;

pub struct Session {
    pub app: Disp,
    started: bool,
    pub revision: String,
}

/// A document opened for migration.
pub struct OpenDoc {
    pub model: Disp,
    /// False when the user already had the file open; we then leave it alone.
    pub opened_by_us: bool,
}

impl Session {
    pub fn connect(progid: &str) -> ComResult<Session> {
        com::init()?;
        let (app, started) = com::connect(progid)?;
        if started {
            // A background instance: no window, no user interaction.
            let _ = app.put("Visible", Arg::Bool(false));
            let _ = app.put("UserControl", Arg::Bool(false));
        }
        let revision = app.string("RevisionNumber", &[])?;
        Ok(Session { app, started, revision })
    }

    /// Release year from the revision number (major 32 is SOLIDWORKS 2024).
    pub fn release_name(&self) -> String {
        let major: i32 = self.revision.split('.').next().and_then(|m| m.parse().ok()).unwrap_or(0);
        if major > 0 {
            format!("SOLIDWORKS {}", major + 1992)
        } else {
            format!("SOLIDWORKS ({})", self.revision)
        }
    }

    pub fn open_part(&self, path: &str) -> ComResult<OpenDoc> {
        if let Some(model) = self.app.opt_obj("GetOpenDocumentByName", &[path.into()])? {
            return Ok(OpenDoc { model, opened_by_us: false });
        }
        let (doc, outs) = self.app.call_out(
            "OpenDoc6",
            &[
                path.into(),
                SW_DOC_PART.into(),
                (SW_OPEN_SILENT | SW_OPEN_READ_ONLY).into(),
                "".into(),
                Arg::OutInt,
                Arg::OutInt,
            ],
        )?;
        match doc {
            Value::Disp(model) => Ok(OpenDoc { model, opened_by_us: true }),
            _ => Err(ComError(format!(
                "SOLIDWORKS could not open the file (error code {:?}, warnings {:?})",
                outs.first(),
                outs.get(1)
            ))),
        }
    }

    pub fn close(&self, doc: &OpenDoc) {
        if doc.opened_by_us {
            if let Ok(title) = doc.model.string("GetTitle", &[]) {
                let _ = self.app.call("CloseDoc", &[title.as_str().into()]);
            }
        }
    }

    pub fn shutdown(self) {
        if self.started {
            let _ = self.app.call("ExitApp", &[]);
        }
    }
}

/// Decodes a SOLIDWORKS MathTransform array (rotation 3x3, translation, scale) that maps
/// local coordinates to model coordinates using SOLIDWORKS' row-vector convention:
/// model = local * R * scale + T.
pub fn local_to_model(a: &[f64]) -> Option<Frame> {
    if a.len() < 13 {
        return None;
    }
    let s = if a[12].abs() > 1e-12 { a[12] } else { 1.0 };
    let row = |i: usize| v3(a[3 * i], a[3 * i + 1], a[3 * i + 2]) * s;
    Some(Frame { origin: v3(a[9], a[10], a[11]) * M_TO_MM, x: row(0), y: row(1), z: row(2) })
}

/// Decodes a model-to-local transform (such as ISketch.ModelToSketchTransform) into the
/// local frame expressed in model coordinates.
pub fn model_to_local(a: &[f64]) -> Option<Frame> {
    if a.len() < 13 {
        return None;
    }
    // local = model * R + T, so the local axes are the columns of R and the local origin
    // satisfies model * R = -T, i.e. origin = -T * R^T.
    let col = |j: usize| v3(a[j], a[3 + j], a[6 + j]);
    let (x, y, z) = (col(0), col(1), col(2));
    let t = v3(a[9], a[10], a[11]);
    let origin = -(x * t.x + y * t.y + z * t.z);
    Some(Frame { origin: origin * M_TO_MM, x: x.normalized(), y: y.normalized(), z: z.normalized() })
}

pub fn frames_equal(a: &Frame, b: &Frame) -> bool {
    a.origin.distance(b.origin) < 1e-6
        && (a.x - b.x).length() < 1e-9
        && (a.y - b.y).length() < 1e-9
        && (a.z - b.z).length() < 1e-9
}

/// Stable key for a sketch point or segment from its GetID() pair.
pub fn id_key(obj: &Disp) -> ComResult<(i64, i64)> {
    let ids = obj.call("GetID", &[])?.list();
    Ok((ids.first().and_then(|v| v.i64()).unwrap_or(-1), ids.get(1).and_then(|v| v.i64()).unwrap_or(-1)))
}

/// A point in the middle of a SOLIDWORKS edge (mm), on its exact curve.
pub fn edge_midpoint(edge: &Disp) -> Option<Vec3> {
    let p = edge.call("GetCurveParams2", &[]).ok()?.floats();
    if p.len() < 8 {
        return None;
    }
    let chord_mid = v3(p[0] + p[3], p[1] + p[4], p[2] + p[5]) * (0.5 * M_TO_MM);
    let mid = 0.5 * (p[6] + p[7]);
    let on_curve = edge
        .obj("GetCurve", &[])
        .and_then(|c| c.call("Evaluate2", &[mid.into(), 0.into()]))
        .map(|v| v.floats())
        .ok()
        .filter(|e| e.len() >= 3)
        .map(|e| v3(e[0], e[1], e[2]) * M_TO_MM);
    Some(on_curve.unwrap_or(chord_mid))
}

/// A point on a SOLIDWORKS face (mm): the centre of the largest display triangle.
pub fn face_point(face: &Disp) -> Option<Vec3> {
    let t = face.call("GetTessTriangles", &[true.into()]).ok()?.floats();
    t.as_chunks::<9>()
        .0
        .iter()
        .map(|c| {
            let (a, b, d) = (v3(c[0], c[1], c[2]), v3(c[3], c[4], c[5]), v3(c[6], c[7], c[8]));
            ((b - a).cross(d - a).length(), (a + b + d) * (M_TO_MM / 3.0))
        })
        .max_by(|x, y| x.0.total_cmp(&y.0))
        .map(|x| x.1)
}

/// Total solid volume in mm^3 according to SOLIDWORKS.
pub fn sw_volume(model: &Disp) -> ComResult<f64> {
    let ext = model.obj("Extension", &[])?;
    let mp = ext.obj("CreateMassProperty", &[])?;
    Ok(mp.f64("Volume", &[])? * 1e9)
}

/// A line in space in millimetres: (point, direction).
pub fn axis_of(obj: &Disp) -> Option<(Vec3, Vec3)> {
    let floats = |v: ComResult<Value>| v.ok().map(|v| v.floats()).filter(|f| f.len() >= 6);
    let line = |f: Vec<f64>| (v3(f[0], f[1], f[2]) * M_TO_MM, v3(f[3], f[4], f[5]));
    // Edge: straight edges give their line, circular edges their axis.
    if let Ok(curve) = obj.obj("GetCurve", &[]) {
        if curve.bool("IsLine", &[]).unwrap_or(false) {
            return floats(curve.call("LineParams", &[])).map(line);
        }
        if curve.bool("IsCircle", &[]).unwrap_or(false) {
            return floats(curve.call("CircleParams", &[])).map(line);
        }
        return None;
    }
    // Cylindrical face.
    if let Ok(surf) = obj.obj("GetSurface", &[]) {
        if surf.bool("IsCylinder", &[]).unwrap_or(false) {
            return floats(surf.call("CylinderParams", &[])).map(line);
        }
        return None;
    }
    // Reference axis (a feature, or the axis object itself).
    let ra = obj.opt_obj("GetSpecificFeature2", &[]).ok().flatten().unwrap_or_else(|| obj.clone());
    if let Some(f) = floats(ra.call("GetRefAxisParams", &[])) {
        let (p, q) = (v3(f[0], f[1], f[2]) * M_TO_MM, v3(f[3], f[4], f[5]) * M_TO_MM);
        return Some((p, q - p));
    }
    // Sketch line: endpoints are in sketch space, so map through the sketch transform.
    if let (Ok(a), Ok(b), Ok(sk)) =
        (obj.obj("GetStartPoint2", &[]), obj.obj("GetEndPoint2", &[]), obj.obj("GetSketch", &[]))
    {
        let frame = sk
            .obj("ModelToSketchTransform", &[])
            .ok()
            .and_then(|t| t.call("ArrayData", &[]).ok())
            .and_then(|d| model_to_local(&d.floats()))?;
        let pt = |p: &Disp| -> Option<Vec3> {
            Some(frame.to_world3(v3(
                p.f64("X", &[]).ok()? * M_TO_MM,
                p.f64("Y", &[]).ok()? * M_TO_MM,
                p.f64("Z", &[]).ok()? * M_TO_MM,
            )))
        };
        let (pa, pb) = (pt(&a)?, pt(&b)?);
        return Some((pa, pb - pa));
    }
    None
}
