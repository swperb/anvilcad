// Proveout front end. Plain JavaScript, no build step. Every piece of user or server
// text is put on the page with textContent, never as HTML.
"use strict";

const $ = (id) => document.getElementById(id);
// replaceChildren without the null placeholders used for optional parts.
const fill = (node, ...kids) => node.replaceChildren(...kids.flat().filter((k) => k !== null && k !== undefined && k !== false));
const state = { meta: null, me: null, machines: [], post: null, drawing: null, result: null, resultSaved: false };

function el(tag, attrs, ...children) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v === undefined || v === null || v === false) continue;
    if (k === "class") e.className = v;
    else if (k.startsWith("on")) e.addEventListener(k.slice(2), v);
    else e.setAttribute(k, v === true ? "" : v);
  }
  for (const c of children.flat()) {
    if (c === null || c === undefined || c === false) continue;
    e.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
  return e;
}

async function api(method, path, body) {
  const opts = { method, credentials: "same-origin", headers: {} };
  if (body !== undefined) {
    opts.headers["content-type"] = "application/json";
    opts.body = JSON.stringify(body);
  }
  const r = await fetch(path, opts);
  let data = null;
  try { data = await r.json(); } catch (_) { /* not JSON */ }
  if (!r.ok) throw new Error((data && data.error) || `request failed (${r.status})`);
  return data;
}

function setStatus(id, text, isError) {
  const s = $(id);
  s.textContent = text || "";
  s.classList.toggle("err", !!isError);
}

function table(headers, rows) {
  return el("div", { class: "table-wrap" },
    el("table", {}, el("thead", {}, el("tr", {}, headers.map((h) => el("th", {}, h)))),
      el("tbody", {}, rows.map((r) => el("tr", {}, r.map((c) => el("td", {}, c)))))));
}

const lines = (ls) => (ls && ls.length ? ls.join(", ") : "-");
const src = (s) => el("span", { class: "src" }, s);

// ----- Tabs -----
function showTab(name) {
  document.querySelectorAll(".tabs button").forEach((b) => b.setAttribute("aria-selected", String(b.dataset.tab === name)));
  document.querySelectorAll(".tab").forEach((t) => { t.hidden = t.id !== `tab-${name}`; });
  if (name === "library") loadLibrary();
}
document.querySelectorAll(".tabs button").forEach((b) => b.addEventListener("click", () => showTab(b.dataset.tab)));

// ----- Account -----
function renderAccount() {
  const a = $("account");
  a.replaceChildren();
  const me = state.me;
  if (me && me.user) {
    const left = Math.max(0, me.plan.credits_per_month - me.credits_used);
    a.append(el("span", {}, me.user.email), src(`${me.plan.name}: ${left} of ${me.plan.credits_per_month} credits left`),
      el("button", { type: "button", onclick: logout }, "Sign out"));
  } else {
    a.append(el("button", { type: "button", onclick: () => openAuth(false) }, "Sign in"),
      el("button", { type: "button", class: "primary", onclick: () => openAuth(true) }, "Create account"));
  }
  renderLevels();
  if (state.meta) renderPlans();
}

let authSignup = false;
function openAuth(signup) {
  authSignup = signup;
  $("auth-title").textContent = signup ? "Create an account" : "Sign in";
  $("auth-submit").textContent = signup ? "Create account" : "Sign in";
  $("auth-switch").textContent = signup ? "I have an account" : "Create an account instead";
  $("auth-password").autocomplete = signup ? "new-password" : "current-password";
  setStatus("auth-status", signup ? "Free accounts get 3 Standard AI reviews a month." : "");
  $("auth").showModal();
}
$("auth-switch").addEventListener("click", () => openAuth(!authSignup));
$("auth-cancel").addEventListener("click", () => $("auth").close());
$("auth-form").addEventListener("submit", async (ev) => {
  ev.preventDefault();
  try {
    await api("POST", authSignup ? "/api/signup" : "/api/login",
      { email: $("auth-email").value, password: $("auth-password").value });
    $("auth").close();
    $("auth-password").value = "";
    await refreshMe();
  } catch (e) { setStatus("auth-status", e.message, true); }
});

