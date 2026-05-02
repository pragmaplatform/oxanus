use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{Mutex, OwnedSemaphorePermit, mpsc};
use tokio::task::JoinSet;

use crate::WorkerBatchConfig;
use crate::config::Config;
use crate::context::ContextValue;
use crate::error::OxanusError;
use crate::executor::ExecutionError;
use crate::job_envelope::JobEnvelope;
use crate::queue::{QueueConfig, QueueKind};
use crate::result_collector::{JobResult, JobResultKind};
use crate::semaphores_map::SemaphoresMap;
use crate::worker_event::WorkerJob;
use crate::{
    dispatcher, executor,
    result_collector::{self, Stats},
};

pub async fn run<DT, ET>(
    config: Arc<Config<DT, ET>>,
    stats: Arc<Mutex<Stats>>,
    ctx: ContextValue<DT>,
    queue_config: QueueConfig,
) -> Result<(), OxanusError>
where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    let concurrency = queue_config.concurrency;
    let (result_tx, result_rx) = mpsc::channel::<JobResult>(concurrency);
    let (job_tx, mut job_rx) = mpsc::channel::<WorkerJob>(concurrency);
    let (batch_error_tx, mut batch_error_rx) = mpsc::channel::<OxanusError>(concurrency.max(1));
    let semaphores = Arc::new(SemaphoresMap::new(concurrency));
    let mut joinset = JoinSet::new();
    let mut batchers: HashMap<BatchKey, mpsc::Sender<PendingJob>> = HashMap::new();

    joinset.spawn(result_collector::run(
        result_rx,
        Arc::clone(&config),
        Arc::clone(&stats),
    ));
    joinset.spawn(run_queue_watcher(
        Arc::clone(&config),
        queue_config.clone(),
        job_tx.clone(),
        Arc::clone(&semaphores),
    ));

    loop {
        tokio::select! {
            job = job_rx.recv() => {
                if let Some(job) = job {
                    route_job(
                        Arc::clone(&config),
                        ctx.clone(),
                        result_tx.clone(),
                        batch_error_tx.clone(),
                        job,
                        &mut batchers,
                        &mut joinset,
                    )
                    .await?;
                }
            }
            Some(task_result) = joinset.join_next() => {
                task_result??;
            }
            batch_error = batch_error_rx.recv() => {
                if let Some(error) = batch_error {
                    return Err(error);
                }
            }
            _ = config.cancel_token.cancelled() => {
                break;
            }
        }
    }

    wait_for_workers_to_finish(config, Arc::clone(&semaphores)).await;

    Ok(())
}

#[derive(Debug, Clone, Eq, Hash, PartialEq)]
struct BatchKey {
    worker: String,
    queue: String,
}

impl BatchKey {
    fn from_envelope(envelope: &JobEnvelope) -> Self {
        Self {
            worker: envelope.job.name.clone(),
            queue: envelope.queue.clone(),
        }
    }
}

struct PendingJob {
    envelope: JobEnvelope,
    permit: OwnedSemaphorePermit,
}

async fn route_job<DT, ET>(
    config: Arc<Config<DT, ET>>,
    ctx: ContextValue<DT>,
    result_tx: mpsc::Sender<JobResult>,
    batch_error_tx: mpsc::Sender<OxanusError>,
    job_event: WorkerJob,
    batchers: &mut HashMap<BatchKey, mpsc::Sender<PendingJob>>,
    joinset: &mut JoinSet<Result<(), OxanusError>>,
) -> Result<(), OxanusError>
where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    let pending = match load_pending_job(Arc::clone(&config), job_event).await? {
        Some(pending) => pending,
        None => return Ok(()),
    };

    if let Some(batch_config) = config.registry.batch_config(&pending.envelope.job.name) {
        send_to_batcher(
            config,
            ctx,
            result_tx,
            batch_error_tx,
            batchers,
            pending,
            batch_config,
        )
        .await;
        return Ok(());
    }

    joinset.spawn(process_pending_job(config, ctx, result_tx, pending));

    Ok(())
}

