use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, OwnedSemaphorePermit, mpsc};
use tokio::task::JoinSet;

use crate::config::Config;
use crate::context::{ContextValue, JobState};
use crate::error::OxanusError;
use crate::executor::ExecutionError;
use crate::job_envelope::{Job, JobEnvelope, JobMeta};
use crate::queue::{QueueConfig, QueueKind};
use crate::result_collector::{JobResult, JobResultKind};
use crate::semaphores_map::SemaphoresMap;
use crate::worker_event::WorkerJob;
use crate::{
    JobContext, dispatcher, executor,
    result_collector::{self, Stats},
};

struct PendingBatchItem {
    envelope: JobEnvelope,
    permit: OwnedSemaphorePermit,
}

struct BatchBuffer {
    items: Vec<PendingBatchItem>,
    first_added: tokio::time::Instant,
}

type BatchBuffers = Arc<Mutex<HashMap<String, BatchBuffer>>>;

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
    let has_batch_configs = !config.registry.batch_configs.is_empty();

    let max_batch_size = config
        .registry
        .batch_configs
        .values()
        .map(|c| c.batch_size)
        .max()
        .unwrap_or(1);
    let effective_concurrency = concurrency * max_batch_size;

    let (result_tx, result_rx) = mpsc::channel::<JobResult>(effective_concurrency);
    let (job_tx, mut job_rx) = mpsc::channel::<WorkerJob>(effective_concurrency);
    let semaphores = Arc::new(SemaphoresMap::new(effective_concurrency));
    let mut joinset = JoinSet::new();
    let batch_buffers: BatchBuffers = Arc::new(Mutex::new(HashMap::new()));

    let batch_check_interval = if has_batch_configs {
        Duration::from_millis(50)
    } else {
        Duration::from_secs(86400)
    };
    let mut batch_tick = tokio::time::interval(batch_check_interval);
    batch_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

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
                    joinset.spawn(route_job(
                        Arc::clone(&config),
                        ctx.clone(),
                        result_tx.clone(),
                        job,
                        Arc::clone(&batch_buffers),
                    ));
                }
            }
            _ = batch_tick.tick(), if has_batch_configs => {
                spawn_ready_batches(
                    &config, &ctx, &result_tx,
                    &batch_buffers, &mut joinset, false,
                ).await;
            }
            Some(task_result) = joinset.join_next() => {
                task_result??;
            }
            _ = config.cancel_token.cancelled() => {
                break;
            }
        }
    }

    if has_batch_configs {
        spawn_ready_batches(
            &config,
            &ctx,
            &result_tx,
            &batch_buffers,
            &mut joinset,
            true,
        )
        .await;
    }

    wait_for_workers_to_finish(config, Arc::clone(&semaphores)).await;

    Ok(())
}

async fn fetch_envelope<DT, ET>(config: &Config<DT, ET>, job_id: &String) -> Option<JobEnvelope> {
    match config.storage.internal.get_job(job_id).await {
        Ok(Some(envelope)) => Some(envelope),
        Ok(None) => {
            tracing::warn!("Job {} not found", job_id);
            if let Err(e) = config.storage.internal.delete_job(job_id).await {
                #[cfg(feature = "sentry")]
                sentry_core::capture_error(&e);
                tracing::error!("Failed to delete job: {}", e);
            }
            None
        }
        Err(e) => {
            #[cfg(feature = "sentry")]
            sentry_core::capture_error(&e);
            tracing::error!("Failed to get job envelope: {}", e);
            None
        }
    }
}

