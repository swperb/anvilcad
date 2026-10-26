// Run with: node --test crates/proveout-web/tests/
import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const { anonymizeProgram, redactWords } = require("../static/anonymize.js");

const PROG = [
  "%",
  "O1234 (ACME BRACKET REV C)",
  "(T1 D=10. - FLAT END MILL)",
  "(CUSTOMER: ACME CORP)",
  "G90 G54 ; SETUP BY JO",
  "T1 M6 (ACME TOOL)",
  "M30",
].join("\n");

test("comments go, tool comments can stay, line numbers do not move", () => {
  const r = anonymizeProgram(PROG, { stripComments: true, keepToolComments: true, programNumber: true });
  const lines = r.text.split("\n");
  assert.equal(lines.length, 7);
  assert.equal(lines[1], "O0001");
  assert.equal(lines[2], "(T1 D=10. - FLAT END MILL)");
  assert.equal(lines[3], "");
  assert.equal(lines[4], "G90 G54");
  assert.equal(lines[5], "T1 M6");
  assert.deepEqual(r.changes, { comments: 4, programNumber: 1, words: 0 });
});

test("listed words are redacted anywhere, ignoring case", () => {
  const r = anonymizeProgram(PROG, { words: ["acme", "Jo", " "] });
  assert.equal(r.changes.words, 4);
  assert.ok(!/acme/i.test(r.text));
  assert.ok(r.text.includes("SETUP BY [REDACTED]"));
  assert.equal(redactWords("a.b a+b", ["a.b"]).count, 1);
});
