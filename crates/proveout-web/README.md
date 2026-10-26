# Proveout

Proveout checks a CNC milling program against the machine it will run on, the post
processor that wrote it and the part drawing, then writes a setup sheet. It is a second
pair of eyes before a program is proved out, not a sign-off.

It is built from AnvilCAD's CAM verifier but is its own product with its own name. The
logic lives in [`proveout-core`](../proveout-core), a library AnvilCAD can call directly
if it grows a full CAM workflow. This crate is the web site.

## How a check works

1. **Program and drawing.** The user pastes or loads the G-code, attaches the drawing
   (image or PDF) and adds notes. They can anonymize first: comments, the program number
   and any listed words are removed in the browser, so the originals never reach the
   server. Line numbers stay the same.
2. **Machine and post.** The user picks a starting profile and edits it to match their
   machine, or picks a saved one. An AnvilCAD post (`.toml`) is applied as rules. Any
   other post (Fusion `.cps`, Mastercam `.pst` and so on) is read as text for the AI
   review and never run.
3. **Accuracy.** Deterministic only (free, nothing leaves the server), or an AI review
   at Standard (Claude Sonnet 5.5), High (Claude Opus 5.5) or Maximum (Claude Fable 5.1).
4. **Send.** An AI review needs an account and the acceptance of two disclosures: no
   privacy guarantee for what is sent to Anthropic's API, and no guarantee for the
   results. The accepted disclosure version is logged with each review.
5. **Report.** The deterministic findings and the AI findings are kept apart and
   labelled, and are combined into an agreement score with its parts and limits shown
   (see `proveout_core::score`). The setup sheet lists tools, offsets, operations,
   suggested additions and a verification checklist with sign-off lines; every entry
   says whether it came from the program, the checker, the AI or the standard list.
6. **Download** as printable HTML, Markdown or JSON.
7. **Save** the report, or the whole project (inputs and report), to the account.
8. **Plans.** Free accounts get 3 Standard reviews a month and 5 MB of storage. Pro and
   Shop add credits, better levels and storage, through Stripe Checkout.

**Profile a machine** checks the profile's numbers, compares an AnvilCAD post's limits
with the machine, scans a foreign post's output strings for codes that write offsets,
call programs or bypass limits, writes a reference program for the control's graphics
mode, and runs a program the user knows is good. A known-good program that fails means
the profile is wrong; the page tells users to fix that before trusting any check.

## The prompt wrapper

`proveout_core::ai` sends one structured request per review: the drawing as an image or
document block, then the machine profile, the post, the deterministic results, a program
summary, job and drawing notes, and the numbered program, each in its own tag. The system
prompt (cached) tells the model that everything in those tags is data, to cite only real
line numbers, to say "unclear" rather than invent a dimension, and never to call a
program safe. The reply is constrained to a JSON schema and checked again on parse:
numbers are clamped, non-existent line numbers dropped, and every list and string
bounded. Programs over the level's size limit are refused with a message, never cut
short. The request opts into server-side refusal fallbacks (`fallbacks: "default"`).

## Running it

```bash
ANTHROPIC_API_KEY=sk-ant-... cargo run -p proveout-web --bin proveout
```

Then open <http://localhost:8080>. Without `ANTHROPIC_API_KEY` the site runs with the
deterministic check and profiling only.

| Variable | Default | Meaning |
|---|---|---|
| `PROVEOUT_BIND` | `127.0.0.1:8080` | Address to listen on |
| `PROVEOUT_PUBLIC_URL` | `http://localhost:8080` | Origin users see; `https://` turns on secure cookies and HSTS |
| `PROVEOUT_DB` | `proveout.sqlite3` | SQLite database file |
| `ANTHROPIC_API_KEY` | unset | Turns on AI reviews |
| `PROVEOUT_DAILY_REVIEW_LIMIT` | `200` | AI reviews per 24 hours across all users, a spending brake |
| `PROVEOUT_TRUST_PROXY` | `0` | `1` behind a proxy that sets `X-Forwarded-For` |
| `STRIPE_SECRET_KEY`, `STRIPE_WEBHOOK_SECRET`, `STRIPE_PRICE_PRO`, `STRIPE_PRICE_SHOP` | unset | Paid plans; the webhook is `/api/billing/webhook` |

Without Stripe, set a plan by hand with `proveout set-plan EMAIL pro`.

Put it behind a TLS-terminating reverse proxy in production.

## What it stores

E-mail, an Argon2 password hash, plan, monthly credit use, and a log of each AI review
(time, level, model, token counts, disclosure version; not the content). Reports,
projects and machines only when the user saves them; users can delete them or their
whole account. Unsaved reports are kept in memory for an hour, then dropped.

## Tests

`cargo test -p proveout-core -p proveout-web` covers the checks, scoring, prompt and
reply handling, rendering, storage, billing signatures and the HTTP flow with a mock AI
service. The browser anonymizer has its own tests: `node --test crates/proveout-web/tests/`.

## Before launch

- The name "Proveout" has not been cleared as a trademark.
- Prices, credit allowances and storage limits in `proveout_core::plan` are placeholders.
- The disclosure texts in `proveout_core` are engineering drafts, not legal advice; have
  them and the site's terms reviewed.
- There is no e-mail verification or password reset yet, so free credits can be farmed
  with throwaway accounts; `PROVEOUT_DAILY_REVIEW_LIMIT` caps the damage.
