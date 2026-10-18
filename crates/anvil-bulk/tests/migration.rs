//! End-to-end migrations of generated file trees. No SOLIDWORKS is involved: parts are
//! compound files with standard property sets, and their STEP exports are written by
//! AnvilCAD's own exact exporter, so imported volumes are known exactly.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use anvil_bulk::project::{Kind, Status};
use anvil_bulk::run::{self, RunOptions};
use anvil_bulk::scan::ScanOptions;
use anvil_bulk::Project;
use anvil_io::props::cfb::writer as cfb;
use anvil_io::props::oleps::{self, writer as ps};
use anvil_kernel::v2;
use anvil_model::*;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("anvil-bulk-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(p: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

/// A stand-in for an older SOLIDWORKS file: a compound file with custom properties.
fn sw_file(props: &[(&str, &str)]) -> Vec<u8> {
    let values: Vec<(u32, ps::V)> =
        props.iter().enumerate().map(|(i, (_, v))| (i as u32 + 2, ps::V::Str(v))).collect();
    let dict: Vec<(u32, &str)> = props.iter().enumerate().map(|(i, (n, _))| (i as u32 + 2, *n)).collect();
    let doc = ps::stream(&[(oleps::FMTID_USER_DEFINED, ps::section(1252, &values, &dict))]);
    cfb::build(&[(oleps::DOC_SUMMARY_STREAM, &doc), ("Contents", props.len().to_string().as_bytes())])
}

/// Exact STEP for a w x h x d block.
fn block_step(w: f64, h: f64, d: f64) -> String {
    let mut doc = Document::new();
    let mut sf = SketchFeature::new(PlaneRef::Front);
    sf.sketch.add_rectangle(v2(0.0, 0.0), v2(w, h));
    let sk = doc.add_feature(FeatureKind::Sketch(sf));
    doc.add_feature(FeatureKind::Extrude(ExtrudeFeature {
        sketch: sk,
        end: EndCondition::Blind,
        depth: Dim::Value(d),
        reverse: false,
        op: BodyOp::Boss,
        ..Default::default()
    }));
    let st = doc.rebuild();
    assert!(st.errors.is_empty(), "{:?}", st.errors);
    step::exact_step(&st, "block").expect("exact block")
}

fn volume(part: &Path) -> f64 {
    let mut doc = Document::load(part).unwrap();
    doc.rebuild().body.mass_properties().volume
}

fn prop(part: &Path, name: &str) -> Option<String> {
    let doc = Document::load(part).unwrap();
    doc.custom_properties.iter().find(|p| p.name == name).map(|p| p.value.clone())
}

fn quiet(_: &str) {}

fn opts(worker: &str) -> RunOptions {
    let mut o = RunOptions::new(worker);
    o.threads = 2;
    o
}

fn csv_rows(p: &Path) -> Vec<String> {
    std::fs::read_to_string(p).unwrap().lines().skip(1).map(str::to_string).collect()
}

#[test]
fn migrates_a_file_share() {
    let d = scratch("share");
    let share = d.join("share");
    let wheel_bytes =
        sw_file(&[("Description", "large wheel"), ("Process", "Lathe"), ("Finish", "galvanized")]);
    write(&share.join("projA/wheel.sldprt"), &wheel_bytes);
    write(&share.join("projA/wheel.step"), block_step(40.0, 20.0, 10.0).as_bytes());
    write(&share.join("projA/~$wheel.sldprt"), b"lock");
    write(&share.join("projB/copy/wheel_copy.sldprt"), &wheel_bytes);
    write(&share.join("projB/axle.SLDPRT"), &sw_file(&[("Description", "axle")]));
    write(&share.join("projB/hub.sldprt"), &sw_file(&[("Description", "hub")]));
    write(&share.join("asm/cart.sldasm"), &sw_file(&[("Description", "cart")]));
    // STEP exports kept in a separate tree that mirrors the share.
    let steps = d.join("steps");
    write(&steps.join("projB/axle.stp"), block_step(10.0, 10.0, 50.0).as_bytes());

    let project = Project::at(d.join("migration"));
    let scan = ScanOptions { roots: vec![share.clone()], step_roots: vec![steps], threads: 3 };
    let s = anvil_bulk::scan(&project, &scan, &quiet).unwrap();
    assert_eq!((s.files, s.jobs, s.with_step, s.hashed), (5, 4, 2, 5));
    let jobs = project.jobs().unwrap();
    assert!(jobs[..3].iter().all(|j| j.kind == Kind::Part), "parts come first");
    assert_eq!(jobs.iter().find(|j| j.path.ends_with("wheel.sldprt")).unwrap().copies, 2);

    let r = anvil_bulk::run(&project, &opts("A"), &quiet).unwrap();
    assert_eq!(r.pending_at_start, 4);
    assert_eq!(r.total(), 4);
    let count = |st: Status| r.done.iter().find(|(k, _)| *k == st).map(|(_, n)| *n).unwrap_or(0);
    assert_eq!(
        (count(Status::Exact), count(Status::NoGeometrySource), count(Status::PropertiesOnly)),
        (2, 1, 1)
    );
    let wheel = jobs.iter().find(|j| j.path.ends_with("wheel.sldprt")).unwrap();
    let res = project.result(&wheel.hash).unwrap();
    assert!((res.volume_mm3.unwrap() - 8000.0).abs() < 1e-6);
    assert_eq!((res.bodies, res.exact_bodies), (1, 1));
    assert_eq!(res.properties.as_ref().unwrap().get("Process").unwrap().value, "Lathe");

    // Nothing left to do: a second run is a no-op.
    assert_eq!(anvil_bulk::run(&project, &opts("A"), &quiet).unwrap().total(), 0);

    // AnvilCAD properties added to one copy through anvil-props follow that copy only.
    let copy = share.join("projB/copy/wheel_copy.sldprt");
    let mut side = anvil_io::props::sidecar::Sidecar::default();
    side.properties.push(CustomProperty {
        name: "Cell".into(),
        kind: PropertyKind::Text,
        value: "Lathe 3".into(),
    });
    side.save(&anvil_io::props::sidecar::path_for(&copy, None)).unwrap();

    let out = d.join("migrated");
    let e = anvil_bulk::export(&project, &out, None, false).unwrap();
    assert_eq!((e.written, e.kept, e.merged, e.without_part), (3, 0, 1, 2), "{:?}", e.problems);
    let a = out.join("share/projA/wheel.anvprt");
    let b = out.join("share/projB/copy/wheel_copy.anvprt");
    let c = out.join("share/projB/axle.anvprt");
    assert!((volume(&a) - 8000.0).abs() < 1e-6);
    assert!((volume(&b) - 8000.0).abs() < 1e-6);
    assert!((volume(&c) - 5000.0).abs() < 1e-6);
    assert_eq!(prop(&a, "Description").as_deref(), Some("large wheel"));
    assert_eq!(prop(&a, "Finish").as_deref(), Some("galvanized"));
    assert_eq!(prop(&a, "Cell"), None);
    assert_eq!(prop(&b, "Cell").as_deref(), Some("Lathe 3"));
    assert_eq!(prop(&c, "Description").as_deref(), Some("axle"));
    assert!(!out.join("share/projB/hub.anvprt").exists());

    // Exporting again leaves existing (possibly edited) parts alone.
    let e = anvil_bulk::export(&project, &out, None, false).unwrap();
    assert_eq!((e.written, e.kept), (0, 3));

    // The originals were only read.
    assert_eq!(std::fs::read(share.join("projA/wheel.sldprt")).unwrap(), wheel_bytes);

    let files = csv_rows(&project.reports_dir().join("files.csv"));
    assert_eq!(files.len(), 5);
    let copy_row = files.iter().find(|r| r.contains("wheel_copy.sldprt")).unwrap();
    // Build the original's path component by component so separators match the OS (the
    // report stores native paths: backslashes on Windows).
    let original = share.join("projA").join("wheel.sldprt").display().to_string();
    assert!(copy_row.contains(&original), "{copy_row}\nexpected to contain {original}");
    assert!(copy_row.contains(",exact,step,1,1,8000.000,3,"), "{copy_row}");
    let hub_row = files.iter().find(|r| r.contains("hub.sldprt")).unwrap();
    assert!(hub_row.contains("no geometry source"), "{hub_row}");
    let props = csv_rows(&project.reports_dir().join("properties.csv"));
    assert_eq!(props.len(), 3 + 3 + 1 + 1 + 1);
    let summary = csv_rows(&project.reports_dir().join("summary.csv"));
    // group,key,files,unique,exact,faceted,parametric,hybrid,properties only,no geometry source,failed,pending
    assert_eq!(summary[0], "all,,5,4,3,0,0,0,1,1,0,0");

    // A STEP export added later: rescanning reads no SOLIDWORKS file again, and only the
    // hub is converted.
    write(&share.join("projB/hub.STEP"), block_step(5.0, 6.0, 7.0).as_bytes());
    let s =
        anvil_bulk::scan(&project, &ScanOptions { threads: 2, ..ScanOptions::default() }, &quiet).unwrap();
    assert_eq!((s.files, s.hashed), (5, 0));
    let r = anvil_bulk::run(&project, &opts("B"), &quiet).unwrap();
    assert_eq!((r.pending_at_start, r.total()), (1, 1));
    let e = anvil_bulk::export(&project, &out, None, false).unwrap();
    assert_eq!((e.written, e.kept), (1, 3));
    assert!((volume(&out.join("share/projB/hub.anvprt")) - 210.0).abs() < 1e-9);
}

#[test]
fn workers_share_the_jobs_and_never_do_one_twice() {
    let d = scratch("workers");
    let share = d.join("share");
    for i in 0..12 {
        let n = format!("p{i:02}");
        write(&share.join(format!("{n}.sldprt")), &sw_file(&[("PartNo", &n)]));
        write(&share.join(format!("{n}.step")), block_step(1.0 + i as f64, 2.0, 3.0).as_bytes());
    }
    let project = Project::at(d.join("migration"));
    anvil_bulk::scan(
        &project,
        &ScanOptions { roots: vec![share], threads: 4, ..ScanOptions::default() },
        &quiet,
    )
    .unwrap();
    let (a, b) = std::thread::scope(|s| {
        let a = s.spawn(|| anvil_bulk::run(&project, &opts("A"), &quiet).unwrap());
        let b = s.spawn(|| anvil_bulk::run(&project, &opts("B"), &quiet).unwrap());
        (a.join().unwrap(), b.join().unwrap())
    });
    assert_eq!(a.total() + b.total(), 12, "A did {}, B did {}", a.total(), b.total());
    for j in project.jobs().unwrap() {
        let r = project.result(&j.hash).unwrap();
        let i: f64 = j.path[j.path.len() - 9..j.path.len() - 7].parse().unwrap();
        assert!((r.volume_mm3.unwrap() - (1.0 + i) * 6.0).abs() < 1e-9, "{}", j.path);
    }
    assert_eq!(anvil_bulk::report::progress(&project, false).unwrap().claimed, 0);
}

#[test]
fn stale_claims_are_taken_over_and_fresh_ones_are_not() {
    let d = scratch("claims");
    let share = d.join("share");
    write(&share.join("a.sldprt"), &sw_file(&[("PartNo", "A")]));
    write(&share.join("b.sldprt"), &sw_file(&[("PartNo", "B")]));
    let project = Project::at(d.join("migration"));
    anvil_bulk::scan(
        &project,
        &ScanOptions { roots: vec![share], threads: 1, ..ScanOptions::default() },
        &quiet,
    )
    .unwrap();
    let jobs = project.jobs().unwrap();
    // A worker on another machine holds the first job, then stops without finishing.
    let held = run::try_claim(&project, &jobs[0].hash, "other", Duration::from_secs(3600)).unwrap().unwrap();
    std::mem::forget(held);

    let r = anvil_bulk::run(&project, &opts("A"), &quiet).unwrap();
    assert_eq!((r.total(), r.busy), (1, 1));
    assert!(project.result(&jobs[0].hash).is_none());

    let mut o = opts("A");
    o.claim_timeout = Duration::ZERO;
    let r = anvil_bulk::run(&project, &o, &quiet).unwrap();
    assert_eq!((r.total(), r.busy), (1, 0));
    assert_eq!(project.result(&jobs[0].hash).unwrap().status, Status::NoGeometrySource);
    assert_eq!(anvil_bulk::report::progress(&project, false).unwrap().pending, 0);
}

#[test]
fn damaged_step_files_fail_alone() {
    let d = scratch("damaged");
    let share = d.join("share");
    write(&share.join("bad.sldprt"), &sw_file(&[("PartNo", "bad")]));
    write(&share.join("bad.step"), b"ISO-10303-21;\nHEADER;\nnot really;\n");
    write(&share.join("good.sldprt"), &sw_file(&[("PartNo", "good")]));
    write(&share.join("good.step"), block_step(2.0, 2.0, 2.0).as_bytes());
    let project = Project::at(d.join("migration"));
    anvil_bulk::scan(
        &project,
        &ScanOptions { roots: vec![share], threads: 1, ..ScanOptions::default() },
        &quiet,
    )
    .unwrap();
    let r = anvil_bulk::run(&project, &opts("A"), &quiet).unwrap();
    assert_eq!(r.total(), 2);
    let jobs = project.jobs().unwrap();
    let st =
        |name: &str| project.result(&jobs.iter().find(|j| j.path.ends_with(name)).unwrap().hash).unwrap();
    assert_eq!(st("bad.sldprt").status, Status::Failed);
    assert!(!st("bad.sldprt").messages.is_empty());
    assert_eq!(st("good.sldprt").status, Status::Exact);
    // Failures stay done unless retried.
    assert_eq!(anvil_bulk::run(&project, &opts("A"), &quiet).unwrap().total(), 0);
    let mut o = opts("A");
    o.retry_failed = true;
    assert_eq!(anvil_bulk::run(&project, &o, &quiet).unwrap().total(), 1);
}

/// The anvil-migrate plug-in contract, checked against a stand-in program that writes the
/// same report anvil-migrate writes.
#[cfg(unix)]
#[test]
fn anvil_migrate_plug_in() {
    use std::os::unix::fs::PermissionsExt;
    let d = scratch("plugin");
    let share = d.join("share");
    // Two different parts with the same file name must go to separate batches.
    write(&share.join("x/wheel.sldprt"), &sw_file(&[("PartNo", "W1")]));
    write(&share.join("y/wheel.sldprt"), &sw_file(&[("PartNo", "W2")]));
    write(&share.join("y/frame.sldprt"), &sw_file(&[("PartNo", "F")]));
    write(&share.join("cart.sldasm"), &sw_file(&[("PartNo", "C")]));
    let template = d.join("template.anvprt");
    Document::new().save(&template).unwrap();
    let calls = d.join("calls.log");
    let script = format!(
        r#"#!/bin/sh
out="$2"; shift 2
echo "$#" >> "{calls}"
mkdir -p "$out"
printf '{{"files":[' > "$out/migration-report.json"
sep=""
for f in "$@"; do
  name=$(basename "$f"); stem="${{name%.*}}"
  cp "{template}" "$out/$stem.anvprt"
  printf '%s{{"source":"%s","output":"%s","status":"Parametric","verified":true,"anvil_volume_mm3":123.0,"warnings":[]}}' "$sep" "$f" "$out/$stem.anvprt" >> "$out/migration-report.json"
  sep=","
done
printf ']}}' >> "$out/migration-report.json"
"#,
        calls = calls.display(),
        template = template.display()
    );
    let exe = d.join("fake-anvil-migrate");
    std::fs::write(&exe, script).unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();

    let project = Project::at(d.join("migration"));
    anvil_bulk::scan(
        &project,
        &ScanOptions { roots: vec![share], threads: 2, ..ScanOptions::default() },
        &quiet,
    )
    .unwrap();
    let mut o = opts("A");
    o.converter = run::Converter::Migrate;
    o.migrate_exe = exe;
    let r = anvil_bulk::run(&project, &o, &quiet).unwrap();
    assert_eq!(r.total(), 4);
    let calls = std::fs::read_to_string(&calls).unwrap();
    let mut sizes: Vec<&str> = calls.lines().collect();
    sizes.sort();
    assert_eq!(sizes, ["1", "2"], "the second wheel.sldprt went to its own batch");
    for j in project.jobs().unwrap() {
        let res = project.result(&j.hash).unwrap();
        if j.kind == Kind::Part {
            assert_eq!(
                (res.status, res.converter.as_str(), res.volume_mm3),
                (Status::Parametric, "anvil-migrate", Some(123.0))
            );
            assert!(project.part_path(&j.hash).exists());
            assert!(res.properties.is_some());
        } else {
            assert_eq!(res.status, Status::PropertiesOnly);
        }
    }
}
