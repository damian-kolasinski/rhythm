//! Runtime composition. The application owns lifecycle; HTTP only transports requests.
use crate::{
    execution::ProcessHost,
    local_access::Access,
    local_api::{self, Envelope},
    private_storage::Root,
    service_store::SqliteStore,
    services::*,
};
use std::path::PathBuf;
use tokio::{
    sync::{mpsc, watch},
    time::{Duration, Instant, interval},
};

pub async fn run(root: Root, executable: PathBuf) -> Result<()> {
    let _lock = root.lock()?;
    let store = SqliteStore::open(&root)?;
    let mut manager = Manager::new(store, ProcessHost::new(root.clone(), executable))?;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| Error::RuntimeUnavailable)?;
    let access = Access::create(
        listener
            .local_addr()
            .map_err(|_| Error::RuntimeUnavailable)?
            .port(),
    )?;
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| Error::ExecutionUnavailable)?;
    access.save(&root)?;
    let (tx, mut rx) = mpsc::channel::<Envelope>(64);
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let server = tokio::spawn(
        axum::serve(listener, local_api::router(access.clone(), tx.clone()))
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.wait_for(|done| *done).await;
            })
            .into_future(),
    );
    let signals = tokio::spawn(async move {
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{}}
        let (reply, _) = tokio::sync::oneshot::channel();
        let _ = tx.send((Request::Shutdown, reply)).await;
    });
    let mut ticks = interval(Duration::from_millis(25));
    let result: Result<()> = async {
        manager.tick()?;
        loop {
            tokio::select! {
                _=ticks.tick()=>manager.tick()?,
                Some((request,reply))=rx.recv()=>{let _=reply.send(manager.request(request));},
            }
            if manager.shutdown_requested() {
                break;
            }
        }
        manager.release();
        let deadline = Instant::now() + Duration::from_secs(3);
        while !manager.quiescent() && Instant::now() < deadline {
            manager.tick()?;
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        Ok(())
    }
    .await;
    rx.close();
    while let Ok((_, reply)) = rx.try_recv() {
        let _ = reply.send(Err(Error::RuntimeUnavailable));
    }
    manager.release();
    signals.abort();
    let _ = shutdown_tx.send(true);
    let mut server = server;
    if tokio::time::timeout(Duration::from_secs(2), &mut server)
        .await
        .is_err()
    {
        server.abort();
    }
    let _ = std::fs::remove_file(root.path().join("access.json"));
    result
}
