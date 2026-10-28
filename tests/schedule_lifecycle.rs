use rhythm::{
    execution_host::ExecutionHost,
    execution_records::*,
    operation_error::{Error, Result},
    private_storage::Root,
    schedule_store::SqliteScheduleStore,
    schedules::{self, *},
};
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};
use tempfile::TempDir;

#[derive(Clone)]
struct Time(Rc<Cell<i64>>);
impl Clock for Time {
    fn now_ms(&self) -> i64 {
        self.0.get()
    }
}
#[derive(Clone, Default)]
struct Host(Rc<RefCell<ExternalProcesses>>);
#[derive(Default)]
struct ExternalProcesses {
    starts: Vec<i64>,
    completions: BTreeMap<i64, Outcome>,
    stops: Vec<i64>,
}
impl ExecutionHost for Host {
    fn start(&mut self, id: i64, _: &rhythm::execution_command::CommandSpec) -> Result<u32> {
        self.0.borrow_mut().starts.push(id);
        Ok(100)
    }
    fn stop(&mut self, id: i64) -> Result<()> {
        let mut p = self.0.borrow_mut();
        p.stops.push(id);
        p.completions.insert(id, Outcome::Stopped);
        Ok(())
    }
    fn poll(&mut self, id: i64) -> Result<Option<Completion>> {
        Ok(self
            .0
            .borrow_mut()
            .completions
            .remove(&id)
            .map(|outcome| Completion {
                outcome,
                finished_at_ms: timestamp_ms(),
            }))
    }
    fn recover(&mut self, e: &Execution) -> Result<Recovery> {
        Ok(match self.poll(e.id)? {
            Some(c) => Recovery::Completed(c),
            None => Recovery::Waiting,
        })
    }
    fn logs(&self, _: i64) -> Result<(String, String)> {
        Ok((String::new(), String::new()))
    }
    fn release(&mut self) {}
}
struct Fixture {
    _temp: TempDir,
    root: Root,
    time: Time,
    host: Host,
}
type Scheduler = schedules::Manager<SqliteScheduleStore, Host, Time>;
impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let root = Root::open(temp.path().join("state")).unwrap();
        Self {
            _temp: temp,
            root,
            time: Time(Rc::new(Cell::new(0))),
            host: Host::default(),
        }
    }
    fn open(&self) -> Scheduler {
        schedules::Manager::new(
            SqliteScheduleStore::open(&self.root).unwrap(),
            self.host.clone(),
            self.time.clone(),
        )
        .unwrap()
    }
    fn at(&self, now: i64) {
        self.time.0.set(now)
    }
    fn create(&self, m: &mut Scheduler, id: &str, overlap: &str, missed: &str, grace: i64) {
        let definition=serde_json::from_value(json!({"id":id,"command":["/usr/bin/true"],"working_directory":"/tmp","trigger":{"type":"interval","every_ms":1000},"overlap":overlap,"missed":missed,"grace_ms":grace})).unwrap();
        m.request(Request::Create { definition }).unwrap();
    }
    fn complete(&self, run: i64, outcome: Outcome) {
        self.host.0.borrow_mut().completions.insert(run, outcome);
    }
}
fn id(s: &str) -> ScheduleId {
    s.parse().unwrap()
}
fn view(m: &mut Scheduler, s: &str) -> View {
    match m.request(Request::Inspect { id: id(s) }).unwrap() {
        Response::Schedule { schedule, .. } => *schedule,
        _ => panic!("view"),
    }
}
fn history(m: &mut Scheduler, s: &str) -> Vec<Execution> {
    match m.request(Request::History { id: id(s) }).unwrap() {
        Response::History { executions } => executions,
        _ => panic!("history"),
    }
}
fn invocation(run: &Execution) -> Invocation {
    run.invocation
}

