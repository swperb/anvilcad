# Interoperability and clean-room policy

AnvilCAD is free and open-source software (MIT OR Apache-2.0). It aims to work well
alongside commercial CAD, PDM and PLM systems, including those from Dassault Systèmes,
without taking anything that is not ours to take. This page sets out how the project does
that, so contributors and users can rely on it.

This is the project's engineering policy, not legal advice. Before a 1.0 release, and
before anyone sells or markets a feature that touches these areas, the policy should be
reviewed by a lawyer in the relevant jurisdictions.

## What AnvilCAD does not do

- It does not decompile, disassemble, debug, trace or otherwise inspect Dassault Systèmes
  software (SOLIDWORKS, CATIA, 3DEXPERIENCE, SOLIDWORKS PDM, or any other), its libraries
  or its installers.
- It does not reverse-engineer proprietary file formats such as `.sldprt`, `.sldasm`,
  `.slddrw` or `.CATPart`. AnvilCAD only recognises such files by their extension and the
  generic, publicly documented Microsoft compound-file signature, so it can tell the user
  what the file is and how to convert it.
- It does not use leaked material, documentation or SDKs obtained under a non-disclosure
  or restricted licence, or anything a contributor learned in confidence.
- It does not circumvent licence checks, copy protection, encryption or any other
  technical protection measure.
- It does not copy code, documentation, help text, icons, user-interface artwork or
  product names from commercial products.

Why: commercial licence agreements, Dassault's included, restrict reverse engineering, and
in some places a contract can override statutory exceptions for interoperability. Laws
against circumventing protection measures and protecting trade secrets apply as well. The
statutory exceptions that do exist (for example the decompilation exception for
interoperability in the EU Software Directive) are narrow and conditional. Open standards
and published interfaces give AnvilCAD all the interoperability it needs, so the project
stays well clear of that ground.

## How AnvilCAD interoperates

1. **Open standards first.** STEP (ISO 10303, AP203/AP214/AP242) is the main path for exact
   geometry in and out, and later for product structure and PMI. STL and OBJ cover meshes.
   Future work follows open standards too: AP242 for assemblies and PMI, ISO 10303-239
   (PLCS) for product life-cycle data, and OASIS OSLC for linking PLM records.
2. **The vendor's published API, on the user's own licensed copy.** `anvil-migrate` reads
   SOLIDWORKS parts by asking an installed, licensed copy of SOLIDWORKS to open them,
   through the automation interface Dassault publishes for third-party programs. SOLIDWORKS
   reads the file; AnvilCAD only receives what the public API returns. AnvilCAD ships no
   Dassault code, libraries or type libraries; it binds late, at run time, to what the
   user's installation provides. Use is governed by the user's own SOLIDWORKS licence.
3. **Published PDM and PLM interfaces.** Integration with SOLIDWORKS PDM and the
   3DEXPERIENCE platform will use only their documented APIs and web services, with the
   customer's own licences and credentials, and respecting those services' terms. No
   undocumented endpoints, no scraping, and credentials are never stored in plain text.

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
Certificate of Origin sign-off, contributions must be your own work, and anything learned
from proprietary code, decompilation or confidential information stays out. If you work for
a CAD vendor or have access to its source code or confidential documentation, check your
obligations before contributing in related areas, and say so in your pull request.
