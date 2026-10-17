//! From a part to certified G-code: recognise features, plan operations, build the tool
//! path, write it with each bundled post, and certify the program text with the verifier
//! and the simulator.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use anvil_cam::job::{Job, Setup, Stock, build_setup, plan_operations};
use anvil_cam::post::{PostDefinition, ProgramSettings, bundled, generate};
use anvil_cam::safety::{SimInputs, certify};
use anvil_cam::sim::HeightField;
use anvil_cam::tool::{Tool, ToolLibrary};
use anvil_cam::verify::VerifyOptions;
use anvil_kernel::build::{block, cylinder};
use anvil_kernel::{Solid, v2, v3};
use anvil_model::material::Material;

/// 80 x 60 x 20 block with a 40 x 30 pocket 10 deep around a 10 x 10 island, and a 6 mm
/// through hole.
fn part() -> Solid {
    let base = block(v3(0.0, 0.0, 0.0), v3(80.0, 60.0, 20.0), 1);
    let pocket = block(v3(20.0, 15.0, 10.0), v3(60.0, 45.0, 25.0), 2);
    let island = block(v3(35.0, 25.0, 10.0), v3(45.0, 35.0, 20.0), 3);
    let through = cylinder(v3(10.0, 10.0, -5.0), 3.0, 30.0, 4);
    base.subtract(&pocket).union(&island).subtract(&through)
}

fn job() -> Job {
    let mut tools = ToolLibrary::default();
    tools.add(Tool::flat_end_mill(1, 16.0, 3)).unwrap();
    tools.add(Tool::flat_end_mill(2, 8.0, 3)).unwrap();
    tools.add(Tool::drill(3, 6.0)).unwrap();
    tools.add(Tool::flat_end_mill(4, 6.0, 3)).unwrap();
    let material = Material::library().into_iter().find(|m| m.name.contains("6061")).unwrap();
    Job { tools, material, max_rpm: 12_000.0, setups: Vec::new() }
}

#[test]
fn part_to_certified_programs() {
    let part = part();
    let mut job = job();
    let stock = Stock { min: v3(0.0, 0.0, 0.0), max: v3(80.0, 60.0, 22.0) };
    let (ops, rec) = plan_operations(&job, &part, &stock, 30.0).unwrap();
    let names: Vec<&str> = ops.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, ["face", "pocket 1 clearing", "pocket 1 walls", "drill 6.00 mm"], "{rec:?}");
    let setup = Setup {
        name: "top".into(),
        work_offset: 1,
        origin: v3(0.0, 0.0, 0.0),
        stock,
        safe_z: 30.0,
        operations: ops,
    };
    job.setups.push(setup.clone());
    let out = build_setup(&job, &setup).unwrap();
    assert_eq!(out.tool_sequence, vec![1, 4, 3]);
    let (_, rep) = &out.adaptive_reports[0];
    // Under 0.5 % of the pocket left for the wall finishing pass to take.
    assert!(rep.remaining_reachable_area < 0.005 * (40.0 * 30.0 - 100.0), "{rep:?}");

    // Stock and finished part as height fields in program coordinates.
    let cell = 0.25;
    let stock_f = HeightField::block(v2(0.0, 0.0), v2(80.0, 60.0), 22.0, cell, 20.0).unwrap();
    let part_f = HeightField::from_solid(&part, &stock_f);

    for (name, text) in bundled() {
        let post = PostDefinition::from_toml(text).unwrap();
        let program = generate(&post, &out.toolpath, &ProgramSettings::default()).unwrap();
        let m = &post.machine;
        let opts = VerifyOptions {
            work_offset: v3((m.x[0] + m.x[1]) * 0.5 - 40.0, (m.y[0] + m.y[1]) * 0.5 - 30.0, m.z[1] - 40.0),
            start: None,
        };
        let cert = certify(
            &program.text,
            &post,
            &opts,
            &job.tools,
            &out.tool_sequence,
            &SimInputs { stock: &stock_f, part: Some(&part_f), tol: 0.02 },
        )
        .unwrap();
        assert!(
            cert.safe,
            "{name}: {:?}\n{:?}",
            cert.reasons,
            cert.verify.errors().take(5).collect::<Vec<_>>()
        );
        assert_eq!(cert.simulations.len(), 3, "{name}");
        // The same program with its first cutting move made a rapid is valid G-code but
        // drives the tool through material at rapid rate: the simulator must catch it.
        if name.starts_with("fanuc") {
            let pos = program.text.find("\nN").and_then(|_| {
                program
                    .text
                    .match_indices(" G1 X")
                    .map(|(i, _)| i)
                    .find(|&i| program.text[..i].contains("POCKET 1 CLEARING"))
            });
            let i = pos.expect("a cutting move in the pocket");
            let mut bad = program.text.clone();
            bad.replace_range(i..i + 3, " G0");
            let cert = certify(
                &bad,
                &post,
                &opts,
                &job.tools,
                &out.tool_sequence,
                &SimInputs { stock: &stock_f, part: Some(&part_f), tol: 0.02 },
            )
            .unwrap();
            assert!(cert.verify.passed());
            assert!(!cert.safe);
            assert!(
                cert.reasons.iter().any(|r| r.contains("rapid move through material")),
                "{:?}",
                cert.reasons
            );
        }
    }
}

#[test]
fn flat_bottomed_blind_hole_is_reported_as_a_gouge() {
    // A drill cannot make a flat bottom: the simulator says so.
    let base = block(v3(0.0, 0.0, 0.0), v3(40.0, 40.0, 20.0), 1);
    let blind = cylinder(v3(20.0, 20.0, 10.0), 3.0, 15.0, 2);
    let part = base.subtract(&blind);
    let job = job();
    let stock = Stock { min: v3(0.0, 0.0, 0.0), max: v3(40.0, 40.0, 20.0) };
    let (ops, _) = plan_operations(&job, &part, &stock, 30.0).unwrap();
    let setup = Setup {
        name: "top".into(),
        work_offset: 1,
        origin: v3(0.0, 0.0, 0.0),
        stock,
        safe_z: 30.0,
        operations: ops,
    };
    let out = build_setup(&job, &setup).unwrap();
    let post = PostDefinition::from_toml(bundled()[1].1).unwrap();
    let program = generate(&post, &out.toolpath, &ProgramSettings::default()).unwrap();
    let stock_f = HeightField::block(v2(0.0, 0.0), v2(40.0, 40.0), 20.0, 0.25, 10.0).unwrap();
    let part_f = HeightField::from_solid(&part, &stock_f);
    let opts = VerifyOptions { work_offset: v3(100.0, 100.0, -100.0), start: None };
    let cert = certify(
        &program.text,
        &post,
        &opts,
        &job.tools,
        &out.tool_sequence,
        &SimInputs { stock: &stock_f, part: Some(&part_f), tol: 0.02 },
    )
    .unwrap();
    assert!(cert.verify.passed());
    assert!(!cert.safe);
    assert!(cert.reasons.iter().any(|r| r.contains("gouge")), "{:?}", cert.reasons);
}