#[test]
fn due_once_backward_clock_and_overlap_skip_preserve_future_cursor() {
    let f = Fixture::new();
    let mut m = f.open();
    f.create(&mut m, "sample-skip", "skip", "skip", 50);
    assert!(!view(&mut m, "sample-skip").enabled);
    m.request(Request::Resume {
        id: id("sample-skip"),
    })
    .unwrap();
    f.at(999);
    m.tick().unwrap();
    assert!(history(&mut m, "sample-skip").is_empty());
    f.at(1000);
    m.tick().unwrap();
    m.tick().unwrap();
    let first = history(&mut m, "sample-skip")[0].id;
    f.at(0);
    m.tick().unwrap();
    assert_eq!(history(&mut m, "sample-skip").len(), 1);
    f.at(5000);
    m.tick().unwrap();
    let s = view(&mut m, "sample-skip");
    assert!(s.pending.is_none());
    assert_eq!(s.next_run_at_ms, Some(6000));
    assert_eq!(s.last_decision.unwrap().action, Action::SkippedOverlap);
    f.complete(first, Outcome::Exited { code: 0 });
    m.tick().unwrap();
    f.at(6000);
    m.tick().unwrap();
    assert_eq!(history(&mut m, "sample-skip").len(), 2);
}

#[test]
fn latest_single_queued_occurrence_survives_reopen_and_cannot_overlap() {
    let f = Fixture::new();
    let mut m = f.open();
    f.create(&mut m, "sample-queue", "queue_one", "run_once", 0);
    m.request(Request::Resume {
        id: id("sample-queue"),
    })
    .unwrap();
    f.at(1000);
    m.tick().unwrap();
    let first = history(&mut m, "sample-queue")[0].id;
    f.at(2000);
    m.tick().unwrap();
    f.at(9500);
    m.tick().unwrap();
    assert_eq!(
        view(&mut m, "sample-queue").pending,
        Some(Invocation::Scheduled {
            scheduled_for_ms: 9000
        })
    );
    assert_eq!(history(&mut m, "sample-queue").len(), 1);
    drop(m);
    let mut m = f.open();
    m.tick().unwrap();
    assert_eq!(history(&mut m, "sample-queue").len(), 1);
    f.complete(first, Outcome::Interrupted);
    m.tick().unwrap();
    let runs = history(&mut m, "sample-queue");
    assert_eq!(runs.len(), 2);
    assert_eq!(
        invocation(&runs[0]),
        Invocation::Scheduled {
            scheduled_for_ms: 9000
        }
    );
    assert!(view(&mut m, "sample-queue").pending.is_none());
}

#[test]
fn missed_run_policy_and_durable_manual_handoff_do_not_replay_on_reopen() {
    for (policy, count) in [("skip", 0), ("run_once", 1)] {
        let f = Fixture::new();
        let mut m = f.open();
        f.create(&mut m, "sample-missed", "skip", policy, 50);
        m.request(Request::Resume {
            id: id("sample-missed"),
        })
        .unwrap();
        drop(m);
        f.at(3500);
        let mut m = f.open();
        m.tick().unwrap();
        let runs = history(&mut m, "sample-missed");
        assert_eq!(runs.len(), count);
        assert_eq!(view(&mut m, "sample-missed").next_run_at_ms, Some(4000));
        if count == 1 {
            assert_eq!(
                invocation(&runs[0]),
                Invocation::Scheduled {
                    scheduled_for_ms: 3000
                }
            );
            f.complete(runs[0].id, Outcome::Exited { code: 0 });
            m.tick().unwrap();
            drop(m);
            let mut m = f.open();
            m.tick().unwrap();
            assert_eq!(history(&mut m, "sample-missed").len(), 1);
        } else {
            assert_eq!(
                view(&mut m, "sample-missed").last_decision.unwrap().action,
                Action::SkippedMissed
            );
        }
    }
    let f = Fixture::new();
    let mut m = f.open();
    f.create(&mut m, "sample-manual", "skip", "skip", 0);
    m.request(Request::RunNow {
        id: id("sample-manual"),
    })
    .unwrap();
    drop(m);
    let mut m = f.open();
    m.tick().unwrap();
    assert_eq!(
        invocation(&history(&mut m, "sample-manual")[0]),
        Invocation::Manual { requested_at_ms: 0 }
    );
    assert!(matches!(
        m.request(Request::RunNow {
            id: id("sample-manual")
        }),
        Err(Error::Busy)
    ));
}

