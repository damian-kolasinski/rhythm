# Rhythm for macOS

The native window reads the same launchd health and recorder ledger as the CLI.
It shows only definitions still configured to run through Rhythm. It refreshes
automatically and keeps the previous successful view if the next read fails.

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

`cargo test --locked -p rhythm-ui --test window` renders the actual view with
Metal in both appearances. A per-channel difference of 8 accommodates
antialiasing differences. Set `RHYTHM_BLESS_GOLDENS=1` for intentional changes,
then inspect the changed PNGs and rerun without it. macOS CI runs the comparison.

Pin Rune, Rune Kit, and the four workspace patches to the same Git revision.
Update the lockfile when changing the pin.
