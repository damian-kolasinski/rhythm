//! Bounded, ephemeral pairing codes and tab-scoped browser sessions.
use crate::{
    operation_error::{Error, Result},
    private_storage::random_hex,
    runtime_protocol::Request,
};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const CODE_TTL: Duration = Duration::from_secs(120);
const SESSION_TTL: Duration = Duration::from_secs(8 * 60 * 60);
struct Credential {
    value: String,
    expires: Instant,
}
#[derive(Default)]
struct State {
    codes: Vec<Credential>,
    sessions: Vec<Credential>,
}
#[derive(Clone)]
pub struct BrowserAccess {
    state: Arc<Mutex<State>>,
    pub demo: bool,
}
impl BrowserAccess {
    pub fn new(demo: bool) -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
            demo,
        }
    }
    pub fn issue_code(&self) -> Result<String> {
        self.issue_at(Instant::now())
    }
    fn issue_at(&self, now: Instant) -> Result<String> {
        let value = random_hex()?;
        let mut state = self.state.lock().expect("browser credentials lock");
        state.codes.retain(|c| c.expires > now);
        if state.codes.len() >= 8 {
            state.codes.remove(0);
        }
        state.codes.push(Credential {
            value: value.clone(),
            expires: now + CODE_TTL,
        });
        Ok(value)
    }
    pub fn pair(&self, code: &str) -> Result<String> {
        self.pair_at(code, Instant::now())
    }
    fn pair_at(&self, code: &str, now: Instant) -> Result<String> {
        let mut state = self.state.lock().expect("browser credentials lock");
        state.codes.retain(|c| c.expires > now);
        let index = state
            .codes
            .iter()
            .position(|c| equal(c.value.as_bytes(), code.as_bytes()))
            .ok_or(Error::Unauthorized)?;
        let value = random_hex()?;
        state.codes.remove(index);
        state.sessions.retain(|c| c.expires > now);
        if state.sessions.len() >= 16 {
            state.sessions.remove(0);
        }
        state.sessions.push(Credential {
            value: value.clone(),
            expires: now + SESSION_TTL,
        });
        Ok(value)
    }
    pub fn accepts(&self, bearer: &[u8]) -> bool {
        self.accepts_at(bearer, Instant::now())
    }
    fn accepts_at(&self, bearer: &[u8], now: Instant) -> bool {
        let mut state = self.state.lock().expect("browser credentials lock");
        state.sessions.retain(|c| c.expires > now);
        state
            .sessions
            .iter()
            .any(|c| equal(format!("Bearer {}", c.value).as_bytes(), bearer))
    }
    pub fn revoke(&self, bearer: &[u8]) {
        self.state
            .lock()
            .expect("browser credentials lock")
            .sessions
            .retain(|c| !equal(format!("Bearer {}", c.value).as_bytes(), bearer));
    }
    pub fn permits(request: &Request) -> bool {
        !matches!(request, Request::Shutdown)
    }
}
fn equal(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len() && left.iter().zip(right).fold(0u8, |n, (a, b)| n | (a ^ b)) == 0
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codes_are_single_use_expiring_and_separate_from_sessions() {
        let a = BrowserAccess::new(false);
        let now = Instant::now();
        let code = a.issue_at(now).unwrap();
        assert!(!a.accepts_at(format!("Bearer {code}").as_bytes(), now));
        let token = a.pair_at(&code, now).unwrap();
        assert!(a.pair_at(&code, now).is_err());
        let bearer = format!("Bearer {token}");
        assert!(a.accepts_at(bearer.as_bytes(), now));
        assert!(!a.accepts_at(bearer.as_bytes(), now + SESSION_TTL));
        let code = a.issue_at(now).unwrap();
        assert!(a.pair_at(&code, now + CODE_TTL).is_err());
    }
    #[test]
    fn capacity_is_bounded_and_disconnect_revokes_only_one_session() {
        let a = BrowserAccess::new(false);
        let now = Instant::now();
        let first = a.issue_at(now).unwrap();
        for _ in 0..8 {
            a.issue_at(now).unwrap();
        }
        assert!(a.pair_at(&first, now).is_err());
        let first = a.pair_at(&a.issue_at(now).unwrap(), now).unwrap();
        let second = a.pair_at(&a.issue_at(now).unwrap(), now).unwrap();
        a.revoke(format!("Bearer {first}").as_bytes());
        assert!(!a.accepts_at(format!("Bearer {first}").as_bytes(), now));
        assert!(a.accepts_at(format!("Bearer {second}").as_bytes(), now));
    }
}
