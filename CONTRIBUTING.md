# Contributing to AnvilCAD

Thank you for helping. AnvilCAD is free and open-source software, licensed under MIT OR
Apache-2.0, and everything in it should be something anyone may use, study, change and
share.

## Developer Certificate of Origin

Every commit must be signed off, which certifies the
[Developer Certificate of Origin 1.1](https://developercertificate.org/): that you wrote
the change or otherwise have the right to submit it under the project's licences. Add the
sign-off with `git commit -s`:

    Signed-off-by: Your Name <you@example.com>

Unless you say otherwise, your contribution is dual licensed under MIT and Apache-2.0, as
the README describes.

## Clean-room rules

AnvilCAD interoperates with commercial CAD, CAM, PDM and PLM systems through open
standards and published APIs only. Please read [INTEROPERABILITY.md](INTEROPERABILITY.md).
In particular, do not contribute anything that comes from:

- decompiling, disassembling or tracing commercial software, or reverse-engineering
  proprietary file formats;
- leaked material, or documentation or SDKs under a non-disclosure or restricted licence;
- confidential information from an employer, client or vendor;
- code under a licence that is not compatible with MIT and Apache-2.0 (GPL and LGPL code
  cannot be copied in, though papers and public descriptions of ideas may be studied and
  cited).

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
