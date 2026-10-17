# Adaptive clearing in AnvilCAD

This page describes how `anvil-cam` clears 2.5D pockets with a capped radial engagement,
the published work it builds on, how it was tested, and the patents that were checked
while designing it. Code: `crates/anvil-cam/src/adaptive.rs` and `src/raster.rs`.

The patent section is engineering diligence by the people who wrote the code. It is not
a legal opinion. Before anyone sells or markets this feature, a patent attorney should do
a freedom-to-operate review in the relevant jurisdictions. Nothing here says the feature
is free of patents; it says what was checked and how the design relates to it.

## What the operation does

Input:

- a region the tool must stay inside: an outer boundary with islands (`geom::Region`);
- optionally, where the stock actually is (the region may extend past it, so the tool
  can start in the air beside the stock);
- the tool diameter, a hard limit on the radial engagement angle (or a stepover, turned
  into an angle with `AdaptiveParams::engagement_for_stepover`), the step-down, the top
  and floor heights, the stock to leave on walls, feeds, the helix ramp angle and an
  optional chip-thinning feed limit.

Output: a machine-independent tool path (`toolpath::Toolpath`) with helical entries,
cutting moves, links at depth through cleared space, and retracts only where no clear
link exists, plus a report: passes, entries, links, retracts, the largest engagement
planned, the area removed, the reachable material left, and the material no tool position
can reach (corners tighter than the tool, gaps narrower than it).

Engagement means the angle of the tool's circumference that lies over uncut material.
For a straight cut at radial depth `ae` with tool radius `R` it is `acos(1 - ae / R)`
(Altintas 2012), so a 10% stepover is about 37 degrees and a full slot is 180 degrees.

## How it works

### The material model

The planner keeps the uncut material of one layer as a square raster of cells, judged at
their centres (`raster::Grid`), by default 1/80 of the tool diameter (0.125 mm for a 10 mm
tool) and never coarser than 1/60. Removing the tool's disk and counting how much of its
circumference is over material are both simple loops over cells, cannot fail on
degenerate geometry, and stay fast over tens of thousands of steps; when the tool moves a
short way, only the crescent between the old and new disks is visited. Polygon booleans
(the other common choice, with Clipper-style libraries) are exact but slower per step and
have to survive slivers and near-coincident edges. The raster's error is bounded by the
cell size, and the planner and verifier are built around that bound (see "Margins").

Where the tool may go is decided exactly, not on the raster: a tool centre is allowed
when it is inside the region and at least the tool radius plus the stock to leave from
every boundary segment (a bucketed segment index makes this quick). So no planned move
can gouge a wall or island, whatever the raster says.

### Rounds of trace, cut, link

The planner works in rounds until no reachable material is left or nothing more can be
cut within the limit.

1. **Trace.** For every allowed cell near the material edge it measures the engagement
   of a tool standing there. Along a straight edge, a tool moving along the edge has
   already cut the half of the contact behind it, so a moving tool engages half of what a
   standing one does. The planner traces, by marching squares, the curves where the
   standing engagement equals twice the target. These curves are offsets of the cut
   boundary at constant engagement, which is the idea of Stori and Wright (1998, 2000):
   the stepover varies with the shape so the engagement does not. At inside corners of
   the material the engagement rises, so the curves pull back from the corner by
   themselves. The curves are smoothed lightly to remove the raster's stair steps. Only
   cells near material cut since the last round are measured again.
2. **Cut.** Each curve becomes the guide for one pass, run with the material on the tool's
   right, which is climb milling with a clockwise spindle. The pass starts clear of the
   material and joins the curve gradually over 1.5 tool diameters (a cosine blend), so the
   bite grows from nothing and the material behind the tool is what it has just cut.
   Joining a curve head-on would leave uncut material behind the tool as well as ahead of
   it and roughly double the engagement. The pass is then simulated against the material
   model point by point, half a cell apart. A point whose engagement would exceed the
   guard is moved out to the free side, by as little as fixes it, and the path eases back
   to the guide afterwards a quarter cell at a time; this is the offset correction of
   Stori and Wright and of Uddin, Ibaraki and colleagues (2006), and enlarging the path's
   turning radius where engagement is too high is the method of Oldani's US 6,428,252,
   which expired in 2017. Where no such point exists the pass stops. Passes that remove
   almost nothing for their length are undone and not emitted.