async function logout() {
  await api("POST", "/api/logout", {}).catch(() => {});
  await refreshMe();
}

async function refreshMe() {
  state.me = await api("GET", "/api/me").catch(() => ({ user: null }));
  state.machines = [];
  if (state.me.user) {
    const m = await api("GET", "/api/machines").catch(() => ({ items: [] }));
    state.machines = m.items;
  }
  renderAccount();
  renderMachineSelect();
}

// ----- Machine profile -----
const FIELDS = [
  ["name", "Name", "text"],
  ["controller", "Control", ["fanuc", "haas", "mazak", "okuma", "linux_cnc", "grbl", "siemens", "heidenhain", "other"]],
  ["kind", "Type", ["vertical_mill", "horizontal_mill", "router", "other"]],
  ["travel.0", "X travel (mm)", "number"],
  ["travel.1", "Y travel (mm)", "number"],
  ["travel.2", "Z travel (mm)", "number"],
  ["max_rpm", "Max spindle (rpm)", "number"],
  ["min_rpm", "Min spindle (rpm)", "number"],
  ["max_feed", "Max cutting feed (mm/min)", "number"],
  ["rapid_rate", "Rapid rate (mm/min)", "number"],
  ["tool_capacity", "Tool pockets (0 = manual)", "number"],
  ["max_tool", "Highest tool / offset number", "number"],
  ["default_units", "Units if the program does not say", ["mm", "inch"]],
  ["notes", "Notes (taper, probe, coolant)", "text"],
];

function buildMachineForm() {
  const f = $("machine-form");
  for (const [key, label, type] of FIELDS) {
    const id = `mf-${key.replace(".", "-")}`;
    const input = Array.isArray(type)
      ? el("select", { id }, type.map((v) => el("option", { value: v }, v.replace("_", " "))))
      : el("input", { id, type, step: type === "number" ? "any" : undefined });
    f.append(el("label", {}, label, input));
  }
}

function fillMachineForm(p) {
  for (const [key] of FIELDS) {
    const id = `mf-${key.replace(".", "-")}`;
    const [k, i] = key.split(".");
    $(id).value = i === undefined ? (p[k] ?? "") : p[k][Number(i)];
  }
}

function readMachineForm() {
  const p = { travel: [0, 0, 0] };
  for (const [key, , type] of FIELDS) {
    const v = $(`mf-${key.replace(".", "-")}`).value;
    const [k, i] = key.split(".");
    const val = type === "number" ? Number(v) : v;
    if (i === undefined) p[k] = val; else p[k][Number(i)] = val;
  }
  return p;
}

function renderMachineSelect() {
  const s = $("machine-select");
  const prev = s.value;
  fill(s,
    el("optgroup", { label: "Starting points - replace the numbers with your machine's" },
      state.meta.presets.map((p) => el("option", { value: `preset:${p.id}` }, p.profile.name))),
    state.machines.length
      ? el("optgroup", { label: "My machines" }, state.machines.map((m) => el("option", { value: `saved:${m.id}` }, m.name)))
      : null);
  if ([...s.options].some((o) => o.value === prev)) s.value = prev;
  loadSelectedMachine();
}

async function loadSelectedMachine() {
  const v = $("machine-select").value;
  if (v.startsWith("preset:")) {
    const p = state.meta.presets.find((x) => `preset:${x.id}` === v);
    if (p) fillMachineForm(p.profile);
  } else if (v.startsWith("saved:")) {
    try {
      const r = await api("GET", `/api/machines/${v.slice(6)}`);
      fillMachineForm(r.data.profile);
    } catch (e) { setStatus("run-status", e.message, true); }
  }
}
$("machine-select").addEventListener("change", loadSelectedMachine);