async fn load_pending_job<DT, ET>(
    config: Arc<Config<DT, ET>>,
    job_event: WorkerJob,
) -> Result<Option<PendingJob>, OxanusError>
where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    tracing::trace!("Processing job: {:?}", job_event);

    let envelope: JobEnvelope = match config.storage.internal.get_job(&job_event.job_id).await {
        Ok(Some(envelope)) => envelope,
        Ok(None) => {
            tracing::warn!("Job {} not found", job_event.job_id);
            if let Err(e) = config.storage.internal.delete_job(&job_event.job_id).await {
                #[cfg(feature = "sentry")]
                sentry_core::capture_error(&e);
                tracing::error!("Failed to delete job: {}", e);
            }
            return Ok(None);
        }
        Err(e) => {
            #[cfg(feature = "sentry")]
            sentry_core::capture_error(&e);
            tracing::error!("Failed to get job envelope: {}", e);
            return Ok(None);
        }
    };

    tracing::debug!(
        job_id = envelope.id,
        latency_ms = envelope.meta.latency_millis(),
        envelope = %serde_json::to_value(&envelope)?,
        "Received envelope"
    );

    Ok(Some(PendingJob {
        envelope,
        permit: job_event.permit,
    }))
}

async fn process_pending_job<DT, ET>(
    config: Arc<Config<DT, ET>>,
    ctx: ContextValue<DT>,
    result_tx: mpsc::Sender<JobResult>,
    pending: PendingJob,
) -> Result<(), OxanusError>
where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    let mut envelope = pending.envelope;
    let job = match config
        .registry
        .build(&envelope.job.name, envelope.job.args.clone(), &ctx.0)
    {
        Ok(job) => job,
        Err(e) => {
            let err_msg = format!("Invalid job: {} - {}", &envelope.job.name, e);
            tracing::error!("{}", err_msg);
            if let Err(e) = config.storage.internal.kill(&envelope, err_msg).await {
                #[cfg(feature = "sentry")]
                sentry_core::capture_error(&e);
                tracing::error!("Failed to kill job: {}", e);
            }
            return Ok(());
        }
    };

    let result = executor::run(Arc::clone(&config), job, &mut envelope).await?;
    drop(pending.permit);

    process_result(result_tx, result, envelope).await;

    Ok(())
}

async fn send_to_batcher<DT, ET>(
    config: Arc<Config<DT, ET>>,
    ctx: ContextValue<DT>,
    result_tx: mpsc::Sender<JobResult>,
    batch_error_tx: mpsc::Sender<OxanusError>,
    batchers: &mut HashMap<BatchKey, mpsc::Sender<PendingJob>>,
    pending: PendingJob,
    batch_config: WorkerBatchConfig,
) where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    let key = BatchKey::from_envelope(&pending.envelope);
    let mut pending = pending;

    loop {
        let tx = batchers
            .entry(key.clone())
            .or_insert_with(|| {
                spawn_batcher(
                    Arc::clone(&config),
                    ctx.clone(),
                    result_tx.clone(),
                    batch_error_tx.clone(),
                    batch_config.clone(),
                )
            })
            .clone();

        match tx.send(pending).await {
            Ok(()) => break,
            Err(err) => {
                pending = err.0;
                batchers.remove(&key);
            }
        }
    }
}

fn spawn_batcher<DT, ET>(
    config: Arc<Config<DT, ET>>,
    ctx: ContextValue<DT>,
    result_tx: mpsc::Sender<JobResult>,
    batch_error_tx: mpsc::Sender<OxanusError>,
    batch_config: WorkerBatchConfig,
) -> mpsc::Sender<PendingJob>
where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    let batch_size = batch_config.size.max(1);
    let (tx, rx) = mpsc::channel(batch_size * 2);
    tokio::spawn(async move {
        if let Err(e) = run_batcher(
            config,
            ctx,
            result_tx,
            batch_error_tx.clone(),
            batch_config,
            rx,
        )
        .await
        {
            tracing::error!(error = %e, "Batcher exited with error");
            batch_error_tx.send(e).await.ok();
        }
    });
    tx
}

