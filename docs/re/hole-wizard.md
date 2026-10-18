# Hole feature

What AnvilCAD's Hole feature does. It makes standard drilled, counterbored, countersunk
and tapped holes at the points of a sketch, sized from fastener tables. SOLIDWORKS users
know the equivalent as its hole wizard, so AnvilCAD follows its published behaviour where
that makes sense. This specification was written for the implementation in
`crates/anvil-model/src/hole.rs` (feature, section, tables), `crates/anvil-model/src/rebuild.rs`
(cutting) and `crates/anvil-app/src/app.rs` (command and editor).

## How this was studied

- **Inputs.** Only published information was used:
  - Dassault's public online help for SOLIDWORKS 2025 and 2016, and the 2024 API help;
  - public reference pages on thread and drill sizes (sources below).

  No SOLIDWORKS software, installer, library or file was downloaded, installed, run or
  analysed. No SOLIDWORKS licence was accepted on the machine used.
- **Retrieval.** The help site renders its articles with JavaScript, and a plain request
  is refused (HTTP 403). Each article was fetched over HTTPS with a browser user agent.
  Its text was read from the page's embedded `__NEXT_DATA__` JSON
  (`props.pageProps.helpContentData.helpText`), with HTML stripped by a script run with
  `python3 -I`. A missing page still returns HTTP 200, with "page cannot be found" in its
  data, so each page's content was checked. The text was paraphrased; none of it is copied
  into this repository.
- **REA.** REA (`rea-agents` 6.5.0, with the `third_party/rea` submodule at `3fc0075`) was
  set up and its research method followed:
  - each finding below is marked **Observed** (stated in a cited source), **Inferred**
    (our reasoning from the sources), **Decision** (AnvilCAD's own choice where the
    sources are silent) or **Unknown**;
  - every observation cites its source.

  REA's analysis tools were **not** run. They inspect artifacts (binaries, packages,
  captures, files), and the only lawful inputs for this behaviour are prose documents.
  Pointing REA at SOLIDWORKS software is ruled out by
  [INTEROPERABILITY.md](../../INTEROPERABILITY.md). The `.sldprt` storage of a hole
  feature was not studied.
- **Size tables.** SOLIDWORKS's own size tables ship inside its installed database
  (S2), which we neither have nor use. AnvilCAD's tables come from published standards
  and public references, so its default sizes can differ from SOLIDWORKS's.

## Sources

Base A is `https://help.solidworks.com/2025/English/SolidWorks/sldworks/`. Base B is
`https://help.solidworks.com/2024/English/api/sldworksapi/`. Base C is
`https://help.solidworks.com/2024/English/api/swconst/`. All were read on 2026-10-18.