3. **Link.** The tool moves to the next pass along straight feed moves through cleared
   space when one is clear of material, or along a path found by A* search on a grid of
   clear tool positions and then shortened to straight pieces. It retracts, moves at
   rapid above the work and feeds down into cleared space only when no such path exists.
   Links are planned against the material as it was before the pass they lead to.
4. **Enter.** When no curve can be started from cleared space (a closed pocket at the
   start, or a separate pocket) the tool enters by a helix at the remaining material
   farthest from the walls, with the preferred radius (0.3 times the diameter) or less,
   never less than 0.1 times the diameter. Where the region extends past the stock,
   passes start in the air beside it and no helix is used. Material with no room for a
   helix is reported, not cut.

When no traced curve can be cut, one more pass runs along the boundary of where the tool
may go, to pick up thin material against the walls within the limit. What is left after
that is reported.

The plan is made once in 2D and repeated at each step-down level: each layer is a full
slab, so the same sequence holds, and helices start just above the previous layer.

### Feed adjustment by engagement

With `chip_thinning_max` set, cutting feeds rise where the engagement is low so the
largest chip stays at the programmed chip load: for a straight-edged cutter entering at
the uncut edge, the largest chip is `fz * sin(phi)` for engagement `phi` up to 90 degrees,
so the feed factor is `1 / sin(phi)`, limited by the setting and quantised to 5% steps.
This is the radial chip thinning relation published by tool makers (Machining Doctor's
`RCTF = 1 / sqrt(1 - (1 - 2 ae / D)^2)`, which is the same expression; Harvey
Performance and Sandvik Coromant give the same relation as figures). Feed scheduling
from engagement or chip thickness in simulation is also the subject of expired patents
(General Electric US 4,833,617; Kakino and others US 6,772,039 and US 6,942,436; Okuma US
6,266,572). It is off by default.

### Margins

The raster places the rim of a cut to within about a cell, which moves a straight cut's
engagement by roughly `cell / R` radians, and much more where the remaining material
hugs the tool (a small helix bore, an inside corner). So:

- passes are traced at 85% of a guard, and the guard sits below the hard limit by two
  cells' worth of angle (`2 cell / R`) or 5%, whichever is more;
- the guard measures engagement a quarter cell outside the tool radius, so the raster's
  error counts against a cut, not for it.

## How it is checked

`adaptive::measure_engagement` replays a finished tool path into a fresh, finer raster
(it shares no state with the planner) and measures the engagement at points half a cell
apart along every cutting move, counting contact where the material at the tool's edge is
at least two of its cells thick (a rim thinner than the raster can resolve, left along an
earlier cut, is not a cut). It reports the largest engagement and where it occurred,
every sample over the limit, any sample near a full slot, link or rapid moves that touch
material (allowing a tolerance of two planner cells, since the planner knows the rims of
its cuts only to a cell, so a link may brush up to about that much), and the area left.

Tests (`cargo test -p anvil-cam adaptive`):

- a 54 x 34 mm pocket with a rectangular and a round island (every channel at least 11 mm,
  1.4 tool diameters, wide), 8 mm tool, 45 degree limit: one helix, links far outnumber
  retracts, under 1% of the reachable area left, and the verifier at half the planner's
  cell size finds no sample over 45 degrees but a maximum over 30 (so the check is not
  vacuous), no slotting, no link touching material, and no tool position closer than its
  radius to a wall;
- 40 x 30 mm stock with room around it, 10 mm tool, 30 degree limit, two layers: entered
  from the side with no helix, cuts at exactly the two layer depths, under 1 mm^2 left, no
  sample over 30 degrees in either layer, chip-thinning feeds between 1 and 2 times the
  base feed;
- bad parameters (negative or NaN sizes, a limit of 180 degrees, a floor above the top, a
  tool wider than the pocket) return errors.

