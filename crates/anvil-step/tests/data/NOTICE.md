# Test data

`nist_ctc_03_asme1_rc.stp`, `nist_ctc_05_asme1_rd.stp`, `nist_ftc_06_asme1_rd.stp`,
`nist_ftc_07_asme1_rd.stp`, `nist_ftc_08_asme1_rc.stp`, `nist_ftc_09_asme1_rd.stp` and
`nist_ftc_11_asme1_rb.stp` are from the NIST MBE PMI
Validation and Conformance Testing Project, "NIST CAD Models and STEP Files with PMI"
(Robert Lipman, National Institute of Standards and Technology, 2015), downloaded from
https://www.nist.gov/el/systems-integration-division-73400/mbe-pmi-validation-and-conformance-testing/download-free-cad

NIST states that these test cases, CAD models and STEP files can be used without any
restrictions. Their use here does not imply endorsement by NIST.

`two_level_assembly.stp` and `mapped_item_assembly.stp` are written for these tests. The
block they share (#1 to #170) is AnvilCAD's own export of a 10 x 20 x 5 mm box; the
assembly structure after it was written by hand, with comments saying where each
occurrence should land.
