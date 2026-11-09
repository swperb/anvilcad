//! `anvil-evals`: runs the kernel evaluation corpus and random cases, and compares timings
//! with a stored baseline. See docs/kernel/EVALS.md.

use anvil_evals::{CaseResult, Options, Status, corpus, fuzz, run};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: anvil-evals [corpus] [--filter TEXT] [--fuzz N] [--seed S] [--kinds 0,1,2,3,4]
                  [--prims N] [--images DIR] [--failed-images] [--repeats N]
                  [--report FILE.json] [--baseline FILE.json] [--save-baseline FILE.json]
                  [--slower PCT] [--quiet] [--probe]

corpus runs the hand-built cases (the default when --fuzz is not given). --fuzz N runs N
random cases from --seed. --baseline compares build times with a saved report and fails
on any case slower by more than --slower percent (default 30) and 5 ms.";

struct Args {
    corpus: bool,
    filter: Option<String>,
    fuzz: u64,
    seed: u64,
    kinds: Vec<usize>,
    prims: usize,
    report: Option<PathBuf>,
    baseline: Option<PathBuf>,
    save_baseline: Option<PathBuf>,
    slower: f64,
    quiet: bool,
    probe: bool,
    opt: Options,
}

fn parse() -> Result<Args, String> {
    let mut a = Args {
        corpus: false,
        filter: None,
        fuzz: 0,
        seed: 1,
        kinds: vec![0, 1, 2, 3, 4],
        prims: 3,
        report: None,
        baseline: None,
        save_baseline: None,
        slower: 30.0,
        quiet: false,
        probe: false,
        opt: Options::default(),
    };
    let mut it = std::env::args().skip(1);
    let num = |s: Option<String>, what: &str| -> Result<f64, String> {
        s.ok_or(format!("{what} needs a value"))?.parse::<f64>().map_err(|e| format!("{what}: {e}"))
    };
    while let Some(x) = it.next() {
        match x.as_str() {
            "corpus" => a.corpus = true,
            "--filter" => a.filter = it.next(),
            "--fuzz" => a.fuzz = num(it.next(), "--fuzz")? as u64,
            "--seed" => a.seed = num(it.next(), "--seed")? as u64,
            "--prims" => a.prims = num(it.next(), "--prims")? as usize,
            "--kinds" => {
                a.kinds = it
                    .next()
                    .ok_or("--kinds needs a value")?
                    .split(',')
                    .map(|k| k.trim().parse::<usize>().map_err(|e| format!("--kinds: {e}")))
                    .collect::<Result<_, _>>()?;
                if a.kinds.is_empty() || a.kinds.iter().any(|k| *k > 4) {
                    return Err("--kinds takes numbers from 0 to 4".into());
                }
            }
            "--images" => a.opt.image_dir = it.next().map(PathBuf::from),
            "--failed-images" => a.opt.images_on_failure_only = true,
            "--repeats" => a.opt.repeats = num(it.next(), "--repeats")? as usize,
            "--report" => a.report = it.next().map(PathBuf::from),
            "--baseline" => a.baseline = it.next().map(PathBuf::from),
            "--save-baseline" => a.save_baseline = it.next().map(PathBuf::from),
            "--slower" => a.slower = num(it.next(), "--slower")?,
            "--quiet" => a.quiet = true,
            "--probe" => a.probe = true,
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown argument {other}\n\n{USAGE}")),
        }
    }
    if a.fuzz == 0 {
        a.corpus = true;
    }
    Ok(a)
}

