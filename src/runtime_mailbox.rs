//! Request/reply channel boundary between HTTP adapters and the runtime actor.
use crate::{
    operation_error::{Error, Result},
    runtime_protocol::{Request, Response},
};
use tokio::sync::{mpsc, oneshot};
pub type Envelope = (Request, oneshot::Sender<Result<Response>>);
pub async fn invoke(sender: &mpsc::Sender<Envelope>, request: Request) -> Result<Response> {
    let (tx, rx) = oneshot::channel();
    sender
        .send((request, tx))
        .await
        .map_err(|_| Error::RuntimeUnavailable)?;
    rx.await.map_err(|_| Error::RuntimeUnavailable)?
}
