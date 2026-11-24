//! Files saved by earlier builds must reopen and rebuild to the same geometry.
//!
//! `tests/fixtures/<build>/` holds parts (`.anvprt`), templates (`.anvprtdot`) and
//! assemblies (`.anvasm`) that the named build of AnvilCAD saved, each with a
//! `<file>.expected.json` beside it recording which build saved it and what reopening it
//! gives: per configuration, the body count, volume, surface area, face count and
//! bounding box. `fixture_generator.rs` writes them; see there before adding any.
//!
//! These tests open every fixture with this build and check that:
//!
//! - it rebuilds without errors to the recorded geometry, to a tight tolerance;
//! - saving it and opening it again gives the same JSON;
//! - every format version this build reads (and every legacy format id) has a fixture,
//!   so raising a format version constant fails here until a fixture of the new version
//!   is committed;
//! - every feature type appears in at least one fixture.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use anvil_model::assembly::{Assembly, Parts, ASSEMBLY_FORMAT, ASSEMBLY_FORMAT_VERSION};
use anvil_model::{
    Document, FeatureKind, LEGACY_PART_FORMATS, PART_FORMAT, PART_FORMAT_VERSION, PART_TEMPLATE_EXT,
};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Volume and surface area may differ from the recorded value by this fraction of it.
const REL_TOL: f64 = 1e-7;
/// Bounding box coordinates may differ from the recorded ones by this much (mm).
const ABS_TOL: f64 = 1e-6;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Part,
    Assembly,
}

struct Fixture {
    path: PathBuf,
    kind: Kind,
    text: String,
    raw: Value,
    expected: Value,
}

impl Fixture {
    fn name(&self) -> String {
        let dir = self.path.parent().and_then(|p| p.file_name()).unwrap_or_default();
        format!("{}/{}", dir.to_string_lossy(), self.path.file_name().unwrap_or_default().to_string_lossy())
    }
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

/// Every fixture, in a stable order.
fn fixtures() -> Vec<Fixture> {
    let mut paths = Vec::new();
    for build in std::fs::read_dir(fixtures_dir()).unwrap() {
        let build = build.unwrap().path();
        if !build.is_dir() {
            continue;
        }
        for f in std::fs::read_dir(&build).unwrap() {
            paths.push(f.unwrap().path());
        }
    }
    paths.sort();
    let mut out = Vec::new();
    for path in paths {
        let kind = match path.extension().and_then(|e| e.to_str()) {
            Some("anvprt" | "anvprtdot") => Kind::Part,
            Some("anvasm") => Kind::Assembly,
            _ => continue,
        };
        let mut exp = path.as_os_str().to_owned();
        exp.push(".expected.json");
        let exp = PathBuf::from(exp);
        let expected = std::fs::read_to_string(&exp)
            .unwrap_or_else(|e| panic!("{} has no expected values ({}): {e}", path.display(), exp.display()));
        let text = std::fs::read_to_string(&path).unwrap();
        let raw = serde_json::from_str(&text).unwrap();
        out.push(Fixture { path, kind, text, raw, expected: serde_json::from_str(&expected).unwrap() });
    }
    assert!(!out.is_empty(), "no fixtures found in {}", fixtures_dir().display());
    out
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("{v} is not a number"))
}

/// Differences between measured and recorded geometry, as readable lines.
fn compare(what: &str, got: &Value, want: &Value, out: &mut Vec<String>) {
    for key in ["bodies", "faces"] {
        if got[key] != want[key] {
            out.push(format!("{what}: {key} is {}, recorded {}", got[key], want[key]));
        }
    }
    for key in ["volume", "surface_area"] {
        let (g, w) = (num(&got[key]), num(&want[key]));
        if (g - w).abs() > REL_TOL * w.abs() {
            out.push(format!("{what}: {key} is {g}, recorded {w}"));
        }
    }
    for key in ["bbox_min", "bbox_max"] {
        for k in 0..3 {
            let (g, w) = (num(&got[key][k]), num(&want[key][k]));
            if (g - w).abs() > ABS_TOL {
                out.push(format!("{what}: {key}[{k}] is {g}, recorded {w}"));
            }
        }
    }
}