The end-to-end test (`tests/end_to_end.rs`) clears a pocket with an island on a part
recognised from a solid, writes the program with each bundled post, and certifies it with
the G-code verifier and the stock simulator (no gouge, no collision, no rapid through
material).

On the 60 x 40 mm example at the default resolution (0.1 mm cells), a release build plans
in about 12 seconds with one helix and no retracts, and the verifier finds a largest
engagement of 40 to 44 degrees against the 45 degree limit at replay cells of 0.04, 0.05
and 0.0625 mm, with no sample over the limit and no link touching material.

## Known limits

- **Speed.** Planning takes seconds in a release build for a small pocket, most of it in
  measuring the engagement field. It is single-threaded and recomputes more of the field
  than it needs to.
- **Links.** Each climb-only pass ends where the next cannot begin, so the tool links back
  often. Links run at feed (default: the cutting feed). Raising them slightly above the
  work, or alternating direction where conventional milling is acceptable, would shorten
  the time.
- **Narrow channels.** A channel only a little wider than the tool cannot be peeled within
  a low limit; the planner leaves such material and reports it. Choosing a smaller tool
  (the automatic planner uses at most 0.6 times the narrowest gap) or a trochoidal pass is
  the answer; trochoidal passes are on the roadmap.
- **Rest material.** Corners tighter than the tool and slivers thinner than a cell are left
  for finishing; the report says how much.
- **2.5D only.** Layers are full slabs with vertical walls. Drafted walls and 3D surfaces
  are not handled.

## Sources

Published methods:

- J. A. Stori, P. K. Wright, "A Constant Engagement Offset for 2-1/2 D Tool Path
  Generation", ASME IMECE 1998, doi:10.1115/IMECE1998-1061; "Constant engagement tool
  path generation for convex geometries", Journal of Manufacturing Systems 19 (2000)
  172-184, doi:10.1016/S0278-6125(00)80010-2.
- M. S. Uddin, S. Ibaraki, A. Matsubara, S. Nishida, Y. Kakino, "Constant Engagement Tool
  Path Generation to Enhance Machining Accuracy in End Milling", JSME International Journal
  Series C 49 (2006) 43-49, doi:10.1299/jsmec.49.43. S. Ibaraki and others, "Constant
  Engagement Tool Path Generation for Two-dimensional End Milling", Japan-USA Symposium on
  Flexible Automation 2004.
- S. Ibaraki, I. Yamaji, A. Matsubara, "On the removal of critical cutting regions by
  trochoidal grooving", Precision Engineering 34 (2010) 467-473,
  doi:10.1016/j.precisioneng.2010.01.007.
- A. Jacso, G. Matyasi, T. Szalay, "The fast constant engagement offsetting method for
  generating milling tool paths", Int. J. Adv. Manuf. Technol. 103 (2019) 4293-4305,
  doi:10.1007/s00170-019-03834-8.
- M. Held, *On the Computational Geometry of Pocket Machining*, LNCS 500, Springer 1991,
  doi:10.1007/3-540-54103-9. M. Held, C. Spielberger, "A smooth spiral tool path for high
  speed machining of 2D pockets", Computer-Aided Design 41 (2009) 539-550,
  doi:10.1016/j.cad.2009.04.002. M. Held, J. Pfeiffer, "Trochoidal Tool Paths for Pocket
  Machining with Full Control of the Tool Engagement Angle", CAD&A 22 (2025) 731-747,
  doi:10.14733/cadaps.2025.731-747 (metadata only; not read).
- M. B. Bieterman, D. R. Sandstrom, "A Curvilinear Tool-Path Method for Pocket Machining",
  J. Manuf. Sci. Eng. 125 (2003) 709-715, doi:10.1115/1.1596579.
- G. Elber, E. Cohen, S. Drake, "MATHSM: medial axis transform toward high speed machining
  of pockets", Computer-Aided Design 37 (2005) 241-250, doi:10.1016/j.cad.2004.05.008.
- V. Pateloup, E. Duc, P. Ray, "Corner optimization for pocket machining", IJMTM 44 (2004)
  1343-1353, doi:10.1016/j.ijmachtools.2004.04.011.