| # | Topic | URL | Version |
|---|---|---|---|
| S1 | Hole wizard overview | A + `c_Hole_Wizard_Overview.htm` | 2025 |
| S2 | Hole type PropertyManager | A + `r_HIDD_DVE_HW_HOLE_SPEC.htm` | 2025 |
| S3 | Hole positions PropertyManager | A + `HIDD_DVE_HW_INSERT_HOLE.htm` | 2025 |
| S4 | Slots from the hole wizard | A + `t_creating_slots_hole_wizard.htm` | 2025 |
| S5 | Match size and preserve settings | A + `c_hole_wizard_match_size_preserve_settings.htm` | 2025 |
| S6 | End condition types | A + `c_End_Condition_Types.htm` | 2025 |
| S7 | Cosmetic threads | A + `c_cosmetic_threads.htm` | 2025 |
| S8 | Simple hole PropertyManager | A + `HIDD_END_SPEC_HOLE.htm` | 2025 |
| S9 | Decimal drill sizes | A + `t_toolbox_holes_drills_numbers_spec.htm` | 2025 |
| S10 | Hole type PropertyManager (older wording) | `https://help.solidworks.com/2016/English/SolidWorks/sldworks/HIDD_DVE_HW_HOLE_SPEC.htm` | 2016 |
| S11 | `IWizardHoleFeatureData2` members | B + `SOLIDWORKS.Interop.sldworks~SOLIDWORKS.Interop.sldworks.IWizardHoleFeatureData2_members.html` | 2024 |
| S12 | `IWizardHoleFeatureData2` interface | B + `SolidWorks.Interop.sldworks~SolidWorks.Interop.sldworks.IWizardHoleFeatureData2.html` | 2024 |
| S13 | Hole types and their valid properties | B + `Table_of_Wizard_Hole_Types_and_Valid_Properties.htm` | 2024 |
| S14 | `IFeatureManager::HoleWizard5` | B + `SolidWorks.Interop.sldworks~SolidWorks.Interop.sldworks.IFeatureManager~HoleWizard5.html` | 2024 |
| S15 | `IWizardHoleFeatureData2` properties (`DrillAngle`, `Type`, `EndCondition`, `CosmeticThreadType`, `HeadClearanceType`, `ThreadEndCondition`, `HoleFit`) | B + `SolidWorks.Interop.sldworks~SolidWorks.Interop.sldworks.IWizardHoleFeatureData2~<property>.html` | 2024 |
| S16 | `swWzdHoleTypes_e` | C + `SOLIDWORKS.Interop.swconst~SOLIDWORKS.Interop.swconst.swWzdHoleTypes_e.html` | 2024 |
| S17 | Hole enumerations (`swWzdGeneralHoleTypes_e`, `swEndConditions_e`, `swWzdHoleStandards_e`, `swWzdHoleStandardFastenerTypes_e`, `swWzdHoleScrewClearanceTypes_e`, `swWzdHoleCounterSinkHeadClearanceTypes_e`, `swWzdHoleThreadEndCondition_e`, `swWzdHoleCosmeticThreadTypes_e`) | C + `SOLIDWORKS.Interop.swconst~SOLIDWORKS.Interop.swconst.<enum>.html` | 2024 |
| S18 | API guide: hole wizard features and their data objects | `https://help.solidworks.com/2024/English/api/sldworksapiprogguide/OVERVIEW/Hole_Wizard_Features_and_WizardHoleFeatureData2_Objects.htm` | 2024 |
| N1 | ISO metric screw thread: coarse pitches, tap drill ≈ nominal less pitch | `https://en.wikipedia.org/wiki/ISO_metric_screw_thread` | not Dassault |
| N2 | Drill and tap sizes: inch tap drills, close and free clearance drills | `https://en.wikipedia.org/wiki/List_of_drill_and_tap_sizes` | not Dassault |
| N3 | Countersinks: 82° for Unified inch screws, 90° for ISO metric | `https://en.wikipedia.org/wiki/Countersink` | not Dassault |
| N4 | Drill bits: common point angles 118° to 135° | `https://en.wikipedia.org/wiki/Drill_bit` | not Dassault |
| K1 | ISO 273 clearance holes, ISO 4762 socket head cap screws, DIN 974-1 counterbores, ISO 10642 countersunk screws, ASME B18.3 inch socket head cap screws | Paid standards, not fetched. Values are from widely reproduced tables. **Check them against the standard texts.** | not Dassault |

## The feature

- **Observed (S1, S3).** One hole feature has two sketches: one places the holes, the other
  holds the hole's section. Each sketch point is a hole centre (S3, S11 `GetSketchPoints`).
- **Observed (S2, S3).** One feature has one hole type and size. It can place many holes of
  that type.
- **Inferred (S1, S13).** Each hole is its section turned about the hole's axis and
  subtracted from the body. Every parameter is a diameter, a depth or an angle about one
  axis. The help never says "revolve".
- **Decision.** AnvilCAD keeps the placement sketch as an ordinary sketch feature that the
  hole depends on. The holes go at the sketch's *free points*: points that are not the
  origin and belong to no line, arc or circle. Those points can be dimensioned and
  constrained like any other. The section is not a separate sketch; it is worked out from
  the hole's dimensions on every rebuild.
- **Decision.** The hole's axis is square to the sketch plane. The hole runs into the side
  away from the plane's normal: on a face, into the material. *Reverse direction* flips it
  (S2 has the same option).
