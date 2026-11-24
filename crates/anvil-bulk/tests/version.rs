//! `anvil-bulk --version`, `-V` and `--help` (issue #147).

#![allow(clippy::expect_used)]

use std::process::Command;

#[test]
fn version_prints_the_binary_name_and_package_version() {
    let exe = env!("CARGO_BIN_EXE_anvil-bulk");
    for flag in ["--version", "-V"] {
        let out = Command::new(exe).arg(flag).output().expect("anvil-bulk runs");
        assert!(out.status.success(), "{flag}: {:?}", out.status);
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            format!("anvil-bulk {}\n", env!("CARGO_PKG_VERSION"))
        );
    }
    // The usage goes to stderr, as it always has.
    let help = Command::new(exe).arg("--help").output().expect("anvil-bulk runs");
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stderr).contains("anvil-bulk --version | --help"));
}
