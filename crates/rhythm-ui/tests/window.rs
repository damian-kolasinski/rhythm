#![cfg(target_os = "macos")]

use std::{fs, io::BufReader, path::Path, process::Command};

const APP: &str = env!("CARGO_BIN_EXE_rhythm-ui");
const SCENES: &[&str] = &[
    "loading",
    "empty",
    "routines",
    "partial",
    "stale",
    "unavailable",
    "release-available",
    "release-offline",
    "release-failed",
];

#[test]
fn scenes_match_goldens() {
    let scratch = tempfile::tempdir().unwrap();
    for scene in SCENES {
        for mode in ["light", "dark"] {
            compare(scratch.path(), scene, mode, "750x462");
        }
    }
    compare(scratch.path(), "stale", "light", "630x330");
    compare(scratch.path(), "release-available", "dark", "630x330");
    compare(scratch.path(), "routines", "light", "630x330");
}

#[test]
fn ownership_filters_include_only_the_matching_routines() {
    let all = replay("routines", 59, 94, "0.7");
    assert!(all.contains("Created by Rhythm"));
    assert!(all.contains("Adopted by Rhythm"));
    assert!(all.contains("com.example.sync-catalog"));
    assert!(all.contains("User crontab"));

    let rhythm = replay("routines", 137, 94, "0.7");
    assert!(rhythm.contains("Created by Rhythm"));
    assert!(rhythm.contains("Adopted by Rhythm"));
    assert!(!rhythm.contains("com.example.sync-catalog"));
    assert!(!rhythm.contains("User crontab"));
    assert!(rhythm.contains("2 of 4 on this Mac"));
    assert!(!rhythm.contains("needs attention"));

    let external = replay("routines", 215, 94, "0.7");
    assert!(external.contains("com.example.sync-catalog"));
    assert!(external.contains("User crontab"));
    assert!(!external.contains("Created by Rhythm"));
    assert!(!external.contains("Adopted by Rhythm"));
    assert!(external.contains("needs attention"));
}

#[test]
fn refresh_replaces_the_stale_status_in_the_window() {
    let window = replay("stale", 693, 43, "0.7");
    assert!(window.contains("Routines refreshed just now"));
    assert!(!window.contains("Couldn’t refresh routines"));
}

#[test]
fn upgrade_button_reports_progress_and_the_new_version_in_the_demo_window() {
    let progress = replay("release-available", 644, 110, "1.0");
    assert!(progress.contains("Verifying the signed release…"));
    let updated = replay("release-available", 644, 110, "3.8");
    assert!(updated.contains("Rhythm 0.3.1"));
    assert!(!updated.contains("Upgrade and reopen"));
}

fn replay(scene: &str, x: u32, y: u32, end: &str) -> String {
    let scratch = tempfile::tempdir().unwrap();
    let script = scratch.path().join("refresh.json");
    let tree = scratch.path().join("window.json");
    fs::write(
        &script,
        format!(
            r#"[{{"t":0.3,"type":"tap","x":{x},"y":{y}}},{{"t":{end},"type":"mark","name":"end"}}]"#
        ),
    )
    .unwrap();
    let result = Command::new(APP)
        .args([
            "--scene", scene, "--theme", "dark", "--size", "750x462", "--at", end, "--script",
        ])
        .arg(script)
        .arg("--tree")
        .arg(&tree)
        .output()
        .unwrap();
    assert!(result.status.success());
    fs::read_to_string(tree).unwrap()
}

fn compare(scratch: &Path, scene: &str, mode: &str, size: &str) {
    let name = format!("{scene}-{mode}-{size}.png");
    let actual = scratch.join(&name);
    let expected = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(&name);
    let result = Command::new(APP)
        .arg("--snap")
        .arg(&actual)
        .args([
            "--scene", scene, "--theme", mode, "--size", size, "--scale", "1",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "Could not render {name}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    if std::env::var_os("RHYTHM_BLESS_GOLDENS").is_some() {
        fs::create_dir_all(expected.parent().unwrap()).unwrap();
        fs::copy(actual, expected).unwrap();
        return;
    }
    let (metadata, pixels) = png(&actual);
    let (expected_metadata, expected_pixels) = png(&expected);
    assert_eq!(metadata, expected_metadata, "{name}");
    let delta = pixels
        .iter()
        .zip(expected_pixels)
        .map(|(a, b)| a.abs_diff(b))
        .max()
        .unwrap_or(0);
    assert!(
        delta <= 8,
        "{name} differs by {delta} in at least one channel"
    );
}

fn png(path: &Path) -> ((u32, u32, png::ColorType, png::BitDepth), Vec<u8>) {
    let file = fs::File::open(path).unwrap_or_else(|_| panic!("Missing {}", path.display()));
    let mut reader = png::Decoder::new(BufReader::new(file)).read_info().unwrap();
    let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut bytes).unwrap();
    bytes.truncate(info.buffer_size());
    (
        (info.width, info.height, info.color_type, info.bit_depth),
        bytes,
    )
}
