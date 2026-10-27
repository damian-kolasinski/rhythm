use super::*;
use std::collections::BTreeMap;

pub struct Manager<S, H> {
    store: S,
    host: H,
    active: BTreeMap<ServiceId, (ExecutionId, u32)>,
    recovering: BTreeMap<ServiceId, Execution>,
    retry: BTreeMap<ServiceId, i64>,
    shutdown: bool,
}
impl<S: ServiceStore, H: ExecutionHost> Manager<S, H> {
    pub fn new(store: S, host: H) -> Result<Self> {
        let recovering = store
            .active()?
            .into_iter()
            .map(|e| (e.owner.id().clone(), e))
            .collect();
        Ok(Self {
            store,
            host,
            active: BTreeMap::new(),
            recovering,
            retry: BTreeMap::new(),
            shutdown: false,
        })
    }
    pub fn release(&mut self) {
        self.shutdown = true;
        self.host.release()
    }
    pub fn quiescent(&self) -> bool {
        self.active.is_empty() && self.recovering.is_empty()
    }
    fn latest(&self, id: &ServiceId) -> Result<Option<Execution>> {
        Ok(self.store.history(id)?.into_iter().next())
    }
    fn view(&self, s: &StoredService) -> Result<ServiceView> {
        let id = s.definition.id();
        let latest = self.latest(id)?;
        let observed = if let Some((execution_id, pid)) = self.active.get(id) {
            Observed::Running {
                execution_id: *execution_id,
                supervisor_pid: *pid,
            }
        } else if let Some(e) = self.recovering.get(id) {
            Observed::Recovering { execution_id: e.id }
        } else if latest.as_ref().is_some_and(Execution::needs_attention) {
            Observed::NeedsAttention
        } else if let Some(retry_at_ms) = self.retry.get(id) {
            Observed::Waiting {
                retry_at_ms: *retry_at_ms,
            }
        } else if s.desired == Desired::Stopped {
            Observed::Stopped
        } else {
            Observed::Exited
        };
        Ok(ServiceView {
            id: id.clone(),
            desired: s.desired,
            observed,
            latest_execution: latest,
        })
    }
    pub fn request(&mut self, request: Request) -> Result<Response> {
        match request {
            Request::Create { definition } => {
                self.store.create(&definition)?;
                Ok(Response::Created {
                    id: definition.id().clone(),
                })
            }
            Request::List => Ok(Response::Services {
                services: self
                    .store
                    .list()?
                    .iter()
                    .map(|s| self.view(s))
                    .collect::<Result<_>>()?,
            }),
            Request::Inspect { id } => {
                let s = self.store.get(&id)?;
                Ok(Response::Service {
                    service: Box::new(self.view(&s)?),
                    definition: s.definition,
                })
            }
            Request::Start { id } => self.running(id, false),
            Request::Restart { id } => self.running(id, true),
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
                self.store.acknowledge(execution_id, timestamp_ms())?;
                self.retry.remove(&id);
                self.request(Request::Inspect { id })
            }
            Request::Stop { id } => {
                self.store.desire(&id, Desired::Stopped)?;
                self.retry.remove(&id);
                if let Some((run, _)) = self.active.get(&id) {
                    self.host.stop(*run)?;
                }
                let s = self.store.get(&id)?;
                Ok(Response::Service {
                    service: Box::new(self.view(&s)?),
                    definition: s.definition,
                })
            }
            Request::History { id } => {
                self.store.get(&id)?;
                Ok(Response::History {
                    executions: self.store.history(&id)?,
                })
            }
        }
    }
    fn running(&mut self, id: ServiceId, restart: bool) -> Result<Response> {
        let stored = self.store.get(&id)?;
        if self.recovering.contains_key(&id)
            || self.latest(&id)?.is_some_and(|e| e.needs_attention())
        {
            return Err(Error::RecoveryRequired);
        }
        if restart
            || (!self.active.contains_key(&id)
                && !(stored.desired == Desired::Running && self.retry.contains_key(&id)))
        {
            self.store.request_start(&id)?;
            self.retry.insert(id.clone(), timestamp_ms());
        } else {
            self.store.desire(&id, Desired::Running)?;
        }
        if restart && let Some((run, _)) = self.active.get(&id) {
            self.host.stop(*run)?;
        }
        Ok(Response::Service {
            service: Box::new(self.view(&self.store.get(&id)?)?),
            definition: self.store.get(&id)?.definition,
        })
    }
    pub fn tick(&mut self) -> Result<()> {
        let now = timestamp_ms();
        for (id, e) in self.recovering.clone() {
            let completion = match self.host.recover(&e)? {
                Recovery::Waiting => None,
                Recovery::NeverAttempted => Some(Completion {
                    outcome: Outcome::Interrupted,
                    finished_at_ms: now,
                }),
                Recovery::Completed(c) => Some(c),
            };
            if let Some(c) = completion {
                self.store.finish(e.id, &c)?;
                self.recovering.remove(&id);
                if c.outcome.requires_attention() {
                    self.retry.remove(&id);
                }
            }
        }
        for (id, (run, _)) in self.active.clone() {
            if let Some(c) = self.host.poll(run)? {
                self.store.finish(run, &c)?;
                self.active.remove(&id);
                if c.outcome.requires_attention() {
                    self.retry.remove(&id);
                }
                let s = self.store.get(&id)?;
                if s.desired == Desired::Running
                    && s.definition.should_restart(&c.outcome)
                    && !self.shutdown
                {
                    self.retry
                        .entry(id)
                        .or_insert(now + s.definition.restart_delay_ms() as i64);
                }
            }
        }
        if self.shutdown {
            return Ok(());
        }
        // Restore desired services after a clean runtime restart as well.
        for s in self.store.list()? {
            let id = s.definition.id();
            if s.desired == Desired::Running
                && !self.active.contains_key(id)
                && !self.recovering.contains_key(id)
            {
                let latest = self.latest(id)?;
                if latest.as_ref().is_some_and(Execution::needs_attention) {
                    self.retry.remove(id);
                    continue;
                }
                if s.pending_start
                    || latest.is_none()
                    || latest
                        .as_ref()
                        .is_some_and(|e| e.outcome == Some(Outcome::Interrupted))
                {
                    self.retry.entry(id.clone()).or_insert(now);
                } else if let Some(e) = latest
                    && let Some(outcome) = e.outcome.as_ref()
                    && s.definition.should_restart(outcome)
                {
                    self.retry.entry(id.clone()).or_insert(
                        e.finished_at_ms.unwrap() + s.definition.restart_delay_ms() as i64,
                    );
                }
            }
        }
        for (id, due) in self.retry.clone() {
            if due > now || self.active.contains_key(&id) || self.recovering.contains_key(&id) {
                continue;
            }
            self.retry.remove(&id);
            let s = self.store.get(&id)?;
            if s.desired != Desired::Running {
                continue;
            }
            if self
                .latest(&id)?
                .as_ref()
                .is_some_and(Execution::needs_attention)
            {
                continue;
            }
            let run = self.store.begin(&id, now)?;
            match self.host.start(run, s.definition.command_spec()) {
                Ok(pid) => {
                    self.store.started(run, pid)?;
                    self.active.insert(id, (run, pid));
                }
                Err(Error::ExecutionUnavailable) => {
                    let c = Completion {
                        outcome: Outcome::SpawnFailed,
                        finished_at_ms: now,
                    };
                    self.store.finish(run, &c)?;
                    if s.definition.should_restart(&c.outcome) {
                        self.retry
                            .insert(id, now + s.definition.restart_delay_ms() as i64);
                    }
                }
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}
