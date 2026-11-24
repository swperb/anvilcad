// Runs the WebAssembly kernel probe in Node, the same engine family a browser uses.
// Usage: node run.mjs <probe.wasm> <file.anvprt | file.step> ...
// Prints one line per file: the file name and what the kernel reported.
import { readFileSync } from "node:fs";
import { basename } from "node:path";

const [wasmPath, ...files] = process.argv.slice(2);
const { instance } = await WebAssembly.instantiate(readFileSync(wasmPath), {});
const k = instance.exports;
let failed = 0;
for (const file of files) {
  const bytes = readFileSync(file);
  const ptr = k.alloc(bytes.length);
  new Uint8Array(k.memory.buffer, ptr, bytes.length).set(bytes);
  const isStep = /\.(step|stp)$/i.test(file);
  const t0 = performance.now();
  const rc = isStep ? k.import_step(ptr, bytes.length) : k.rebuild_part(ptr, bytes.length);
  const ms = performance.now() - t0;
  const out = new TextDecoder().decode(new Uint8Array(k.memory.buffer, k.out_ptr(), k.out_len()));
  if (rc !== 0) failed++;
  console.log(`${basename(file)}: ${rc === 0 ? "" : "ERROR "}${out} ms=${ms.toFixed(1)}`);
}
process.exit(failed ? 1 : 0);
