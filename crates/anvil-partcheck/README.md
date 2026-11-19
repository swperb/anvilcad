# anvil-partcheck

Part Check reads a STEP file and reports what it takes to machine the part. The report
includes the recognised features, design-for-manufacturing (DFM) flags, and an estimated
machine time for each operation type. It writes the report as a versioned JSON document.
This document is the only contract between AnvilCAD and Kerfa, the shop-side app: Kerfa
saves the result against a quote line.

```bash
cargo run -p anvil-partcheck -- part.step --material 6061 --quantity 10
cargo run -p anvil-partcheck -- --schema    # prints the JSON Schema
```

## The contract

- **Schema:** [`schema/part-check.v1.schema.json`](schema/part-check.v1.schema.json) is
  generated from the Rust types in `src/lib.rs`. A test fails if the two don't match.
  Consumers pin a copy of this file.
- **Golden example:** [`fixtures/plate-pocket-hole.v1.json`](fixtures/plate-pocket-hole.v1.json)
  is the result for [`fixtures/plate-pocket-hole.step`](fixtures/plate-pocket-hole.step).
  That part is an 80 × 60 × 20 synthetic plate with a 40 × 30 × 8 pocket and a 6 mm
  through hole. It was built by AnvilCAD from the test in `tests/contract.rs`, and no real
  part was used. Consumers can build their stubs and tests on this pair.
- **Versioning:** every result carries `schema: "anvilcad.part-check"` and a semantic
  `schema_version`.
  - A minor version only adds optional fields, so consumers must ignore fields they don't
    know.
  - Renaming or removing a field, or changing what it means, needs a new major version
    and a new schema file (`v2`).
- **Units:** lengths are in mm, areas in mm², volume in mm³, mass in g and time in
  minutes. Values are rounded to 0.001.

## How to use the result

- **Machine time is a suggestion.** `machine_time.status` stays `"unvalidated"` until a
  benchmark on synthetic parts shows the estimate is close enough to quote from.
  - Until then, show the estimate to the estimator, and never price from it
    automatically.
  - The estimate assumes a stock tool library and one setup from +Z.
  - `machine_time.assumptions` lists what it leaves out.
- **DFM flags are information.** Show each flag, with its severity, to the estimator.
  Never change a price because of a flag.
- `machine_time` is `null` when no estimate could be made, for example when the material
  is unknown. `warnings` explains why.

## Privacy

Part Check runs locally on the bytes it is given. It makes no network calls, writes
nothing to disk and logs nothing about the geometry. The result holds a hash of the file
and numbers derived from it, never the geometry itself. Customer geometry must never be
used for training, and must not leave the shop's deployment unless the shop opts in.

## Regenerating

After a change you intended, run `ANVIL_BLESS=1 cargo test -p anvil-partcheck` and review
the diff. Any change to the schema file is a contract change for Kerfa.
