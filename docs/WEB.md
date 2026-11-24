# AnvilCAD in the browser

What it takes to run AnvilCAD from a web page, so people can try it without installing
anything ([#101](https://github.com/swperb/anvilcad/issues/101), ROADMAP CAD priority 8).
This is the result of a spike: it records what was measured on 2026-11-24 and what is still
an estimate. Measured facts and estimates are kept in separate sections.

**Short answer.** The geometry is ready. Every kernel crate builds for
`wasm32-unknown-unknown` unchanged, and a small WebAssembly build rebuilds the saved test
parts and imports the NIST STEP files with the same volumes and areas as native, at
similar speed. The work left is in `anvil-app`: Lua, file dialogs, the clock, autosave and
moving rebuilds off the page's main thread, estimated at four to six weeks for one
person to a first public demo.

## Keeping it buildable

```bash
scripts/wasm-check.sh
```

type-checks the kernel crates for `wasm32-unknown-unknown` (install the target once with
`rustup target add wasm32-unknown-unknown`).

```bash
scripts/wasm-check.sh --probe
```

also builds [`scripts/wasm-probe`](../scripts/wasm-probe), prints its size, runs it in
Node on every `.anvprt` fixture in `crates/anvil-model/tests/fixtures/v0.1.0` and every
STEP file in `crates/anvil-step/tests/data`, runs the same code natively, and fails if the
results differ. It needs Node.js and gzip (brotli is optional). The script is run by hand;
there is no CI job for it.

The probe is a throwaway: a `cdylib` outside the workspace with a raw C-style interface
(`alloc`, `rebuild_part`, `import_step`, `out_ptr`, `out_len`) so that it needs no
`wasm-bindgen` tooling. It reads a part or STEP file from bytes the host copies in and
returns a one-line report. It is not shipped.

## Measured

All measurements: Apple M5 Pro, macOS, Rust stable, Node v26, 2026-11-24. The machine was
shared with other builds, so single timings vary by tens of percent between runs; treat
them as orders of magnitude, not benchmarks.

### What builds for wasm32-unknown-unknown

| Crate | `cargo build --release --target wasm32-unknown-unknown` | Notes |
| --- | --- | --- |
| `anvil-kernel` | builds | |
| `anvil-sketch` | builds | |
| `anvil-brep` | builds | |
| `anvil-step` | builds | |
| `anvil-model` | builds | `Document::save`/`load` use `std::fs` and fail at run time; `to_json`/`from_json` work |
| `anvil-io` | builds (`cargo check`) | |
| `anvil-props` | builds (`cargo check`) | |
| `anvil-3dmouse` | builds (`cargo check`) | no device access in a browser |
| `anvil-app` | **fails** | see below |

No code changes were needed for any of the crates that build.

Library code in the kernel crates uses no threads, clock or environment. The only
`std::fs` calls are `Document::save`/`load` and `Assembly::save`/`load`, which a browser
build would not call. The one `std::env::var` is a debug switch in `anvil-step`'s mesher,
which returns an error (and so stays off) on wasm32.

### anvil-app

`cargo check -p anvil-app --target wasm32-unknown-unknown` stops in the build script of
`mlua-sys`, which compiles the bundled Lua and does not know how to for
`wasm32-unknown-unknown`.

With `mlua` removed from `crates/anvil-app/Cargo.toml` for the experiment (then put back),
every other dependency compiled for wasm32, including `eframe` 0.31.1, `wgpu` 24.0.5 and
`rfd` 0.15. The library then failed with 18 errors, all name resolution:

- 11 in `script.rs`: the missing `mlua`.
- 7 in `app.rs`, `app/assembly.rs` and `app/scripting.rs`: `rfd::FileDialog`, the blocking
  file dialog, does not exist on wasm32. Only `rfd::AsyncFileDialog` does.

Type errors show up only after names resolve, so more errors may follow these. Found by
reading the code, the run-time blockers are:

- **Threads.** `recovery.rs` runs autosave on a `std::thread`, which panics on
  `wasm32-unknown-unknown`. Nothing else spawns threads.
- **The clock.** `std::time::Instant::now()` panics on `wasm32-unknown-unknown`. It is
  called in autosave (`app/autosave.rs`) and in script time limits (`script.rs`).
- **The file system.** About 65 `std::fs` calls in `anvil-app`, for open, save, recent
  files, recovery, `init.lua` and `require`.
- **The window.** `main.rs` uses `eframe::run_native`; the web needs `eframe::WebRunner`
  and an HTML canvas.
- **Rebuilds run on the UI thread.** `App::rebuild` calls `Document::rebuild` directly.
  Natively a slow part stalls the window; in a browser it freezes the tab, and the
  browser may offer to kill it.

### Results match native

`scripts/wasm-check.sh --probe` on the 7 part fixtures and 9 STEP files: every rebuilt
part has the same feature count, error count, volume, surface area (to 6 decimal places)
and exact-STEP size in WebAssembly as natively. STEP imports find the same bodies, and the
same bodies are exact.

One measured difference: `nist_ftc_08_asme1_rc.stp` meshes to 69,642 triangles in
WebAssembly and 69,640 natively. wasm32 has no system maths library, so Rust uses its own
implementation of `sin`, `cos` and the like, whose last bits can differ; a tessellation
decision near a tolerance went the other way. The script compares triangle counts loosely
for this reason. Rebuilt parts agreed exactly.

### Speed

Single runs, milliseconds. "Native" is the probe built for `aarch64-apple-darwin` with
the same settings as the opt-level 3 WebAssembly build.

| File | wasm `z` | wasm `s` | wasm `3` | native `3` |
| --- | ---: | ---: | ---: | ---: |
| `prismatic.anvprt` (15 features) | 452 | 218 | 148 | 142 |
| `swept.anvprt` (13 features) | 54 | 26 | 17 | 18 |
| `turned.anvprt` (8 features) | 22,989 | 16,267 | 12,342 | 16,580 |
| `nist_ctc_05_asme1_rd.stp` (120k triangles) | 889 | 545 | 470 | 358 |

At opt-level 3, WebAssembly in V8 runs within about 1.3 times native on these files.
`turned.anvprt` is slow natively too (12 to 33 s depending on build and load); that is a
kernel matter, not a WebAssembly one, but it shows why rebuilds must leave the main thread.

### Bundle size

The probe links all of `anvil-model` and `anvil-step` (and through them `anvil-brep`,
`anvil-sketch`, `anvil-kernel`, `serde_json` and `spade`) with `lto = true`,
`codegen-units = 1`, `panic = "abort"`, `strip = true`. Sizes in bytes; `wasm-opt` was not
installed and was not run.

| opt-level | raw | gzip -9 | brotli -q 11 |
| --- | ---: | ---: | ---: |
| `z` | 1,976,608 | 638,925 | 477,735 |
| `s` | 2,239,379 | 719,809 | 535,955 |
| `3` | 2,563,931 | 902,790 | 647,133 |

The probe uses opt-level 3: 170 KB more over the wire than `z` buys about three times the
speed on `prismatic.anvprt`.

## Estimates

Nothing in this section was built or measured.

### The whole app

`eframe` with `wgpu` on WebGPU, plus egui, accesskit and the fonts, typically adds 2 to 4
MB raw (about 1 to 1.5 MB brotli) to a release build. A first web build of AnvilCAD would
therefore be roughly **4 to 7 MB raw, 1.5 to 2.5 MB brotli**, before `wasm-opt`, which
usually takes off another 10 to 20 percent. That is a few seconds' download on a
home connection, once, and then cached.

### Rendering

`eframe` 0.31 runs on the web through `WebRunner`. With `wgpu` 24 it can use WebGPU
(current Chrome, Edge and Safari; Firefox on some platforms) and fall back to WebGL2
(every current browser) when the `webgl` feature of `wgpu` is enabled. AnvilCAD's viewport
draws with `wgpu` directly through an egui paint callback. Its shaders and features need
checking against WebGL2's limits (no storage buffers, no compute, fewer bind groups);
anything that does not fit needs a WebGL2 path or a "needs WebGPU" message. Headless
screenshots (`headless.rs`, which uses `pollster` to block on the GPU) stay native-only.

### Opening and saving files, with no server

Everything stays on the user's machine; the page is static files.

- **Open:** an `<input type="file">` picker, or drag and drop onto the canvas, through
  `rfd::AsyncFileDialog` (which uses the picker on the web) and egui's dropped-files API.
  The bytes go to `Document::from_json` for `.anvprt` and to `anvil_step::import` (or
  `anvil_model::step::document_from_step`) for STEP. Both already take text, not paths;
  the probe uses exactly these calls.
- **Save:** `Document::to_json` or the STEP writer produces text; the page offers it as a
  download (a `Blob` and an `<a download>` link, which `rfd`'s async save dialog does on
  the web). Where the File System Access API exists (Chrome, Edge), Save can write back to
  the same file after the first save, like a desktop app; elsewhere every save is a
  download.
- **Recovery and autosave:** browser storage (IndexedDB, or the Origin Private File
  System) in place of the recovery folder, written from the main thread or a worker
  instead of `std::thread`. Losing work on a closed tab is the failure to design against.
- **Recent files:** names only, in browser storage; a browser cannot reopen a path.
- **Assemblies:** they refer to part files by path, which a browser cannot follow. The
  first web build would open parts only, or ask for a folder (File System Access API, or
  a multi-file upload) so references resolve inside it.

### Steps and effort

For one engineer who knows the codebase. Each step is meant to land on its own.

| # | Step | Estimate |
| --- | --- | --- |
| 1 | Make Lua optional in `anvil-app` (a default-on `scripting` feature; menus that need it hidden when off, not dead). Lua on the web later via a wasm Lua build or an emscripten target. | 3 to 5 days |
| 2 | Replace `std::time::Instant` with a crate that works on both (for example `web-time`), and gate autosave's thread and the recovery folder behind `cfg(not(target_arch = "wasm32"))` with a browser-storage version. | 3 to 5 days |
| 3 | Move file open and save onto `rfd::AsyncFileDialog` (native too, so there is one code path), plus drag and drop and downloads. | 4 to 6 days |
| 4 | Web entry point with `eframe::WebRunner`, an `index.html`, `trunk` or `wasm-bindgen` to build it, and a hosted static page. | 3 to 5 days |
| 5 | Check the viewport on WebGPU and WebGL2; fix or gate what WebGL2 cannot do. | 3 to 8 days |
| 6 | Run rebuilds in a Web Worker so the tab never freezes, with progress and cancel. Natively this should move to a thread too. | 1 to 2 weeks |
| 7 | Size pass: `wasm-opt`, dropping unused fonts and features, measuring again. | 2 days |

Total: about **4 to 6 weeks** to a public demo that opens, edits, rebuilds and saves parts
and STEP files. Steps 1 to 3 also tidy the native app, and step 6 fixes a native stall.

### Things the web build should not attempt

- The SOLIDWORKS paths: `anvil-migrate` and the Document Manager plug-in drive Windows
  software and stay native. Reading `.sldprt` natively in AnvilCAD's own reader works in
  the browser like any other file format.
- `anvilcad-agent`, the MCP server and `anvil-bulk`: these are native tools.
- The 3D mouse: browsers expose no HID access by default (WebHID exists in Chrome only,
  behind a permission prompt).

## Open questions

- Whether Lua should run on the web at all in the first version, or whether scripting is
  a desktop feature until a Lua build for `wasm32-unknown-unknown` is chosen.
- Which browsers to support: WebGPU only is simpler and faster; WebGL2 reaches more people.
- Where to host the page (GitHub Pages fits a static build and costs nothing).