$("machine-save").addEventListener("click", async () => {
  if (!state.me || !state.me.user) return openAuth(false);
  const profile = readMachineForm();
  try {
    await api("POST", "/api/machines", { name: profile.name, data: { profile } });
    await refreshMe();
    setStatus("run-status", "Machine saved.");
  } catch (e) { setStatus("run-status", e.message, true); }
});

// ----- Files -----
function readText(file, max) {
  return new Promise((res, rej) => {
    if (file.size > max) return rej(new Error(`${file.name} is larger than ${Math.round(max / 1024)} KB`));
    const r = new FileReader();
    r.onload = () => res(String(r.result));
    r.onerror = () => rej(r.error);
    r.readAsText(file);
  });
}

$("gcode-file").addEventListener("change", async (ev) => {
  const f = ev.target.files[0];
  if (!f) return;
  try {
    $("gcode").value = await readText(f, 64 * 1024 * 1024);
    if (!$("title").value) $("title").value = f.name;
    updateAnon();
  } catch (e) { setStatus("run-status", e.message, true); }
});

$("drawing-file").addEventListener("change", (ev) => {
  const f = ev.target.files[0];
  state.drawing = null;
  if (!f) return;
  if (!state.meta.drawing_types.includes(f.type)) return setStatus("run-status", "The drawing must be an image or a PDF.", true);
  if (f.size > 20 * 1024 * 1024) return setStatus("run-status", "The drawing is larger than 20 MB.", true);
  const r = new FileReader();
  r.onload = () => {
    const s = String(r.result);
    state.drawing = { name: f.name, media_type: f.type, data_base64: s.slice(s.indexOf(",") + 1) };
    setStatus("run-status", `Drawing ${f.name} ready.`);
  };
  r.readAsDataURL(f);
});

$("post-file").addEventListener("change", async (ev) => {
  const f = ev.target.files[0];
  state.post = null;
  if (!f) return;
  try {
    const text = await readText(f, 512 * 1024);
    state.post = { file_name: f.name, text };
    const isToml = f.name.toLowerCase().endsWith(".toml");
    $("post-label").textContent = isToml
      ? `${f.name}: applied as AnvilCAD rules if it loads.`
      : `${f.name}: read as text for the AI review only; it is never run. Profile it to scan it for risky codes.`;
  } catch (e) { setStatus("run-status", e.message, true); }
});

// ----- Anonymizing -----
function anonOptions() {
  return {
    stripComments: $("anon-comments").checked,
    keepToolComments: $("anon-keep-tools").checked,
    programNumber: $("anon-onum").checked,
    words: $("anon-words").value.split(",").map((w) => w.trim()).filter(Boolean),
  };
}
function anonymized() {
  const o = anonOptions();
  const prog = self.ProveoutAnon.anonymizeProgram($("gcode").value, o);
  const red = (t) => self.ProveoutAnon.redactWords(t, o.words).text;
  const any = o.stripComments || o.programNumber || o.words.length > 0;
  return { gcode: prog.text, changes: prog.changes, title: red($("title").value),
    drawingNotes: red($("drawing-notes").value), jobNotes: red($("job-notes").value), any };
}
function updateAnon() {
  const a = anonymized();
  $("anon-result").textContent = a.any
    ? `Will remove ${a.changes.comments} comment(s), replace ${a.changes.programNumber} program number(s) and redact ${a.changes.words} word(s) in the program. Drawing files are sent as they are: crop or mask title blocks first.`
    : "Nothing is anonymized.";
}
["anon-comments", "anon-keep-tools", "anon-onum", "anon-words", "gcode"].forEach((id) => $(id).addEventListener("input", updateAnon));

// ----- Levels and disclosures -----
function selectedLevel() {
  const r = document.querySelector('input[name="level"]:checked');
  return r ? r.value : "check";
}

