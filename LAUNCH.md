# Launch playbook

How to launch AnvilCAD publicly, written so that the maintainer and an agent can run it in a
few minutes once the preflight gates pass. Researched 2026-10-20; the numbers in "What we
learned from SolidCraft" are from that date. Before relying on this months later, re-check
the venues' rules and the landscape in [docs/POSITIONING.md](docs/POSITIONING.md).

**Division of labour.** The agent checks the gates, fills in the drafts, prepares the
links and watches the metrics. The maintainer submits every post and comment from their
own accounts. An agent never posts, votes, sends a message or opens a pull request on
another project without the maintainer's explicit go-ahead for that specific item.

---

## 1. What we learned from SolidCraft

SolidCraft (Rust parametric CAD, Fusion-style) went from its first commit on 2026-10-07 to
about 7,900 stars and 900 forks by 2026-10-20. Observations, then what we infer from them.

### Observations (sourced)

| Date (2026) | Event | Stars that day ([Trendshift](https://trendshift.io/repositories/296178)) |
|---|---|---|
| Oct 7 | First commit | |
| Oct 9 | v0.1 released as **SolveCraft** | 0 |
| Oct 11 | Renamed **SolidCraft** (v0.3.0), at the peak of the "Reverse Engineer Everything" / ArtCraft wave on X | +434 |
| Oct 12 | Releases v0.3.2 to v0.5.0 | +516 |
| Oct 13, 00:11 UTC | [Show HN](https://news.ycombinator.com/item?id=50064840) (a Sunday evening in the US), posted just as v0.6.0 shipped. Front page, 144 points, 17 top-level comments | +725 |
| Oct 13 to 16 | Several releases a day (v0.7 to v0.14); reposted by X curator accounts | +360 to +550 a day |
| Oct 16 | v0.14 imports FreeCAD and Rhino files; [3Druck covers it](https://3druck.com/en/programs/solidcraft-open-source-cad-version-0-14-0-opens-files-from-freecad-and-rhino-13164153/) | |
| Oct 17 to 19 | Many reposts by X aggregator accounts, several of them Chinese-language. By Oct 20 it is the featured project on [remade.wiki](https://remade.wiki/projects/solidcraft/) (when that started is not recorded) | +1,173, +1,378, +1,602 |

- About 30 GitHub releases in 9 days, plus nightlies. Every release was a fresh thing to
  share.
- Easy to try: a [browser build](https://solidcraft.io), installers for every OS
  (MSI and winget, DMG, AppImage, Snap), and unsigned binaries with the extra click
  documented.
- A built-in MCP server with a one-command Claude Code setup, and a README video of an
  agent driving the live app ("a pointer glides to toolbar buttons while the agent builds").
- An honest status line, "early but usable for simple parts".
- GitHub topics set (`cad`, `parametric-cad`, `rust`, `brep`, `step`, `mcp`, `wasm`,
  `3d-modeling`, `egui`), a homepage, docs and a Discord.
- The Hacker News thread asked about: the geometry kernel (it is built on Truck),
  whether it is wrong to copy someone else's CAD program, MCP compared with an LLM writing
  FreeCAD Python, whether it would "survive DMCA", whether it can 3D print, FreeCAD-style
  breakage when parameters change, "Ok CAD is great but I want CAM", and whether an
  AI-built project will still be maintained in five years.

Sources: [HN thread](https://news.ycombinator.com/item?id=50064840) (read through the
Algolia API), [Trendshift](https://trendshift.io/repositories/296178),
[explainx.ai write-up](https://www.explainx.ai/blog/solidcraft-open-source-rust-parametric-cad-mcp-agents-2026),
[sudolabs on the clone wave](https://sudolabs.nz/blog/reverse-engineer-everything-free-alternatives-ai-clone-wave),
[KDnuggets](https://www.kdnuggets.com/the-rise-of-ai-software-clones-why-rea-and-artcraft-are-going-viral),
`gh release list -R tangency-labs/solidcraft`.

### Inferences (our reading, not proven)

1. **The wave did more than Hacker News did.** The jump started on Oct 11 with the rename
   into the "*Craft" family, two days before Show HN. HN gave a respectable one-day spike;
   the biggest days (Oct 17 to 19) line up with X reposts and press, and probably the remade.wiki feature.
   HN seeds the story; reposts carry it.
2. **Release velocity was the content engine.** Each release had a one-line hook ("now
   opens FreeCAD and Rhino files") that curators and press could repeat.
3. **Being able to try it in under a minute is the price of entry.** The browser build
   and installers turned a click into a try.
4. **An agent moving a real UI is the demo that travels.** It is what the AI crowd shares.
5. **The silence on SOLIDWORKS is our opening.** remade.wiki's SOLIDWORKS page lists only
   PartCraft (12 stars, JavaScript, migration through an export macro run inside
   SOLIDWORKS). Nobody owns "open-source SOLIDWORKS migration" yet.

### What we copy, and what we do not

- **Copy:** easy to try, the agent demo video, release velocity with a hook per release,
  topics and homepage, an honest status line, a remade.wiki listing, prepared answers to
  the predictable questions.
- **Do not copy the rename.** We keep the name AnvilCAD. remade.wiki's policy discourages
  simply appending "Craft", and the name is part of our identity. We join the wave through
  the connection we actually have: **AnvilCAD's SOLIDWORKS research is done with REA**
  (see [CLAUDE.md](CLAUDE.md) and `docs/re/`). Say that, and say how: from published
  documentation and lawfully held sample files only, never by analysing SOLIDWORKS
  binaries.

---

## 2. Preflight gates

Do not launch until every gate passes. The agent runs the checks and reports pass or fail
with evidence; the maintainer decides.

| # | Gate | How to check |
|---|---|---|
| G1 | A GitHub release exists with installers for macOS, Windows and Linux | `gh release view --json tagName,assets --jq '{tag:.tagName,assets:[.assets[].name]}'` |
| G2 | Each installer opens the sample part on a clean machine | Windows: `scripts/winvm/winvm` with the downloaded asset. Linux: `scripts/linuxvm/linuxvm`. macOS: download, open, Help > Open Sample Part. Record the time from download to sample part; the target is under 5 minutes. |
| G3 | Install instructions for unsigned apps (macOS Gatekeeper, Windows SmartScreen) are in the README and on `docs/index.html` | Read both pages |
| G4 | The MCP server works from a one-line setup | Run the README's `claude mcp add ...` line in a fresh shell and build a 10x20x30 mm block; the volume must be 6000 mm³ |
| G5 | Every claim in the README and the drafts below is true today | Check against the "Say / Do not say" table in [docs/POSITIONING.md](docs/POSITIONING.md) and the README's "What works today" |
| G6 | Films rendered from the current release: the 60-second trailer, the `agent` film, and the feature spots. Add a migration before-and-after with the volume check when there is a part the project may publish | `python3 scripts/cinema/films.py` ([scripts/cinema/README.md](scripts/cinema/README.md)); watch each one, and check its captions against the README's "What works today" |
| G7 | GitHub topics and homepage set | `gh repo view --json homepageUrl,repositoryTopics`. Suggested topics: `cad`, `parametric-cad`, `rust`, `brep`, `step`, `mcp`, `solidworks`, `cam`, `3d-modeling`. Ask the maintainer before running `gh repo edit --add-topic`. |
| G8 | A place to talk: GitHub Discussions on (or a Discord), linked from the README | `gh repo view --json hasDiscussionsEnabled`. Ask before changing the setting. |
| G9 | 5 to 10 open issues labelled `good first issue`, each with the files involved and what done looks like | `gh issue list --label "good first issue"` |
| G10 | CI is green on `main`, and `cargo test --workspace` passes locally | `gh run list --branch main --limit 1` |
| G11 | The next two releases are ready to tag (small, real improvements with a one-line hook each) | A list of merged or nearly merged PRs |
| G12 | The maintainer has time blocked: 4 hours from the HN post, and an hour a day for a week | Ask |

Nice to have, not gates: native `.sldprt` geometry for simple parts, drawings, a browser
build, code-signed installers.

---

## 3. Launch day

Pick a Tuesday, Wednesday or Thursday. The common advice is about 14:00 to 16:00 UTC (7 to
9 am in California, the morning in New York, the afternoon in Europe). SolidCraft posted on
a Sunday at 00:11 UTC, when there is less competition but fewer readers, and still reached
the front page. Either works; the weekday morning is the default.

| Time (UTC) | Who | Action |
|---|---|---|
| T-24 h | Agent | Rerun every gate. Fill the placeholders in section 5 from the release notes. Report. |
| T-60 min | Maintainer | Tag and publish the release (`vX.Y.0`). Agent confirms the assets are attached and download. |
| T-0 | Maintainer | Submit the Show HN (5.1). |
| T+2 min | Maintainer | Post the Show HN first comment (5.2). |
| T+30 min | Maintainer | Post the X thread with the video (5.3). |
| T+60 min | Maintainer | Post to r/rust (5.4). |
| T+90 min | Maintainer | Share in REA's Discord, Show and Tell channel (5.6). |
| T+0 to T+4 h | Maintainer, agent drafting | Answer every HN comment (section 4). The agent drafts replies from section 6; the maintainer edits and posts. |
| T+6 h | Agent | Snapshot the metrics (section 7). |

Days 1 to 7:

| Day | Action |
|---|---|
| 1 | Open the remade.wiki pull request (5.8). Post the REA showcase issue (5.7). Ship release +1 with its hook; post it on X as a reply to the launch thread. |
| 2 | Post to r/cad and r/MechanicalEngineering (5.5), lead with migration. Ship release +2. |
| 3 | Post the clean-room write-up to r/ReverseEngineering (5.5). Send the Hackaday tip (5.9). |
| 4 | Post the MCP angle to r/ClaudeAI (5.5). Submit to MCP directories (5.10). |
| 5 | Submit to This Week in Rust (5.9). Email 3Druck and similar CAD and 3D-printing outlets (5.9). |
| 6 to 7 | Keep releasing with hooks. Turn the most common questions into README FAQ entries. Run the retro (section 7). |

Spread the subreddit posts over the days, as above. Several posts in an hour from one
account look like spam to Reddit's filters and to moderators.

---

## 4. How to engage

- **Reply to everything in the first four hours,** briefly and specifically. Thank people
  who report a problem, then open an issue and link it in the reply.
- **Lead with what is true and what is missing.** "Not yet; it is on the roadmap, here is
  the issue" earns more trust than a hedge. Never claim something the README does not.
- **Disclose.** Say "I'm the maintainer" in every post. Say how it was built (AI coding
  agents, with tests against exact values) when asked, or before anyone asks.
- **No vote games.** Do not ask anyone to upvote, do not post links that invite voting,
  do not post from second accounts, do not ask friends to comment. Hacker News and Reddit
  both detect and penalise this, and it would cost us the trust this project depends on.
- **Do not argue with critics.** Answer the substance once, then move on. "Copying CAD is
  the wrong approach" deserves a real answer (6.2), not a debate.
- **Turn feedback into releases.** A fix shipped the next day, with "thanks to @user", is
  the best reply there is, and it is the next release's hook.
- **Talk about competitors generously.** SolidCraft, PartCraft and FreeCAD serve different
  people; say who each is for (6.6) and never run them down.
- **Legal lines.** Name SOLIDWORKS only to describe compatibility, keep the non-affiliation
  line, use no Dassault logos or artwork, and use AnvilCAD's names for features. Never call
  it cracking, never invite people to decrypt anything, stay out of warez and "cracked
  SOLIDWORKS" communities, and do not post leaked or NDA material, or let others post it in
  our spaces. If someone asks for help defeating licence checks, decline and link
  [INTEROPERABILITY.md](INTEROPERABILITY.md).

---

## 5. Ready-to-post drafts

Fill the `{{placeholders}}` from the release at T-24 h. Check every sentence against gate
G5 before posting; if a claim stopped being true, cut it.

Placeholders: `{{version}}`, `{{release_url}}`, `{{video_url}}` (the trailer, or the
`agent` film for AI audiences, from `target/cinema/`), `{{mcp_setup}}` (the one-line command from the README),
`{{new_this_release}}`.

### 5.1 Show HN title

Submit with the repository as the URL: `https://github.com/swperb/anvilcad`. Hacker News
allows 80 characters. Options, best first:

1. `Show HN: AnvilCAD – Open-source CAD that keeps your SOLIDWORKS feature trees`
2. `Show HN: AnvilCAD – Parametric CAD in Rust with its own exact B-rep kernel`
3. `Show HN: AnvilCAD – Open-source CAD/CAM in Rust, with MCP for agents`

Use title 1 only if "Migrates SOLIDWORKS parts with their feature trees, using your own
SOLIDWORKS licence" is still the headline in POSITIONING.md.

### 5.2 Show HN first comment

```text
I'm the maintainer. AnvilCAD is free, open-source (MIT/Apache-2.0) parametric CAD written
in Rust, with its own exact B-rep kernel and STEP reader. It's early ({{version}}), but the
modelling loop works end to end.

Who it's for: engineers and firms with years of SOLIDWORKS parts. What keeps them on
SOLIDWORKS is their data and their habits, so AnvilCAD tries to answer both:

- Migration that keeps the history. anvil-migrate drives your own licensed SOLIDWORKS
  through its published API and rebuilds each part as editable AnvilCAD features,
  checking every solid feature against SOLIDWORKS' own volume. Each file comes out
  Parametric or Hybrid, with a report that says what did and didn't come across.
  anvil-bulk runs a whole archive across many machines.
- A familiar workflow: feature tree, fully defined sketches, relations, smart dimensions,
  equations, configurations, custom properties.
- No SOLIDWORKS required, ever. Everything works on macOS, Linux and Windows without it;
  a licence only adds fidelity while you still have one.

How the SOLIDWORKS behaviour was studied: clean-room, from Dassault's published help and
API docs and sample files we may use, never by analysing their binaries. Each feature has
a written spec in docs/re/ listing its sources, done with the REA (Reverse Engineer
Anything) workflow, and the code is written from the spec.

There's also a 2.5D CAM library (adaptive clearing, simulation, verified G-code; no UI
yet), and an MCP server so agents can model headlessly: {{mcp_setup}}

What's missing: drawings, assembly migration, and reading .sldprt geometry natively
(today it reads custom properties; geometry comes through migration or STEP). The roadmap
is in ROADMAP.md.

Installers: {{release_url}} (unsigned for now; the README has the extra click).
I'd love feedback, especially from people who use SOLIDWORKS daily.
```

### 5.3 X thread

1. `AnvilCAD {{version}}: free, open-source parametric CAD in Rust, with its own exact
   kernel. Built to bring your SOLIDWORKS feature trees with you. {{video_url}}`
2. `Migration rebuilds each part as editable features, using your own SOLIDWORKS licence,
   and checks every feature's volume against SOLIDWORKS. Hybrid where it can't, with a
   report saying why.`
3. `No SOLIDWORKS needed to use it: macOS, Linux, Windows. STEP in and out. MCP built in,
   so your agent can model: {{mcp_setup}}`
4. `The SOLIDWORKS research is clean-room, done with REA from published docs and sample
   files only. Every spec is public in docs/re/. @{{rea_author_handle}}`
5. `Early, but the modelling loop works. Installers: {{release_url}}
   Code: https://github.com/swperb/anvilcad`

Tag REA's author only if they have said they welcome it. Attach the video natively, not
as a link. Optional: one post in Chinese summarising tweets 1 and 3, since many of the
accounts that amplified SolidCraft post in Chinese. Have a fluent speaker check it.

### 5.4 r/rust

Title: `AnvilCAD {{version}}: parametric CAD with its own exact B-rep kernel, in Rust`

Body: what the kernel does (exact planes, cylinders, cones, spheres, tori and B-splines;
exact fillets and chamfers; STEP AP203/214/242 import healed to the file's tolerance, and
32 of the 33 NIST test models import as exact solids), how the crates are split (the
Architecture section of the README), the lint policy (no `unsafe`, no `unwrap` in the
kernel and STEP crates), what was hard, and what is next. Link the repository. Keep the
SOLIDWORKS angle to one line; r/rust wants the engineering.

### 5.5 Other subreddits

Read each sidebar's self-promotion rules first and follow them. If a subreddit has a
weekly showcase thread, use it.

| Subreddit | Angle | Title |
|---|---|---|
| r/cad | Migration and the familiar workflow | `Open-source CAD that migrates SOLIDWORKS parts as editable features (early)` |
| r/MechanicalEngineering | The same, plus no subscription and files on your own disk | `I'm building a free CAD that keeps your SOLIDWORKS feature trees, looking for feedback` |
| r/SolidWorks | Only if the rules allow it. Ask what migrations people need, don't sell. | `What would you need to move your parts off SOLIDWORKS? (open-source project, feedback wanted)` |
| r/ReverseEngineering | A technical write-up of the clean-room method: specs from published docs and sample files, observations kept apart from inferences, nothing taken from binaries | `Writing clean-room specs of SOLIDWORKS behaviour with an agent, from public docs only` |
| r/ClaudeAI | MCP: an agent modelling a real part, with the video | `I gave Claude a parametric CAD kernel over MCP` |

### 5.6 REA Discord, Show and Tell

```text
AnvilCAD (open-source parametric CAD in Rust) uses REA's workflow to write clean-room
specifications of SOLIDWORKS behaviour, from Dassault's published docs and sample files
only, with no binary analysis. Each spec records its sources and separates observations
from inferences, as REA does, and the code is written from the spec. Specs:
https://github.com/swperb/anvilcad/tree/main/docs/re. Happy to answer questions.
```

### 5.7 REA showcase issue

REA's README invites showcases through an issue at <https://github.com/morluto/rea/issues>,
"including the target, your question, how REA helped, and what you found". Draft one with
those four parts, using one spec (for example `docs/re/hole-wizard.md`) as the worked
example. Be clear that this is documentation-driven work, not binary analysis. The
maintainer opens the issue.

### 5.8 remade.wiki listing

Pull request to <https://github.com/replicraft/remade>, adding
`apps/frontend/remade-wiki/src/content/catalog/projects/swperb/anvilcad.yaml`. The
`solidworks` original already exists. Read their `POLICY.md` and tick its acknowledgment
in the PR template. Do not set `featured`.

```yaml
# Scope and limitations are maintainer-reported in the repository README.
slug: anvilcad
name: AnvilCAD
authors: [swperb]
repository: https://github.com/swperb/anvilcad
website: https://swperb.github.io/anvilcad/
docs: https://github.com/swperb/anvilcad#readme
originals: [solidworks]
category: engineering
languages: [Rust]
platforms: [Linux, macOS, Windows]
status: alpha
compatibility: partial
description: An independent parametric modeler following SOLIDWORKS conventions (feature tree, fully defined sketches, relations, equations, configurations), with its own exact B-rep kernel and STEP interchange. Migrates SOLIDWORKS parts as editable features through the published API of the user's own licensed copy.
license: MIT OR Apache-2.0
limitations: Early. Migration requires a licensed SOLIDWORKS on Windows and covers parts only; without SOLIDWORKS, native files are identified and their custom properties read, and geometry comes through STEP. Drawings are not implemented. Full compatibility is not claimed.
```

Check every field against the README on the day, and update `status` and `limitations` if
they have changed.

### 5.9 This Week in Rust, Hackaday, press

- **This Week in Rust:** a pull request adding the release post or the kernel write-up to
  the current draft in <https://github.com/rust-lang/this-week-in-rust> (read its
  CONTRIBUTING first), under the projects and updates section.
- **Hackaday:** <https://hackaday.com/submit-a-tip/>. Two sentences and the video; the hook
  is the CAM library or migration, whichever has the better story that week.
- **CAD and 3D-printing press** (3Druck covered SOLIDWORKS-free CAD news for SolidCraft):
  a five-line email with what is new, the video, the release link and the maintainer's
  name. The agent drafts it; the maintainer sends it.

### 5.10 MCP directories

Once the MCP server has shipped in a release: the official MCP registry, the
`awesome-mcp-servers` lists on GitHub (by pull request) and Smithery. Each entry: one
line, the setup command and a link to the docs.

---

## 6. Prepared answers

Draft replies from these, then adjust to the question. Keep each to a few sentences and
link to evidence.

1. **"Writing a B-rep kernel is a huge amount of work. Is it your own?"** Yes: exact
   geometry in `anvil-brep` and `anvil-kernel`. Point to the STEP results (NIST corpus,
   volumes agree within 0.15% across CAD systems) and the exact fillets. Be frank about
   what the kernel cannot do yet (ROADMAP.md, item 1).
2. **"Copying someone else's CAD program is the wrong approach."** We are not copying
   SOLIDWORKS; we are making it possible to leave. The conventions are familiar so
   trained engineers stay productive, the features carry our own names, and the point is
   getting your data out with its history. Link INTEROPERABILITY.md.
3. **"Will this survive a DMCA / is it legal?"** Our method: interoperability studied from
   published documentation and sample files only, never their binaries, with no
   circumvention and a clean-room split between the specs and the code. Migration uses
   the published API on the user's own licence. Link INTEROPERABILITY.md and `docs/re/`.
   This is not legal advice, and we don't pretend otherwise.
4. **"Can it open my .sldprt without SOLIDWORKS?"** Not the geometry yet. It identifies
   the file and reads its custom properties; geometry comes through `anvil-migrate` (with
   your licence) or STEP. A native reader is the next big piece. Never round this up.
5. **"Why not FreeCAD?"** FreeCAD is the established choice and gets geometry from
   SOLIDWORKS files at best. AnvilCAD's focus is bringing the feature tree and the
   SOLIDWORKS habits along. Both can exist.
6. **"How is this different from SolidCraft / PartCraft?"** SolidCraft is a fine
   Fusion-style CAD for Fusion users, with no SOLIDWORKS migration. PartCraft is
   SOLIDWORKS-style too, and migrates through an export macro run inside SOLIDWORKS.
   AnvilCAD reads the feature tree through the published API, checks each feature's
   volume, runs migrations across many machines, and has its own exact kernel and a CAM
   library. Check these facts on launch day before saying them.
7. **"MCP vs. just having the LLM write a script?"** Both work here: the MCP server for
   agents, and Lua scripting (docs/SCRIPTING.md) for scripts and key bindings.
8. **"I want CAM."** There is a 2.5D milling library: adaptive clearing with an
   engagement cap, simulation, gouge and holder checks, and G-code that is verified and
   certified. No user interface yet; link `docs/cam/` and issue #1.
9. **"3D printing?"** STL and OBJ export, plus STEP. No slicer built in.
10. **"Does it break when parameters change, like FreeCAD?"** Answer only from what the
    code and tests show on the day; say what is handled and what is not. Do not promise
    it never breaks.
11. **"Was this built with AI? Will it be maintained in five years?"** Answer honestly
    about the tools used. Then the evidence: tests against exact values, no panics on
    bad input, versioned JSON files you can read without AnvilCAD, STEP export, and a
    permissive licence, so your data never depends on the project surviving.
12. **"Is it affiliated with Dassault?"** No. Independent project; SOLIDWORKS is a
    trademark of Dassault Systèmes, named only to describe compatibility.

---

## 7. Metrics and retro

The agent snapshots these at T+6 h, then daily for a week, and keeps a table in the launch
issue or a local note:

```bash
gh repo view swperb/anvilcad --json stargazerCount,forkCount
```

```bash
gh api repos/swperb/anvilcad/traffic/popular/referrers
```

```bash
gh api repos/swperb/anvilcad/traffic/views
```

```bash
gh release view --json assets --jq '[.assets[] | {name, downloadCount}]'
```

```bash
gh issue list --state all --limit 50 --json number,title,createdAt
```

Also: the HN rank and points, upvotes and comments on each Reddit post, the X impressions
the maintainer can see, and stars per day on Trendshift once it lists the repository.

Retro at day 7: which venue brought the most stars and issues, which questions came up
most (turn them into README FAQ entries), what broke for new users (fix first), and which
release hooks were picked up. Update this file with what we learned.
