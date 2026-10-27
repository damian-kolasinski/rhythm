//! Credentials exist only in private runtime discovery storage, never in URLs.
use crate::{
    private_storage::{self, Root},
    services::*,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct Access {
    pub port: u16,
    token: String,
}
impl Access {
    pub fn create(port: u16) -> Result<Self> {
        Ok(Self {
            port,
            token: private_storage::random_hex()?,
        })
    }
    pub fn save(&self, root: &Root) -> Result<()> {
        private_storage::write_json(&root.path().join("access.json"), self)
    }
    pub fn read(root: &Root) -> Result<Self> {
        let a: Self = private_storage::read_json(&root.path().join("access.json"))
            .map_err(|_| Error::RuntimeUnavailable)?;
        if a.port == 0 || a.token.len() != 64 || !a.token.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::RuntimeUnavailable);
        }
        Ok(a)
    }
    pub fn credential(&self) -> &str {
        &self.token
    }
    pub fn accepts(&self, actual: &[u8]) -> bool {
        let expected = format!("Bearer {}", self.token);
        actual.len() == expected.len()
            && actual
                .iter()
                .zip(expected.as_bytes())
                .fold(0u8, |v, (a, b)| v | (a ^ b))
                == 0
    }
}