function renderLevels() {
  if (!state.meta) return;
  const box = $("levels");
  const prev = selectedLevel();
  const me = state.me && state.me.user ? state.me : null;
  const opts = [{ id: "check", label: "Deterministic check only", description: "Free and unlimited. Nothing leaves our server.", credits: 0 }]
    .concat(state.meta.levels);
  box.replaceChildren(...opts.map((l) => {
    let why = "";
    if (l.id !== "check") {
      if (!state.meta.ai_available) why = "AI review is not configured on this server.";
      else if (!me) why = "Sign in to use AI reviews.";
      else if (!me.plan.levels.includes(l.id)) why = "Needs a plan that includes it.";
      else if (me.plan.credits_per_month - me.credits_used < l.credits) why = "Not enough credits left this month.";
    }
    const input = el("input", { type: "radio", name: "level", value: l.id, disabled: !!why, checked: l.id === prev && !why });
    input.addEventListener("change", renderDisclosure);
    return el("label", { class: `level${why ? " disabled" : ""}` }, input,
      el("span", {}, el("b", {}, l.label), l.credits ? ` (${l.credits} credit${l.credits > 1 ? "s" : ""})` : "",
        el("br"), el("span", { class: "hint" }, why || l.description,
          l.max_program_kb ? ` Programs up to ${l.max_program_kb} KB.` : "")));
  }));
  if (!document.querySelector('input[name="level"]:checked')) box.querySelector('input[value="check"]').checked = true;
  renderDisclosure();
}

function renderDisclosure() {
  const ai = selectedLevel() !== "check";
  $("disclosure").hidden = !ai;
  $("run").textContent = ai ? "Send for AI review" : "Run deterministic check";
}

// ----- Running a check -----
function payload() {
  const a = anonymized();
  const p = {
    title: a.title,
    gcode: a.gcode,
    machine: { profile: readMachineForm() },
    post: state.post || undefined,
  };
  return { p, a };
}

$("run").addEventListener("click", async () => {
  const level = selectedLevel();
  const { p, a } = payload();
  if (!p.gcode.trim()) return setStatus("run-status", "Paste or load a program first.", true);
  const btn = $("run");
  btn.disabled = true;
  try {
    let r;
    if (level === "check") {
      setStatus("run-status", "Checking...");
      r = await api("POST", "/api/check", p);
    } else {
      if (!$("accept-privacy").checked || !$("accept-guarantee").checked) {
        throw new Error("Tick both boxes in step 4 to send for AI review.");
      }
      setStatus("run-status", "Reviewing. This can take a few minutes for long programs...");
      r = await api("POST", "/api/review", Object.assign(p, {
        level,
        anonymized: a.any,
        job_notes: a.jobNotes,
        drawing: { notes: a.drawingNotes, file: state.drawing || undefined },
        accept: { privacy: true, no_guarantee: true, version: state.meta.disclosure_version },
      }));
      refreshMe();
    }
    setStatus("run-status", "Done. Check the results below yourself.");
    showResult(r, `/api/pending/${r.pending_id}/download`);
  } catch (e) {
    setStatus("run-status", e.message, true);
  } finally {
    btn.disabled = false;
  }
});

