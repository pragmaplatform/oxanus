use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tokio::time::{Duration, Instant, MissedTickBehavior};

use crate::{OxanusError, config::Config, metrics::JobMetricsBuffer};

#[derive(Default, Debug)]
pub struct Stats {
    pub processed: u64,
    pub succeeded: u64,
    pub panicked: u64,
    pub failed: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct WorkerResult {
    pub kind: WorkerResultKind,
    pub worker_name: String,
    pub queue: String,
    pub execution_ms: u64,
    pub job_count: u64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkerResultKind {
    Success,
    Panicked,
    Failed,
}

pub async fn run<DT, ET>(
    mut rx: mpsc::Receiver<WorkerResult>,
    config: Arc<Config<DT, ET>>,
    stats: Arc<Mutex<Stats>>,
) -> Result<(), OxanusError>
where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    let mut metrics = JobMetricsBuffer::default();
    let mut flush_interval = tokio::time::interval_at(
        Instant::now() + Duration::from_secs(5),
        Duration::from_secs(5),
    );
    flush_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Some(result) => {
                        metrics.record(&result);
                        config.storage.internal.track_redis_result(
                            update_stats(Arc::clone(&config), Arc::clone(&stats), &result).await
                        )?;
                    }
                    None => {
                        flush_metrics(Arc::clone(&config), &mut metrics).await?;
                        return Ok(());
                    }
                }
            }
            _ = flush_interval.tick() => {
                flush_metrics(Arc::clone(&config), &mut metrics).await?;
            }
        }
    }
}

async fn update_stats<DT, ET>(
    config: Arc<Config<DT, ET>>,
    stats: Arc<Mutex<Stats>>,
    result: &WorkerResult,
) -> Result<(), OxanusError>
where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    let processed = {
        let mut stats = stats.lock().await;
        stats.processed = stats.processed.saturating_add(result.job_count);
        match result.kind {
            WorkerResultKind::Success => {
                stats.succeeded = stats.succeeded.saturating_add(result.job_count);
            }
            WorkerResultKind::Panicked => {
                stats.panicked = stats.panicked.saturating_add(result.job_count);
                stats.failed = stats.failed.saturating_add(result.job_count);
            }
            WorkerResultKind::Failed => {
                stats.failed = stats.failed.saturating_add(result.job_count);
            }
        }

        stats.processed
    };

    config.storage.internal.update_stats(result).await?;

    if let Some(exit_when_processed) = config.exit_when_processed
        && processed >= exit_when_processed
    {
        config.cancel_token.cancel();
    }

    Ok(())
}

async fn flush_metrics<DT, ET>(
    config: Arc<Config<DT, ET>>,
    metrics: &mut JobMetricsBuffer,
) -> Result<(), OxanusError>
where
    DT: Send + Sync + Clone + 'static,
    ET: std::error::Error + Send + Sync + 'static,
{
    if metrics.is_empty() {
        return Ok(());
    }

    if config
        .storage
        .internal
        .track_redis_result(config.storage.internal.flush_job_metrics(metrics).await)?
        .is_some()
    {
        metrics.clear();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{JobMetricsQuery, MetricIdentity};
    use crate::test_helper::{random_string, redis_pool};
    use crate::{Config, Storage};
    use testresult::TestResult;

    #[tokio::test]
    async fn result_collector_drains_results_after_cancellation() -> TestResult {
        let storage = Storage::builder()
            .namespace(random_string())
            .build_from_pool(redis_pool().await?)?;
        let config: Arc<Config<(), std::io::Error>> =
            Arc::new(Config::new(&storage).exit_when_processed(1));
        let stats = Arc::new(Mutex::new(Stats::default()));
        let (tx, rx) = mpsc::channel(2);
        let collector = tokio::spawn(run(rx, Arc::clone(&config), Arc::clone(&stats)));

        tx.send(WorkerResult {
            kind: WorkerResultKind::Success,
            worker_name: "CollectorWorker".to_string(),
            queue: "collector".to_string(),
            execution_ms: 25,
            job_count: 1,
        })
        .await
        .expect("collector should receive first result");

        tokio::time::timeout(Duration::from_secs(1), config.cancel_token.cancelled())
            .await
            .expect("first result should trigger cancellation");

        tx.send(WorkerResult {
            kind: WorkerResultKind::Success,
            worker_name: "CollectorWorker".to_string(),
            queue: "collector".to_string(),
            execution_ms: 50,
            job_count: 1,
        })
        .await
        .expect("collector should keep receiving after cancellation");
        drop(tx);

        collector.await??;

        let stats = stats.lock().await;
        assert_eq!(stats.processed, 2);
        assert_eq!(stats.succeeded, 2);
        drop(stats);

        let metrics = storage
            .job_metrics_for(
                &MetricIdentity {
                    worker: "CollectorWorker".to_string(),
                },
                JobMetricsQuery::default(),
            )
            .await?;
        assert_eq!(metrics.totals.processed, 2);
        assert_eq!(metrics.totals.succeeded, 2);
        assert_eq!(metrics.totals.successful_executions, 2);
        assert_eq!(metrics.totals.execution_ms, 75);

        Ok(())
    }
}
