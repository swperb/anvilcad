# Contributing to AnvilCAD

Thank you for helping. AnvilCAD is free and open-source software, licensed under MIT OR
Apache-2.0, and everything in it should be something anyone may use, study, change and
share.

## Build deliberately: no vibe coding

AnvilCAD is not to be vibe coded, whether you write the code by hand or with an AI
assistant. People will move their real work onto it, so every change is made on purpose:
understood before it is written, tested before it is called done, and finished before it is
merged. Being free is no excuse for being worse. The aim is for AnvilCAD to feel *better* to
use than the paid tools it replaces, so no one has a reason to go back to them.

- **Understand before you write.** Read the code you are changing and the code that calls
  it. For anything that touches SOLIDWORKS, follow the interoperability rules below
  strictly: the `docs/re/` specification comes first and the implementation follows it. Do
  not submit code you could not explain line by line, and do not paste in patterns you have
  not checked against this codebase.
- **Finish what you ship.** No half-built features: no buttons that do nothing, menu items
  that open nothing, stub dialogs, placeholder text or `todo!()` paths a user can reach. If a
  feature is too big for one change, land it in slices that each work end to end, and keep
  unfinished parts where users cannot reach them. Say in the pull request what is not done.
- **Stable first.** A feature that crashes, loses work or corrupts a file is worse than no
  feature. Handle bad input, cancelled operations, undo, save and reopen, and large models.
  A user must never lose data to AnvilCAD.
- **Usable and consistent.** New UI follows the existing layout, spacing, naming, icons, key
  bindings and theme. Error messages say what went wrong and what to do about it, in plain
  words; never show a raw debug string or panic. Long operations show progress and can be
  cancelled. Defaults are the ones a working engineer would choose.
- **Look finished.** Check every visible change in the app or the headless harness
  ([docs/AGENT_HARNESS.md](docs/AGENT_HARNESS.md)), in each theme and at more than one
  window size, before calling it done. Misaligned controls, clipped text, flicker, and
  inconsistent units or precision are bugs, not polish.
- **Feel fast.** Anything that is instant in paid CAD must be instant here. Do not block the
  UI thread on kernel work or file I/O, and measure before and after when changing a hot
  path.
- **Better, not just equal.** When matching a workflow users know from paid tools, keep what
  works and fix what annoys: fewer clicks, clearer feedback, no licence prompts, open
  formats. Do not copy their frustrations along with their features.
- **Small, reviewable changes.** Each pull request does one thing and says why. Do not mix
  refactors with features, and do not rewrite working code without a stated reason.
- **When unsure, ask.** If a requirement, design choice or SOLIDWORKS behaviour is unclear,
  open an issue or ask in the pull request, or record the open question, rather than
  guessing and moving on.

## Developer Certificate of Origin

Every commit must be signed off, which certifies the
[Developer Certificate of Origin 1.1](https://developercertificate.org/): that you wrote
the change or otherwise have the right to submit it under the project's licences. Add the
sign-off with `git commit -s`:

    Signed-off-by: Your Name <you@example.com>

Unless you say otherwise, your contribution is dual licensed under MIT and Apache-2.0, as
the README describes.

## Interoperability rules

AnvilCAD studies commercial CAD, CAM, PDM and PLM systems, including by reverse
engineering, so it can match them and read their data natively. Please read
[INTEROPERABILITY.md](INTEROPERABILITY.md). In particular:

- for SOLIDWORKS, study published information (public documentation and tutorials) and
  files you or the project lawfully hold, never the software, its binaries, installers or
  libraries;
- write reverse-engineering findings up as a specification in `docs/re/` first, then write
  AnvilCAD's code from it; never copy code, libraries, help text, icons or artwork from
  commercial products;
- do not circumvent licence checks, copy protection, encryption or other protection
  measures;
- do not contribute leaked material, documentation or SDKs under a non-disclosure or
  restricted licence, or confidential information from an employer, client or vendor;
- do not contribute code under a licence that is not compatible with MIT and Apache-2.0 (GPL
  and LGPL code cannot be copied in, though papers and public descriptions of ideas may be
  studied and cited);
- features must work without SOLIDWORKS or any other commercial software installed.

If you work for a CAD, CAM or PLM vendor, or have seen a vendor's source code or
confidential documentation, check your obligations before working in related areas, and
mention it in the pull request so maintainers can route the review.

Cite your sources. Algorithms based on published papers should name the papers in the code
or in the docs next to it. Where a technique is known to be patented, say so and explain the
design choices that keep clear of the claims (see `docs/cam/` for an example).

## Post processors and other shared content

Community post processors are data, not code: they are declarative definitions checked by
a verifier and a simulator before use. Submissions go through review like code; see
`docs/cam/POSTS.md`. Never submit a post that writes machine parameters, changes offsets or
disables safety functions without declaring it and explaining why.

## Engineering standards

- Stability and performance come first. Library code never panics on bad input; it returns
  errors. The exact kernel and STEP crates forbid `unsafe`, deny `unwrap` and warn on
  `expect` and `panic`; new library crates should adopt the same `[lints]` section.
- Run `cargo fmt --all`, `cargo clippy --workspace --all-targets` and
  `cargo test --workspace` before opening a pull request. Changes that affect Linux or
  Windows should also pass `cargo check --workspace --target x86_64-unknown-linux-gnu` and
  `--target x86_64-pc-windows-msvc`.
- Write comments and documentation in plain, direct prose.
- Add tests that would fail without your change, ideally against exact values (volumes,
  areas, positions) rather than "it runs".