function showResult(r, downloadBase) {
  state.result = r;
  state.resultSaved = false;
  const rep = r.report;
  $("results").hidden = false;
  const sc = rep.score;
  const part = (n, v) => `${n} ${v === null || v === undefined ? "n/a" : v + "%"}`;
  $("score").className = `score ${sc.band}`;
  fill($("score"), 
    el("div", { class: "pct" }, `${sc.percent}%`),
    el("div", {}, el("b", {}, { consistent: "Checks agree", review: "Needs review", problems: "Problems found" }[sc.band]),
      el("div", { class: "parts" }, [part("rules", sc.rules), part("drawing", sc.drawing), part("AI", sc.ai)].join(" · ")),
      el("div", { class: "parts" }, sc.caps.join("; ")),
      el("div", { class: "parts" }, "Agreement score, not a probability that the program is right.")));

  const ai = rep.ai;
  fill($("summary"), ...(ai ? [
    el("p", {}, ai.summary),
    el("p", { class: "hint" }, `AI confidence ${Math.round(ai.confidence)}%: ${ai.confidence_reason} (${ai.model}${ai.fell_back ? ", fallback model" : ""})`),
  ] : [el("p", { class: "hint" }, `Deterministic check only. Rules: ${rep.deterministic.rules}.`)]));

  const fs = rep.deterministic.findings.concat(ai ? ai.findings : []);
  const order = { error: 0, review: 1, warning: 2, info: 3 };
  fs.sort((x, y) => order[x.level] - order[y.level] || (x.lines[0] || 0) - (y.lines[0] || 0));
  fill($("findings"), fs.length ? table(["Level", "From", "Category", "Lines", "Finding", "Suggestion"],
    fs.map((f) => [el("span", { class: `lvl-${f.level}` }, f.level), src(f.source === "ai" ? "AI" : "check"),
      f.category, lines(f.lines), f.message, f.suggestion || ""])) : el("p", {}, "No findings."));

  fill($("drawing-checks"), ...(ai && ai.drawing_checks.length ? [el("h3", {}, "Drawing comparison (AI)"),
    table(["Feature", "Drawing", "Program", "Status", "Lines", "Note"], ai.drawing_checks.map((d) => [d.feature,
      d.drawing_value, d.program_value, el("span", { class: d.status === "match" ? "state-pass" : d.status === "unclear" ? "state-warn" : "state-fail" },
        d.status.replace(/_/g, " ")), lines(d.lines), d.note]))] : []));

  const st = rep.setup;
  fill($("setup"), 
    table(["", ""], [["Program", st.program], ["Part", st.part], ["Material", st.material], ["Stock", st.stock],
      ["Work holding", st.workholding], ["Program zero", st.program_zero], ["Units", st.units],
      ["Extents", st.extents], ["Cycle time", st.cycle_time]]),
    el("h3", {}, "Tools"),
    table(["T", "Description", "H", "D", "RPM", "Feed", "Min Z", "Line", "Holder / stick-out", "Notes"],
      st.tools.map((t) => [`T${t.tool}`, el("span", {}, t.description, " ", src(t.description_source)), t.length_offsets,
        t.diameter_offsets, t.speeds, t.feeds, t.min_z, t.first_line || "-", t.holder_and_stickout, t.notes])),
    el("h3", {}, "Offsets"),
    table(["Offset", "Value / note", "Source"], st.offsets.map((o) => [o.name, o.note, src(o.source)])),
    st.operations.length ? el("h3", {}, "Operations") : null,
    st.operations.length ? el("ol", {}, st.operations.map((o) => el("li", {}, o.text, " ", src(o.source)))) : null,
    el("h3", {}, "Verification checklist"),
    el("ul", {}, st.checklist.map((c) => el("li", {}, c.text, " ", src(c.source)))));

  fill($("additions"), ...(st.additions.length ? [el("h3", {}, "Suggested additions and changes (AI)"),
    el("ul", {}, st.additions.map((x) => el("li", {}, x.text)))] : []),
  ...(ai && ai.questions.length ? [el("h3", {}, "Open questions (AI)"), el("ul", {}, ai.questions.map((q) => el("li", {}, q)))] : []));

  $("open-sheet").href = `${downloadBase}?format=html&inline=1`;
  $("open-sheet").removeAttribute("download");
  for (const [id, f] of [["dl-html", "html"], ["dl-md", "md"], ["dl-json", "json"]]) {
    $(id).href = `${downloadBase}?format=${f}`;
    $(id).setAttribute("download", "");
  }
  const pending = !!r.pending_id;
  $("save-report").disabled = !pending;
  $("save-project").disabled = !pending;
  setStatus("save-status", pending ? "Unsaved reports are discarded after an hour." : "Saved report.");
  $("results").scrollIntoView({ behavior: "smooth" });
}

$("save-report").addEventListener("click", async () => {
  if (!state.me || !state.me.user) return openAuth(false);
  try {
    await api("POST", "/api/reports", { pending_id: state.result.pending_id });
    state.resultSaved = true;
    setStatus("save-status", "Report saved to your account.");
    refreshMe();
  } catch (e) { setStatus("save-status", e.message, true); }
});

