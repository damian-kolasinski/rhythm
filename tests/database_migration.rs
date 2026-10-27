use rhythm::{
    private_storage::{Root, private_file},
    schedule_store::SqliteScheduleStore,
    service_store::SqliteStore,
    services::*,
};
use rusqlite::{Connection, params};
use serde_json::json;
use tempfile::TempDir;

#[test]
fn upgrades_service_schema_without_losing_history_intent_or_execution_sequence() {
    let temp = TempDir::new().unwrap();
    let root = Root::open(temp.path().join("state")).unwrap();
    private_file(&root.path().join("state.sqlite")).unwrap();
    let connection = Connection::open(root.path().join("state.sqlite")).unwrap();
    connection.execute_batch("CREATE TABLE services(id TEXT PRIMARY KEY,definition TEXT NOT NULL,desired TEXT NOT NULL,pending_start INTEGER NOT NULL DEFAULT 0);
    CREATE TABLE executions(id INTEGER PRIMARY KEY AUTOINCREMENT,service_id TEXT NOT NULL REFERENCES services(id),started_at_ms INTEGER NOT NULL,finished_at_ms INTEGER,outcome TEXT,supervisor_pid INTEGER,recovery_acknowledged_at_ms INTEGER);
    CREATE UNIQUE INDEX one_active_execution ON executions(service_id) WHERE finished_at_ms IS NULL;
    PRAGMA user_version=1;").unwrap();
    let definition = json!({"id":"sample-existing","command":["/usr/bin/true"],"working_directory":"/tmp","restart":"never"});
    connection
        .execute(
            "INSERT INTO services VALUES(?1,?2,'running',1)",
            params!["sample-existing", definition.to_string()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO executions VALUES(7,'sample-existing',1000,2000,?1,NULL,2500)",
            [json!({"type":"monitor_lost"}).to_string()],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE sqlite_sequence SET seq=99 WHERE name='executions'",
            [],
        )
        .unwrap();
    drop(connection);
    let mut store = SqliteStore::open(&root).unwrap();
    let id = "sample-existing".parse().unwrap();
    assert!(store.get(&id).unwrap().pending_start);
    let history = store.history(&id).unwrap();
    assert_eq!(history[0].id, 7);
    assert_eq!(history[0].outcome, Some(Outcome::MonitorLost));
    assert_eq!(history[0].recovery_acknowledged_at_ms, Some(2500));
    assert_eq!(store.begin(&id, 3000).unwrap(), 100);
    assert!(SqliteScheduleStore::open(&root).is_ok());
    assert_eq!(
        Connection::open(root.path().join("state.sqlite"))
            .unwrap()
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn unsupported_future_schema_is_not_modified() {
    let temp = TempDir::new().unwrap();
    let root = Root::open(temp.path().join("state")).unwrap();
    private_file(&root.path().join("state.sqlite")).unwrap();
    let connection = Connection::open(root.path().join("state.sqlite")).unwrap();
    connection.execute_batch("PRAGMA user_version=99;").unwrap();
    assert!(matches!(
        SqliteStore::open(&root),
        Err(Error::StorageUnavailable)
    ));
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        99
    );
}