async fn route_job<DT, ET>(
    config: Arc<Config<DT, ET>>,
    ctx: ContextValue<DT>,
    result_tx: mpsc::Sender<JobResult>,
    job_event: WorkerJob,
    batch_buffers: BatchBuffers,
) -> Result<(), OxanusError>
where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    let envelope = match fetch_envelope(&config, &job_event.job_id).await {
        Some(e) => e,
        None => {
            let envelope = JobEnvelope {
                id: job_event.job_id.clone(),
                queue: job_event.queue,
                job: Job {
                    name: "unknown".to_string(),
                    args: serde_json::Value::Null,
                },
                meta: JobMeta {
                    id: job_event.job_id,
                    retries: 0,
                    unique: false,
                    on_conflict: None,
                    created_at: 0,
                    scheduled_at: 0,
                    started_at: None,
                    state: None,
                    resurrect: false,
                    error: Some("Job not found in storage".to_string()),
                },
            };
            report_result(&result_tx, envelope, JobResultKind::Failed).await;
            return Ok(());
        }
    };

    if let Some(batch_config) = config.registry.get_batch_config(&envelope.job.name) {
        let worker_name = envelope.job.name.clone();
        let batch_size = batch_config.batch_size;

        let ready_items = {
            let mut buffers = batch_buffers.lock().await;
            let buffer = buffers
                .entry(worker_name.clone())
                .or_insert_with(|| BatchBuffer {
                    items: Vec::new(),
                    first_added: tokio::time::Instant::now(),
                });
            buffer.items.push(PendingBatchItem {
                envelope,
                permit: job_event.permit,
            });

            if buffer.items.len() >= batch_size {
                buffers.remove(&worker_name)
            } else {
                None
            }
        };

        if let Some(buffer) = ready_items {
            execute_batch(config, ctx, result_tx, buffer.items).await?;
        }
    } else {
        process_job(config, ctx, result_tx, envelope, job_event.permit).await?;
    }

    Ok(())
}

async fn spawn_ready_batches<DT, ET>(
    config: &Arc<Config<DT, ET>>,
    ctx: &ContextValue<DT>,
    result_tx: &mpsc::Sender<JobResult>,
    batch_buffers: &BatchBuffers,
    joinset: &mut JoinSet<Result<(), OxanusError>>,
    drain_all: bool,
) where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    let ready_batches = {
        let mut buffers = batch_buffers.lock().await;

        let ready_keys: Vec<String> = buffers
            .iter()
            .filter(|(worker_name, buffer)| {
                if drain_all {
                    !buffer.items.is_empty()
                } else if let Some(cfg) = config.registry.get_batch_config(worker_name) {
                    buffer.first_added.elapsed() >= Duration::from_millis(cfg.batch_linger_ms)
                } else {
                    false
                }
            })
            .map(|(name, _)| name.clone())
            .collect();

        ready_keys
            .into_iter()
            .filter_map(|key| buffers.remove(&key).map(|b| b.items))
            .collect::<Vec<_>>()
    };

    for items in ready_batches {
        joinset.spawn(execute_batch(
            Arc::clone(config),
            ctx.clone(),
            result_tx.clone(),
            items,
        ));
    }
}

