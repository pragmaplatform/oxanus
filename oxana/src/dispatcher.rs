use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::sleep;

use crate::error::OxanaError;
use crate::queue::{QueueConfig, QueueThrottle};
use crate::semaphores_map::QueueControl;
use crate::storage_internal::StorageInternal;
use crate::throttler::Throttler;
use crate::worker_event::WorkerJob;
use crate::{Config, JobId};

pub async fn run<DT, ET>(
    config: Arc<Config<DT, ET>>,
    queue_config: QueueConfig,
    queue_key: String,
    job_tx: mpsc::Sender<WorkerJob>,
    queue_control: Arc<QueueControl>,
) -> Result<(), OxanaError>
where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
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
            result = pop_queue_message(&config.storage.internal, &queue_config, &queue_key) => {
                match config.storage.internal.track_redis_result(result)? {
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
                        sleep(Duration::from_secs(1)).await;
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
) -> Result<Option<JobId>, OxanaError> {
    match &queue_config.throttle {
        Some(throttle) => pop_queue_message_w_throttle(storage, queue_key, throttle).await,
        None => pop_queue_message_wo_throttle(storage, queue_key, 1.0).await,
    }
}

async fn pop_queue_message_wo_throttle(
    storage: &StorageInternal,
    queue_key: &str,
    timeout: f64,
) -> Result<Option<JobId>, OxanaError> {
    storage.blocking_dequeue(queue_key, timeout).await
}

async fn pop_queue_message_w_throttle(
    storage: &StorageInternal,
    queue_key: &str,
    throttle: &QueueThrottle,
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

    sleep(Duration::from_millis(
        u64::try_from(state.throttled_for.unwrap_or(100)).unwrap_or(100),
    ))
    .await;
    Ok(None)
}