async fn run_batcher<DT, ET>(
    config: Arc<Config<DT, ET>>,
    ctx: ContextValue<DT>,
    result_tx: mpsc::Sender<JobResult>,
    batch_error_tx: mpsc::Sender<OxanusError>,
    batch_config: WorkerBatchConfig,
    mut rx: mpsc::Receiver<PendingJob>,
) -> Result<(), OxanusError>
where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    let batch_size = batch_config.size.max(1);
    let mut pending = Vec::with_capacity(batch_size);

    loop {
        tokio::select! {
            job = rx.recv(), if pending.is_empty() => {
                match job {
                    Some(job) => pending.push(job),
                    None => return Ok(()),
                }
            }
            _ = config.cancel_token.cancelled(), if pending.is_empty() => {
                return Ok(());
            }
        }

        if pending.len() >= batch_size || batch_config.timeout.is_zero() {
            spawn_batch(
                Arc::clone(&config),
                ctx.clone(),
                result_tx.clone(),
                batch_error_tx.clone(),
                &mut pending,
            );
            continue;
        }

        let timeout = tokio::time::sleep(batch_config.timeout);
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                job = rx.recv() => {
                    match job {
                        Some(job) => {
                            pending.push(job);
                            if pending.len() >= batch_size {
                                break;
                            }
                        }
                        None => {
                            spawn_batch(
                                Arc::clone(&config),
                                ctx.clone(),
                                result_tx.clone(),
                                batch_error_tx.clone(),
                                &mut pending,
                            );
                            return Ok(());
                        }
                    }
                }
                _ = &mut timeout => {
                    break;
                }
                _ = config.cancel_token.cancelled() => {
                    spawn_batch(
                        Arc::clone(&config),
                        ctx.clone(),
                        result_tx.clone(),
                        batch_error_tx.clone(),
                        &mut pending,
                    );
                    return Ok(());
                }
            }
        }

        spawn_batch(
            Arc::clone(&config),
            ctx.clone(),
            result_tx.clone(),
            batch_error_tx.clone(),
            &mut pending,
        );
    }
}

fn spawn_batch<DT, ET>(
    config: Arc<Config<DT, ET>>,
    ctx: ContextValue<DT>,
    result_tx: mpsc::Sender<JobResult>,
    batch_error_tx: mpsc::Sender<OxanusError>,
    pending: &mut Vec<PendingJob>,
) where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    if pending.is_empty() {
        return;
    }

    let batch = std::mem::take(pending);
    tokio::spawn(async move {
        if let Err(e) = process_pending_batch(config, ctx, result_tx, batch).await {
            tracing::error!(error = %e, "Failed to process job batch");
            batch_error_tx.send(e).await.ok();
        }
    });
}

async fn process_pending_batch<DT, ET>(
    config: Arc<Config<DT, ET>>,
    ctx: ContextValue<DT>,
    result_tx: mpsc::Sender<JobResult>,
    pending: Vec<PendingJob>,
) -> Result<(), OxanusError>
where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    let mut envelopes = Vec::with_capacity(pending.len());
    let mut permits = Vec::with_capacity(pending.len());
    for pending_job in pending {
        envelopes.push(pending_job.envelope);
        permits.push(pending_job.permit);
    }

    let Some(first_envelope) = envelopes.first() else {
        return Ok(());
    };
    let worker_name = first_envelope.job.name.clone();
    let args = envelopes
        .iter()
        .map(|envelope| envelope.job.args.clone())
        .collect();
    let batch = match config.registry.build_batch(&worker_name, args, &ctx.0) {
        Ok(batch) => batch,
        Err(e) => {
            let err_msg = format!("Invalid job batch: {worker_name} - {e}");
            tracing::error!("{}", err_msg);
            for envelope in &envelopes {
                if let Err(e) = config
                    .storage
                    .internal
                    .kill(envelope, err_msg.clone())
                    .await
                {
                    #[cfg(feature = "sentry")]
                    sentry_core::capture_error(&e);
                    tracing::error!("Failed to kill job: {}", e);
                }
            }
            return Ok(());
        }
    };

    for invalid in batch.invalid.iter().rev() {
        if invalid.index >= envelopes.len() {
            continue;
        }

        let envelope = envelopes.remove(invalid.index);
        let permit = permits.remove(invalid.index);
        let err_msg = format!("Invalid job: {worker_name} - {}", invalid.error);
        tracing::error!("{}", err_msg);

        if let Err(e) = config.storage.internal.kill(&envelope, err_msg).await {
            #[cfg(feature = "sentry")]
            sentry_core::capture_error(&e);
            tracing::error!("Failed to kill job: {}", e);
        }

        drop(permit);
    }

    let Some(job) = batch.job else {
        return Ok(());
    };

    let result = executor::run_batch(Arc::clone(&config), job, &mut envelopes).await?;
    drop(permits);

    process_batch_result(result_tx, &result, envelopes).await;

    Ok(())
}

