//! The evaluation gate `cargo test` enforces: every corpus case meets its expectation,
//! and a fixed stream of random cases produces no wrong body and no panic. Timings are
//! not gated here (CI machines vary); `anvil-evals --baseline` compares them.

use anvil_evals::{Expect, Options, Status, corpus, fuzz, run};

fn report(results: &[anvil_evals::CaseResult]) -> String {
    results
        .iter()
        .filter(|r| r.regressed())
        .map(|r| format!("{} ({:?}): {}", r.name, r.status, r.reasons.join("; ")))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn corpus_meets_its_expectations() {
    let opt = Options::default();
    let results: Vec<_> = corpus::cases().iter().map(|c| run(c, &opt)).collect();
    let bad = report(&results);
    assert!(bad.is_empty(), "cases that broke their expectation:\n{bad}");
    // A case marked graceful that now passes should be promoted, so it stays fixed.
    let promoted: Vec<_> = results
        .iter()
        .filter(|r| r.expect == Expect::Graceful && r.status == Status::Pass)
        .map(|r| r.name.as_str())
        .collect();
    assert!(promoted.is_empty(), "these cases now pass; mark them Pass in corpus.rs: {promoted:?}");
}

#[test]
fn random_cases_are_never_wrong() {
    // Smaller images and fewer samples keep this quick; the full stream runs from the
    // command line (docs/kernel/EVALS.md).
    let opt = Options { image_size: 48, membership_samples: 150, volume_grid: 300, ..Options::default() };
    let mut results = Vec::new();
    for i in 0..60 {
        let c = fuzz::case(20261109, i, &[0, 1, 2, 3, 4], 3);
        if !fuzz::degenerate(&c.tree) {
            results.push(run(&c, &opt));
        }
    }
    let bad = report(&results);
    assert!(bad.is_empty(), "random cases gave wrong results:\n{bad}");
    let passed = results.iter().filter(|r| r.status == Status::Pass).count();
    // Most random cases build; a drop means the kernel started refusing work it did.
    assert!(passed * 10 >= results.len() * 8, "only {passed} of {} random cases built", results.len());
}
