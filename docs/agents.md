# Setting agents up

`rhythm skill setup` gives every coding agent on the machine the Rhythm skill
and a session hook, the way mergegate sets agents up for its hub.

```sh
rhythm skill setup               # every agent found; again to update
rhythm skill setup --dry-run     # what would change
rhythm skill status              # where each agent stands
rhythm skill remove              # undo it, leaving everything else
rhythm skill show [NAME]         # the guide agents follow, or a reference
```

| Agent | Found by | Skill | Session hook |
| --- | --- | --- | --- |
| Claude Code | `~/.claude` | `~/.claude/skills/rhythm` | `~/.claude/settings.json` |
| Codex | `~/.codex` | `~/.agents/skills/rhythm` | `~/.codex/hooks.json` |
| OpenCode | `~/.config/opencode` | `~/.agents/skills/rhythm` | none |

`--agent claude|codex|opencode` (repeatable) limits setup or removal to those
agents. Codex and OpenCode share one skill folder, so removing it from one
removes it from both.

## The skill

Each skill folder gets a short `SKILL.md`: the skill's name and the
description that tells an agent when to load it, the rule never to write
launchd definitions by hand, and an instruction to run `rhythm skill show`.
That prints the guide embedded in the installed Rhythm, and `rhythm skill show
NAME` prints its references (`status`, `runs`, `authoring`, `attention`), so
upgrading Rhythm updates what agents read without touching their folders.

## The session hook

Setup adds one handler to `hooks.SessionStart`:

```text
/bin/sh -c 'rhythm skill session-start --agent claude 2>/dev/null || true'   (timeout 5)
```

At the start of every session, and after a clear or compaction, it prints what
`rhythm attention --since-last-seen --as claude` would: background jobs on this
machine that broke since that agent last looked, with a pointer to the skill.
With nothing new it prints nothing, so sessions carry nothing extra. It reads
only this machine and gives up after three seconds, so a slow launchd or an
unreachable host never holds a session up, and whatever goes wrong it prints
nothing and succeeds. Inside a job Rhythm runs (`RHYTHM_JOB` is set), as a
scheduled `claude -p`, it stays silent and records no look, so the operator's
next interactive session still hears the news; an agent run by a job Rhythm
doesn't record would take it, which is one more reason to `rhythm adopt` it.

The hook runs `rhythm` from the agent's PATH. An agent started where PATH
lacks Rhythm's install, such as from the Dock without the shell's profile,
runs the hook as a quiet no-op while `rhythm skill status` still reports it
current.

The command never changes between releases: Codex runs a hook only once the
operator trusts it with `/hooks`, by its hash, so setup reports that step once.

## What setup touches

Only what a Rhythm wrote: a stub, recognised by its `rhythm-stub` marker, and
a handler whose command contains `rhythm skill session-start`, or the
`rhythm attention --since-last-seen` hook earlier docs described, which setup
replaces so sessions don't hear the same news twice. Keys added to that
handler by hand, such as a longer `timeout` or a `matcher`, are kept until the
command itself changes. A skill named
`rhythm` that Rhythm did not write is left alone and reported, unless `--force`
replaces it; `remove` keeps it. Everything else in a settings file stays as it
was, in its order, including other tools' hooks. Files are replaced atomically
with their permissions, through links into a dotfiles checkout; a linked skill
folder keeps its link. A settings file that isn't valid JSON, or not in the
shape agents read, is reported and left untouched while the other agents go
ahead. A file another program changed while setup worked on it is left as
that program wrote it and reported. Setup exits 1 when anything is left to
sort out.

After an upgrade, the session hook itself updates the stubs and hooks an older
Rhythm wrote, once per session start; it never installs new ones, and never
replaces a stub a newer Rhythm wrote. No other command touches agent files.
