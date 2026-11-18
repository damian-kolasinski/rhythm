//! Make only the Rhythm request point to the already verified release.
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use semver::Version;
use sha2::{Digest, Sha256};
use toml_edit::{DocumentMut, InlineTable, Item, Value};

use crate::operation_error::{Error, Result};

use super::{
    super::{Paths, mise::Mise},
    download::{self, ISSUER},
};

pub(super) struct Selection {
    config: PathBuf,
    previous: Item,
    next: Item,
    request: String,
    pub linked: bool,
    applied: bool,
}

impl Selection {
    pub fn prepare(
        paths: &Paths,
        mise: &Mise,
        repository: &str,
        version: &Version,
    ) -> Result<Option<Self>> {
        let Some(request) = mise.request(&paths.home)? else {
            return Ok(None);
        };
        let config = request
            .source
            .path
            .canonicalize()
            .map_err(|_| Error::MiseUnavailable)?;
        if !config.starts_with(&paths.home) {
            return Err(Error::MiseUnavailable);
        }
        let (document, _) = read(&config)?;
        let previous = item(&document, &mise.tool)?.clone();
        let mut next = previous.clone();
        if next.as_str().is_some() {
            next = Item::Value(Value::InlineTable(InlineTable::new()));
        }
        let fields = next.as_table_like_mut().ok_or(Error::MiseUnavailable)?;
        fields.insert("version", toml_edit::value(version.to_string()));
        fields.insert(
            "identity",
            toml_edit::value(download::identity(repository, version)),
        );
        fields.insert("issuer", toml_edit::value(ISSUER));
        Ok(Some(Self {
            config,
            previous,
            next,
            request: format!("{}@{version}", mise.tool),
            linked: false,
            applied: false,
        }))
    }

    pub fn activate(
        &mut self,
        paths: &Paths,
        mise: &Mise,
        verified_directory: &Path,
    ) -> Result<()> {
        if let Ok(stdout) = mise.run(&paths.home, ["where", &self.request]) {
            let installed = PathBuf::from(
                std::str::from_utf8(&stdout)
                    .map_err(|_| Error::MiseUnavailable)?
                    .trim(),
            );
            if !installed.is_absolute() || !installed.is_dir() {
                return Err(Error::MiseUnavailable);
            }
            for name in ["rhythm", "rhythm-ui", super::super::window::ICON] {
                if digest(&installed.join(name))? != digest(&verified_directory.join(name))? {
                    return Err(Error::MiseUnavailable);
                }
            }
        } else {
            mise.run(
                &paths.home,
                [
                    std::ffi::OsStr::new("link"),
                    std::ffi::OsStr::new(&self.request),
                    verified_directory.as_os_str(),
                ],
            )?;
            self.linked = true;
        }
        replace(&self.config, &mise.tool, &self.previous, &self.next)?;
        self.applied = true;
        Ok(())
    }

    pub fn restore(&mut self, paths: &Paths, mise: &Mise) -> Result<()> {
        if self.applied {
            replace(&self.config, &mise.tool, &self.next, &self.previous)?;
        }
        if self.linked {
            mise.run(&paths.home, ["uninstall", "--yes", &self.request])?;
            self.linked = false;
        }
        Ok(())
    }
}

fn replace(path: &Path, tool: &str, expected: &Item, next: &Item) -> Result<()> {
    let (mut document, original) = read(path)?;
    if item(&document, tool)?.to_string() != expected.to_string() {
        return Err(Error::MiseUnavailable);
    }
    document["tools"][tool] = next.clone();
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap())
        .map_err(|_| Error::MiseUnavailable)?;
    let metadata = fs::metadata(path).map_err(|_| Error::MiseUnavailable)?;
    temp.as_file()
        .set_permissions(metadata.permissions())
        .map_err(|_| Error::MiseUnavailable)?;
    temp.write_all(document.to_string().as_bytes())
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|_| Error::MiseUnavailable)?;
    if read(path)?.1 != original {
        return Err(Error::MiseUnavailable);
    }
    temp.persist(path).map_err(|_| Error::MiseUnavailable)?;
    Ok(())
}

fn read(path: &Path) -> Result<(DocumentMut, String)> {
    let metadata = fs::symlink_metadata(path).map_err(|_| Error::MiseUnavailable)?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::getuid() }
        || metadata.mode() & 0o022 != 0
        || metadata.len() > 1024 * 1024
    {
        return Err(Error::MiseUnavailable);
    }
    let original = fs::read_to_string(path).map_err(|_| Error::MiseUnavailable)?;
    let document = original.parse().map_err(|_| Error::MiseUnavailable)?;
    Ok((document, original))
}

fn item<'a>(document: &'a DocumentMut, name: &str) -> Result<&'a Item> {
    document
        .get("tools")
        .and_then(Item::as_table)
        .and_then(|tools| tools.get(name))
        .ok_or(Error::MiseUnavailable)
}

fn digest(path: &Path) -> Result<[u8; 32]> {
    if !fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file()) {
        return Err(Error::MiseUnavailable);
    }
    let mut file = fs::File::open(path).map_err(|_| Error::MiseUnavailable)?;
    let mut hasher = Sha256::new();
    let count = std::io::copy(
        &mut Read::take(&mut file, 100 * 1024 * 1024 + 1),
        &mut hasher,
    )
    .map_err(|_| Error::MiseUnavailable)?;
    if count > 100 * 1024 * 1024 {
        return Err(Error::MiseUnavailable);
    }
    Ok(hasher.finalize().into())
}
