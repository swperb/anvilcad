# Positioning: where AnvilCAD stands in the market

Context for anyone, human or agent, writing AnvilCAD's README, landing page
(`docs/index.html`), release notes or announcements. It says who AnvilCAD is for, what no
other project offers, and which claims are true today. Researched 2026-10-18; re-check the
landscape section before relying on it months later.

## One line

The open-source way to leave SOLIDWORKS without losing your feature trees.

## Who it is for

Professional mechanical engineers and the firms that employ them: teams with years of
`.sldprt` files, a SOLIDWORKS-trained staff, and a vault they cannot afford to strand.
What keeps them on SOLIDWORKS is their data and their habits, not the price. AnvilCAD
answers both.

Not the primary audience: hobbyists coming from Fusion, or people who want code-first CAD.
Other projects serve them (see below), and chasing them would blur the message.

## The moat

No other project we found combines all four:

1. **A SOLIDWORKS workflow.** Feature tree, sketches on planes or faces, fully defined
   sketches, relations, smart dimensions, equations, configurations, custom properties.
   A SOLIDWORKS user is productive on day one.
2. **Migration that keeps the history.** `anvil-migrate` drives the user's own licensed
   SOLIDWORKS through its published API and rebuilds each part as editable AnvilCAD
   features, checking every solid feature against SOLIDWORKS' own volume. `anvil-bulk`
   moves a whole archive across many machines, with or without SOLIDWORKS.
3. **No SOLIDWORKS required, ever.** Everything works on a machine that never had it, on
   macOS, Linux and Windows. A licence only adds fidelity while it lasts.
4. **Its own exact kernels, in Rust.** Exact B-rep geometry, built to never panic on bad
   input, with STEP in and out.

Plus a road to the whole stack: CAM (2.5D milling exists as a library), then PDM and PLM.

## Claims: say these, not those

Keep every claim true of the code on the day it is published. Check the README's "What
works today" first.

| Say | Do not say (yet) |
|---|---|
| Migrates SOLIDWORKS parts with their feature trees, using your own SOLIDWORKS licence | Opens `.sldprt` geometry natively (today it identifies the file and reads custom properties; geometry comes from STEP or `anvil-migrate`) |
| Parts migrate as Parametric or Hybrid, with a per-file report and volume check | Migrates assemblies or drawings (they are recorded, not migrated yet) |
| Works without SOLIDWORKS installed | A drop-in SOLIDWORKS replacement |
| Familiar to SOLIDWORKS users | Uses SOLIDWORKS' names for features, or its icons or artwork |
| Free and open source (MIT OR Apache-2.0) | Affiliated with or endorsed by Dassault Systèmes |

Name SOLIDWORKS only to describe compatibility, and keep the existing non-affiliation
notice. See [INTEROPERABILITY.md](../INTEROPERABILITY.md).

## Landscape (as of 2026-10-18)

| Project | What it is | Overlap with AnvilCAD |
|---|---|---|
| [SolidCraft](https://github.com/tangency-labs/solidcraft) | Rust parametric CAD on the truck kernel; desktop, browser, MCP; about 4.8k stars | Low on audience. It is built for Fusion users ("Coming from Autodesk Fusion?"; timeline, joints, workspaces) and has no SOLIDWORKS import or migration. Competes for Rust CAD attention and contributors. |
| [FreeCAD](https://www.freecad.org/) | The established open-source parametric CAD | The default "free SOLIDWORKS alternative". Gets geometry from SOLIDWORKS files at best, never feature trees. |
| [vcad](https://github.com/ecto/vcad), next.BREP | Rust B-rep CAD, AI- and browser-oriented | Kernel and tooling peers; no SOLIDWORKS angle. |
| [openswx](https://github.com/schwitters/openswx), swformat, sldprt2step, sld2step | Small SOLIDWORKS file-format readers from the 2026 reverse-engineering wave | Properties, BOMs and some geometry; none is a CAD application or recovers feature trees. |
| [Carbonpoint/anvil-cad](https://github.com/Carbonpoint/anvil-cad) | Early Rust CAD/CAM modelled on Siemens NX | Not a competitor, but the name is nearly ours. Always write "AnvilCAD" and link `swperb/anvilcad`. |

Takeaway for copy: lead with migration and familiarity, not with "free CAD in Rust".
Plenty of projects are free CAD in Rust; none brings your SOLIDWORKS history with it.

## Messages, in order of weight

1. **Bring your feature trees.** Your parts come across as editable features, verified
   against SOLIDWORKS, with a report that says exactly what did and did not.
2. **Move a whole organisation, not one file.** `anvil-bulk` runs across many machines,
   deduplicates a 20-year archive and never touches the originals.
3. **You already know how to use it.** The workflow follows SOLIDWORKS conventions.
4. **Yours to keep.** No subscription, no licence server, no cloud lock-in; your files are
   versioned JSON on your own disk.
5. **Built to be trusted.** Exact geometry, no crashes on bad input, open code.

## Proof to show, not tell

- The migration status table and a sample `migration-report.csv` row.
- A side-by-side of a feature tree before and after migration (only from a part the
  project may publish; generate screenshots with `anvilcad-agent`, see
  [AGENT_HARNESS.md](AGENT_HARNESS.md)).
- The `anvil-bulk` four-command walkthrough.
- The specifications in `docs/re/`, as evidence of careful, clean-room work.
