#![cfg(target_os = "macos")]

use std::{fs, io::BufReader, path::Path, process::Command};

const APP: &str = env!("CARGO_BIN_EXE_rhythm-ui");
const SCENES: &[&str] = &["loading", "empty", "routines", "stale", "unavailable"];

#[test]
fn scenes_match_goldens() {
    let scratch = tempfile::tempdir().unwrap();
    for scene in SCENES {
        for mode in ["light", "dark"] {
            compare(scratch.path(), scene, mode, "750x462");
        }
    }
    compare(scratch.path(), "stale", "light", "630x330");
}

#[test]
fn refresh_replaces_the_stale_status_in_the_window() {
    let scratch = tempfile::tempdir().unwrap();
    let script = scratch.path().join("refresh.json");
    let tree = scratch.path().join("window.json");
    fs::write(
        &script,
        r#"[{"at":0.3,"type":"tap","x":693,"y":43},{"at":0.7,"type":"mark","name":"end"}]"#,
    )
    .unwrap();
    let result = Command::new(APP)
        .args([
            "--scene", "stale", "--theme", "dark", "--size", "750x462", "--script",
        ])
        .arg(script)
        .arg("--tree")
        .arg(&tree)
        .output()
        .unwrap();
    assert!(result.status.success());
    let window = fs::read_to_string(tree).unwrap();
    assert!(window.contains("Routines refreshed just now"));
    assert!(!window.contains("Couldn’t refresh routines"));
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