fn check_part(f: &Fixture, out: &mut Vec<String>) {
    let doc = match Document::load(&f.path) {
        Ok(d) => d,
        Err(e) => return out.push(format!("{}: does not open: {e}", f.name())),
    };
    let want = f.expected["configurations"].as_array().unwrap();
    let names: Vec<&str> = doc.configurations.iter().map(|c| c.name.as_str()).collect();
    let want_names: Vec<&str> = want.iter().map(|c| c["name"].as_str().unwrap()).collect();
    if names != want_names {
        out.push(format!("{}: configurations are {names:?}, recorded {want_names:?}", f.name()));
        return;
    }
    let active = doc.configuration(doc.active_configuration).map(|c| c.name.as_str());
    if active != f.expected["active_configuration"].as_str() {
        out.push(format!(
            "{}: active configuration is {active:?}, recorded {}",
            f.name(),
            f.expected["active_configuration"]
        ));
    }
    for (c, w) in doc.configurations.iter().zip(want) {
        let what = format!("{} ({})", f.name(), c.name);
        let st = doc.rebuild_configuration(c.id).unwrap();
        if !st.errors.is_empty() || !st.variable_errors.is_empty() {
            out.push(format!("{what}: rebuild errors {:?} {:?}", st.errors, st.variable_errors));
        }
        compare(&what, &common::part_geometry(&st), w, out);
    }
}

fn check_assembly(f: &Fixture, out: &mut Vec<String>) {
    let mut asm = match Assembly::load(&f.path) {
        Ok(a) => a,
        Err(e) => return out.push(format!("{}: does not open: {e}", f.name())),
    };
    let mut parts = Parts::for_file(&f.path);
    let status = asm.solve(&mut parts);
    for (key, got) in [
        ("components", Value::from(asm.components.len())),
        ("mates", Value::from(asm.mates.len())),
        ("mates_satisfied", Value::from(status.ok())),
    ] {
        if got != f.expected[key] {
            out.push(format!("{}: {key} is {got}, recorded {} ({status:?})", f.name(), f.expected[key]));
        }
    }
    let mut solids = Vec::new();
    for (id, body) in asm.bodies(&mut parts) {
        match body {
            Ok(s) => solids.push(s),
            Err(e) => out.push(format!("{}: component #{id}: {e}", f.name())),
        }
    }
    let bodies = solids.iter().map(common::pieces).sum();
    let refs: Vec<_> = solids.iter().collect();
    compare(&f.name(), &common::geometry(&refs, bodies), &f.expected["geometry"], out);
}

#[test]
fn every_fixture_rebuilds_to_its_recorded_geometry() {
    let mut out = Vec::new();
    for f in fixtures() {
        match f.kind {
            Kind::Part => check_part(&f, &mut out),
            Kind::Assembly => check_assembly(&f, &mut out),
        }
    }
    assert!(out.is_empty(), "fixtures no longer rebuild as recorded:\n{}", out.join("\n"));
}

/// True when everything in `old` is still in `new` with the same value: saving again
/// may add fields with their defaults, but must not drop or change any.
fn kept(old: &Value, new: &Value, at: &str, out: &mut Vec<String>) {
    match (old, new) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in a {
                match b.get(k) {
                    Some(w) => kept(v, w, &format!("{at}.{k}"), out),
                    None => out.push(format!("{at}.{k} was dropped")),
                }
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (v, w)) in a.iter().zip(b).enumerate() {
                kept(v, w, &format!("{at}[{i}]"), out);
            }
        }
        _ if old == new => {}
        _ => out.push(format!("{at} was {old}, is now {new}")),
    }
}

#[test]
fn every_fixture_saves_and_reopens_unchanged() {
    let mut out = Vec::new();
    for f in fixtures() {
        let resaved = match f.kind {
            Kind::Part => Document::load(&f.path).map(|doc| {
                let first = doc.to_json().unwrap();
                let back = Document::from_json(&first).unwrap();
                (first, back.to_json().unwrap(), back == doc)
            }),
            Kind::Assembly => Assembly::load(&f.path).map(|asm| {
                let first = asm.to_json().unwrap();
                let back = Assembly::from_json(&first).unwrap();
                (first, back.to_json().unwrap(), back == asm)
            }),
        };
        let (first, second, same_doc) = match resaved {
            Ok(r) => r,
            Err(e) => {
                out.push(format!("{}: does not open: {e}", f.name()));
                continue;
            }
        };
        if !same_doc {
            out.push(format!("{}: reopening the saved file gives a different document", f.name()));
        }
        if second != first {
            out.push(format!("{}: saving a second time changes the file", f.name()));
        }
        if f.expected["resaves_unchanged"] == Value::Bool(true) {
            if first != f.text {
                out.push(format!("{}: saving it again changes the file", f.name()));
            }
        } else {
            // Saved by a build that wrote less: the migrated file keeps all it had.
            kept(&f.raw, &serde_json::from_str(&first).unwrap(), &f.name(), &mut out);
        }
    }
    assert!(out.is_empty(), "fixtures do not survive saving and reopening:\n{}", out.join("\n"));
}

