# Post processors: shared, declarative and checked

A post processor turns a machine-independent tool path into one machine's G-code. Posts
are where machine-specific knowledge lives, so they should be easy to share. They are also
where a mistake or a malicious edit can crash a machine or reach outside it, so they
must be safe to share. AnvilCAD's answer:

1. A post is **data**, not code: a TOML file with a tiny template language that cannot run
   anything.
2. Every program a post writes goes through an **authoritative verifier** that does not
   trust the post, and then through the **simulator**, before it is called safe.
3. Shared posts live in a **community registry** with content hashes, signatures, version
   pinning, human review and reporting.
4. **AI** can help write an edit and can review one, but it can only add warnings or block;
   it can never approve what the verifier rejects, and registry merges still need a
   human maintainer.

Code: `crates/anvil-cam/src/post.rs` (format and generator), `verify.rs` (verifier),
`safety.rs` (certification), `registry.rs` (hashes and edit review), `ai.rs` (optional).

## The post format

See the three examples in `crates/anvil-cam/posts/`: a generic Fanuc-style mill,
LinuxCNC and GRBL 1.1. A post has these tables:

- `[post]`: name, version (`1.2.3`), description, authors, and the SPDX licence of the
  definition itself.
- `[format]`: units (`mm` or `inch`), decimal places for coordinates and feeds, block
  numbers (start and step, 0 for none), comment style (`parens`, `semicolon`, `none`),
  longest line, `%` wrapping, two-digit codes (`G00`).
- `[capabilities]`: arcs, helical arcs, canned drilling cycles, automatic tool changer,
  largest arc sweep per block (arcs are split above it).
- `[machine]`: travel limits per axis in machine coordinates, maximum feed, spindle speed
  range, highest tool number.
- `[allow]`: the G codes, M codes and address letters the post may emit. Anything else in
  a program is rejected.
- `[[declared]]`: dangerous constructs the post needs, each with a reason of at least 20
  characters (see below). Most posts declare nothing.
- `[blocks]`: templates (lists of lines) for program start and end, tool change, spindle on
  and off, coolant, optional stop.

Templates have one feature: `{name}` is replaced by a value the generator provides, and
`{{` and `}}` are literal braces. Each block may use only its own variables
(`program_number`, `program_name`, `units_code`, `work_offset`, `safe_z` everywhere;
`tool`, `tool_name`, `length_offset` in `tool_change`; `rpm`, `spindle_code` in
`spindle_on`). An unknown variable, a stray brace or a control character is a load error.
There are no conditions, loops, expressions, includes, file, network or process access.
Lines are at most 256 bytes, blocks at most 64 lines, and definitions at most 64 KB.
Unknown tables or keys are rejected (`deny_unknown_fields`), so a definition cannot carry
anything the loader ignores.

The motion itself is written by the generator, not by templates: modal G codes, only the
axes that change, arcs with incremental centres (split at the post's largest sweep, or
written as lines when arcs are not supported), and canned drilling cycles (G81, G82,
G83, G73 with G98 and G80) when the post supports them, otherwise plain moves. Comments are
reduced to letters, digits, spaces and `.-_`, so nothing a control could read as a
command can be smuggled through a tool or program name. Feeds, speeds and tool numbers
outside the machine's limits are refused before anything is written.

## The verifier

`verify::verify` reads the program as text with no help from the post that wrote it, and
checks it against the post's declared rules. It is the authority: no post, user setting or
AI can override a rejection.

Rejected outright:

- characters other than printable ASCII; lines longer than the post allows; nested or
  unclosed comments;
- any letter, G code or M code not in the post's allow lists, and any code the verifier
  itself cannot follow (it accepts G0 to G3, G4, G17, G20/G21, G28, G40, G43, G49, G54 to
  G59, G61, G64, G73, G80 to G83, G90, G91, G91.1, G94, G98, G99, and M0 to M9, M30; other
  planes, inverse-time feed, radius-format arcs and absolute arc centres are not
  followed and so not accepted);
- expressions and any character outside plain words (`[`, `]`, `=`, `<`, `>`, `*` and so on);
- feeds, spindle speeds, tool and offset numbers out of the machine's range; any
  coordinate beyond 100 m; any point of any move (arcs sampled along their length,
  canned cycles expanded) outside the machine's travel, given where program zero is;
- an arc whose end is not on its circle; a feed move before a feed is set, with the
  spindle stopped, or (on machines with a tool changer) without a tool length offset; a
  length offset that does not match the tool in the spindle; a peck cycle without a peck;
  a cycle with R below Z; text after the program end; a program with no M2 or M30.

Rejected unless the post declares the construct with a reason (and, for codes, also lists
the code in its allow list):

| Construct | Examples |
|---|---|
| Offset or parameter write | `G10` (L1, L2, L10 to L13, L20, L50...) |
| Coordinate shift | `G92`, `G92.1` to `G92.3`, `G52` |
| Position stored into parameters | `G28.1`, `G30.1` |
| Macro or system variables, macro programming | `#3000=1`, `#5221=...`, `IF`, `GOTO`, `WHILE` |
| Macro call | `G65`, `G66`, `G67` |
| Subprogram or external program call | `M98`, `M99`, `M198`, `O` words after the first block |
| User M code that runs a program | `M100` to `M199` (LinuxCNC runs a file of that name) |
| File or port output | `POPEN`, `DPRNT`, `BPRNT`, `PCLOS`; LinuxCNC `(LOGOPEN,...)`, `(PRINT,...)`, `(DEBUG,...)`, `(PROBEOPEN ...)` |
| Override lock | `M49`; `M50 P0`, `M51 P0`, `M53 P0` |
| Soft limits off | `G23` |
| Controller settings | GRBL `$` lines |
| Block delete | `/` |