#[test]
fn manual_queue_is_protected_pause_suppresses_triggers_and_cancel_clears_work() {
    let f = Fixture::new();
    let mut m = f.open();
    f.create(&mut m, "sample-manual", "queue_one", "run_once", 0);
    m.request(Request::Resume {
        id: id("sample-manual"),
    })
    .unwrap();
    f.at(1000);
    m.tick().unwrap();
    f.at(1200);
    m.request(Request::RunNow {
        id: id("sample-manual"),
    })
    .unwrap();
    assert!(matches!(
        m.request(Request::RunNow {
            id: id("sample-manual")
        }),
        Err(Error::Busy)
    ));
    f.at(2000);
    m.tick().unwrap();
    assert_eq!(
        view(&mut m, "sample-manual").pending,
        Some(Invocation::Manual {
            requested_at_ms: 1200
        })
    );
    m.request(Request::Pause {
        id: id("sample-manual"),
    })
    .unwrap();
    assert!(view(&mut m, "sample-manual").pending.is_some());
    m.request(Request::Cancel {
        id: id("sample-manual"),
    })
    .unwrap();
    m.tick().unwrap();
    assert!(view(&mut m, "sample-manual").pending.is_none());
    f.at(5000);
    m.tick().unwrap();
    assert_eq!(history(&mut m, "sample-manual").len(), 1);
    m.request(Request::Resume {
        id: id("sample-manual"),
    })
    .unwrap();
    assert_eq!(view(&mut m, "sample-manual").next_run_at_ms, Some(6000));
    f.at(5500);
    m.request(Request::Resume {
        id: id("sample-manual"),
    })
    .unwrap();
    assert_eq!(view(&mut m, "sample-manual").next_run_at_ms, Some(6000));
}

#[test]
fn uncertain_outcome_blocks_pending_and_future_runs_until_specific_acknowledgment() {
    let f = Fixture::new();
    let mut m = f.open();
    f.create(&mut m, "sample-uncertain", "queue_one", "run_once", 0);
    m.request(Request::Resume {
        id: id("sample-uncertain"),
    })
    .unwrap();
    f.at(1000);
    m.tick().unwrap();
    let first = history(&mut m, "sample-uncertain")[0].id;
    f.at(2000);
    m.tick().unwrap();
    f.complete(first, Outcome::MonitorLost);
    m.tick().unwrap();
    assert_eq!(history(&mut m, "sample-uncertain").len(), 1);
    drop(m);
    f.at(10000);
    let mut m = f.open();
    m.tick().unwrap();
    assert_eq!(history(&mut m, "sample-uncertain").len(), 1);
    assert!(matches!(
        view(&mut m, "sample-uncertain").observed,
        Observed::NeedsAttention
    ));
    assert!(matches!(
        m.request(Request::RunNow {
            id: id("sample-uncertain")
        }),
        Err(Error::RecoveryRequired)
    ));
    assert!(matches!(
        m.request(Request::Acknowledge {
            id: id("sample-uncertain"),
            execution_id: first + 1
        }),
        Err(Error::RecoveryRequired)
    ));
    m.request(Request::Acknowledge {
        id: id("sample-uncertain"),
        execution_id: first,
    })
    .unwrap();
    let s = view(&mut m, "sample-uncertain");
    assert!(!s.enabled);
    assert!(s.pending.is_none());
    assert_eq!(
        s.latest_execution.unwrap().outcome,
        Some(Outcome::MonitorLost)
    );
    m.request(Request::RunNow {
        id: id("sample-uncertain"),
    })
    .unwrap();
    m.tick().unwrap();
    assert_eq!(history(&mut m, "sample-uncertain").len(), 2);
}
