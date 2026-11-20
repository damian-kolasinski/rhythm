# Run results

A recorded job can say how its run went: that it checked and had nothing to
do, and one line describing the run. A job polling every 15 minutes that only
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