async fn process_result<ET>(
    result_tx: mpsc::Sender<JobResult>,
    result: Result<(), ExecutionError<ET>>,
    envelope: JobEnvelope,
) where
    ET: std::error::Error + Send + Sync + 'static,
{
    let kind = match result {
        Ok(()) => JobResultKind::Success,
        Err(e) => match e {
            ExecutionError::NotPanic(_) => JobResultKind::Failed,
            ExecutionError::Panic() => JobResultKind::Panicked,
        },
    };

    result_tx.send(JobResult { envelope, kind }).await.ok();
}

async fn process_batch_result<ET>(
    result_tx: mpsc::Sender<JobResult>,
    result: &Result<(), ExecutionError<ET>>,
    envelopes: Vec<JobEnvelope>,
) where
    ET: std::error::Error + Send + Sync + 'static,
{
    for envelope in envelopes {
        let kind = match result {
            Ok(()) => JobResultKind::Success,
            Err(e) => match e {
                ExecutionError::NotPanic(_) => JobResultKind::Failed,
                ExecutionError::Panic() => JobResultKind::Panicked,
            },
        };

        result_tx.send(JobResult { envelope, kind }).await.ok();
    }
}

async fn run_queue_watcher<DT, ET>(
    config: Arc<Config<DT, ET>>,
    queue_config: QueueConfig,
    job_tx: mpsc::Sender<WorkerJob>,
    semaphores: Arc<SemaphoresMap>,
) -> Result<(), OxanusError>
where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    let mut tracked_queues = HashSet::new();

    loop {
        let all_queues: HashSet<String> = match &queue_config.kind {
            QueueKind::Static { key } => HashSet::from([key.clone()]),
            QueueKind::Dynamic { prefix, .. } => config
                .storage
                .internal
                .track_redis_result(
                    config
                        .storage
                        .internal
                        .queue_keys(&format!("{prefix}*"))
                        .await,
                )?
                .unwrap_or_default(),
        };
        let new_queues: HashSet<String> = all_queues.difference(&tracked_queues).cloned().collect();

        for queue in new_queues {
            tracing::info!(
                queue = queue,
                config.throttle = format!("{:?}", queue_config.throttle),
                "Tracking queue"
            );

            let dispatcher_config = Arc::clone(&config);
            let dispatcher_queue_config = queue_config.clone();
            let dispatcher_job_tx = job_tx.clone();
            let dispatcher_semaphores = Arc::clone(&semaphores);
            let dispatcher_queue = queue.clone();
            tokio::spawn(async move {
                if let Err(e) = dispatcher::run(
                    dispatcher_config,
                    dispatcher_queue_config,
                    dispatcher_queue,
                    dispatcher_job_tx,
                    dispatcher_semaphores,
                )
                .await
                {
                    tracing::error!(error = %e, "Dispatcher exited with error");
                }
            });

            tracked_queues.insert(queue);
        }

        if config.cancel_token.is_cancelled() {
            return Ok(());
        } else if let QueueKind::Dynamic { sleep_period, .. } = queue_config.kind {
            tokio::time::sleep(sleep_period).await;
        } else {
            return Ok(());
        }
    }
}

async fn wait_for_workers_to_finish<DT, ET>(
    config: Arc<Config<DT, ET>>,
    semaphores: Arc<SemaphoresMap>,
) where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    let t_start = std::time::Instant::now();
    let mut ticks = 0;

    loop {
        ticks += 1;

        let busy_count = semaphores.busy_count().await;
        if busy_count == 0 {
            break;
        }

        if ticks % 200 == 0 {
            tracing::info!("Waiting for {} workers to finish...", busy_count);
        }

        if t_start.elapsed() > config.shutdown_timeout {
            tracing::error!("Shutdown timeout reached");
            break;
        }

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}
