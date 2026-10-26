//! Proveout: a second pair of eyes on a CNC program before it is proved out on the
//! machine.
//!
//! A check has two layers, and the report keeps them apart:
//!
//! 1. [`check`] runs the deterministic G-code verifier from `anvil-cam` against a
//!    [`machine`] profile and, when the user has one, an AnvilCAD post definition. It
//!    needs no network, costs nothing to run, and gives the same answer every time.
//! 2. [`ai`] packs the program, the drawing, the machine, the post and the deterministic
//!    results into one structured request to the Claude Messages API, and parses the
//!    reply against a fixed schema. Its findings are advisory and are labelled as such.
//!
//! [`score`] combines the two into an agreement score, [`report`] builds the report and
//! the setup sheet and renders them, [`profile`] checks a machine profile or a post on
//! its own, and [`plan`] holds the free and paid limits. Nothing here says a program is
//! safe to run: the report always tells the operator to prove it out.
//!
//! The crate is a library so AnvilCAD can call it directly if it grows a full CAM
//! workflow; `proveout-web` is the site that serves it.

pub mod ai;
pub mod check;
pub mod error;
pub mod machine;
pub mod plan;
pub mod profile;
pub mod program;
pub mod report;
pub mod score;

pub use error::{Error, Result};

/// The product name, in one place so it can be changed.
pub const APP_NAME: &str = "Proveout";

/// Version of the disclosure text users accept before an AI review. Bump it whenever
/// [`PRIVACY_DISCLOSURE`] or [`NO_GUARANTEE_DISCLOSURE`] changes, so the site asks again.
pub const DISCLOSURE_VERSION: &str = "2026-10-26";

/// Shown before anything is sent to the AI service, and in every report.
pub const PRIVACY_DISCLOSURE: &str = "An AI review sends what you submit (the program, the drawing, your notes, the \
machine profile and the post processor) to Anthropic's API to be processed by a Claude model. We make no privacy \
guarantee for data sent this way: it leaves our servers, it is handled under Anthropic's terms and policies rather \
than ours, and it may be retained by Anthropic. Do not submit anything you are not allowed to share with a third \
party, such as export-controlled (ITAR/EAR) data, classified material or a customer's confidential data. Anonymize \
part numbers, customer names and other identifying text first; the results may be less accurate when you do. The \
deterministic check on its own runs only on our server; nothing goes to the AI service unless you start an AI \
review.";

/// Shown with every result.
pub const NO_GUARANTEE_DISCLOSURE: &str = "Proveout's checks, its score and its setup sheet carry no guarantee of \
any kind. The deterministic checker only knows the rules it was given and cannot see your machine, fixture, stock or \
tooling. The AI review can be wrong, incomplete or confidently mistaken. Check every result yourself against the \
drawing, the machine's manual and the setup on the machine, and prove the program out as you normally would: \
graphics or simulation on the control, single block, reduced rapids and feed, and a first-article inspection. \
You are responsible for what runs on your machine.";
