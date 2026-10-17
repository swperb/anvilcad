//! End-to-end: convert a folder tree the way a company would, then read it back.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use anvil_io::props::cfb::writer as cfb;
use anvil_io::props::oleps::{self, writer as ps};
use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("anvil-props-cli-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// An older-format part with custom properties in its standard property set.
fn part(props: &[(&str, &str)]) -> Vec<u8> {
    let values: Vec<(u32, ps::V)> =
        props.iter().enumerate().map(|(i, (_, v))| (i as u32 + 2, ps::V::Str(v))).collect();
    let dict: Vec<(u32, &str)> = props.iter().enumerate().map(|(i, (n, _))| (i as u32 + 2, *n)).collect();
    let user = ps::section(1252, &values, &dict);
    let doc = ps::stream(&[(oleps::FMTID_USER_DEFINED, user)]);
    let summary = ps::stream(&[(oleps::FMTID_SUMMARY, ps::section(1252, &[(4, ps::V::Str("J. Doe"))], &[]))]);
    cfb::build(&[
        (oleps::SUMMARY_STREAM, &summary),
        (oleps::DOC_SUMMARY_STREAM, &doc),
        ("Contents", b"not read by AnvilCAD"),
    ])
}

fn run(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_anvil-props"))
        .args(args)
        .env_remove("ANVIL_SWDM_KEY")
        .env_remove("ANVIL_PROPS_STORE")
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into(),
        String::from_utf8_lossy(&out.stderr).into(),
    )
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn converts_a_share_and_shows_the_properties() {
    let d = scratch("share");
    let share = d.join("share");
    std::fs::create_dir_all(share.join("parts/wheels")).unwrap();
    std::fs::create_dir_all(share.join("modern")).unwrap();
    let wheel = share.join("parts/wheels/wheel.sldprt");
    let wheel_bytes = part(&[("Description", "large wheel"), ("Process", "Lathe"), ("Finish", "galvanized")]);
    std::fs::write(&wheel, &wheel_bytes).unwrap();
    std::fs::write(
        share.join("parts/axle.SLDPRT"),
        part(&[("Description", "axle, 20 mm"), ("Material", "=1045")]),
    )
    .unwrap();
    // A file saved by a newer release: not a compound file, so it needs the Document Manager.
    std::fs::write(share.join("modern/hub.sldprt"), b"newer container").unwrap();
    std::fs::write(share.join("parts/~$wheel.sldprt"), b"lock file").unwrap();
    std::fs::write(share.join("parts/notes.txt"), b"ignored").unwrap();
    let report = d.join("report");

    let (ok, _, err) = run(&["convert", s(&share), "-o", s(&report)]);
    assert!(!ok, "one file needs the Document Manager, so the run reports a failure\n{err}");
    assert!(err.contains("3 SOLIDWORKS file(s) found"), "{err}");
    assert!(err.contains("2 converted, 0 already up to date, 1 failed"), "{err}");

    // The SOLIDWORKS file is untouched; the sidecar sits beside it.
    assert_eq!(std::fs::read(&wheel).unwrap(), wheel_bytes);
    let side = share.join("parts/wheels/wheel.sldprt.anvilprops.json");
    let json = std::fs::read_to_string(&side).unwrap();
    assert!(
        json.contains("\"large wheel\"") && json.contains("\"Lathe\"") && json.contains("\"galvanized\"")
    );
    assert!(!share.join("parts/~$wheel.sldprt.anvilprops.json").exists());

    let index = std::fs::read_to_string(report.join("properties.csv")).unwrap();
    assert!(index.contains("wheel.sldprt,,Process,Text,Lathe,"), "{index}");
    assert!(index.contains("\"axle, 20 mm\""), "{index}");
    assert!(index.contains(",'=1045,"), "formula-like values are neutralised: {index}");
    let files = std::fs::read_to_string(report.join("conversion-report.csv")).unwrap();
    assert!(files.contains("hub.sldprt,failed,0,,"), "{files}");
    assert!(files.contains("Document Manager"), "{files}");

    // A second run finds the snapshots current.
    let (_, _, err) = run(&["convert", s(&share), "-o", s(&report)]);
    assert!(err.contains("0 converted, 2 already up to date, 1 failed"), "{err}");

    let (ok, out, _) = run(&["show", s(&wheel)]);
    assert!(ok);
    assert!(out.contains("Description: large wheel"), "{out}");
    assert!(out.contains("Process    : Lathe"), "{out}");
    assert!(out.contains("Finish     : galvanized"), "{out}");
    assert!(out.contains("Author     : J. Doe   [summary]"), "{out}");

    let (ok, out, _) = run(&["show", "--json", s(&wheel)]);
    assert!(ok);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v[0]["properties"][1]["name"], "Process");
    assert_eq!(v[0]["properties"][1]["value"], "Lathe");
    assert_eq!(v[0]["properties"][1]["from_file"], true);
}

#[test]
fn store_folder_for_read_only_shares() {
    let d = scratch("store");
    let share = d.join("share");
    std::fs::create_dir_all(&share).unwrap();
    let wheel = share.join("wheel.sldprt");
    std::fs::write(&wheel, part(&[("Description", "large wheel")])).unwrap();
    let store = d.join("store");
    let (ok, _, err) = run(&["convert", s(&share), "--store", s(&store), "-o", s(&d.join("r"))]);
    assert!(ok, "{err}");
    assert!(!share.join("wheel.sldprt.anvilprops.json").exists());
    let n = std::fs::read_dir(&share).unwrap().count();
    assert_eq!(n, 1, "nothing is written to the share");
    let side = anvil_io::props::sidecar::path_for(&wheel, Some(&store));
    assert!(side.starts_with(&store) && side.exists(), "{}", side.display());

    let (ok, out, _) = run(&["show", "--store", s(&store), s(&wheel)]);
    assert!(ok && out.contains("Description: large wheel"), "{out}");
}

#[test]
fn dry_run_writes_no_property_files() {
    let d = scratch("dry");
    let wheel = d.join("wheel.sldprt");
    std::fs::write(&wheel, part(&[("Description", "large wheel")])).unwrap();
    let (ok, _, err) = run(&["convert", "--dry-run", s(&wheel), "-o", s(&d.join("r"))]);
    assert!(ok, "{err}");
    assert!(err.contains("1 read"), "{err}");
    assert!(!d.join("wheel.sldprt.anvilprops.json").exists());
    let index = std::fs::read_to_string(d.join("r/properties.csv")).unwrap();
    assert!(index.contains("Description,Text,large wheel"));
}
