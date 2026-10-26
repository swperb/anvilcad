// Proveout anonymizer. Runs in the browser before anything is sent, so the server
// never sees the original text. Also loadable in Node for tests.
(function (root) {
  "use strict";

  function escapeRegExp(s) {
    return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  }

  // Replaces each listed word or phrase, ignoring case.
  function redactWords(text, words) {
    let count = 0;
    let out = text;
    for (const w of words) {
      const word = w.trim();
      if (word.length < 2) continue;
      const re = new RegExp(escapeRegExp(word), "gi");
      out = out.replace(re, () => {
        count++;
        return "[REDACTED]";
      });
    }
    return { text: out, count };
  }

  // A comment that only describes a tool, such as "(T1 D=10. - FLAT END MILL)".
  function isToolComment(c) {
    return /^\s*(T\d{1,4}\b|TOOL\s*\d)/i.test(c);
  }

  // options: { stripComments, keepToolComments, programNumber, words: [] }
  function anonymizeProgram(text, options) {
    const o = options || {};
    const changes = { comments: 0, programNumber: 0, words: 0 };
    let lines = text.split(/\r?\n/);
    if (o.stripComments) {
      lines = lines.map((line) => {
        let out = "";
        let depth = 0;
        let current = "";
        for (let i = 0; i < line.length; i++) {
          const ch = line[i];
          if (depth === 0 && ch === ";") {
            const rest = line.slice(i + 1);
            if (o.keepToolComments && isToolComment(rest)) out += ";" + rest;
            else changes.comments++;
            break;
          }
          if (ch === "(" && depth === 0) {
            depth = 1;
            current = "";
            continue;
          }
          if (ch === ")" && depth === 1) {
            depth = 0;
            if (o.keepToolComments && isToolComment(current)) out += "(" + current + ")";
            else changes.comments++;
            continue;
          }
          if (depth === 1) current += ch;
          else out += ch;
        }
        if (depth === 1) changes.comments++;
        return out.replace(/\s+$/, "");
      });
      // Lines that held only a comment stay as empty lines, so line numbers in the
      // report still match the original file.
    }
    if (o.programNumber) {
      lines = lines.map((line) =>
        line.replace(/^(\s*)[O:](\d{1,5})\b/i, (m, sp) => {
          changes.programNumber++;
          return sp + "O0001";
        })
      );
    }
    let out = lines.join("\n");
    if (o.words && o.words.length) {
      const r = redactWords(out, o.words);
      out = r.text;
      changes.words = r.count;
    }
    return { text: out, changes };
  }

  const api = { anonymizeProgram, redactWords, isToolComment };
  if (typeof module !== "undefined" && module.exports) module.exports = api;
  else root.ProveoutAnon = api;
})(typeof self !== "undefined" ? self : this);
