//! What the session hook tells each new agent session: background jobs on this
//! machine that broke since that agent last looked, and nothing at all when
//! nothing did. The agent gives the hook five seconds and a failing hook must
//! not get in a session's way, so it reads only this machine.
use crate::{
    agent_setup::Agent,
    attention::{self, AttentionHost, AttentionOutcome, AttentionResponse, Consumer, Snapshot},
    attention_render,
    attention_store::AttentionStore,
    jobs::{RunStore, Supervisor},
    ledger::Ledger,
    operation_error::Result,
    runs::Timestamp,
    targets::Target,
};

pub struct Context {
    /// Empty when nothing is new.
    pub text: String,
    /// What to remember as seen once `text` has reached the session.
    pub seen: Snapshot,
}

pub fn context(
    agent: Agent,
    supervisor: &impl Supervisor,
    ledgers: &dyn Fn(&RunStore) -> Option<Ledger>,
    store: &AttentionStore,
    now: Timestamp,
) -> Result<Context> {
    let consumer = Consumer::parse(agent.id())?;
    let previous = store.load(&consumer)?;
    let (items, seen) = attention::look(supervisor, ledgers, Some(&previous))?;
    let seen = seen.unwrap_or_default();
    if items.is_empty() {
        return Ok(Context {
            text: String::new(),
            seen,
        });
    }
    let response = AttentionResponse {
        hosts: vec![AttentionHost {
            host: Target::LOCAL.into(),
            outcome: AttentionOutcome::Items { items },
        }],
    };
    let mut text = attention_render::render(&response, now);
    text.push_str(
        "\nLoad the rhythm skill to investigate (`rhythm skill show`), and tell the user what \
         broke before working around it.\n",
    );
    Ok(Context { text, seen })
}