$("save-project").addEventListener("click", async () => {
  if (!state.me || !state.me.user) return openAuth(false);
  const name = $("project-name").value.trim() || $("title").value.trim() || "Untitled project";
  const a = anonymized();
  const data = { title: a.title, gcode: a.gcode, drawing_notes: a.drawingNotes, job_notes: a.jobNotes,
    machine: readMachineForm(), post: state.post, drawing: state.drawing };
  try {
    const p = await api("POST", "/api/projects", { name, data });
    if (!state.resultSaved) {
      await api("POST", "/api/reports", { pending_id: state.result.pending_id, project_id: p.id });
      state.resultSaved = true;
    }
    setStatus("save-status", `Project "${name}" saved with its report.`);
    refreshMe();
  } catch (e) { setStatus("save-status", e.message, true); }
});

// ----- Profile -----
$("run-profile").addEventListener("click", async () => {
  setStatus("profile-status", "Profiling...");
  try {
    const r = await api("POST", "/api/profile", { machine: { profile: readMachineForm() }, post: state.post || undefined,
      known_good: $("known-good").value });
    const p = r.profile;
    const out = $("profile-out");
    fill(out, 
      el("h3", {}, `Profile of ${p.subject}`),
      table(["Check", "Result", "Detail"], p.checks.map((c) => [c.name, el("span", { class: `state-${c.state}` }, c.state), c.detail])),
      p.risky_lines.length ? el("h3", {}, "Post lines that write risky codes") : null,
      p.risky_lines.length ? table(["Line", "Code", "Text"], p.risky_lines.map((l) => [l.line, l.token, l.text])) : null,
      p.reference_program ? el("h3", {}, "Reference program (graphics or simulation only)") : null,
      p.reference_program ? el("pre", {}, p.reference_program) : null,
      p.reference_program ? el("button", { type: "button", onclick: () => saveBlob(p.reference_program, "proveout-reference.nc") }, "Download reference program") : null,
      el("h3", {}, "Check it yourself"),
      el("ul", {}, p.next_steps.map((s) => el("li", {}, s))),
      el("p", { class: "hint" }, r.no_guarantee));
    setStatus("profile-status", "Done.");
  } catch (e) { setStatus("profile-status", e.message, true); }
});

