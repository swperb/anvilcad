# Interoperability policy

AnvilCAD is free and open-source software (MIT OR Apache-2.0). It aims to work well
alongside commercial CAD, PDM and PLM systems, including those from Dassault Systèmes, and
to work fully on its own, without any of them installed. This page sets out how the project
does that, so contributors and users can rely on it.

This is the project's engineering policy, not legal advice. Reverse engineering commercial
software carries legal risk: licence agreements, Dassault's included, restrict it, and the
statutory exceptions for interoperability (for example in the EU Software Directive) are
narrow and conditional. This policy must be reviewed by a lawyer in the relevant
jurisdictions before a 1.0 release, and before anyone sells or markets a feature built from
reverse-engineering work.

## Reverse engineering for interoperability

AnvilCAD studies how SOLIDWORKS and other commercial systems behave, and how their files are
structured, so that it can match their workflows and read and write their data natively.
Features that adapt to SOLIDWORKS are developed with REA (Reverse Engineer Anything,
<https://github.com/morluto/rea>), which may include analysing SOLIDWORKS binaries and
proprietary formats such as `.sldprt`, `.sldasm` and `.slddrw`.

The work follows these rules:

1. **Study, then write from scratch.** Findings are written up as a specification in
   `docs/re/`, saying what was studied, with which version, and how. AnvilCAD's code is
   written from that specification. No code, libraries, type libraries, help text, icons or
   user-interface artwork from commercial products is copied into the repository.
2. **No circumvention.** AnvilCAD does not circumvent licence checks, copy protection,
   encryption or any other technical protection measure, and does not help anyone else do
   so.
3. **No leaked or confidential material.** AnvilCAD does not use leaked material,
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

`anvil-migrate`, which converts parts by driving an installed, licensed copy of SOLIDWORKS
through its published automation interface, is an optional convenience. AnvilCAD ships no
Dassault code for it and binds late, at run time, to what the user's installation provides.
No other feature depends on it.

Integration with SOLIDWORKS PDM and the 3DEXPERIENCE platform uses the customer's own
licences and credentials, and credentials are never stored in plain text.

## Analysis tools in the repository

`third_party/rea` is a git submodule pinning [REA](https://github.com/morluto/rea) (MIT),
a general-purpose reverse-engineering toolkit, kept so it is available offline and in cloud
development sessions. It is not part of AnvilCAD's build and nothing in AnvilCAD links to or
ships it. Having it here changes none of the rules above: it is never pointed at Dassault
Systèmes software, its libraries or installers, at proprietary CAD formats, or at any other
software whose licence forbids reverse engineering, and nothing learned that way may enter
AnvilCAD. Fetch it with `git submodule update --init third_party/rea`.

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
