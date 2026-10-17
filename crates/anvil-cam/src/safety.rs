//! Certification: a program is called safe only when the verifier accepts it with no
//! lines it could not follow, and the simulator, running the motion the verifier rebuilt
//! from the program text (not the tool path it was generated from), finds no gouge,
//! collision, rapid through material or link cutting material, for every tool.

use crate::error::{CamError, Result};
use crate::post::PostDefinition;
use crate::sim::{HeightField, SimReport, simulate};
use crate::tool::ToolLibrary;
use crate::toolpath::{Item, Toolpath};
use crate::verify::{VerifyOptions, VerifyReport, verify};

/// The stock, the finished part and the tolerance the simulator uses.
#[derive(Clone, Copy, Debug)]
pub struct SimInputs<'a> {
    /// Stock before the program, in program coordinates.
    pub stock: &'a HeightField,
    /// The finished part's top surface, for gouge and holder checks.
    pub part: Option<&'a HeightField>,
    /// Depths below this are ignored (mm).
    pub tol: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Certificate {
    pub verify: VerifyReport,
    /// One simulation per tool, in program order.
    pub simulations: Vec<(u32, SimReport)>,
    /// True only when every check passed.
    pub safe: bool,
    /// Why it is not safe (empty when safe).
    pub reasons: Vec<String>,
}

/// Verifies `program` against `post` and simulates it on the stock in `sim`, checking
/// against the part if given. `tools` must hold every tool the program uses. For
/// machines without a tool changer, the program cannot name its tools, so
/// `manual_tools` gives them in order: each stop (M0) followed by a spindle start is a
/// manual tool change to the next one in the list.
pub fn certify(
    program: &str,
    post: &PostDefinition,
    opts: &VerifyOptions,
    tools: &ToolLibrary,
    manual_tools: &[u32],
    sim: &SimInputs,
) -> Result<Certificate> {
    let SimInputs { stock, part, tol } = *sim;
    let report = verify(program, post, opts);
    let mut reasons = Vec::new();
    if !report.passed() {
        reasons.push(format!("the verifier rejected the program ({} errors)", report.errors().count()));
    }
    if report.opaque_lines > 0 {
        reasons.push(format!(
            "{} line(s) use declared constructs the simulator cannot follow; a person must review them",
            report.opaque_lines
        ));
    }
    let mut simulations = Vec::new();
    if report.passed() {
        let segments = split_by_tool(&report.toolpath, post.capabilities.tool_changer, manual_tools)?;
        let mut stock = stock.clone();
        for (tool, seg) in segments {
            let Some(tool_no) = tool else {
                if seg.moves().any(|m| m.feed.is_some()) {
                    reasons.push("the program cuts before a tool is loaded".into());
                }
                continue;
            };
            let t = match tools.get(tool_no) {
                Ok(t) => t,
                Err(e) => {
                    reasons.push(e.to_string());
                    continue;
                }
            };
            let sim = simulate(&seg, t, &mut stock, part, tol)?;
            if !sim.is_clean() {
                let mut kinds: Vec<&str> = sim.findings.iter().map(|f| f.kind.label()).collect();
                kinds.sort();
                kinds.dedup();
                reasons.push(format!("tool {tool_no}: {}", kinds.join(", ")));
            }
            simulations.push((tool_no, sim));
        }
    }
    Ok(Certificate { safe: reasons.is_empty(), verify: report, simulations, reasons })
}

/// Splits the rebuilt path into runs of one tool each.
fn split_by_tool(tp: &Toolpath, changer: bool, manual: &[u32]) -> Result<Vec<(Option<u32>, Toolpath)>> {
    let mut out: Vec<(Option<u32>, Toolpath)> = Vec::new();
    let mut cur: Option<u32> = None;
    let mut next_manual = 0usize;
    let mut seg = Toolpath { items: Vec::new(), start: tp.start };
    let mut pending_stop = false;
    for it in &tp.items {
        let switch = match it {
            Item::ToolChange { tool, .. } if changer => Some(*tool),
            Item::OptionalStop if !changer => {
                pending_stop = true;
                None
            }
            Item::Spindle { .. } if !changer && pending_stop => {
                pending_stop = false;
                let t = manual.get(next_manual).copied();
                next_manual += 1;
                if t.is_none() {
                    return Err(CamError::InvalidParameter {
                        name: "manual tools",
                        reason: "the program changes tools more often than the list gives".into(),
                    });
                }
                t
            }
            _ => None,
        };
        if let Some(t) = switch {
            let start = seg.end();
            out.push((cur, std::mem::replace(&mut seg, Toolpath { items: Vec::new(), start })));
            cur = Some(t);
        }
        seg.items.push(it.clone());
    }
    out.push((cur, seg));
    Ok(out)
}
