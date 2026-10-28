use rhythm::{
    execution_records::Invocation,
    operation_error::Error,
    private_storage::{Root, private_file},
    schedule_store::SqliteScheduleStore,
    schedules::{Definition, ScheduleStore, Stored},
};
use rusqlite::Connection;
use serde_json::json;
use tempfile::TempDir;

fn fixture() -> (TempDir, Root) {
    let temp = TempDir::new().unwrap();
    let root = Root::open(temp.path().join("state")).unwrap();
    (temp, root)
}

#[test]
fn fresh_storage_contains_only_schedule_tables_and_reopens_with_history() {
    let (_temp, root) = fixture();
    let mut store = SqliteScheduleStore::open(&root).unwrap();
    let definition: Definition = serde_json::from_value(json!({"id":"sample-report","command":["/usr/bin/true"],"working_directory":"/tmp","trigger":{"type":"interval","every_ms":1000},"overlap":"skip","missed":"skip"})).unwrap();
    let id = definition.id().clone();
    store
        .create(&Stored {
            definition,
            enabled: false,
            next_due_ms: 2000,
            pending: None,
            last_decision: None,
        })
        .unwrap();
    let mut schedule = store.get(&id).unwrap();
    schedule.pending = Some(Invocation::Manual {
        requested_at_ms: 1000,
    });
    store.save(&schedule).unwrap();
    let run = store.begin(&id, 1000).unwrap();
    drop(store);
    let store = SqliteScheduleStore::open(&root).unwrap();
    let records = store.history(&id).unwrap();
    assert_eq!(records[0].id, run);
    assert_eq!(records[0].schedule_id, id);
    assert_eq!(
        records[0].invocation,
        Invocation::Manual {
            requested_at_ms: 1000
        }
    );
    let connection = Connection::open(root.path().join("state.sqlite")).unwrap();
    let tables: Vec<String> = connection
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(tables, ["executions", "schedules", "sqlite_sequence"]);
    let columns: Vec<String> = connection
        .prepare("PRAGMA table_info(executions)")
        .unwrap()
        .query_map([], |r| r.get(1))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(!columns.iter().any(|name| name == "service_id"));
    assert!(
        connection
            .execute(
                "INSERT INTO executions(schedule_id,invocation,started_at_ms) VALUES(NULL,'{}',0)",
                []
            )
            .is_err()
    );
    assert!(connection.execute("INSERT INTO executions(schedule_id,invocation,started_at_ms) VALUES('sample-report',NULL,0)", []).is_err());
    assert!(connection.execute("INSERT INTO executions(schedule_id,invocation,started_at_ms) VALUES('sample-report','{}',0)", []).is_err());
}

#[test]
fn other_schema_versions_are_refused_without_mutating_the_database() {
    for version in [1, 2, 99] {
        let (_temp, root) = fixture();
        private_file(&root.path().join("state.sqlite")).unwrap();
        let connection = Connection::open(root.path().join("state.sqlite")).unwrap();
        connection.execute_batch(&format!("CREATE TABLE fixture(value TEXT); INSERT INTO fixture VALUES('synthetic'); PRAGMA user_version={version};")).unwrap();
        assert!(matches!(
            SqliteScheduleStore::open(&root),
            Err(Error::UnsupportedSchema)
        ));
        assert_eq!(
            connection
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            version
        );
        assert_eq!(
            connection
                .query_row("SELECT value FROM fixture", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "synthetic"
        );
    }
}
