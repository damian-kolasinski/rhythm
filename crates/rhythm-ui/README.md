# Rhythm for macOS

The native window shares the CLI's launchd health and recorded run history.
It also lists periodic system and user launchd jobs and readable cron entries.
The ownership labels and filters distinguish jobs Rhythm created, jobs it
adopted for recording, and external jobs. The window refreshes automatically,
lists sources it could not fully read, and keeps the previous successful view
if a refresh fails.

A routine whose recorded runs report open problems is flagged in amber and
shows its problem count. Clicking it opens a dialog listing those problems as
`rhythm findings` does; the dialog follows each refresh and closes once the
problems are resolved. When skipped runs followed the newest real run, the
latest activity adds when the routine last checked.

When a run starts an agent session, its latest activity links to the sessions
from that run. The dialog updates while open and shows the outcome, any pull
requests, and how to open the thread or attach to the session.

Each routine Rhythm created or adopted has a "⋯" button, and a secondary
click on its row opens the same menu. **Remove…** opens a confirmation that
says what stays: the routine's recorded runs and, when it runs a script (a
program starting with `#!`, or a file given to an interpreter such as
`python3`), that script, with **Show in Finder**. Removal unloads the job and
deletes its definition, as `rhythm remove` does, stopping a run in progress.
Once it is gone, a notice repeats where the script is. External routines,
system jobs, and cron entries offer no actions.
[Watch removal with sample data](../../docs/rhythm-macos-remove-demo.mp4).

Run the development window with `cargo run --locked -p rhythm-ui`. Add
`-- --scene routines` for a window with synthetic data. `--list-scenes`
lists the scenes available for snapshots.

## Appearance

The Pulse app icon comes from the earlier Rhythm wordmark's waveform. Its
1024-pixel source is `assets/pulse.png` and `assets/Rhythm.icns` contains
the macOS icon sizes.

The shared `theme.rs` maps Rhythm's existing blue and neutral palette to Kit.
The [Academy Colors frame](https://www.figma.com/design/KiTRYqOIJ3I30j1VjB1156/Academy?node-id=68-43)
defines surface, border, and text roles and pairs status fill, foreground, and
icon colors in light and dark. The [Web type scale](https://www.figma.com/design/KiTRYqOIJ3I30j1VjB1156/Academy?node-id=82-42)
and [mobile type specimens](https://www.figma.com/design/KiTRYqOIJ3I30j1VjB1156/Academy?node-id=80-25)
guide DM Sans headings, body, and labels. Read these frames with
`figma-framelink-cli get-figma-data`. Routine identifiers use the system
monospace family. DM Sans is bundled under the SIL Open Font License.

## Verification

Render the removal demonstration with
`cargo run --locked -p rhythm-ui -- --scene routines --theme light --size 750x462 --scale 2 --fps 30 --pointer --script crates/rhythm-ui/demos/remove-routine.json --video docs/rhythm-macos-remove-demo.mp4`
(needs `ffmpeg`).

`cargo test --locked -p rhythm-ui --test window` renders the actual view with
Metal in both appearances. A per-channel difference of 8 accommodates
antialiasing differences. Set `RHYTHM_BLESS_GOLDENS=1` for intentional changes,
then inspect the changed PNGs and rerun without it. macOS CI runs the comparison.

Pin Rune, Rune Kit, and the four workspace patches to the same Git revision.
Update the lockfile when changing the pin.
