//! Explicit delivery of single-use browser credentials through the CLI.
use crate::{
    local_access::Access,
    operation_error::{Error, Result},
    private_storage::Root,
};
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
pub struct Launch {
    pub url: String,
    pub code: String,
    pub demo: bool,
}
pub async fn prepare(root: &Root) -> Result<Launch> {
    let access = Access::read(root)?;
    #[derive(Deserialize)]
    struct Pairing {
        code: String,
        demo: bool,
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|_| Error::RuntimeUnavailable)?;
    let mut response = client
        .post(format!("http://127.0.0.1:{}/v1/browser-code", access.port))
        .bearer_auth(access.credential())
        .send()
        .await
        .map_err(|_| Error::RuntimeUnavailable)?;
    if !response.status().is_success() {
        return Err(Error::RuntimeUnavailable);
    }
    let mut bytes = vec![];
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| Error::RuntimeUnavailable)?
    {
        if bytes.len() + chunk.len() > 4096 {
            return Err(Error::RuntimeUnavailable);
        }
        bytes.extend_from_slice(&chunk);
    }
    let pairing: Pairing = serde_json::from_slice(&bytes).map_err(|_| Error::RuntimeUnavailable)?;
    if pairing.code.len() != 64 || !pairing.code.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::RuntimeUnavailable);
    }
    Ok(Launch {
        url: format!("http://127.0.0.1:{}/", access.port),
        code: pairing.code,
        demo: pairing.demo,
    })
}
pub fn deliver(launch: &Launch, json: bool, no_open: bool) -> Result<()> {
    use std::io::Write;
    let mut output = std::io::stdout().lock();
    if json {
        writeln!(output, "{}", serde_json::to_string(launch).unwrap())
    } else {
        writeln!(
            output,
            "Open {}\nSingle-use browser code (expires in 2 minutes):\n{}",
            launch.url, launch.code
        )
    }
    .map_err(|_| Error::RuntimeUnavailable)?;
    drop(output);
    if !no_open && cfg!(target_os = "macos") {
        let _ = std::process::Command::new("/usr/bin/open")
            .arg(&launch.url)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    Ok(())
}
