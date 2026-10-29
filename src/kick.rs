//! Start a job now and, when asked, wait for its run to finish.
use crate::{
    jobs::{Exit, Label, Observation, RunStore, Supervisor},
    ledger::Ledger,
    operation_error::{Error, Result},
    run_output,
    runs::Run,
    wrapper_command::Wrapped,
};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(100);
/// Output included with a finished run.
const OUTPUT_BYTES: u64 = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename = "kicked")]
pub struct KickResponse {
    pub label: String,
    /// Present when waiting: how the started run ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished: Option<Finished>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finished {
    /// As launchd observed it; absent when launchd reports no exit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<Exit>,
    /// The recorded run, for jobs that record their runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<Run>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(default)]
    pub truncated: bool,
}
impl Finished {
    pub fn succeeded(&self) -> bool {
        self.exit.is_some_and(Exit::succeeded)
    }
}

/// Refuses a job that is already running, since launchd would not start another.
pub fn kick(
    supervisor: &impl Supervisor,
    label: &Label,
    wait: Option<Duration>,
    ledgers: &dyn Fn(&RunStore) -> Option<Ledger>,
) -> Result<KickResponse> {
    let before = match supervisor.observe(label)? {
        Observation::Unloaded => return Err(Error::JobNotLoaded),
        Observation::Loaded { pid: Some(_), .. } => return Err(Error::Busy),
        Observation::Loaded { runs, .. } => runs,
    };
    let store = Wrapped::parse(&supervisor.program(label)?.arguments)
        .filter(|w| w.label == *label)
        .map(|w| w.data_dir.map_or(RunStore::Default, RunStore::Directory));
    let ledger = store.as_ref().and_then(ledgers);
    // Only a run recorded after this point belongs to this kick.
    let newest_before = match &ledger {
        Some(ledger) => ledger.latest(label.as_str())?.map_or(0, |r| r.id),
        None => 0,
    };
    supervisor.start(label)?;
    let Some(wait) = wait else {
        return Ok(KickResponse {
            label: label.to_string(),
            finished: None,
        });
    };
    let deadline = Instant::now()
        .checked_add(wait)
        .ok_or(Error::InvalidRequest)?;
    let exit = loop {
        match supervisor.observe(label)? {
            Observation::Loaded {
                pid: None,
                runs,
                last_exit,
            } if runs > before => break last_exit,
            Observation::Unloaded => return Err(Error::JobNotLoaded),
            _ if Instant::now() >= deadline => return Err(Error::TimedOut),
            _ => std::thread::sleep(POLL),
        }
    };
    let mut finished = Finished {
        exit,
        run: None,
        output: None,
        truncated: false,
    };
    // A ledger created by this very run was not there to open before it.
    let ledger = ledger.or_else(|| store.as_ref().and_then(ledgers));
    if let Some(ledger) = ledger
        && let Some(run) = ledger
            .latest(label.as_str())?
            .filter(|r| r.id > newest_before)
    {
        let tail = run_output::tail(&ledger.output_location(run.id), OUTPUT_BYTES)?;
        finished.output = Some(String::from_utf8_lossy(&tail.bytes).into_owned());
        finished.truncated = tail.truncated;
        finished.run = Some(run);
    }
    Ok(KickResponse {
        label: label.to_string(),
        finished: Some(finished),
    })
}
