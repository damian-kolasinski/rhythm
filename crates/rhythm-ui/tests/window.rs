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
    "flagged",
    "skipped",
    "problems",
    "problems-scroll",
    "problems-unreadable",
    "actions",
    "remove-script",
    "remove-running",
    "remove-failed",
    "removed",
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
    compare(scratch.path(), "problems-scroll", "dark", "630x330");
    compare(scratch.path(), "remove-failed", "dark", "630x330");
}

const ARCHIVE: &str = "UserAgent/0/dev.rhythm.archive-samples";
const SCRIPT: &str = "/Users/example/scripts/archive-samples.sh";

fn has_key(window: &str, key: &str) -> bool {
    tree(window)["elements"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["key"] == key)
}

#[test]
fn removing_a_routine_warns_about_its_script_then_hides_the_row() {
    let more = format!(r#"{{"t":0.3,"type":"tap","key":"{ARCHIVE}/more"}}"#);
    let item = format!(r#"{{"t":0.6,"type":"tap","key":"{ARCHIVE}/actions-item0"}}"#);
    let confirm = r#"{"t":1.0,"type":"tap","key":"remove-confirm"}"#;

    let asked = script("routines", &[&more, &item], "0.9");
    assert!(asked.contains("Remove Archive samples?"));
    assert!(asked.contains("This routine runs a script"));
    assert!(asked.contains(SCRIPT));
    assert!(has_key(&asked, "reveal-script"));

    let removing = script("routines", &[&more, &item, confirm], "1.3");
    assert!(removing.contains("Removing…"));
    assert!(
        has_key(&removing, ARCHIVE),
        "the row stays until it is gone"
    );

    let removed = script("routines", &[&more, &item, confirm], "2.4");
    assert!(!has_key(&removed, ARCHIVE));
    assert!(!removed.contains("Remove Archive samples?"));
    assert!(removed.contains("Removed Archive samples"));
    assert!(removed.contains(SCRIPT));
    assert!(has_key(&removed, "removed-reveal"));
    assert!(removed.contains("3 on this Mac"));

    let dismiss = r#"{"t":2.5,"type":"tap","key":"removed-dismiss"}"#;
    let dismissed = script("routines", &[&more, &item, confirm, dismiss], "2.9");
    assert!(!dismissed.contains("Removed Archive samples"));
}

#[test]
fn cancelling_keeps_the_routine() {
    let open = format!(r#"{{"t":0.3,"type":"tap","button":"secondary","key":"{ARCHIVE}"}}"#);
    let item = format!(r#"{{"t":0.6,"type":"tap","key":"{ARCHIVE}/actions-item0"}}"#);
    for close in [
        r#"{"t":1.0,"type":"tap","key":"remove-cancel"}"#,
        r#"{"t":1.0,"type":"key","name":"Escape"}"#,
    ] {
        let window = script("routines", &[&open, &item, close], "1.6");
        assert!(!window.contains("Remove Archive samples?"));
        assert!(has_key(&window, ARCHIVE));
        assert!(!window.contains("Removed Archive samples"));
    }
}

#[test]
fn only_routines_rhythm_records_offer_removal() {
    let window = script("routines", &[], "0.5");
    assert!(has_key(&window, &format!("{ARCHIVE}/more")));
    assert!(has_key(
        &window,
        "UserAgent/0/com.example.index-samples/more"
    ));
    assert!(!has_key(&window, "Daemon/0/com.example.sync-catalog/more"));

    let external =
        r#"{"t":0.3,"type":"tap","button":"secondary","key":"Daemon/0/com.example.sync-catalog"}"#;
    assert!(!script("routines", &[external], "0.8").contains("Remove…"));
    let recorded = format!(r#"{{"t":0.3,"type":"tap","button":"secondary","key":"{ARCHIVE}"}}"#);
    assert!(script("routines", &[&recorded], "0.8").contains("Remove…"));
}

#[test]
fn removing_a_running_routine_says_it_stops_the_run() {
    let window = script("remove-running", &[], "0.5");
    assert!(window.contains("It is running now; removing it stops that run."));
    assert!(!window.contains("This routine runs a script"));
}

const FLAGGED: &str = "UserAgent/0/dev.rhythm.check-links";
const PROBLEM: &str = "docs/setup links to a removed page";

#[test]
fn tapping_a_routine_with_problems_opens_them_and_escape_closes_them() {
    let tap = format!(r#"{{"t":0.3,"type":"tap","key":"{FLAGGED}"}}"#);
    let open = script("flagged", &[&tap], "1.0");
    assert!(open.contains(PROBLEM));
    assert!(open.contains("blog/archive takes 14s to load"));
    assert!(open.contains("Open link"));

    let escape = r#"{"t":1.0,"type":"key","name":"Escape"}"#;
    let closed = script("flagged", &[&tap, escape], "1.8");
    assert!(!closed.contains(PROBLEM));
    assert!(closed.contains("2 problems"));

    let outside = r#"{"t":1.0,"type":"tap","x":40,"y":440}"#;
    assert!(!script("flagged", &[&tap, outside], "1.8").contains(PROBLEM));
}

#[test]
fn only_a_routine_with_problems_can_be_pressed() {
    let window = tree(&script("flagged", &[], "0.5"));
    let role = |key: &str| element(&window, |e| e["key"] == key)["role"].clone();
    assert_eq!(role(FLAGGED), "button");
    assert_eq!(role("UserAgent/0/dev.rhythm.archive-samples"), "group");
    assert_eq!(role("Daemon/0/com.example.sync-catalog"), "group");

    let tap = r#"{"t":0.3,"type":"tap","key":"UserAgent/0/dev.rhythm.archive-samples"}"#;
    let window = script("flagged", &[tap], "1.0");
    assert!(!window.contains("problems-dialog"));
}

#[test]
fn needs_attention_is_red_only_while_a_routine_is_broken() {
    let color = |window: &str| {
        element(&tree(window), |e| {
            e["text"].as_str().is_some_and(|t| t.ends_with("attention"))
        })["color"]
            .clone()
    };
    // The dark theme's danger and warning foregrounds.
    let all = script("flagged", &[], "0.7");
    assert!(all.contains("2 need attention"));
    assert_eq!(color(&all), "#ffb3b3ff");
    let rhythm = replay("flagged", 137, 94, "0.7");
    assert!(rhythm.contains("1 needs attention"));
    assert_eq!(color(&rhythm), "#f2c055ff");
}

#[test]
fn a_long_list_of_problems_scrolls_to_its_end() {
    let bottoms = |events: &[&str]| {
        let window = tree(&script("problems-scroll", events, "1.5"));
        let bottom = |key: &str| {
            let rect = &element(&window, |e| e["key"] == key)["rect"];
            rect[1].as_f64().unwrap() + rect[3].as_f64().unwrap()
        };
        (bottom("unlisted"), bottom("problems-dialog"))
    };
    let (end, dialog) = bottoms(&[]);
    assert!(end > dialog, "the end of the list starts out of view");
    let wheel = r#"{"t":0.3,"type":"wheel","key":"problem-list","dy":-100000}"#;
    let (end, dialog) = bottoms(&[wheel]);
    assert!(end <= dialog, "the end of the list scrolls into view");
}

fn tree(window: &str) -> serde_json::Value {
    serde_json::from_str(window).unwrap()
}

fn element(
    tree: &serde_json::Value,
    matches: impl Fn(&serde_json::Value) -> bool,
) -> &serde_json::Value {
    tree["elements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| matches(e))
        .unwrap()
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
    script(
        scene,
        &[&format!(r#"{{"t":0.3,"type":"tap","x":{x},"y":{y}}}"#)],
        end,
    )
}

/// The window's tree after `events`, as of `end` seconds.
fn script(scene: &str, events: &[&str], end: &str) -> String {
    let scratch = tempfile::tempdir().unwrap();
    let script = scratch.path().join("events.json");
    let tree = scratch.path().join("window.json");
    let mut events = events.to_vec();
    let mark = format!(r#"{{"t":{end},"type":"mark","name":"end"}}"#);
    events.push(&mark);
    fs::write(&script, format!("[{}]", events.join(","))).unwrap();
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