function saveBlob(text, name) {
  const url = URL.createObjectURL(new Blob([text], { type: "text/plain" }));
  const a = el("a", { href: url, download: name });
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

// ----- Library -----
async function loadLibrary() {
  const signedIn = state.me && state.me.user;
  $("library-signin").hidden = !!signedIn;
  for (const id of ["lib-reports", "lib-projects", "lib-machines", "storage"]) $(id).replaceChildren();
  if (!signedIn) return;
  const me = state.me;
  fill($("storage"), el("p", { class: "hint" },
    `Storage: ${(me.storage_used / 1048576).toFixed(2)} MB of ${(me.plan.storage_bytes / 1048576).toFixed(0)} MB.`));
  const date = (t) => new Date(t * 1000).toLocaleString();
  try {
    const [rep, proj, mach] = await Promise.all([api("GET", "/api/reports"), api("GET", "/api/projects"), api("GET", "/api/machines")]);
    const del = (kind, id) => async () => {
      if (!confirm("Delete this permanently?")) return;
      await api("DELETE", `/api/${kind}/${id}`).catch((e) => alert(e.message));
      await refreshMe();
      loadLibrary();
    };
    fill($("lib-reports"), rep.items.length ? table(["Title", "Score", "Saved", ""], rep.items.map((i) => [i.name,
      `${i.score}%`, date(i.created), el("span", {},
        el("button", { type: "button", onclick: () => openReport(i.id) }, "Open"), " ",
        el("a", { class: "button", href: `/api/reports/${i.id}/download?format=html`, target: "_blank", rel: "noopener" }, "Sheet"), " ",
        el("button", { type: "button", onclick: del("reports", i.id) }, "Delete"))])) : el("p", { class: "hint" }, "No saved reports."));
    fill($("lib-projects"), proj.items.length ? table(["Name", "Saved", ""], proj.items.map((i) => [i.name, date(i.created),
      el("span", {}, el("button", { type: "button", onclick: () => loadProject(i.id) }, "Load"), " ",
        el("button", { type: "button", onclick: del("projects", i.id) }, "Delete"))])) : el("p", { class: "hint" }, "No saved projects."));
    fill($("lib-machines"), mach.items.length ? table(["Name", "Saved", ""], mach.items.map((i) => [i.name, date(i.created),
      el("button", { type: "button", onclick: del("machines", i.id) }, "Delete")])) : el("p", { class: "hint" }, "No saved machines."));
  } catch (e) { fill($("lib-reports"), el("p", { class: "status err" }, e.message)); }
}

async function openReport(id) {
  try {
    const r = await api("GET", `/api/reports/${id}`);
    showTab("check");
    showResult(r, `/api/reports/${id}/download`);
  } catch (e) { alert(e.message); }
}

async function loadProject(id) {
  try {
    const { data } = await api("GET", `/api/projects/${id}`);
    $("title").value = data.title || "";
    $("gcode").value = data.gcode || "";
    $("drawing-notes").value = data.drawing_notes || "";
    $("job-notes").value = data.job_notes || "";
    if (data.machine) fillMachineForm(data.machine);
    state.post = data.post || null;
    state.drawing = data.drawing || null;
    $("post-label").textContent = state.post ? `${state.post.file_name} (from the project)` : $("post-label").textContent;
    showTab("check");
    updateAnon();
    setStatus("run-status", "Project loaded. Run the check again to get a fresh report.");
  } catch (e) { alert(e.message); }
}

$("delete-account").addEventListener("click", async () => {
  if (!confirm("Delete your account and everything saved in it? This cannot be undone.")) return;
  try {
    await api("DELETE", "/api/me");
    await refreshMe();
    showTab("check");
  } catch (e) { alert(e.message); }
});

// ----- Plans -----
function renderPlans() {
  const me = state.me && state.me.user ? state.me : null;
  fill($("plans"), ...state.meta.plans.map((p) => el("div", { class: "plan" },
    el("b", {}, p.name), el("span", { class: "price" }, p.price_usd ? `$${p.price_usd}/month` : "Free"),
    el("span", {}, p.summary),
    p.price_usd && state.meta.billing_available ? el("button", { type: "button", class: "primary", disabled: me && me.user.plan === p.plan,
      onclick: () => upgrade(p.plan) }, me && me.user.plan === p.plan ? "Your plan" : `Choose ${p.name}`) : null)));
  $("billing-note").textContent = state.meta.billing_available
    ? "Payments are handled by Stripe's hosted checkout; card details never reach this site."
    : "Paid plans are not open yet on this server.";
}

async function upgrade(plan) {
  if (!state.me || !state.me.user) return openAuth(true);
  try {
    const r = await api("POST", "/api/billing/checkout", { plan });
    if (/^https:\/\/checkout\.stripe\.com\//.test(r.url)) location.href = r.url;
    else throw new Error("unexpected checkout address");
  } catch (e) { alert(e.message); }
}

// ----- Start -----
(async function init() {
  try {
    state.meta = await api("GET", "/api/meta");
  } catch (e) {
    document.querySelector("main").prepend(el("p", { class: "status err" }, `Could not load the site: ${e.message}`));
    return;
  }
  $("app-name").textContent = state.meta.app;
  $("foot-app").textContent = state.meta.app;
  $("privacy-text").textContent = state.meta.privacy;
  $("guarantee-text").textContent = state.meta.no_guarantee;
  buildMachineForm();
  await refreshMe();
  renderPlans();
  updateAnon();
  const q = new URLSearchParams(location.search).get("billing");
  if (q === "success") setStatus("run-status", "Thanks. Your plan updates as soon as the payment is confirmed.");
})();
