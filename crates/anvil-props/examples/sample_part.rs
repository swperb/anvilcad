//! Writes a stand-in for an older SOLIDWORKS part, for trying anvil-props and AnvilCAD
//! without SOLIDWORKS: a compound file with custom properties in its standard property set
//! and no geometry.
//!
//!   cargo run -p anvil-props --example sample_part -- wheel.sldprt

#![allow(clippy::expect_used)]

use anvil_io::props::cfb::writer as cfb;
use anvil_io::props::oleps::{self, writer as ps};

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "wheel.sldprt".into());
    let props = [
        ("Description", "large wheel"),
        ("Process", "Lathe"),
        ("Finish", "galvanized"),
        ("PartNo", "WHL-0420"),
        ("Revision", "C"),
        ("Material", "AISI 1045"),
    ];
    let values: Vec<(u32, ps::V)> =
        props.iter().enumerate().map(|(i, (_, v))| (i as u32 + 2, ps::V::Str(v))).collect();
    let dict: Vec<(u32, &str)> = props.iter().enumerate().map(|(i, (n, _))| (i as u32 + 2, *n)).collect();
    let user = ps::section(1252, &values, &dict);
    let doc = ps::stream(&[(oleps::FMTID_USER_DEFINED, user)]);
    let summary = ps::stream(&[(
        oleps::FMTID_SUMMARY,
        ps::section(1252, &[(2, ps::V::Str("Wheel")), (4, ps::V::Str("J. Doe"))], &[]),
    )]);
    let bytes = cfb::build(&[(oleps::SUMMARY_STREAM, &summary), (oleps::DOC_SUMMARY_STREAM, &doc)]);
    std::fs::write(&out, bytes).expect("write sample part");
    println!("wrote {out}");
}
