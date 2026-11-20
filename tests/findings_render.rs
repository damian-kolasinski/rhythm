//! Golden tests for `rhythm findings` output. Set UPDATE_GOLDEN=1 to rewrite them.
use rhythm::{findings_render::render, open_findings::FindingsResponse, runs::Timestamp};
use serde_json::json;
use std::path::Path;

const NOW: Timestamp = Timestamp(1_790_000_000_000);

fn golden(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/golden/{name}.txt"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, actual).unwrap();
    }
    assert_eq!(actual, std::fs::read_to_string(&path).unwrap(), "{name}");
}

fn response(value: serde_json::Value) -> FindingsResponse {
    serde_json::from_value(value).unwrap()
}

#[test]
fn renders_each_open_problem_with_what_is_known_about_it() {
    let open = response(json!({
        "type": "findings",
        "label": "dev.rhythm.check",
        "run": 42,
        "reported_at": "2026-09-21T11:13:20.000Z",
        "dropped": 3,
        "findings": [
            {"key": "regression/example-module@1a2b3c4d",
             "title": "example-module: 2 tests broken by #123",
             "detail": "test_parse fails on empty input.\n\nSee the second commit.",
             "link": "https://example.com/pull/123",
             "handled_by": "the example team",
             "first_seen": "2026-09-19T09:00:00.000Z"},
            {"key": "rhythm:result",
             "title": "The job's result could not be read (invalid)",
             "first_seen": "2026-09-21T11:13:20.000Z"}
        ]
    }));
    golden("findings", &render(&open, NOW));
}

#[test]
fn says_whether_a_job_reported_nothing_or_reported_no_problems() {
    let never = response(json!({"type": "findings", "label": "com.example.scan", "findings": []}));
    let clear = response(json!({
        "type": "findings", "label": "com.example.scan", "run": 7,
        "reported_at": "2026-09-21T14:03:20.000Z", "findings": []
    }));
    golden(
        "findings-empty",
        &(render(&never, NOW) + "---\n" + &render(&clear, NOW)),
    );
}
