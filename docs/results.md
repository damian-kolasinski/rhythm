# Run results

A recorded job can say how its run went: that it checked and had nothing to
do, one line describing the run, and the problems it found that are open right
now. A job polling every 15 minutes that only
does real work a few times a day would otherwise record every empty check as a
successful run, bury its real runs, and look recovered after an empty check
that followed a failure.

Writing a result is optional. Without one, a run is recorded exactly as before.

## The result file

When the recorder can store a run, it sets `RHYTHM_RESULT` in the command's
environment to `result.json` in the run's private output directory. When it
cannot, the variable is absent, so check it before writing. The recorder reads
the file once the command has exited, so write it before exiting.

```json
{"version": 1, "skipped": true, "summary": "nothing new since the last check"}
```

| Field | Meaning |
| --- | --- |
| `version` | Required. Always `1`. |
| `skipped` | The job checked and had nothing to do. Honored only when the command exits 0; any other exit is recorded as it was. |
| `summary` | One line describing the run, skipped or not. |
| `findings` | The complete set of problems open right now; see [findings](#findings). |

Unknown fields are ignored. The file must be a regular file, not a symlink,
of at most 256 KiB. Summaries are cleaned of terminal controls and shortened
to 200 characters.

Write the file atomically: write a temporary file in the same directory, then
rename it over `result.json`, so the recorder never reads half a result.

```sh
#!/bin/sh
set -eu
new=$(sync-tool --count-new)
if [ "$new" -eq 0 ]; then
  if [ -n "${RHYTHM_RESULT:-}" ]; then
    printf '%s\n' '{"version": 1, "skipped": true, "summary": "nothing new"}' \
      > "$RHYTHM_RESULT.tmp"
    mv "$RHYTHM_RESULT.tmp" "$RHYTHM_RESULT"
  fi
  exit 0
fi
sync-tool --apply
```

```python
import json
import os


def report(skipped, summary):
    path = os.environ.get("RHYTHM_RESULT")
    if not path:
        return
    temporary = path + ".tmp"
    with open(temporary, "w") as file:
        json.dump({"version": 1, "skipped": skipped, "summary": summary}, file)
    os.replace(temporary, path)


report(skipped=True, summary="nothing new since the last check")
```

Build the JSON with a JSON library whenever the summary contains text from
elsewhere, so quotes and newlines stay valid.

## What a skip changes

A skipped run says nothing about whether the job works, like a run stopped on
request:

- It never counts as a recovery and never sends a failure or recovery
  notification. A job that failed stays failing through any number of skips,
  until its next real run succeeds.
- `status` judges a recorded job by its newest run that was neither skipped
  nor stopped, rather than by launchd's last exit. When launchd saw a failure
  after the newest recorded run exited 0, as when the recorder could not
  record that run, the job still shows as failing.
- `attention --since-last-seen` does not report a failing job again for skips.
- Each job keeps its newest 100 runs that were not skipped plus its newest 20
  skipped runs.

## What agents see

```text
$ rhythm history poll
  RUN  STARTED              DURATION  RESULT
  140  2026-09-21 14:09:20     200ms  skipped ×12 since 11:24: nothing new since the last check
  128  2026-09-21 11:13:20     41.0s  exit 0: synced 3 new items
  127  2026-09-21 10:58:20     900ms  exit 0, result file invalid
! 125  2026-09-21 10:28:20      2.0s  exit 1: the source answered 503

$ rhythm status
  JOB              SCHEDULE   STATE  LAST
  dev.rhythm.poll  every 15m  ok     exit 0, 3h ago · skipped 4m ago
```

`history` puts consecutive skipped runs on one row, described by the newest;
`--all` lists each one. `--limit` counts runs that were not skipped, and the
skipped runs among and after them come along. `logs` without `--run` shows the
newest run that was not skipped.

In JSON, finished runs carry `skipped: true`, `summary`, and `result_error`
when they have them; older Rhythm versions omit all three. `result_error` says
why the file was ignored, and the run is recorded otherwise unchanged:

| `result_error` | Meaning |
| --- | --- |
| `invalid` | Not JSON, not an object, no numeric `version`, or a field of the wrong type. |
| `unsupported_version` | A `version` other than `1`. |
| `too_large` | Larger than 256 KiB. |
| `unreadable` | A symlink, not a regular file, or could not be read. |

A missing file is not an error.

## Findings

A job that checks something, such as a test suite, a dependency audit, or a
set of links, can report the problems it found. Rhythm tells the operator and
agents when that set changes: when a problem opens and when the last one is
resolved, never again while one stays open.

```json
{"version": 1, "summary": "checked 41 modules", "findings": [
  {"key": "regression/example-module@1a2b3c4d",
   "title": "example-module: 2 tests broken by #123",
   "detail": "test_parse fails on empty input.",
   "link": "https://example.com/pull/123",
   "handled_by": "the example team"}
]}
```

`findings` is always the complete set of problems open now, not what changed:

- Without a `findings` key, the run says nothing about them and the open set
  stays as it was, as for a skipped run or a run without a result. `[]` says
  everything is resolved.
- Findings count whatever the exit code, so a job can fail at one part of its
  work and still report what is open.
- A problem keeps its `key` across runs; Rhythm compares keys to tell what is
  new and what was resolved, and keeps the time a key was first seen while it
  stays open. A key that disappears and comes back is open since its return.
- Every finding is a problem; there are no severities.

| Field | Meaning |
| --- | --- |
| `key` | Required. 1 to 200 bytes of printable ASCII, unique in the file. Keys starting with `rhythm:` are Rhythm's own. |
| `title` | Required. One line, at most 200 characters. |
| `detail` | Longer text, at most 2000 characters; line breaks are kept. |
| `link` | Where to read more, at most 500 characters. |
| `handled_by` | Who is fixing it, at most 500 characters. |

Text is cleaned of terminal controls. An entry that breaks these rules makes
the whole result `invalid`. Only the first 100 findings are kept, and the
number left out is reported; a job with more should list them in a stable
order, so the same ones are kept each run. When two runs of a job overlap,
the one that ends later states the open set.

When Rhythm cannot read a result, it adds a problem of its own,
`rhythm:result`, titled with the error code, and keeps the job's open
problems as they were. The next readable result resolves it, with or without
`findings`.

```python
import json
import os


def report(problems, summary):
    path = os.environ.get("RHYTHM_RESULT")
    if not path:
        return
    temporary = path + ".tmp"
    with open(temporary, "w") as file:
        json.dump({"version": 1, "summary": summary, "findings": problems}, file)
    os.replace(temporary, path)


report(
    [{"key": "broken-link/docs/setup", "title": "docs/setup links to a removed page"}],
    "checked 120 links",
)
```

### What findings change

- **Health.** A job with open problems that is not otherwise broken is
  `flagged` and needs attention. It ranks below `invalid`, `failing`, and
  `down`, and above everything else, so a long-running job never hides its
  problems while it runs.
- **Notifications.** After a run, the recorder posts one notification when the
  run opened new problems (the first title and `+N more`), and one when the
  open set becomes empty. A failure notification takes precedence, then new
  problems, then a recovery, so a run never posts two.
- **Attention.** `attention` lists flagged jobs with their open problems;
  `attention --since-last-seen` reports per job the problems opened and those
  resolved since that consumer's last look.
- **Retention.** The open set lives on the run that stated it, which is kept
  however many runs follow, so open problems never vanish with old runs.
- **Window.** `rhythm ui` marks a flagged routine with an amber pill and
  shows its problem count as its latest activity. Clicking the routine lists
  its open problems, each with how long it has been open, who handles it, its
  detail, and an `Open link` button for a web link.

```text
$ rhythm status
  JOB               SCHEDULE     STATE    LAST
! dev.rhythm.check  daily 08:07  flagged  2 problems · exit 0, 3h ago

$ rhythm attention --since-last-seen --as claude-code
Background jobs need attention:
! dev.rhythm.check found 1 new problem: example-module: 2 tests broken by #123. See `rhythm findings check`.
~ dev.rhythm.links resolved 2 problems since last check: docs/setup links to a removed page, +1 more.

$ rhythm findings check
dev.rhythm.check has 2 problems open (reported by run 42, 3h ago):

! example-module: 2 tests broken by #123
  open for 2d 5h · handled by the example team
  https://example.com/pull/123
  test_parse fails on empty input.
  key regression/example-module@1a2b3c4d
…

$ rhythm history check
  RUN  STARTED              DURATION  RESULT
   42  2026-09-21 11:13:20     12.0s  exit 0, +1 problem, 1 resolved: checked 41 modules
```

`rhythm findings NAME` (with `--json` and `--host`) returns
`{"type":"findings","label":…,"run":42,"reported_at":…,"findings":[…],"dropped":3}`:
each finding has `key`, `title`, the optional fields, and `first_seen`. `run`
and `reported_at` are absent when no run of the job has listed findings, and
`dropped` when nothing was left out. A run that stated the open set carries a
summary wherever runs appear in JSON, as in `history`, `status`, and
`attention`: `findings: {"open": 2, "new": ["regression/example-module@1a2b3c4d"],
"resolved": 1, "dropped": 3}`, with empty parts left out. The problems
themselves come from `rhythm findings`.

Status and attention from another machine name the `flagged` state, which a
version of Rhythm from before findings cannot read: update the machine you
check from before, or together with, the machines it checks. Later versions
read a state they do not know as `unknown`.
