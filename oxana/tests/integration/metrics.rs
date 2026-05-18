use crate::shared::*;
use deadpool_redis::redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use testresult::TestResult;

#[derive(Serialize)]
struct MetricsQueueOne;

impl oxana::Queue for MetricsQueueOne {
    fn to_config() -> oxana::QueueConfig {
        oxana::QueueConfig::as_static("metrics_one").concurrency(2)
    }
}

#[derive(Serialize)]
struct MetricsQueueTwo;

impl oxana::Queue for MetricsQueueTwo {
    fn to_config() -> oxana::QueueConfig {
        oxana::QueueConfig::as_static("metrics_two").concurrency(2)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct MetricSuccessJob {
    sleep_ms: u64,
}

impl oxana::Job for MetricSuccessJob {}

struct MetricSuccessWorker;

impl oxana::FromContext<()> for MetricSuccessWorker {
    fn from_context(_ctx: &()) -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl oxana::Worker<MetricSuccessJob> for MetricSuccessWorker {
    type Error = WorkerError;

    async fn run_batch(
        &self,
        jobs: Vec<oxana::BatchItem<MetricSuccessJob>>,
    ) -> Result<(), WorkerError> {
        for item in jobs {
            tokio::time::sleep(std::time::Duration::from_millis(item.job.sleep_ms)).await;
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct MetricFailJob;

impl oxana::Job for MetricFailJob {}

struct MetricFailWorker;

impl oxana::FromContext<()> for MetricFailWorker {
    fn from_context(_ctx: &()) -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl oxana::Worker<MetricFailJob> for MetricFailWorker {
    type Error = WorkerError;

    async fn run_batch(
        &self,
        _jobs: Vec<oxana::BatchItem<MetricFailJob>>,
    ) -> Result<(), WorkerError> {
        Err(WorkerError::Generic("expected failure".to_string()))
    }

    fn max_retries(&self, _job: &MetricFailJob) -> u32 {
        0
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct MetricPanicJob;

impl oxana::Job for MetricPanicJob {}

struct MetricPanicWorker;

impl oxana::FromContext<()> for MetricPanicWorker {
    fn from_context(_ctx: &()) -> Self {
        Self
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct MetricBatchJob {
    sleep_ms: u64,
}

impl oxana::Job for MetricBatchJob {}

struct MetricBatchWorker;

impl oxana::FromContext<()> for MetricBatchWorker {
    fn from_context(_ctx: &()) -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl oxana::Worker<MetricBatchJob> for MetricBatchWorker {
    type Error = WorkerError;

    async fn run_batch(
        &self,
        jobs: Vec<oxana::BatchItem<MetricBatchJob>>,
    ) -> Result<(), WorkerError> {
        let sleep_ms = jobs.first().map_or(0, |item| item.job.sleep_ms);
        tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
        Ok(())
    }

    fn batch_config() -> Option<oxana::WorkerBatchConfig> {
        Some(oxana::WorkerBatchConfig::new(2, Duration::from_secs(1)))
    }
}

#[async_trait::async_trait]
impl oxana::Worker<MetricPanicJob> for MetricPanicWorker {
    type Error = WorkerError;

    async fn run_batch(
        &self,
        _jobs: Vec<oxana::BatchItem<MetricPanicJob>>,
    ) -> Result<(), WorkerError> {
        panic!("expected panic")
    }

    fn max_retries(&self, _job: &MetricPanicJob) -> u32 {
        0
    }
}

#[tokio::test]
async fn test_job_metrics_record_job_counts_execution_counts_and_ttls() -> TestResult {
    let redis_pool = setup();
    let ctx = ();
    let storage = oxana::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool.clone())?;
    let runtime = storage
        .runtime(ctx)
        .queue::<MetricsQueueOne>()
        .queue::<MetricsQueueTwo>()
        .worker::<MetricSuccessWorker, MetricSuccessJob>()
        .worker::<MetricFailWorker, MetricFailJob>()
        .worker::<MetricPanicWorker, MetricPanicJob>()
        .exit_when_processed(4);

    storage
        .enqueue(MetricsQueueOne, MetricSuccessJob { sleep_ms: 25 })
        .await?;
    storage
        .enqueue(MetricsQueueTwo, MetricSuccessJob { sleep_ms: 15 })
        .await?;
    storage.enqueue(MetricsQueueOne, MetricFailJob).await?;
    storage.enqueue(MetricsQueueTwo, MetricPanicJob).await?;

    let run_stats = runtime.run().await?;
    assert_eq!(run_stats.processed, 4);
    assert_eq!(run_stats.failed, 2);
    assert_eq!(run_stats.panicked, 1);

    let stats = storage.stats().await?;
    assert_eq!(stats.global.failed, 2);
    let queue_two = stats
        .queues
        .iter()
        .find(|queue| queue.key == "metrics_two")
        .expect("metrics_two queue should exist");
    assert_eq!(queue_two.failed, 1);
    assert_eq!(queue_two.panicked, 1);

    let snapshot = storage
        .job_metrics(oxana::JobMetricsQuery::default())
        .await?;
    assert_eq!(snapshot.totals.processed, 4);
    assert_eq!(snapshot.totals.failed, 2);
    assert_eq!(snapshot.totals.panicked, 1);
    assert_eq!(snapshot.totals.succeeded, 2);
    assert_eq!(snapshot.totals.successful_executions, 2);
    assert_eq!(snapshot.totals.failed_executions, 2);
    assert_eq!(snapshot.totals.panicked_executions, 1);
    assert!(snapshot.totals.execution_ms >= 30);
    assert_eq!(snapshot.workers.len(), 3);

    let success = oxana::MetricIdentity {
        worker: std::any::type_name::<MetricSuccessJob>().to_string(),
    };
    let failure = oxana::MetricIdentity {
        worker: std::any::type_name::<MetricFailJob>().to_string(),
    };
    let panic = oxana::MetricIdentity {
        worker: std::any::type_name::<MetricPanicJob>().to_string(),
    };

    let success_metrics = storage
        .job_metrics_for(&success, oxana::JobMetricsQuery::default())
        .await?;
    assert_eq!(success_metrics.totals.processed, 2);
    assert_eq!(success_metrics.totals.failed, 0);
    assert_eq!(success_metrics.totals.succeeded, 2);
    assert_eq!(success_metrics.totals.successful_executions, 2);
    assert!(success_metrics.totals.execution_ms >= 30);
    assert_eq!(
        success_metrics
            .histogram
            .iter()
            .map(|bucket| bucket.count)
            .sum::<u64>(),
        2
    );

    let failure_metrics = storage
        .job_metrics_for(&failure, oxana::JobMetricsQuery::default())
        .await?;
    assert_eq!(failure_metrics.totals.processed, 1);
    assert_eq!(failure_metrics.totals.failed, 1);
    assert_eq!(failure_metrics.totals.panicked, 0);
    assert_eq!(failure_metrics.totals.succeeded, 0);
    assert_eq!(failure_metrics.totals.failed_executions, 1);
    assert_eq!(failure_metrics.totals.execution_ms, 0);
    assert_eq!(
        failure_metrics
            .histogram
            .iter()
            .map(|bucket| bucket.count)
            .sum::<u64>(),
        0
    );

    let panic_metrics = storage
        .job_metrics_for(&panic, oxana::JobMetricsQuery::default())
        .await?;
    assert_eq!(panic_metrics.totals.processed, 1);
    assert_eq!(panic_metrics.totals.failed, 1);
    assert_eq!(panic_metrics.totals.panicked, 1);
    assert_eq!(panic_metrics.totals.succeeded, 0);
    assert_eq!(panic_metrics.totals.failed_executions, 1);
    assert_eq!(panic_metrics.totals.panicked_executions, 1);
    assert_eq!(panic_metrics.totals.failed_executions_without_panics(), 0);
    assert_eq!(panic_metrics.totals.execution_ms, 0);

    let mut redis = redis_pool.get().await?;
    let keys: Vec<String> = redis
        .keys(format!("{}:metrics:*", storage.namespace()))
        .await?;
    assert!(!keys.is_empty());

    for key in keys {
        let ttl: i64 = redis.ttl(&key).await?;
        assert!(ttl > 0);
        assert!(ttl <= 24 * 60 * 60);
    }

    Ok(())
}

#[tokio::test]
async fn test_batch_metrics_record_one_execution_time_for_the_batch() -> TestResult {
    let redis_pool = setup();
    let ctx = ();
    let storage = oxana::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool)?;
    let runtime = storage
        .runtime(ctx)
        .queue::<MetricsQueueOne>()
        .worker::<MetricBatchWorker, MetricBatchJob>()
        .exit_when_processed(2);

    storage
        .enqueue(MetricsQueueOne, MetricBatchJob { sleep_ms: 250 })
        .await?;
    storage
        .enqueue(MetricsQueueOne, MetricBatchJob { sleep_ms: 250 })
        .await?;

    let run_stats = runtime.run().await?;
    assert_eq!(run_stats.processed, 2);
    assert_eq!(run_stats.succeeded, 2);

    let identity = oxana::MetricIdentity {
        worker: std::any::type_name::<MetricBatchJob>().to_string(),
    };
    let metrics = storage
        .job_metrics_for(&identity, oxana::JobMetricsQuery::default())
        .await?;
    assert_eq!(metrics.totals.processed, 2);
    assert_eq!(metrics.totals.succeeded, 2);
    assert_eq!(metrics.totals.successful_executions, 1);
    assert!(metrics.totals.execution_ms >= 250);
    assert!(
        metrics.totals.execution_ms < 475,
        "batch execution time should be recorded once, not multiplied by job count"
    );
    assert_eq!(
        metrics
            .histogram
            .iter()
            .map(|bucket| bucket.count)
            .sum::<u64>(),
        1
    );

    Ok(())
}