- H. S. Choy, K. W. Chan, "A corner-looping based tool path for pocket milling",
  Computer-Aided Design 35 (2003) 155-166, doi:10.1016/S0010-4485(02)00049-0.
- Z. Y. Zhao and others, "Pocketing toolpath optimization for sharp corners", J. Materials
  Processing Technology 192-193 (2007) 175-180, doi:10.1016/j.jmatprotec.2007.04.096.
- M. Otkur, I. Lazoglu, "Trochoidal milling", IJMTM 47 (2007) 1324-1332,
  doi:10.1016/j.ijmachtools.2006.08.002. M. Rauch, E. Duc, J.-Y. Hascoet, IJMTM 49 (2009)
  375-383, doi:10.1016/j.ijmachtools.2008.12.006.
- B. K. Choi, B. H. Kim, "Die-cavity pocketing via cutting simulation", Computer-Aided
  Design 29 (1997) 837-846, doi:10.1016/S0010-4485(97)00031-6.
- Y. Altintas, *Manufacturing Automation*, 2nd ed., Cambridge University Press 2012,
  doi:10.1017/CBO9780511843723 (engagement and chip thickness geometry).
- P. F. Felzenszwalb, D. P. Huttenlocher, "Distance Transforms of Sampled Functions",
  Theory of Computing 8 (2012) 415-428, doi:10.4086/toc.2012.v008a019 (the exact distance
  transform used for wall distances and link search).
- Chip thinning: Machining Doctor, "Chip Thinning Calculator",
  https://www.machiningdoctor.com/calculators/chip-thinning-calculator/ ; Harvey
  Performance, "How to Combat Chip Thinning" (2017),
  https://www.harveyperformance.com/in-the-loupe/combat-chip-thinning/ ; Sandvik Coromant,
  "Milling formulas and definitions" and "Milling inside corners".

Open-source prior art (descriptions and licences only; no code read or copied):

- FreeCAD CAM "Adaptive" (the add-on by GitHub user kreso-t, merged into FreeCAD, which is
  LGPL-2.1). Its public description says it keeps engagement and material removal below a
  maximum. How it does that internally was not confirmed from a public description, and
  this design does not depend on it.
- libarea (BSD-3-Clause per its Debian copyright file), OpenCAMLib (LGPL-2.1; drop-cutter
  and waterline, no adaptive clearing), PyCAM (GPL-3.0-or-later).
- `cavalier_contours` (MIT OR Apache-2.0) is used for exact offsets in profiling.

## Patents checked

Searched on 2026-10-17 in the USPTO Patent Public Search full text (Google Patents was
unavailable that day); two European counterparts were confirmed on the EPO publication
server. Expiry dates are upper bounds computed from the filing date plus patent term
adjustment; terminal disclaimers and unpaid maintenance fees, which were not checked, can
only make them earlier. The one-line summaries paraphrase the first independent claim.
The full table, with URLs, is kept with the research notes; the patents that matter most
for this design are below.

