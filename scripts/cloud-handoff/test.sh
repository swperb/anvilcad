#!/bin/bash
# Tests cloud-handoff end to end without touching the real ~/.claude, launchd or the
# network: a temporary home, a local bare git remote and a fake `claude` that records its
# arguments. Run: scripts/cloud-handoff/test.sh
set -u

HERE="$(cd "$(dirname "$0")" && pwd)"
TOOL="$HERE/cloud-handoff"
T="$(mktemp -d "${TMPDIR:-/tmp}/cloud-handoff-test.XXXXXX")"
trap 'rm -rf "$T"' EXIT

export CLOUD_HANDOFF_HOME="$T/handoff"
export CLOUD_HANDOFF_SETTINGS="$T/settings.json"
export CLOUD_HANDOFF_PLIST="$T/agent.plist"
export CLOUD_HANDOFF_NO_LAUNCHD=1
export CLAUDE_BIN="$T/fake-claude"
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.com
export GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.com

fails=0
check() { # check <description> <command...>
  local what="$1"; shift
  if "$@"; then echo "ok   $what"; else echo "FAIL $what"; fails=$((fails + 1)); fi
}

cat > "$CLAUDE_BIN" <<'EOF'
#!/bin/bash
n=$(ls "$CLOUD_HANDOFF_HOME" | grep -c '^claude-args-')
printf '%s\0' "$@" > "$CLOUD_HANDOFF_HOME/claude-args-$(printf %03d "$n")"
echo '{"ok": true, "session_id": "session_01TEST", "url": "https://claude.ai/code/session_01TEST"}'
EOF
chmod +x "$CLAUDE_BIN"

# A repository with an origin remote, on a feature branch, with uncommitted work.
git init -q --bare "$T/origin.git"
git init -q -b main "$T/repo"
( cd "$T/repo" && git remote add origin "$T/origin.git" && echo one > a.txt && git add a.txt \
  && git commit -q -m init && git push -q origin main && git switch -q -c feature \
  && echo two >> a.txt && echo new > b.txt )

# A transcript in Claude Code's JSONL shape.
cat > "$T/transcript.jsonl" <<'EOF'
{"type":"user","message":{"role":"user","content":[{"type":"text","text":"<system-reminder>ignore me</system-reminder>"},{"type":"text","text":"Add a fillet tool"}]}}
{"type":"assistant","message":{"content":[{"type":"text","text":"Working on the fillet tool."},{"type":"tool_use","id":"x","name":"Bash","input":{}}]}}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"done"}]}}
{"type":"user","isMeta":true,"message":{"role":"user","content":"meta text"}}
{"type":"assistant","message":{"content":[{"type":"text","text":"Tests pass for the edge case."}]}}
EOF

hook() { # hook <event> [session]
  jq -n --arg e "$1" --arg s "${2:-sess-aaaa1111}" --arg c "$T/repo" --arg tp "$T/transcript.jsonl" \
    '{session_id: $s, hook_event_name: $e, cwd: $c, transcript_path: $tp}' | "$TOOL" hook
}
rec() { cat "$CLOUD_HANDOFF_HOME/sessions/${1:-sess-aaaa1111}.json"; }

# --- install writes hooks once, keeps other settings, and uninstall removes them.
echo '{"model":"opus","hooks":{"Stop":[{"hooks":[{"type":"command","command":"say done"}]}]}}' > "$CLOUD_HANDOFF_SETTINGS"
"$TOOL" install >/dev/null 2>&1
"$TOOL" install >/dev/null 2>&1
check "install adds one hook per event" \
  [ "$(jq '[.hooks[][] .hooks[] | select(.command | contains("cloud-handoff"))] | length' "$CLOUD_HANDOFF_SETTINGS")" = 5 ]
check "install keeps other hooks and settings" \
  [ "$(jq -r '.model + " " + (.hooks.Stop | map(.hooks[].command) | join(","))' "$CLOUD_HANDOFF_SETTINGS")" = "opus say done,\"$CLOUD_HANDOFF_HOME/bin/cloud-handoff\" hook" ]
check "PreToolUse hook matches every tool" [ "$(jq -r '.hooks.PreToolUse[0].matcher' "$CLOUD_HANDOFF_SETTINGS")" = "*" ]
check "install builds the sleep watcher" [ -x "$CLOUD_HANDOFF_HOME/bin/sleepwatch" ]
check "install writes the launch agent" grep -q "<string>$CLOUD_HANDOFF_HOME/bin/sleepwatch</string>" "$CLOUD_HANDOFF_PLIST"
check "install writes a commented config" grep -q '^# TRIGGER=lid' "$CLOUD_HANDOFF_HOME/config"

# --- hooks track state.
hook SessionStart
check "SessionStart records an idle session" [ "$(rec | jq -r .state)" = idle ]
hook UserPromptSubmit
check "UserPromptSubmit marks it busy" [ "$(rec | jq -r .state)" = busy ]
hook Stop
check "Stop marks it idle" [ "$(rec | jq -r .state)" = idle ]

# --- sleep with the lid open, or with only idle sessions, does nothing.
CLOUD_HANDOFF_LID=open "$TOOL" sleep
check "lid open: no handoff" [ "$(rec | jq -r '.handoff.status // "none"')" = none ]
CLOUD_HANDOFF_LID=closed "$TOOL" sleep
check "idle session: no handoff" [ "$(rec | jq -r '.handoff.status // "none"')" = none ]

