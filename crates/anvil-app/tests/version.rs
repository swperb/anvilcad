//! `--version`, `-V` and `--help` on the app's two binaries (issue #147). Each answers
//! before any window or harness starts.

#![allow(clippy::expect_used)]

use std::process::Command;

fn run(exe: &str, arg: &str) -> String {
    let out = Command::new(exe).arg(arg).output().expect("the binary runs");
    assert!(out.status.success(), "{exe} {arg} exited with {:?}", out.status);
    String::from_utf8(out.stdout).expect("UTF-8 output")
}

#[test]
fn version_prints_the_binary_name_and_package_version() {
    let version = env!("CARGO_PKG_VERSION");
    for (exe, name) in [
        (env!("CARGO_BIN_EXE_anvilcad-agent"), "anvilcad-agent"),
        (env!("CARGO_BIN_EXE_anvilcad"), "anvilcad"),
    ] {
        for flag in ["--version", "-V"] {
            assert_eq!(run(exe, flag), format!("{name} {version}\n"), "{name} {flag}");
        }
        assert!(run(exe, "--help").contains(&format!("{name} --version | --help")), "{name} --help");
    }
}
