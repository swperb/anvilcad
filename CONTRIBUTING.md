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

## Interoperability rules

AnvilCAD studies commercial CAD, CAM, PDM and PLM systems, including by reverse
engineering, so it can match them and read their data natively. Please read
[INTEROPERABILITY.md](INTEROPERABILITY.md). In particular:

- for SOLIDWORKS, study published information only (public documentation, tutorials and
  example files), never the software, its installers or its libraries;
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
