# Interoperability policy

AnvilCAD is free and open-source software (MIT OR Apache-2.0). It aims to work well
alongside commercial CAD, PDM and PLM systems, including those from Dassault Systèmes, and
to work fully on its own, without any of them installed. This page sets out how the project
does that, so contributors and users can rely on it.

This is the project's engineering policy, not legal advice. Reverse engineering commercial
software carries legal risk: licence agreements, Dassault's included, restrict it, and the
statutory exceptions for interoperability (for example in the EU Software Directive) are
narrow and conditional.

## Reverse engineering for interoperability

AnvilCAD studies how SOLIDWORKS and other commercial systems behave, and how their files are
structured, so that it can match their workflows and read and write their data natively.
Features that adapt to SOLIDWORKS are developed with REA (Reverse Engineer Anything,
<https://github.com/morluto/rea>), working from two kinds of input only: published
SOLIDWORKS information (Dassault's public help and API documentation, release notes,
published tutorials and training material) and `.sldprt`, `.sldasm` and `.slddrw` files the
project or a user lawfully holds, such as example files published for download. SOLIDWORKS
itself, its installers and its libraries are not installed, run or analysed for this work.

Reverse engineering for interoperability has strong precedent and is lawful in many
jurisdictions, but how it is carried out decides the legal risk. AnvilCAD follows these
rules to stay on the low-risk side:

1. **Study the format from files, never the binaries.** AnvilCAD reverse-engineers file
   formats from sample files it or a user lawfully holds, together with published
   documentation, which is usually enough to read properties and geometry. File formats,
   facts and interfaces are not themselves protected by copyright. SOLIDWORKS binaries are
   not decompiled or otherwise analysed: that is what licences most squarely prohibit and
   the closest to anti-circumvention law. Where a format cannot be understood from files,
   the specification says so and that part is left unread.
2. **The reverse engineering is not done under an anti-RE licence.** A software licence that
   forbids reverse engineering binds whoever accepted it. Format study is therefore done from
   files, on a machine that has not installed SOLIDWORKS or accepted its EULA, rather than by
   studying an installed, licensed copy. (Driving an installed copy through its *published*
   API, as `anvil-migrate` does, is a separate and permitted path; see below.)
3. **Study, then write from scratch, clean-room.** Findings are written up as a specification
   in `docs/re/`, listing each published source and sample file used (with its URL or origin
   and the SOLIDWORKS version it comes from), how REA was run, and where the sources are
   silent or ambiguous. AnvilCAD's code is
   written from that specification. No code, libraries, type libraries, help text, icons or
   user-interface artwork from commercial products is copied into the repository.
4. **No circumvention.** AnvilCAD does not circumvent licence checks, copy protection,
   encryption or any other technical protection measure, and does not help anyone else do
   so. An encrypted stream is left unread rather than decrypted.
5. **No leaked or confidential material.** AnvilCAD does not use leaked material,
   documentation or SDKs obtained under a non-disclosure or restricted licence, or anything a
   contributor learned in confidence.

## No dependence on commercial software

Every AnvilCAD feature works on a machine that has never had SOLIDWORKS or any other
commercial CAD system installed. Behaviour is implemented natively in AnvilCAD's own
kernels; SOLIDWORKS files are read and written by native code wherever possible.

STEP (ISO 10303, AP203/AP214/AP242) remains a supported path for exact geometry in and out,
and later for product structure and PMI; STL and OBJ cover meshes. Future work follows open
standards too: AP242 for assemblies and PMI, ISO 10303-239 (PLCS) for product life-cycle
data, and OASIS OSLC for linking PLM records.

Where a user still holds a SOLIDWORKS licence, AnvilCAD may use it to recover more detail, as
long as nothing a user needs to open their own data is gated behind it. `anvil-migrate`
converts parts, keeping their feature tree, by driving the user's own installed, licensed
copy of SOLIDWORKS through its *published* automation interface. This is not reverse
engineering: SOLIDWORKS reads the file and AnvilCAD receives only what the public API returns.
It is the highest-fidelity path and how an organisation can move to AnvilCAD in a day while
its seats are live. AnvilCAD ships no Dassault code for it and binds late, at run time, to
what the user's installation provides.

Other optional plug-ins may call a Dassault library the user is licensed for — the SOLIDWORKS
Document Manager, which reads custom properties of newer files, is the current one — on the
same terms: the user's own licence, and every workflow still completes without it. These add
fidelity; they are never required.

Integration with SOLIDWORKS PDM and the 3DEXPERIENCE platform uses the customer's own
licences and credentials, and credentials are never stored in plain text.

## Analysis tools in the repository

`third_party/rea` is a git submodule pinning [REA](https://github.com/morluto/rea) (MIT),
a general-purpose reverse-engineering toolkit, kept so it is available offline and in cloud
development sessions. It is a deliberate, trusted reference, not third-party code that got
in by accident: contributors and coding agents read its documentation for research method,
agent and skill structure, handoff and reporting conventions, and how to record evidence
and unknowns. It is not part of AnvilCAD's build and nothing in AnvilCAD links to or ships
it. Where its guidance differs from AnvilCAD's own rules, AnvilCAD's rules apply. Having
it here changes none of the rules above: for SOLIDWORKS it is used on published
information and lawfully held files only, as described above; it is never pointed at
Dassault Systèmes software, its libraries or installers, or at any other software whose
licence forbids reverse engineering, and nothing learned that way may enter AnvilCAD.
Fetch it with `git submodule update --init third_party/rea`.

## Trademarks

SOLIDWORKS, CATIA, 3DEXPERIENCE, eDrawings and Dassault Systèmes are trademarks or
registered trademarks of Dassault Systèmes or its subsidiaries. CAMWorks, SolidCAM,
Mastercam, Fusion and other product names are trademarks of their owners. AnvilCAD uses
these names only to say what it is compatible with or how it differs, in plain text, without
logos, and without suggesting any affiliation or endorsement. AnvilCAD is not affiliated
with, sponsored by or endorsed by any of these companies.

AnvilCAD describes its own interface in its own words: a feature tree, a property panel, a
hole feature. It does not adopt other products' trademarked names for its own features.

## Ideas, methods and patents

Copyright protects code and text, not ideas: a feature tree, sketch constraints or an
adaptive roughing strategy can be built independently. Methods, however, can be patented.
Where AnvilCAD implements something that commercial products patent (adaptive tool paths
in CAM, for example), the design is based on published academic work and expired or
unclaimed techniques, the patents that were checked are listed with the design (see
`docs/cam/`), and a freedom-to-operate review is recommended before the feature is
marketed. Apache-2.0 gives every user an explicit patent licence from each contributor
for their contributions.

## For contributors

See [CONTRIBUTING.md](CONTRIBUTING.md). In short: every commit carries a Developer
Certificate of Origin sign-off, contributions must be your own work, reverse-engineering
findings go into a specification in `docs/re/` before code is written, and confidential
information stays out. If you work for a CAD vendor or have access to its source code or
confidential documentation, check your obligations before contributing in related areas,
and say so in your pull request.