- **Decision.** A new Hole command works one of two ways:
  - with a sketch selected or open, it uses that sketch, which must have points;
  - with a planar face selected, it makes a sketch on that face with one point where the
    face was clicked (or at the face's middle).

  *Edit positions* opens the sketch to move points or add more with the sketch's **Point**
  tool.

## Hole types

- **Observed (S2, S17).** SOLIDWORKS offers counterbore, countersink, hole, straight tap,
  tapered tap, legacy, counterbore slot, countersink slot and slot.
- **Implemented.** Hole, counterbore, countersink and tapped hole (straight tap). Slots,
  tapered taps and legacy holes are not implemented.

The section, from the face down:

- **Hole.** A bore of the hole diameter.
- **Counterbore.** A flat-bottomed bore of the counterbore diameter and depth, then the
  hole.
  - **Observed (S13).** There is no counterbore angle parameter, so the floor is flat.
- **Countersink.** A cone whose diameter at the face is the countersink diameter, with
  the countersink's included angle. It narrows to the hole diameter, and the hole carries
  on below.
  - **Observed (S13).** The countersink diameter and angle are the parameters.
- **Tapped hole.** A bore at the tap drill diameter. The thread (nominal diameter, pitch
  and depth) is recorded on the feature, not modelled.
  - **Observed (S2).** SOLIDWORKS's thread choices are "tap drill diameter", "cosmetic
    thread" and "remove thread". A cosmetic thread stands for the major diameter (S7).
  - **Decision.** AnvilCAD's tapped hole behaves like the tap drill choice and records the
    thread. Cosmetic threads in the graphics and drawings are future work.
  - **Observed (S14).** With no thread depth given, the default is twice the thread
    diameter. **Decision.** AnvilCAD does the same, with a hole depth of 2.5 times the
    thread diameter.
- **Near-side countersink.** An extra cone at the face on top of a hole, counterbore or
  tapped hole. On a counterbore it opens onto the counterbore.
  - **Observed (S13, S14).** It is an option with its own diameter and angle.
  - **Not implemented.** Under-head and far-side countersinks (S13, S14). Head clearance
    (S2, S14), beyond what the counterbore depth already allows.

## End conditions and depth

- **Observed (S2).** Holes offer Blind, Through All, Up To Next, Up To Vertex, Up To Surface
  and Offset From Surface, plus Reverse Direction.
- **Implemented.** Blind and Through All.
- **Observed (S2).** A blind depth can be measured to the shoulder (the end of the full
  diameter) or to the tip.
  - **Decision.** AnvilCAD measures to the shoulder, and the editor labels the field
    "Depth to shoulder".
- **Observed (S14).** The drill point angle defaults to 118°. It applies only to holes that
  do not run through.
  - **Decision.** A blind hole has a 118° drill point by default. It can be turned off for
    a flat bottom. A through hole has none.
- **Decision.** *Through all* reaches past the far side of the whole body.

## Standards, sizes and fit

- **Observed (S2, S14, S17).** A hole is chosen by standard, fastener type and size, and
  for clearance holes a fit of close, normal or loose (normal by default).
- **Observed (S2).** The size fills in the dimensions, and the user can then type over any
  of them.
- **Decision.** The feature stores every dimension itself, so a typed value or an equation
  (`=width/10`) works like any other dimension. The standard and size are stored only so
  that the editor can show them and refill from them.
  - Typing over a dimension marks the size "(custom)".
  - Choosing a new size, fit or standard refills the dimensions.
  - Changing the hole type refills that type's dimensions from the same size.
- **Decision.** The standards are ISO metric and ANSI inch (Unified coarse). A new hole is
  a through M6 clearance hole of normal fit.

### ISO metric (millimetres)

| Size | Pitch (N1) | Tap drill | Close / normal / loose (ISO 273 fine / medium / coarse, K1) | Counterbore Ø (DIN 974-1, K1) | Counterbore depth | Countersink Ø |
|---|---|---|---|---|---|---|
| M2 | 0.4 | 1.6 | 2.2 / 2.4 / 2.6 | 4.4 | 2.5 | 4.48 |
| M2.5 | 0.45 | 2.05 | 2.7 / 2.9 / 3.1 | 5.5 | 3.0 | 5.6 |
| M3 | 0.5 | 2.5 | 3.2 / 3.4 / 3.6 | 6.5 | 3.5 | 6.72 |
| M4 | 0.7 | 3.3 | 4.3 / 4.5 / 4.8 | 8 | 4.5 | 8.96 |
| M5 | 0.8 | 4.2 | 5.3 / 5.5 / 5.8 | 10 | 5.5 | 11.2 |
| M6 | 1.0 | 5.0 | 6.4 / 6.6 / 7.0 | 11 | 6.5 | 13.44 |
| M8 | 1.25 | 6.8 | 8.4 / 9 / 10 | 15 | 8.5 | 17.92 |
| M10 | 1.5 | 8.5 | 10.5 / 11 / 12 | 18 | 10.5 | 22.4 |
| M12 | 1.75 | 10.2 | 13 / 13.5 / 14.5 | 20 | 12.5 | 26.88 |
| M16 | 2.0 | 14.0 | 17 / 17.5 / 18.5 | 26 | 16.5 | 33.6 |
| M20 | 2.5 | 17.5 | 21 / 22 / 24 | 33 | 20.5 | 40.32 |

- **Inferred.** Close, normal and loose map onto ISO 273's fine, medium and coarse
  series.
- **Decision.** The tap drill is the usual drill size nearest the nominal diameter less the
  pitch (N1). That gives 6.8 for M8 and 10.2 for M12.
- **Decision.** The counterbore depth is the ISO 4762 head height (equal to the nominal
  diameter, K1) plus 0.5 mm, so the head sits below the face. S14 adds head clearance on
  top of the counterbore depth in the same way.
- **Decision.** The countersink diameter is the ISO 10642 theoretical head diameter
  (2.24 times the nominal diameter, K1), at 90° (N3). ISO 10642 starts at M3, so M2 and
  M2.5 use the same ratio.

### ANSI inch (inches; held in millimetres)

| Size | Tap drill (N2) | Close (N2) | Normal: free (N2) | Loose | Head Ø / height (ASME B18.3, K1) |
|---|---|---|---|---|---|
| #4-40 | 0.0890 | 0.1160 | 0.1285 | 0.1441 | 0.183 / 0.112 |
| #6-32 | 0.1065 | 0.1440 | 0.1495 | 0.1651 | 0.226 / 0.138 |
| #8-32 | 0.1360 | 0.1695 | 0.1770 | 0.1926 | 0.270 / 0.164 |
| #10-24 | 0.1495 | 0.1960 | 0.2010 | 0.2166 | 0.312 / 0.190 |
| 1/4-20 | 0.2010 | 0.2570 | 0.2660 | 0.2816 | 0.375 / 0.250 |
| 5/16-18 | 0.2570 | 0.3230 | 0.3320 | 0.3476 | 0.469 / 0.3125 |
| 3/8-16 | 0.3125 | 0.3860 | 0.3970 | 0.4126 | 0.5625 / 0.375 |
| 1/2-13 | 0.4219 | 0.5156 | 0.5313 | 0.5469 | 0.750 / 0.500 |

- **Decision.** N2 gives only close and free clearance drills, so AnvilCAD uses:
  - normal: the free drill;
  - loose: the free drill plus 1/64 in;
  - counterbore: the head diameter plus 1/32 in, and the head height plus 0.02 in deep;
  - countersink: 82° (N3), twice the nominal diameter across.

  An ASME B18.2.8 three-fit inch table was not available.

## Errors

A hole that cannot be made is a rebuild error on the feature, not a panic. Later features
still rebuild. The errors are:

- a sketch with no free points;
- no body to cut;
- a diameter, depth or pitch that is not positive;
- an angle outside 0–180°;
- a counterbore no wider than the hole, or as deep as the hole;
- a countersink no wider than the bore it opens, or deeper than the counterbore or hole;
- a thread no wider than the tap drill, or deeper than the hole.

## Unknown or not covered

1. How countersink head clearance changes the section ("increased countersink" against
   "add to counterbore", S17). Only the names are published.
2. Where under-head and far-side countersinks sit and how their diameters are measured.
3. How "depth to shoulder" and "depth to tip" map onto the API's `HoleDepth`.
4. Tapered taps: which end the thread diameter refers to, and whether the thread angle is
   a half or an included angle. The S17 descriptions of the two tap types also read as
   swapped.
5. Holes on curved faces and 3D placement sketches (S1, S3).
6. How slots shape their counterbore and countersink levels at the ends (S4).
7. SOLIDWORKS's default sizes, which live only in its installed database.
8. The K1 values have not been checked against the standard texts.
