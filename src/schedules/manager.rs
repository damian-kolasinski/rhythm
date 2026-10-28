use super::*;
use crate::{
    execution_host::ExecutionHost,
    execution_records::*,
    operation_error::{Error, Result},
};
use std::collections::BTreeMap;

pub struct Manager<S, H, C> {
    store: S,
    host: H,
    clock: C,
    active: BTreeMap<ScheduleId, ExecutionId>,
    recovering: BTreeMap<ScheduleId, Execution>,
    shutdown: bool,
}
impl<S: ScheduleStore, H: ExecutionHost, C: Clock> Manager<S, H, C> {
    pub fn new(store: S, host: H, clock: C) -> Result<Self> {
        let recovering = store
            .active()?
            .into_iter()
            .map(|e| (e.schedule_id.clone(), e))
            .collect();
        Ok(Self {
            store,
            host,
            clock,
            active: BTreeMap::new(),
            recovering,
            shutdown: false,
        })
    }
    pub fn shutdown(&mut self) {
        self.shutdown = true;
        self.host.release()
    }
    pub fn quiescent(&self) -> bool {
        self.active.is_empty() && self.recovering.is_empty()
    }
    fn latest(&self, id: &ScheduleId) -> Result<Option<Execution>> {
        Ok(self.store.history(id)?.into_iter().next())
    }
    fn attention(&self, id: &ScheduleId) -> Result<bool> {
        Ok(self
            .latest(id)?
            .as_ref()
            .is_some_and(Execution::needs_attention))
    }
    fn view(&self, s: &Stored) -> Result<View> {
        let id = s.definition.id();
        let latest = self.latest(id)?;
        let observed = if let Some(execution_id) = self.active.get(id) {
            Observed::Running {
                execution_id: *execution_id,
            }
        } else if let Some(e) = self.recovering.get(id) {
            Observed::Recovering { execution_id: e.id }
        } else if latest.as_ref().is_some_and(Execution::needs_attention) {
            Observed::NeedsAttention
        } else {
            Observed::Idle
        };
        Ok(View {
            id: id.clone(),
            enabled: s.enabled,
            next_run_at_ms: if s.enabled && !matches!(observed, Observed::NeedsAttention) {
                Some(s.next_due_ms)
            } else {
                None
            },
            pending: s.pending,
            last_decision: s.last_decision.clone(),
            observed,
            latest_execution: latest,
        })
    }
    fn inspect(&self, id: &ScheduleId) -> Result<Response> {
        let stored = self.store.get(id)?;
        Ok(Response::Schedule {
            schedule: Box::new(self.view(&stored)?),
            definition: stored.definition,
        })
    }
    pub fn request(&mut self, request: Request) -> Result<Response> {
        let now = self.clock.now_ms();
        match request {
            Request::Create { definition } => {
                let next_due_ms = definition.trigger().next_after(now)?;
                let id = definition.id().clone();
                self.store.create(&Stored {
                    definition,
                    enabled: false,
                    next_due_ms,
                    pending: None,
                    last_decision: None,
                })?;
                Ok(Response::Created { id })
            }
            Request::List => Ok(Response::Schedules {
                schedules: self
                    .store
                    .list()?
                    .iter()
                    .map(|s| self.view(s))
                    .collect::<Result<_>>()?,
            }),
            Request::Inspect { id } => self.inspect(&id),
            Request::History { id } => {
                self.store.get(&id)?;
                Ok(Response::History {
                    executions: self.store.history(&id)?,
                })
            }
            Request::Pause { id } => {
                let mut s = self.store.get(&id)?;
                s.enabled = false;
                if matches!(s.pending, Some(Invocation::Scheduled { .. })) {
                    s.pending = None;
                }
                self.store.save(&s)?;
                self.inspect(&id)
            }
            Request::Resume { id } => {
                let mut s = self.store.get(&id)?;
                if self.attention(&id)? {
                    return Err(Error::RecoveryRequired);
                }
                if !s.enabled {
                    s.next_due_ms = s.definition.trigger().next_after(now)?;
                    s.enabled = true;
                    self.store.save(&s)?;
                }
                self.inspect(&id)
            }
            Request::RunNow { id } => {
                let mut s = self.store.get(&id)?;
                if self.attention(&id)? || self.recovering.contains_key(&id) {
                    return Err(Error::RecoveryRequired);
                }
                if s.pending.is_some()
                    || (self.active.contains_key(&id) && s.definition.overlap() == Overlap::Skip)
                {
                    return Err(Error::Busy);
                }
                s.pending = Some(Invocation::Manual {
                    requested_at_ms: now,
                });
                self.store.save(&s)?;
                self.inspect(&id)
            }
            Request::Cancel { id } => {
                let mut s = self.store.get(&id)?;
                if self.recovering.contains_key(&id) {
                    return Err(Error::RecoveryRequired);
                }
                s.pending = None;
                self.store.save(&s)?;
                if let Some(run) = self.active.get(&id) {
                    self.host.stop(*run)?;
                }
                self.inspect(&id)
            }
            Request::Acknowledge { id, execution_id } => {
                self.store.get(&id)?;
                if self.active.contains_key(&id)
                    || self.recovering.contains_key(&id)
                    || !self
                        .latest(&id)?
                        .is_some_and(|e| e.id == execution_id && e.needs_attention())
                {
                    return Err(Error::RecoveryRequired);
                }
                self.store.acknowledge(execution_id, now)?;
                self.inspect(&id)
            }
        }
    }
    pub fn tick(&mut self) -> Result<()> {
        let now = self.clock.now_ms();
        for (id, e) in self.recovering.clone() {
            match self.host.recover(&e)? {
                Recovery::Waiting => {}
                Recovery::NeverAttempted => {
                    if self.shutdown {
                        continue;
                    }
                    let s = self.store.get(&id)?;
                    match self.host.start(e.id, s.definition.command()) {
                        Ok(pid) => {
                            self.store.started(e.id, pid)?;
                            self.active.insert(id.clone(), e.id);
                        }
                        Err(Error::ExecutionUnavailable) => self.store.finish(
                            e.id,
                            &Completion {
                                outcome: Outcome::SpawnFailed,
                                finished_at_ms: now,
                            },
                        )?,
                        Err(e) => return Err(e),
                    }
                    self.recovering.remove(&id);
                }
                Recovery::Completed(completion) => {
                    self.store.finish(e.id, &completion)?;
                    self.recovering.remove(&id);
                }
            }
        }
        for (id, run) in self.active.clone() {
            if let Some(completion) = self.host.poll(run)? {
                self.store.finish(run, &completion)?;
                self.active.remove(&id);
            }
        }
        if self.shutdown {
            return Ok(());
        }
        for mut s in self.store.list()? {
            let id = s.definition.id().clone();
            if self.attention(&id)? {
                continue;
            }
            if s.enabled && s.next_due_ms <= now {
                let (due, next) = s.definition.trigger().due_window(s.next_due_ms, now)?;
                s.next_due_ms = next;
                let action = if now - due > s.definition.grace_ms()
                    && s.definition.missed() == Missed::Skip
                {
                    Action::SkippedMissed
                } else if (self.active.contains_key(&id)
                    || self.recovering.contains_key(&id)
                    || s.pending.is_some())
                    && (s.definition.overlap() == Overlap::Skip
                        || matches!(s.pending, Some(Invocation::Manual { .. })))
                {
                    Action::SkippedOverlap
                } else {
                    s.pending = Some(Invocation::Scheduled {
                        scheduled_for_ms: due,
                    });
                    Action::Queued
                };
                s.last_decision = Some(Decision {
                    scheduled_for_ms: due,
                    decided_at_ms: now,
                    action,
                });
                self.store.save(&s)?;
            }
            if s.pending.is_none()
                || self.active.contains_key(&id)
                || self.recovering.contains_key(&id)
            {
                continue;
            }
            let run = self.store.begin(&id, now)?;
            match self.host.start(run, s.definition.command()) {
                Ok(pid) => {
                    self.store.started(run, pid)?;
                    self.active.insert(id, run);
                }
                Err(Error::ExecutionUnavailable) => self.store.finish(
                    run,
                    &Completion {
                        outcome: Outcome::SpawnFailed,
                        finished_at_ms: now,
                    },
                )?,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}
