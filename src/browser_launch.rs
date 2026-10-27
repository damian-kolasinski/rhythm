//! Resolve and open the local workspace URL without browser credentials.
use crate::{
    local_access::Access,
    operation_error::{Error, Result},
    private_storage::Root,
};
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
pub struct Launch {
    pub url: String,
    pub demo: bool,
}
pub async fn prepare(root: &Root) -> Result<Launch> {
    let access = Access::read(root)?;
    #[derive(Deserialize)]
    struct Info {
        demo: bool,
    }
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|_| Error::RuntimeUnavailable)?;
    let mut response = client
        .post(format!("http://127.0.0.1:{}/v1/browser/info", access.port))
        .header("Origin", format!("http://127.0.0.1:{}", access.port))
        .header(crate::http_authorization::BROWSER_CLIENT_HEADER, "1")
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
    let info: Info = serde_json::from_slice(&bytes).map_err(|_| Error::RuntimeUnavailable)?;
    Ok(Launch {
        url: format!("http://127.0.0.1:{}/", access.port),
        demo: info.demo,
    })
}
pub fn deliver(launch: &Launch, json: bool, no_open: bool) -> Result<()> {
    use std::io::Write;
    let mut output = std::io::stdout().lock();
    if json {
        writeln!(output, "{}", serde_json::to_string(launch).unwrap())
    } else {
        writeln!(output, "Open {}", launch.url)
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
