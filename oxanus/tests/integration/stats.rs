use crate::shared::*;
use serde::Serialize;
use testresult::TestResult;

#[derive(Serialize)]
struct QueueDynamic(i32);

#[derive(Serialize)]
struct QueueStatic;

impl oxanus::Queue for QueueDynamic {
    fn to_config() -> oxanus::QueueConfig {
        oxanus::QueueConfig::as_dynamic("dynamic").concurrency(6)
    }
}

impl oxanus::Queue for QueueStatic {
    fn to_config() -> oxanus::QueueConfig {
        oxanus::QueueConfig::as_static("static").concurrency(2)
    }
}

#[tokio::test]
pub async fn test_stats() -> TestResult {
    let redis_pool = setup();
    let ctx = oxanus::ContextValue::new(());
    let storage = oxanus::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool)?;
    let config = oxanus::Config::new(&storage)
        .register_queue::<QueueDynamic>()
        .register_queue::<QueueStatic>()
        .register_worker::<WorkerNoop, WorkerNoopJob>()
        .exit_when_processed(8);

    storage.enqueue(QueueDynamic(1), WorkerNoopJob {}).await?;
    storage.enqueue(QueueDynamic(2), WorkerNoopJob {}).await?;
    storage.enqueue(QueueStatic, WorkerNoopJob {}).await?;
    storage.enqueue(QueueStatic, WorkerNoopJob {}).await?;
    storage.enqueue(QueueDynamic(1), WorkerNoopJob {}).await?;
    storage.enqueue(QueueDynamic(2), WorkerNoopJob {}).await?;
    storage.enqueue(QueueDynamic(3), WorkerNoopJob {}).await?;
    storage.enqueue(QueueDynamic(4), WorkerNoopJob {}).await?;

    let stats = storage.stats().await?;

    assert_eq!(stats.queues.len(), 2);
    assert_eq!(stats.queues[0].key, "dynamic");
    assert_eq!(stats.queues[0].queues.len(), 4);

    assert_eq!(stats.queues[0].queues[0].suffix, "1");
    assert_eq!(stats.queues[0].queues[0].enqueued, 2);
    assert_eq!(stats.queues[0].queues[0].processed, 0);
    assert_eq!(stats.queues[0].queues[0].succeeded, 0);
    assert_eq!(stats.queues[0].queues[0].panicked, 0);
    assert_eq!(stats.queues[0].queues[0].failed, 0);
    assert!(stats.queues[0].queues[0].latency_s > 0.0);

    assert_eq!(stats.queues[0].queues[1].suffix, "2");
    assert_eq!(stats.queues[0].queues[1].enqueued, 2);
    assert_eq!(stats.queues[0].queues[1].processed, 0);
    assert_eq!(stats.queues[0].queues[1].succeeded, 0);
    assert_eq!(stats.queues[0].queues[1].panicked, 0);
    assert_eq!(stats.queues[0].queues[1].failed, 0);
    assert!(stats.queues[0].queues[1].latency_s > 0.0);

    assert_eq!(stats.queues[0].queues[2].suffix, "3");
    assert_eq!(stats.queues[0].queues[2].enqueued, 1);
    assert_eq!(stats.queues[0].queues[2].processed, 0);
    assert_eq!(stats.queues[0].queues[2].succeeded, 0);
    assert_eq!(stats.queues[0].queues[2].panicked, 0);
    assert_eq!(stats.queues[0].queues[2].failed, 0);
    assert!(stats.queues[0].queues[2].latency_s > 0.0);

    assert_eq!(stats.queues[0].queues[3].suffix, "4");
    assert_eq!(stats.queues[0].queues[3].enqueued, 1);
    assert_eq!(stats.queues[0].queues[3].processed, 0);
    assert_eq!(stats.queues[0].queues[3].succeeded, 0);
    assert_eq!(stats.queues[0].queues[3].panicked, 0);
    assert_eq!(stats.queues[0].queues[3].failed, 0);
    assert!(stats.queues[0].queues[3].latency_s > 0.0);

    assert_eq!(stats.queues[0].enqueued, 6);
    assert_eq!(stats.queues[0].processed, 0);
    assert_eq!(stats.queues[0].succeeded, 0);
    assert_eq!(stats.queues[0].panicked, 0);
    assert_eq!(stats.queues[0].failed, 0);
    assert!(stats.queues[0].latency_s > 0.0);

    assert_eq!(stats.queues[1].key, "static");
    assert_eq!(stats.queues[1].queues.len(), 0);

    assert_eq!(stats.queues[1].enqueued, 2);
    assert_eq!(stats.queues[1].processed, 0);
    assert_eq!(stats.queues[1].succeeded, 0);
    assert_eq!(stats.queues[1].panicked, 0);
    assert_eq!(stats.queues[1].failed, 0);
    assert!(stats.queues[1].latency_s > 0.0);

    // stats_queues returns all queues, same as stats().queues
    let queue_stats = storage.stats_queues().await?;
    assert_eq!(queue_stats.len(), 2);
    assert_eq!(queue_stats[0].key, "dynamic");
    assert_eq!(queue_stats[0].enqueued, 6);
    assert_eq!(queue_stats[0].queues.len(), 4);
    assert_eq!(queue_stats[1].key, "static");
    assert_eq!(queue_stats[1].enqueued, 2);

    // stats_queues_for with a single pattern
    let queue_stats = storage.stats_queues_for(&["static"]).await?;
    assert_eq!(queue_stats.len(), 1);
    assert_eq!(queue_stats[0].key, "static");
    assert_eq!(queue_stats[0].enqueued, 2);

    // stats_queues_for with a single pattern matching the dynamic queue
    let queue_stats = storage.stats_queues_for(&["dynamic*"]).await?;
    assert_eq!(queue_stats.len(), 1);
    assert_eq!(queue_stats[0].key, "dynamic");
    assert_eq!(queue_stats[0].enqueued, 6);
    assert_eq!(queue_stats[0].queues.len(), 4);

    // stats_queues_for with multiple patterns
    let queue_stats = storage.stats_queues_for(&["static", "dynamic*"]).await?;
    assert_eq!(queue_stats.len(), 2);

    // stats_queues_for with a pattern matching nothing
    let queue_stats = storage.stats_queues_for(&["nonexistent"]).await?;
    assert_eq!(queue_stats.len(), 0);

    let stats = oxanus::run(config, ctx).await?;

    assert_eq!(stats.processed, 8);

    let stats = storage.stats().await?;

    assert_eq!(stats.global.processed, 8);
    assert_eq!(stats.global.failed, 0);

    assert_eq!(stats.queues.len(), 2);
    assert_eq!(stats.queues[0].key, "dynamic");
    assert_eq!(stats.queues[0].queues.len(), 4);

    assert_eq!(stats.queues[0].enqueued, 0);
    assert_eq!(stats.queues[0].processed, 6);
    assert_eq!(stats.queues[0].succeeded, 6);
    assert_eq!(stats.queues[0].panicked, 0);
    assert_eq!(stats.queues[0].failed, 0);
    assert_eq!(stats.queues[0].latency_s, 0.0);

    assert_eq!(stats.queues[0].queues[0].suffix, "1");
    assert_eq!(stats.queues[0].queues[0].enqueued, 0);
    assert_eq!(stats.queues[0].queues[0].processed, 2);
    assert_eq!(stats.queues[0].queues[0].succeeded, 2);
    assert_eq!(stats.queues[0].queues[0].panicked, 0);
    assert_eq!(stats.queues[0].queues[0].failed, 0);
    assert_eq!(stats.queues[0].queues[0].latency_s, 0.0);

    assert_eq!(stats.queues[0].queues[1].suffix, "2");
    assert_eq!(stats.queues[0].queues[1].enqueued, 0);
    assert_eq!(stats.queues[0].queues[1].processed, 2);
    assert_eq!(stats.queues[0].queues[1].succeeded, 2);
    assert_eq!(stats.queues[0].queues[1].panicked, 0);
    assert_eq!(stats.queues[0].queues[1].failed, 0);
    assert_eq!(stats.queues[0].queues[1].latency_s, 0.0);

    assert_eq!(stats.queues[0].queues[2].suffix, "3");
    assert_eq!(stats.queues[0].queues[2].enqueued, 0);
    assert_eq!(stats.queues[0].queues[2].processed, 1);
    assert_eq!(stats.queues[0].queues[2].succeeded, 1);
    assert_eq!(stats.queues[0].queues[2].panicked, 0);
    assert_eq!(stats.queues[0].queues[2].failed, 0);
    assert_eq!(stats.queues[0].queues[2].latency_s, 0.0);

    assert_eq!(stats.queues[0].queues[3].suffix, "4");
    assert_eq!(stats.queues[0].queues[3].enqueued, 0);
    assert_eq!(stats.queues[0].queues[3].processed, 1);
    assert_eq!(stats.queues[0].queues[3].succeeded, 1);
    assert_eq!(stats.queues[0].queues[3].panicked, 0);
    assert_eq!(stats.queues[0].queues[3].failed, 0);
    assert_eq!(stats.queues[0].queues[3].latency_s, 0.0);

    assert_eq!(stats.queues[1].key, "static");
    assert_eq!(stats.queues[1].queues.len(), 0);

    assert_eq!(stats.queues[1].enqueued, 0);
    assert_eq!(stats.queues[1].processed, 2);
    assert_eq!(stats.queues[1].succeeded, 2);
    assert_eq!(stats.queues[1].panicked, 0);
    assert_eq!(stats.queues[1].failed, 0);
    assert_eq!(stats.queues[1].latency_s, 0.0);

    // stats_queues after processing
    let queue_stats = storage.stats_queues().await?;
    assert_eq!(queue_stats.len(), 2);
    assert_eq!(queue_stats[0].key, "dynamic");
    assert_eq!(queue_stats[0].processed, 6);
    assert_eq!(queue_stats[0].succeeded, 6);
    assert_eq!(queue_stats[0].enqueued, 0);
    assert_eq!(queue_stats[1].key, "static");
    assert_eq!(queue_stats[1].processed, 2);
    assert_eq!(queue_stats[1].succeeded, 2);
    assert_eq!(queue_stats[1].enqueued, 0);

    // stats_queues_for after processing -- queues are drained so keys no longer
    // exist in Redis; stats_queues_for only returns stats for queues whose keys
    // still exist, so these should be empty
    let queue_stats = storage.stats_queues_for(&["dynamic*"]).await?;
    assert_eq!(queue_stats.len(), 0);

    let queue_stats = storage.stats_queues_for(&["static"]).await?;
    assert_eq!(queue_stats.len(), 0);

    Ok(())
}