#[test]
fn every_format_version_has_a_fixture() {
    let all = fixtures();
    let parts: Vec<&Fixture> = all.iter().filter(|f| f.kind == Kind::Part).collect();
    let version = |f: &Fixture| f.raw["format_version"].as_u64().unwrap();
    for v in 1..=u64::from(PART_FORMAT_VERSION) {
        assert!(
            parts.iter().any(|f| f.raw["format"] == PART_FORMAT && version(f) == v),
            "no part fixture of format version {v}: save one with the build that writes it \
             (see fixture_generator.rs)"
        );
    }
    for id in LEGACY_PART_FORMATS {
        assert!(
            parts.iter().any(|f| f.raw["format"] == *id),
            "no part fixture with the legacy format id {id}"
        );
    }
    // Version 1 parts were written both before configurations existed (with no
    // configuration fields at all) and after (with one "Default"); both must open.
    assert!(
        parts.iter().any(|f| version(f) == 1 && f.raw.get("configurations").is_none()),
        "no part fixture from before configurations"
    );
    assert!(
        parts.iter().any(|f| version(f) == 1 && f.raw.get("configurations").is_some()),
        "no version 1 part fixture with its configuration recorded"
    );
    assert!(
        parts.iter().any(|f| f.path.extension().is_some_and(|e| e == PART_TEMPLATE_EXT)),
        "no part template fixture"
    );
    let asms: Vec<&Fixture> = all.iter().filter(|f| f.kind == Kind::Assembly).collect();
    for v in 1..=u64::from(ASSEMBLY_FORMAT_VERSION) {
        assert!(
            asms.iter().any(|f| f.raw["format"] == ASSEMBLY_FORMAT && version(f) == v),
            "no assembly fixture of format version {v}"
        );
    }
}

/// The names of an enum's variants, as serde reads and writes them.
fn variant_names<T: for<'de> serde::Deserialize<'de>>() -> &'static [&'static str] {
    use serde::de::{self, Visitor};
    struct Variants<'a>(&'a mut &'static [&'static str]);
    impl<'de> serde::Deserializer<'de> for Variants<'_> {
        type Error = de::value::Error;
        fn deserialize_any<V: Visitor<'de>>(self, _: V) -> Result<V::Value, Self::Error> {
            Err(de::Error::custom("not an enum"))
        }
        fn deserialize_enum<V: Visitor<'de>>(
            self,
            _: &'static str,
            variants: &'static [&'static str],
            _: V,
        ) -> Result<V::Value, Self::Error> {
            *self.0 = variants;
            Err(de::Error::custom("variants read"))
        }
        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
            option unit unit_struct newtype_struct seq tuple tuple_struct map struct identifier
            ignored_any
        }
    }
    let mut names: &'static [&'static str] = &[];
    let _ = T::deserialize(Variants(&mut names));
    names
}

#[test]
fn every_feature_type_is_in_a_fixture() {
    let all: BTreeSet<&str> = variant_names::<FeatureKind>().iter().copied().collect();
    assert!(all.len() >= 26 && all.contains("Sketch") && all.contains("Rib"), "{all:?}");
    let mut seen = BTreeSet::new();
    for f in fixtures().iter().filter(|f| f.kind == Kind::Part) {
        for feature in f.raw["features"].as_array().unwrap() {
            if let Some(kind) = feature["kind"].as_object() {
                seen.extend(kind.keys().cloned());
            }
        }
    }
    let missing: Vec<&&str> = all.iter().filter(|k| !seen.contains(**k)).collect();
    assert!(missing.is_empty(), "feature types with no fixture: {missing:?} (see fixture_generator.rs)");
}

/// A saved number must read back as the same number, or a file drifts each time it is
/// saved. These values once came back one bit off.
#[test]
fn saved_numbers_reopen_bit_for_bit() {
    for x in [2.9999987932726195e-18, 100.00000000000003, 1.369703827611891e-14, 0.1 + 0.2] {
        let mut doc = Document::new();
        let mut sf = anvil_model::SketchFeature::new(anvil_model::PlaneRef::Front);
        sf.sketch.add_point(anvil_kernel::v2(x, -x));
        doc.add_feature(FeatureKind::Sketch(sf));
        let text = doc.to_json().unwrap();
        let back = Document::from_json(&text).unwrap();
        assert_eq!(back, doc, "{x:e}");
        assert_eq!(back.to_json().unwrap(), text, "{x:e}");
    }
}
