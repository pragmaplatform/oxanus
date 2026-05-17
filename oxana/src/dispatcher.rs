use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;

use crate::JobId;
use crate::error::OxanaError;
use crate::queue::{QueueConfig, QueueThrottle};
use crate::runtime::Runtime;
use crate::semaphores_map::QueueControl;
use crate::storage_internal::StorageInternal;
use crate::throttler::Throttler;
use crate::worker_event::WorkerJob;

pub async fn run<DT>(
    config: Arc<Runtime<DT>>,
    queue_config: QueueConfig,
    queue_key: String,
    job_tx: mpsc::Sender<WorkerJob>,
    queue_control: Arc<QueueControl>,
) -> Result<(), OxanaError>
where
    DT: Send + Sync + Clone + 'static,
{
    loop {
        let permit = tokio::select! {
            permit = queue_control.acquire() => permit,
            _ = config.cancel_token.cancelled() => {
                tracing::debug!("Stopping dispatcher for queue {}", queue_key);
                break;
            }
        };

        tokio::select! {
            result = pop_queue_message(&config.storage.internal, &queue_config, &queue_key, config.settings.dequeue_timeout, config.settings.throttled_queue_fallback_wait) => {
                match config.storage.internal.track_redis_result(result, config.settings.redis_failure_tolerance)? {
                    Some(Some(job_id)) => {
                        let job = WorkerJob { job_id, permit };
                        job_tx
                            .send(job)
                            .await
                            .expect("Failed to send job to worker");
                    }
                    Some(None) => {
                        drop(permit);
                    }
                    None => {
                        drop(permit);
                        sleep(config.settings.dispatcher_idle_sleep).await;
                    }
                }
            }
            _ = config.cancel_token.cancelled() => {
                tracing::debug!("Stopping dispatcher for queue {}", queue_key);
                drop(permit);
                break;
            }
        }
    }

    Ok(())
}

async fn pop_queue_message(
    storage: &StorageInternal,
    queue_config: &QueueConfig,
    queue_key: &str,
    dequeue_timeout: std::time::Duration,
    throttled_queue_fallback_wait: std::time::Duration,
) -> Result<Option<JobId>, OxanaError> {
    match &queue_config.throttle {
        Some(throttle) => {
            pop_queue_message_w_throttle(
                storage,
                queue_key,
                throttle,
                throttled_queue_fallback_wait,
            )
            .await
        }
        None => pop_queue_message_wo_throttle(storage, queue_key, dequeue_timeout).await,
    }
}

async fn pop_queue_message_wo_throttle(
    storage: &StorageInternal,
    queue_key: &str,
    timeout: Duration,
) -> Result<Option<JobId>, OxanaError> {
    storage
        .blocking_dequeue(queue_key, timeout.as_secs_f64())
        .await
}

async fn pop_queue_message_w_throttle(
    storage: &StorageInternal,
    queue_key: &str,
    throttle: &QueueThrottle,
    fallback_wait: Duration,
) -> Result<Option<JobId>, OxanaError> {
    let pool = storage.pool().await?;
    let throttler = Throttler::new(pool, queue_key, throttle.limit, throttle.window_ms);

    let state = throttler.state().await?;

    if state.is_allowed
        && let Some(job_id) = storage.dequeue(queue_key).await?
    {
        let cost = storage
            .get_job(&job_id)
            .await?
            .and_then(|envelope| envelope.meta.throttle_cost);
        throttler.consume(cost).await?;
        return Ok(Some(job_id));
    }

    let wait = state
        .throttled_for
        .and_then(|millis| u64::try_from(millis).ok())
        .map_or(fallback_wait, Duration::from_millis);
    sleep(wait).await;
    Ok(None)
}