fn main() -> ExitCode {
    let args = match parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    // Panics are caught and reported per case; keep their default printout short.
    std::panic::set_hook(Box::new(|info| {
        let at = info.location().map(|l| format!(" at {}:{}", l.file(), l.line())).unwrap_or_default();
        eprintln!("  (panic{at})");
    }));

    let mut cases = Vec::new();
    if args.corpus {
        cases.extend(corpus::cases());
    }
    for i in 0..args.fuzz {
        let c = fuzz::case(args.seed, i, &args.kinds, args.prims);
        if !fuzz::degenerate(&c.tree) {
            cases.push(c);
        }
    }
    if let Some(f) = &args.filter {
        cases.retain(|c| c.name.contains(f.as_str()));
    }
    if args.probe {
        // Prints each case's tree and how its reference volume converges with the grid.
        for c in &cases {
            println!("{}: {:?}", c.name, c.tree);
            for n in [150, 300, 600, 1200] {
                println!("  grid {n:5}: {:.9}", c.tree.volume(n));
            }
        }
        return ExitCode::SUCCESS;
    }

    let mut results: Vec<CaseResult> = Vec::new();
    for c in &cases {
        let r = run(c, &args.opt);
        if !args.quiet || r.status != Status::Pass {
            let mark = match r.status {
                Status::Pass => "pass",
                Status::Graceful => "error",
                Status::Fail => "FAIL",
                Status::Panic => "PANIC",
            };
            println!(
                "{mark:5} {:40} {:8.1} ms  {:6.1} ms tess  {} bodies",
                r.name, r.build_ms, r.tess_ms, r.bodies
            );
            for reason in &r.reasons {
                println!("        {reason}");
            }
        }
        results.push(r);
    }

    let mut count: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &results {
        *count
            .entry(match r.status {
                Status::Pass => "pass",
                Status::Graceful => "error",
                Status::Fail => "fail",
                Status::Panic => "panic",
            })
            .or_default() += 1;
    }
    let regressed: Vec<&CaseResult> = results.iter().filter(|r| r.regressed()).collect();
    let promoted: Vec<&CaseResult> = results
        .iter()
        .filter(|r| {
            r.expect == anvil_evals::Expect::Graceful && r.status == Status::Pass && r.category != "fuzz"
        })
        .collect();
    println!(
        "\n{} cases: {:?}; {} broke their expectation; {} expected to fail now pass",
        results.len(),
        count,
        regressed.len(),
        promoted.len()
    );
    for r in &promoted {
        println!("  now passes: {}", r.name);
    }
    let total_build: f64 = results.iter().map(|r| r.build_ms).sum();
    let total_tess: f64 = results.iter().map(|r| r.tess_ms).sum();
    println!("total build {total_build:.1} ms, tessellation {total_tess:.1} ms");

    let json = serde_json::to_string_pretty(&results).unwrap_or_default();
    for path in [&args.report, &args.save_baseline].into_iter().flatten() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(path, &json) {
            eprintln!("could not write {}: {e}", path.display());
            return ExitCode::from(2);
        }
    }

    let mut slow = 0;
    if let Some(path) = &args.baseline {
        let base: Vec<serde_json::Value> =
            match std::fs::read_to_string(path).map(|s| serde_json::from_str(&s)) {
                Ok(Ok(v)) => v,
                Ok(Err(e)) => {
                    eprintln!("{} is not a report: {e}", path.display());
                    return ExitCode::from(2);
                }
                Err(e) => {
                    eprintln!("could not read {}: {e}", path.display());
                    return ExitCode::from(2);
                }
            };
        let base: BTreeMap<String, (f64, f64, String)> = base
            .iter()
            .filter_map(|v| {
                Some((
                    v.get("name")?.as_str()?.to_string(),
                    (
                        v.get("build_ms")?.as_f64()?,
                        v.get("tess_ms")?.as_f64()?,
                        v.get("status")?.as_str()?.to_string(),
                    ),
                ))
            })
            .collect();
        println!("\nAgainst {}:", path.display());
        for r in &results {
            let Some((b, t, status)) = base.get(&r.name) else { continue };
            for (what, now, then) in [("build", r.build_ms, *b), ("tessellation", r.tess_ms, *t)] {
                if now > then * (1.0 + args.slower / 100.0) && now - then > 5.0 {
                    slow += 1;
                    println!("  SLOWER {:40} {what} {then:.1} ms -> {now:.1} ms", r.name);
                }
            }
            if status == "Pass" && r.status != Status::Pass {
                println!("  REGRESSED {:40} passed in the baseline", r.name);
                slow += 1;
            }
        }
        let then: f64 = base.values().map(|v| v.0).sum();
        println!("  total build {then:.1} ms -> {total_build:.1} ms");
    }

    if regressed.is_empty() && slow == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
