//! The published results page (issue #89): generated the same way twice, from a small
//! corpus, it is byte for byte the same, and says what it should.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use anvil_evals::publish::{Baseline, report};
use anvil_evals::{Options, corpus, run};

#[test]
fn the_page_is_reproducible_and_complete() {
    let cases: Vec<_> = corpus::cases()
        .into_iter()
        .filter(|c| c.name == "box/overlap-union" || c.name == "box/identical-union")
        .collect();
    assert_eq!(cases.len(), 2, "both cases still exist in the corpus");
    let opt = Options { image_size: 32, ..Options::default() };
    let baseline = Baseline {
        path: "baselines/test.json",
        machine: "a test",
        json: r#"[{"name":"box/overlap-union","build_ms":0.35,"tess_ms":0.04,"status":"Pass"}]"#,
    };
    let page = |_: ()| {
        let results: Vec<_> = cases.iter().map(|c| run(c, &opt)).collect();
        report(&cases, &results, Some(&baseline)).unwrap()
    };
    let (a, b) = (page(()), page(()));
    assert_eq!(a, b, "two runs of the same commit give the same page");

    assert!(a.starts_with("# Kernel evaluation results\n\n**2 cases: 2 pass, 0 refused, 0 fail, 0 panic. Every case met its expectation.**\n"), "{a}");
    assert!(a.contains("## box\n"));
    // The overlap is 15 mm³ in closed form, and the baseline's timings are shown.
    assert!(
        a.contains("| `box/overlap-union` |") && a.contains("| 15 (exact) | pass | pass | 0.35 | 0.04 |"),
        "{a}"
    );
    // A case the baseline does not have gets dashes, not numbers.
    assert!(a.contains("| `box/identical-union` |") && a.contains("| – | – |"), "{a}");
    assert!(a.contains("`baselines/test.json` (a test)"));
}