async fn execute_batch<DT, ET>(
    config: Arc<Config<DT, ET>>,
    ctx: ContextValue<DT>,
    result_tx: mpsc::Sender<JobResult>,
    items: Vec<PendingBatchItem>,
) -> Result<(), OxanusError>
where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    if items.is_empty() {
        return Ok(());
    }

    let first_envelope = &items.first().expect("items is not empty").envelope;
    let worker_name = first_envelope.job.name.clone();
    let batch_config = match config.registry.get_batch_config(&worker_name) {
        Some(cfg) => cfg,
        None => {
            let err_msg = format!("Batch config not found for {}", worker_name);
            tracing::error!("{}", err_msg);
            for item in items {
                if let Err(e) = config
                    .storage
                    .internal
                    .kill(&item.envelope, err_msg.clone())
                    .await
                {
                    tracing::error!("Failed to kill batch item: {}", e);
                }
                report_result(&result_tx, item.envelope, JobResultKind::Failed).await;
            }
            return Ok(());
        }
    };

    let args: Vec<serde_json::Value> = items.iter().map(|i| i.envelope.job.args.clone()).collect();

    for item in &items {
        if let Err(e) = config
            .storage
            .internal
            .set_started_at(&item.envelope.id)
            .await
        {
            tracing::error!("Failed to set started_at for batch item: {}", e);
        }
    }

    let batch_size = items.len();
    tracing::info!(
        worker = worker_name,
        batch_size = batch_size,
        "Batch started"
    );

    let batch_worker = match (batch_config.factory)(args, &ctx.0) {
        Ok(w) => w,
        Err(e) => {
            let err_msg = format!("Failed to build batch processor: {e}");
            tracing::error!("{}", err_msg);
            for item in items {
                if let Err(e) = config
                    .storage
                    .internal
                    .kill(&item.envelope, err_msg.clone())
                    .await
                {
                    tracing::error!("Failed to kill batch item: {}", e);
                }
                result_tx
                    .send(JobResult {
                        envelope: item.envelope,
                        kind: JobResultKind::Failed,
                    })
                    .await
                    .ok();
            }
            return Ok(());
        }
    };

    let job_ctx = JobContext {
        meta: first_envelope.meta.clone(),
        state: JobState::new(
            config.storage.clone(),
            first_envelope.id.clone(),
            first_envelope.meta.state.clone(),
        ),
    };

    let start = std::time::Instant::now();
    let result = batch_worker.process(&job_ctx).await;
    let duration = start.elapsed();

    tracing::info!(
        worker = worker_name,
        batch_size = batch_size,
        success = result.is_ok(),
        duration_ms = duration.as_millis(),
        "Batch finished"
    );

    match result {
        Ok(()) => {
            for item in items {
                if let Err(e) = config
                    .storage
                    .internal
                    .finish_with_success(&item.envelope)
                    .await
                {
                    tracing::error!("Failed to finish batch item: {}", e);
                }
                report_result(&result_tx, item.envelope, JobResultKind::Success).await;
                drop(item.permit);
            }
        }
        Err(e) => {
            let err_msg = e.to_string();
            let max_retries = batch_worker.max_retries();

            for item in items {
                let retry_delay = batch_worker.retry_delay(item.envelope.meta.retries);
                if item.envelope.meta.retries < max_retries {
                    if let Err(e) = config
                        .storage
                        .internal
                        .finish_with_failure(&item.envelope)
                        .await
                    {
                        tracing::error!("Failed to finish batch item: {}", e);
                    }
                    if let Err(e) = config
                        .storage
                        .internal
                        .retry_in(item.envelope.id.clone(), retry_delay, err_msg.clone())
                        .await
                    {
                        tracing::error!("Failed to retry batch item: {}", e);
                    }
                } else if let Err(e) = config
                    .storage
                    .internal
                    .kill(&item.envelope, err_msg.clone())
                    .await
                {
                    tracing::error!("Failed to kill batch item: {}", e);
                }
                report_result(&result_tx, item.envelope, JobResultKind::Failed).await;
                drop(item.permit);
            }
        }
    }

    Ok(())
}

async fn process_job<DT, ET>(
    config: Arc<Config<DT, ET>>,
    ctx: ContextValue<DT>,
    result_tx: mpsc::Sender<JobResult>,
    envelope: JobEnvelope,
    permit: OwnedSemaphorePermit,
) -> Result<(), OxanusError>
where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    tracing::debug!(
        job_id = envelope.id,
        latency_ms = envelope.meta.latency_millis(),
        envelope = %serde_json::to_value(&envelope)?,
        "Received envelope"
    );
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
            report_result(&result_tx, envelope, JobResultKind::Failed).await;
            return Ok(());
        }
    };

    let result = executor::run(config, job, &envelope).await?;
    drop(permit);

    let kind = match result {
        Ok(()) => JobResultKind::Success,
        Err(e) => match e {
            ExecutionError::NotPanic(_) => JobResultKind::Failed,
            ExecutionError::Panic() => JobResultKind::Panicked,
        },
    };
    report_result(&result_tx, envelope, kind).await;

    Ok(())
}

async fn report_result(
    result_tx: &mpsc::Sender<JobResult>,
    envelope: JobEnvelope,
    kind: JobResultKind,
) {
    result_tx.send(JobResult { envelope, kind }).await.ok();
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
            QueueKind::Dynamic { prefix, .. } => {
                config
                    .storage
                    .internal
                    .queue_keys(&format!("{prefix}*"))
                    .await?
            }
        };
        let new_queues: HashSet<String> = all_queues.difference(&tracked_queues).cloned().collect();

        for queue in new_queues {
            tracing::info!(
                queue = queue,
                config.throttle = format!("{:?}", queue_config.throttle),
                "Tracking queue"
            );

            tokio::spawn(dispatcher::run(
                Arc::clone(&config),
                queue_config.clone(),
                queue.clone(),
                job_tx.clone(),
                Arc::clone(&semaphores),
            ));

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

        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
