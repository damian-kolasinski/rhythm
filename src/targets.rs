//! The machines a command reads: this one or ssh destinations, queried at once.
use crate::{
    operation_error::{Error, Result},
    remote,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Local,
    Remote(remote::Host),
}
impl Target {
    pub const LOCAL: &str = "local";
    pub fn parse(name: &str) -> Result<Self> {
        if name == Self::LOCAL {
            Ok(Self::Local)
        } else {
            name.parse().map(Self::Remote)
        }
    }
    pub fn name(&self) -> &str {
        match self {
            Self::Local => Self::LOCAL,
            Self::Remote(host) => host.as_str(),
        }
    }
    /// Distinct targets in the order given, or this machine when none are.
    pub fn list(names: &[String]) -> Result<Vec<Self>> {
        let mut targets: Vec<Self> = vec![];
        for name in names {
            let target = Self::parse(name)?;
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
        if targets.is_empty() {
            targets.push(Self::Local);
        }
        Ok(targets)
    }
}

/// Queries every target concurrently; one unreachable host does not hide the others.
pub fn each<T: Send>(
    targets: &[Target],
    local: impl Fn() -> Result<T> + Sync,
    remote: impl Fn(&remote::Host) -> Result<T> + Sync,
) -> Vec<(String, Result<T>)> {
    std::thread::scope(|scope| {
        let pending: Vec<_> = targets
            .iter()
            .map(|target| {
                let (local, remote) = (&local, &remote);
                scope.spawn(move || match target {
                    Target::Local => local(),
                    Target::Remote(host) => remote(host),
                })
            })
            .collect();
        targets
            .iter()
            .zip(pending)
            .map(|(target, pending)| {
                let result = pending.join().unwrap_or(Err(Error::RemoteUnavailable));
                (target.name().to_owned(), result)
            })
            .collect()
    })
}