| Patent | Holder | Est. expiry | Core claimed idea | How this design relates |
|---|---|---|---|---|
| US 8,489,224; US 9,052,704 | SolidCAM | about 2031 | Preselect a maximum and a minimum engagement angle, and make engagement vary gradually between them along the path. | Takes only a maximum, as a hard cap; there is no minimum and no deliberate variation between limits. Passes are traced at one engagement level; where the cap would be exceeded the path is moved out. Engagement does vary along any path (at blends, corners and pass ends), so counsel should read these claims closely: they are the broadest found. |
| US 9,823,645; US 10,895,861; US 10,324,445 | SolidCAM | about 2031 | An asymmetric, non-circular spiral with engagement swinging between max and min within each loop. | No spirals; passes are separate offset curves. |
| US 10,620,607; US 12,248,293; US 11,625,019; US 9,690,282 | SolidCAM | 2031 to 2033 | Depth and width from spindle power; choosing trochoidal or spiral by estimated time; two-sided thin walls; step-up roughing. | Depth and stepover are user inputs; no strategy choice by predicted time; none of the others. |
| US 8,295,972; US 8,880,212 | Celeritive | 2029 to 2030 | Pre-cut "transition areas" across the main cuts, trim the cuts there, and link through those areas while disengaged. | Links run through space the passes have already cleared; no dedicated transition areas are computed or cut, and passes are not trimmed against them. |
| US 9,946,245; US 10,579,040; US 10,108,176 | Celeritive | about 2032 to 2033 | A Voronoi diagram modified with extra branches; replacing an arc with a non-concentric arc nearer the boundary. | No Voronoi diagram; no arc replacement. |
| US 7,577,490 | Surfware/TrueMill | possibly until about 2026-12-17 (unverified) | Tangent-continuous arcs advancing by a set pitch into a three-sided region, the first tangent to a wall. | No arc-by-pitch passes. The rest of this family (US 7,451,013 and others) has expired. |
| US 8,560,113 | Surfware/TrueMill | about 2031 | First arc tangent to two sides, parallel arcs stepped toward the third side, transition passes. | Not used. |
| US 9,489,339 | Delcam (Autodesk) | about 2035 | Per-corner subsets of offset paths with a deliberately larger gap, joined at corners, plus backfill paths; claim 18 enlarges the preceding pass's corner when corner engagement is too high. | No corner subsets, gaps or backfill. Where engagement would be too high the current pass is moved out (Oldani's expired method), not a preceding pass. |
| US 9,778,649; US 10,488,847 | DP Technology | about 2033 to 2034 | Blend-arc radius from an allowed variation of engagement, used to smooth offsets, within a long chain of steps. | No blend-arc radius from engagement variation; curves are smoothed by neighbour averaging for raster noise only. |
| US 10,108,172 | Autodesk | about 2036 | A spiral built from a Voronoi diagram modified into a single cycle. | No spiral, no Voronoi diagram. |
| US 12,736,945 | HCL Technologies | about 2045 | On a pixel model, for each next segment evaluate engagement for candidate moves and pick the one closest to a required engagement (or follow the pocket boundary when engagement is not positive); then split the raw path into cutting and non-cutting sections, group them into "trochoidal region groups" and "boundary region groups", and join cutting sections with link moves into a trochoidal path. | Uses a pixel model, which this claim names, so it was read in full (claims 1 and 9). This design does not choose among candidate moves by closeness to a target: passes are whole level-set curves traced in advance and corrected only to stay under a cap, and it has no grouping into trochoidal or boundary region groups and no assembly of a trochoidal path from them. An earlier prototype of this crate did choose headings step by step against a target and was replaced, partly for this reason. Counsel should still compare the claims with the code. |
| US 2025/0004442 (pending) | Röders | if granted, about 2042 | Simulate removal per path increment, compute an engagement ratio, adjust feed or speed from it. | The optional chip-thinning feed sets feed from the planned engagement. The idea is in expired patents (GE US 4,833,617; Kakino US 6,772,039) and tool-maker formulas, but this application is pending; the feature is off by default. |
| US 9,977,421; US 12,547,143 | Open Mind | about 2035; about 2044 | Links bent toward the segment with higher feed; edge strips left by adjacent sections and covered by a transition section. | Neither is done. |

Expired patents this design draws on: Oldani US 6,428,252 (cap the engagement and enlarge
the turning radius where it is exceeded, about 2017), General Electric US 4,833,617 and
US 4,907,164, Kakino and others US 6,772,039 and US 6,942,436, Okuma US 6,266,572,
POSTECH US 7,050,883, Boeing US 6,591,158 (Bieterman and Sandstrom's spiral), and most
of the Surfware engagement-milling family.

Not found: no US patent on the core of Autodesk/HSMWorks Adaptive Clearing, Delcam Vortex,
Mastercam Dynamic Motion, CGTech Vericut OptiPath or Force, ModuleWorks, Gibbs or Siemens
NX adaptive milling. Foreign filings were not searched. Absence from this search is not
evidence that none exist.

Recommendation: before AnvilCAD's adaptive clearing is sold or marketed, have a patent
attorney review it against at least the SolidCAM family (and its pending continuation),
HCL US 12,736,945, Delcam US 9,489,339, Celeritive US 8,295,972 and US 8,880,212, and the
pending Röders application, in the jurisdictions where it will be offered.
