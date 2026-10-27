//! Runtime composition. The application owns lifecycle; HTTP only transports requests.
use crate::{
    execution::ProcessHost,
    execution_host::ExecutionHost,
    local_access::Access,
    local_api::{self, Envelope},
    operation_error::{Error, Result},
    private_storage::Root,
    runtime_protocol::{Request, Response},
    service_store::SqliteStore,
    services::Manager,
};
use std::path::PathBuf;
use tokio::{
    sync::{mpsc, watch},
    time::{Duration, Instant, interval},
};

pub async fn run(root: Root, executable: PathBuf) -> Result<()> {
    let _lock = root.lock()?;
    let store = SqliteStore::open(&root)?;
    let executions = crate::execution_reader::SqliteExecutionReader::open(&root)?;
    let log_host = ProcessHost::new(root.clone(), executable.clone());
    let mut manager = Manager::new(store, ProcessHost::new(root.clone(), executable.clone()))?;
    let mut schedules = crate::schedules::Manager::new(
        crate::schedule_store::SqliteScheduleStore::open(&root)?,
        ProcessHost::new(root.clone(), executable),
        crate::schedules::SystemClock,
    )?;
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
    let mut shutdown_requested = false;
    let result: Result<()> = async {
        manager.tick()?;
        schedules.tick()?;
        loop {
            tokio::select! {
                _=ticks.tick()=>{manager.tick()?;schedules.tick()?;},
                Some((request,reply))=rx.recv()=>{
                    let result=match request {Request::Schedule{command}=>schedules.request(command).map(|response|Response::Schedule{response}),Request::Service(request)=>manager.request(request).map(Response::Service),
                        Request::Status=>Ok(Response::Runtime{version:env!("CARGO_PKG_VERSION").into()}),
                        Request::Shutdown=>{shutdown_requested=true;Ok(Response::Runtime{version:env!("CARGO_PKG_VERSION").into()})},
                        Request::Logs{execution_id}=>executions.get(execution_id).and_then(|_|log_host.logs(execution_id)).map(|(stdout,stderr)|Response::Logs{stdout,stderr})};
                    let _=reply.send(result);
                },
            }
            if shutdown_requested {
                break;
            }
        }
        manager.release();
        schedules.shutdown();
        let deadline = Instant::now() + Duration::from_secs(3);
        while (!manager.quiescent() || !schedules.quiescent()) && Instant::now() < deadline {
            manager.tick()?;
            schedules.tick()?;
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
    schedules.shutdown();
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