# --- a dry run writes the brief and changes nothing.
hook PreToolUse
CLOUD_HANDOFF_LID=closed "$TOOL" sleep --dry-run
brief="$CLOUD_HANDOFF_HOME/dry-run-brief-sess-aaaa1111.txt"
check "dry run leaves the tree uncommitted" [ -n "$(git -C "$T/repo" status --porcelain)" ]
check "brief names the branch" grep -q 'The branch feature holds' "$brief"
check "brief has the original request" grep -q '^USER: Add a fillet tool$' "$brief"
check "brief has the latest reply" grep -q '^CLAUDE: Tests pass for the edge case.$' "$brief"
check "brief drops reminders, tool results and meta" sh -c "! grep -qE 'ignore me|done\$|meta text' '$brief'"

# --- a real handoff commits, pushes and starts a cloud session.
CLOUD_HANDOFF_LID=closed "$TOOL" sleep
check "handoff records the cloud session" [ "$(rec | jq -r '.handoff.status + " " + .handoff.cloud_id + " " + .handoff.branch')" = "cloud session_01TEST feature" ]
check "handoff commits untracked files" [ -z "$(git -C "$T/repo" status --porcelain)" ]
check "handoff commit is signed off" sh -c "git -C '$T/repo' log -1 --format=%B | grep -q '^Signed-off-by: test'"
check "handoff pushes the branch" [ "$(git -C "$T/origin.git" rev-parse feature)" = "$(git -C "$T/repo" rev-parse HEAD)" ]
args="$(cat "$CLOUD_HANDOFF_HOME"/claude-args-* | tr '\0' '\n')"
check "claude is called with -p --cloud and the brief" sh -c "printf '%s' \"\$1\" | grep -q '^--cloud\$' && printf '%s' \"\$1\" | grep -q 'Add a fillet tool'" _ "$args"

# --- the paused local session is blocked.
out="$(hook PreToolUse)"
check "PreToolUse is denied while in the cloud" [ "$(printf '%s' "$out" | jq -r .hookSpecificOutput.permissionDecision)" = deny ]
out="$(hook UserPromptSubmit)"
check "prompts are blocked while in the cloud" [ "$(printf '%s' "$out" | jq -r .decision)" = block ]
check "other sessions are not blocked" [ -z "$(hook PreToolUse sess-bbbb2222)" ]
hook SessionEnd sess-bbbb2222
hook SessionEnd
check "SessionEnd keeps a handed-off record" [ -f "$CLOUD_HANDOFF_HOME/sessions/sess-aaaa1111.json" ]
CLOUD_HANDOFF_LID=closed "$TOOL" sleep
check "a second sleep does not hand off again" [ "$(ls "$CLOUD_HANDOFF_HOME"/claude-args-* | wc -l | tr -d ' ')" = 1 ]

# --- on main, the handoff moves the work to its own branch.
hook PreToolUse sess-cccc3333
( cd "$T/repo" && git switch -q main )
echo three > "$T/repo/c.txt"
CLOUD_HANDOFF_LID=closed "$TOOL" sleep
br="$(rec sess-cccc3333 | jq -r .handoff.branch)"
check "work on main goes to a claude-handoff branch" sh -c "case '$br' in claude-handoff/*-sess-ccc) exit 0;; *) exit 1;; esac"
check "main itself is not pushed with the WIP" [ "$(git -C "$T/origin.git" rev-parse main)" = "$(git -C "$T/repo" rev-parse main)" ]

# --- release unpauses and tells the cloud session to wrap up.
"$TOOL" release sess-cccc >/dev/null
check "release clears the handoff" [ "$(rec sess-cccc3333 | jq -r '.handoff.status // "none"')" = none ]
check "release messages the cloud session" sh -c "cat '$CLOUD_HANDOFF_HOME'/claude-args-* | tr '\0' '\n' | grep -q 'The user has opened their laptop'"

# --- resume teleports back (exec replaced by the fake claude).
WRAPUP_WAIT=0 "$TOOL" resume sess-aaaa </dev/null >/dev/null
check "resume marks the session resumed" [ "$(rec | jq -r .handoff.status)" = resumed ]
check "resume runs claude --teleport <id>" sh -c "ls '$CLOUD_HANDOFF_HOME'/claude-args-* | tail -1 | xargs cat | tr '\0' ' ' | grep -q '^--teleport session_01TEST'"
check "the stale local copy stays blocked" [ "$(hook PreToolUse | jq -r .hookSpecificOutput.permissionDecision)" = deny ]

# --- two busy sessions in one tree: only the most recent is handed off.
hook Stop sess-cccc3333
hook PreToolUse sess-eeee5555
sleep 1
hook PreToolUse sess-ffff6666
( cd "$T/repo" && git switch -q feature && echo five > e.txt )
CLOUD_HANDOFF_LID=closed "$TOOL" sleep
check "the most recent session in a shared tree is handed off" [ "$(rec sess-ffff6666 | jq -r .handoff.status)" = cloud ]
check "the older session in that tree is left alone" [ "$(rec sess-eeee5555 | jq -r '.handoff.status // "none"')" = none ]
hook SessionEnd sess-eeee5555

# --- a failed push is recorded and does not pause the session.
hook PreToolUse sess-dddd4444
( cd "$T/repo" && git remote set-url origin "$T/missing.git" && echo four > d.txt )
CLOUD_HANDOFF_LID=closed "$TOOL" sleep
check "a failed push is recorded" [ "$(rec sess-dddd4444 | jq -r .handoff.status)" = failed ]
check "a failed session is not blocked" [ -z "$(hook PreToolUse sess-dddd4444)" ]

"$TOOL" uninstall >/dev/null
check "uninstall removes only its hooks" [ "$(jq -c .hooks "$CLOUD_HANDOFF_SETTINGS")" = '{"Stop":[{"hooks":[{"type":"command","command":"say done"}]}]}' ]

echo
[ "$fails" = 0 ] && echo "all tests passed" || { echo "$fails failed"; exit 1; }
