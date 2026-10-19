# Cloud handoff: keep Claude Code working while the laptop sleeps

`scripts/cloud-handoff` hands any Claude Code session that is in the middle of a turn to
Claude Code on the web when you close the lid of your Mac, and brings it back to the
laptop when you open it again. It is developer tooling for macOS and has nothing to do
with AnvilCAD's own code; it works in any repository with a GitHub `origin`.

## What happens

**While you work.** Claude Code hooks (`SessionStart`, `UserPromptSubmit`, `PreToolUse`,
`Stop`, `SessionEnd`) keep a small record for each session in
`~/.claude/cloud-handoff/sessions/`: its directory, its transcript, and whether it is
*busy* (a turn is running) or *idle* (waiting for you).

**When you close the lid.** A LaunchAgent, `sleepwatch`, gets macOS's "will sleep"
notice and holds sleep off for up to `SLEEP_BUDGET` seconds (default 22; macOS forces
sleep at about 30) while `cloud-handoff sleep` runs. For each busy session, at most one
per working tree:

1. If the checkout is on the default branch, or detached, it moves to a new
   `claude-handoff/<time>-<session>` branch, so WIP never lands on `main`.
2. It commits everything uncommitted as `WIP: hand off to cloud while the laptop sleeps`
   (signed off, hooks skipped for speed) and pushes the branch to `origin`.
3. It starts a cloud session with `claude -p --cloud "<brief>"`. The brief holds the
   original request and the latest part of the conversation, taken from the local
   transcript, plus instructions to carry on, push small steps often, and stop when told
   the user is back.
4. It marks the local session as handed off. From then on the hooks block that session's
   tool calls and prompts, with a message saying where the work went, so the local agent
   and the cloud agent never edit the same tree. (When the Mac wakes, the local turn's
   request resumes or fails, and any tool call it tries is denied.)

**When you open the lid.** On a full wake (not a Power Nap dark wake), `cloud-handoff
wake` waits for the network, sends each cloud session a message asking it to commit, push
and stop, and opens a Terminal window running `cloud-handoff resume`. That gives the cloud
session `WRAPUP_WAIT` seconds (press Enter to skip the wait) and then runs
`claude --teleport <cloud-session>`, which checks out the cloud branch and loads the
whole cloud conversation into a new local session.

If a handoff fails (no network, push rejected, no session ID returned), the session is
not paused, the failure goes to `~/.claude/cloud-handoff/handoff.log`, and a notification
appears when the Mac wakes.

## Install

Needs macOS with the Xcode command line tools (`swiftc`), `jq`, `git`, and `claude`
signed in with a claude.ai account that can use Claude Code on the web. The repository
needs a GitHub `origin` that the Claude GitHub app can read.

```bash
scripts/cloud-handoff/cloud-handoff install
```

This copies the script to `~/.claude/cloud-handoff/bin/` (so it keeps working after a
worktree is removed), builds `sleepwatch`, loads it as the LaunchAgent
`com.anvilcad.cloud-handoff`, and adds the five hooks to `~/.claude/settings.json`,
leaving your other settings and hooks alone and keeping a backup at
`settings.json.bak-cloud-handoff`. Sessions that are already open pick up the hooks when
they next start or resume. Running `install` again is safe. To remove it:

```bash
~/.claude/cloud-handoff/bin/cloud-handoff uninstall
```

The first time it runs, macOS may ask whether the LaunchAgent may open Terminal or show
notifications.

## Commands

| Command | What it does |
|---|---|
| `cloud-handoff status` | Lists tracked sessions, their state and any handoff, and whether the watcher is running |
| `cloud-handoff resume <session>` | Brings a handed-off session back by hand (for example after a wake with no network) |
| `cloud-handoff release <session>` | Drops a handoff, tells the cloud session to wrap up, and unpauses the local copy. Pull the cloud branch first if you want its work |
| `cloud-handoff sleep --dry-run` | Shows what a sleep would do: logs the plan and writes the brief to `dry-run-brief-<session>.txt` without committing, pushing or starting anything |

A session can be given as a unique prefix of its ID, as shown by `status`.

## Settings

Edit `~/.claude/cloud-handoff/config` (shell syntax; `install` writes it with the
defaults commented out).

| Setting | Default | Meaning |
|---|---|---|
| `TRIGGER` | `lid` | `lid` hands off only when the lid is closed; `any` on every sleep, including idle sleep |
| `SCOPE` | `busy` | `busy` hands off only sessions in the middle of a turn; `recent` also idle sessions used in the last `RECENT_MINUTES` |
| `STALE_MINUTES` | `180` | A "busy" session with no activity for this long is assumed to have crashed and is skipped |
| `INCLUDE_UNTRACKED` | `1` | Commit untracked, non-ignored files too. Set `0` to commit tracked changes only |
| `AUTO_RESUME` | `1` | Open a Terminal on wake; `0` shows a notification instead |
| `WRAPUP_WAIT` | `45` | Seconds to let the cloud session push before teleporting |
| `TERMINAL_APP` | `Terminal` | App that opens the resume window, for example `iTerm` |
| `SLEEP_BUDGET` | `22` | Seconds the sleep step may take. Run `install` again after changing it |

## Limits and things to know

- **Untracked files are pushed.** With `INCLUDE_UNTRACKED=1`, any file not covered by
  `.gitignore` goes into the WIP commit and to GitHub. Keep secrets ignored. GitHub push
  protection catches known token formats, but not everything.
- **The cloud agent gets a brief, not the conversation.** Claude Code has no documented
  way to move a local transcript into a cloud session, so the cloud agent sees the
  original request, the recent messages, and the repository. Coming back is the
  complete direction: `--teleport` loads the full cloud conversation.
- **What `--cloud` prints when it creates a session is undocumented.** The script reads
  `session_id` and `url` from JSON output, and falls back to the `Session ID:` line or a
  `claude.ai/code/...` URL in plain output. If a CLI update changes this, handoffs fail
  safely and log the output.
- **The cloud session may not have finished when you teleport.** There is no documented
  status query for cloud sessions, so `resume` waits a fixed time. Anything the cloud
  agent pushes later can be pulled from its branch; the session itself stays open at
  claude.ai/code.
- **Sessions in the desktop app** are tracked and handed off like terminal sessions, but
  they come back as a terminal session through `--teleport`.
- **A closed lid with an external display** does not sleep, so nothing is handed off.

## Testing

```bash
scripts/cloud-handoff/test.sh
```

It runs install, the hooks, sleep, release, resume and uninstall (not `wake`, which needs the network and opens Terminal) against a
temporary home, a local bare remote and a fake `claude`, so it never touches
`~/.claude`, launchd or the network. It does not exercise a real sleep, a real cloud
session or `--teleport`; check those by hand after installing, with `status` and the log.