A declared construct is reported as a warning on every line that uses it, and the line is
counted as opaque: the verifier does not know its effect, so it cannot rebuild the motion
around it and the simulator cannot vouch for it. A program with any opaque line is never
certified safe; a person must review it.

The verifier also rebuilds the tool path the program describes (canned cycles expanded,
G28 through its intermediate point to the top of travel), in program coordinates.

## Certification

`safety::certify` calls a program safe only when:

1. the verifier accepts it with no errors and no opaque lines, and
2. the stock simulator (`sim::simulate`), running the motion the verifier rebuilt from the
   program text, not the tool path it was generated from, finds no gouge into the
   finished part, no rapid move through material, no link that cuts, no material above
   the flutes against the shank, and no holder collision with stock or part, for every
   tool in turn.

For machines without a tool changer the program cannot name its tools, so the caller
gives the tool order and each stop (M0) followed by a spindle start is taken as the next
manual change.

The tests check that programs generated for a recognised part with every bundled post are
certified safe, that the same program with one cutting move turned into a rapid is not, and
that drilling a flat-bottomed blind hole is reported as a gouge.

## The community registry

This is the design for a registry of shared posts. The crate implements the parts a client
needs (hashing, pinning, the deterministic edit review); the service is future work.

**Submissions.** A post is a pull request against a public repository of definitions, one
file per post and version (`posts/<vendor>/<machine>/<name>-<version>.toml`). Each
submission states the machine and control it was tested on, by whom, and how (dry run,
air cut, real cut). Contributions carry a Developer Certificate of Origin sign-off and the
definition states its own licence (MIT OR Apache-2.0 or CC0-1.0 are accepted).

**Hashes and signatures.** Every accepted file is identified by the SHA-256 of its text
with line endings normalised (`registry::content_hash`). The registry index lists name,
version, hash, review status and the reviewers. Releases of the index are signed by the
maintainers (Sigstore or minisign, with keys published in the repository and on the
project site), so a client can tell an official index from a copy. Contributors may sign
their own commits as well; that identifies them but does not stand in for review.

**Version pinning.** A job records the post's name, version and hash (`registry::Pin`).
`registry::load_pinned` refuses to load anything else, so an updated or tampered post
never changes an existing job silently. Updating a job to a new post version is an
explicit act that shows the diff and re-runs certification.

**Review workflow.** Every new post or edit goes through:

1. Automated checks in continuous integration: the definition loads; a reference program
   that exercises every kind of item (`registry::reference_toolpath`) is written with it
   and passes the verifier; and the edit is compared with the previous version
   (`registry::review_edit`). Anything that widens what the post may do, such as a new
   declared construct, a new G or M code or address letter, a raised limit, a changed
   template, or an unchanged version number, is flagged.
2. An optional AI review (below), whose findings are attached to the pull request.
3. Review and approval by at least one maintainer; two for any edit that declares a
   dangerous construct, raises a limit, or comes from a first-time contributor. A
   maintainer's approval is required in every case: `Decision::AutomatedChecksPassed`
   means only that the machines found nothing.

**Reporting.** Anyone can report a post (a crash, a wrong code, suspected malice) through
the issue tracker or a private security address. A report against a post marks its
versions "under review" in the index within a day; clients warn before using a post under
review and refuse one that has been withdrawn. Withdrawn versions stay in the history,
marked, so pinned jobs can be traced. Security reports follow the project's disclosure
policy.

## AI-assisted edits and AI review

Behind the `ai` cargo feature, which is off by default (`cargo build -p anvil-cam
--features ai`). It calls the Claude Messages API (model `claude-opus-5-5`) over HTTPS
with a structured JSON reply, a 300-second timeout, and server-side refusal fallback. The
API key is read only from the `ANTHROPIC_API_KEY` environment variable; it is never
written to files or logs and never shown by `Debug`. Post text is passed as delimited
data, and the prompts tell the model to treat it as data and to report any instructions
it contains. No test touches the network; the transport is a trait with a mock.

- `ai::propose_edit` turns a person's request ("use three decimal places", "add mist
  coolant") into a changed definition. The deterministic review runs on the result at
  once, and the proposal comes back with the diff and the verdict. Nothing is applied:
  `ProposedEdit::accept` returns the new text only when the person has confirmed it, and
  never when the deterministic review rejected it.
- `ai::review_edit_with_ai` asks the model whether an edit looks malicious or dangerous
  and why (`no_concerns`, `concerns`, `block`, with explanations). An unexpected reply is
  treated as `block`.

How the answers combine (`registry::decide`), and this is enforced in code and tested:

| Deterministic verdict | AI says | Outcome |
|---|---|---|
| Reject | anything | Rejected |
| Needs attention | block | Rejected |
| Needs attention | no concerns or concerns | Needs review by a person |
| Pass | block | Rejected |
| Pass | concerns | Needs review by a person |
| Pass | no concerns, or no AI review | Automated checks passed (a maintainer still reviews) |

The AI can make the outcome stricter, never looser. The tests include an "AI" that proposes
a post writing a parameter with `G10 L50`, and a friendly AI review of it: the edit is
rejected and cannot be accepted.
