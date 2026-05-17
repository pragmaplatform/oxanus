use crate::shared::*;
use deadpool_redis::redis::AsyncCommands;
use oxana::Queue as _;
use std::time::Duration;
use testresult::TestResult;

#[derive(serde::Serialize)]
struct DynamicConcurrencyQueue;

impl oxana::Queue for DynamicConcurrencyQueue {
    fn to_config() -> oxana::QueueConfig {
        oxana::QueueConfig::as_static("dynamic_concurrency").dynamic_concurrency(3)
    }
}

#[derive(serde::Serialize)]
struct DynamicTenantBase;

impl oxana::Queue for DynamicTenantBase {
    fn key(&self) -> String {
        "tenant".to_string()
    }

    fn to_config() -> oxana::QueueConfig {
        oxana::QueueConfig::as_dynamic("tenant").dynamic_concurrency(3)
    }
}

#[derive(serde::Serialize)]
struct DynamicTenantQueue {
    tenant: String,
}

impl oxana::Queue for DynamicTenantQueue {
    fn to_config() -> oxana::QueueConfig {
        oxana::QueueConfig::as_dynamic("tenant").dynamic_concurrency(3)
    }
}

#[tokio::test]
pub async fn test_standard() -> TestResult {
    let redis_pool = setup();
    let mut redis_conn = redis_pool.get().await?;

    let ctx = oxana::ContextValue::new(WorkerState {
        redis: redis_pool.clone(),
    });

    let storage = oxana::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool.clone())?
        .register_queue::<QueueOne>()
        .register_worker::<WorkerRedisSet, WorkerRedisSetJob, WorkerState>()
        .exit_when_processed(1);

    let random_key = uuid::Uuid::new_v4().to_string();
    let random_value = uuid::Uuid::new_v4().to_string();

    storage
        .enqueue(
            QueueOne,
            WorkerRedisSetJob {
                key: random_key.clone(),
                value: random_value.clone(),
            },
        )
        .await?;

    assert_eq!(storage.enqueued_count(QueueOne).await?, 1);

    storage.clone().run(ctx).await?;

    let value: Option<String> = redis_conn.get(random_key).await?;

    assert_eq!(value, Some(random_value));
    assert_eq!(storage.enqueued_count(QueueOne).await?, 0);
    assert_eq!(storage.jobs_count().await?, 0);

    Ok(())
}

#[tokio::test]
pub async fn test_paused_queue_resumes_after_runtime_config_update() -> TestResult {
    let redis_pool = setup();
    let mut redis_conn = redis_pool.get().await?;

    let ctx = oxana::ContextValue::new(WorkerState {
        redis: redis_pool.clone(),
    });

    let storage = oxana::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool.clone())?
        .register_queue::<QueueOne>()
        .register_worker::<WorkerRedisSet, WorkerRedisSetJob, WorkerState>()
        .exit_when_processed(1);
    storage
        .set_queue_state(QueueOne, oxana::QueueState::Paused)
        .await?;

    let random_key = uuid::Uuid::new_v4().to_string();
    let random_value = uuid::Uuid::new_v4().to_string();

    storage
        .enqueue(
            QueueOne,
            WorkerRedisSetJob {
                key: random_key.clone(),
                value: random_value.clone(),
            },
        )
        .await?;

    let run_storage = storage.clone();
    let handle = tokio::spawn(async move { run_storage.run(ctx).await });

    tokio::time::sleep(Duration::from_millis(500)).await;
    let value: Option<String> = redis_conn.get(&random_key).await?;
    assert_eq!(value, None);
    assert_eq!(storage.enqueued_count(QueueOne).await?, 1);

    storage.unpause_queue(QueueOne).await?;

    tokio::time::timeout(Duration::from_secs(5), handle).await???;
    let value: Option<String> = redis_conn.get(random_key).await?;

    assert_eq!(value, Some(random_value));
    assert_eq!(storage.enqueued_count(QueueOne).await?, 0);

    Ok(())
}

#[tokio::test]
pub async fn test_fixed_queue_rejects_runtime_concurrency_override() -> TestResult {
    let redis_pool = setup();
    let storage = oxana::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool)?;

    let error = storage
        .set_queue_concurrency(QueueOne, 2)
        .await
        .expect_err("fixed queues should reject runtime concurrency overrides");

    assert!(matches!(error, oxana::OxanaError::ConfigError(_)));

    Ok(())
}

#[tokio::test]
pub async fn test_dynamic_queue_unsets_runtime_concurrency_when_default_is_set() -> TestResult {
    let redis_pool = setup();
    let storage = oxana::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool)?;
    let queue_key = "dynamic_concurrency".to_string();

    storage
        .set_queue_concurrency(DynamicConcurrencyQueue, 3)
        .await?;
    let configs = storage
        .queue_configs(std::slice::from_ref(&queue_key))
        .await?;
    assert!(configs.is_empty());

    storage
        .set_queue_concurrency(DynamicConcurrencyQueue, 5)
        .await?;
    let configs = storage
        .queue_configs(std::slice::from_ref(&queue_key))
        .await?;
    assert_eq!(
        configs
            .get(&queue_key)
            .and_then(|config| config.concurrency),
        Some(5)
    );

    storage
        .set_queue_state(DynamicConcurrencyQueue, oxana::QueueState::Paused)
        .await?;
    storage
        .set_queue_concurrency(DynamicConcurrencyQueue, 3)
        .await?;

    let configs = storage
        .queue_configs(std::slice::from_ref(&queue_key))
        .await?;
    let config = configs
        .get(&queue_key)
        .expect("queue state should preserve runtime config");
    assert_eq!(config.concurrency, None);
    assert_eq!(config.state, oxana::QueueState::Paused);

    storage.reset_queue_config(DynamicConcurrencyQueue).await?;
    let configs = storage
        .queue_configs(std::slice::from_ref(&queue_key))
        .await?;
    assert!(configs.is_empty());

    Ok(())
}

#[tokio::test]
pub async fn test_dynamic_child_queue_unsets_runtime_concurrency_when_inherited_default_is_set()
-> TestResult {
    let redis_pool = setup();
    let storage = oxana::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool)?;
    let child_queue = DynamicTenantQueue {
        tenant: "acme".to_string(),
    };
    let child_queue_key = child_queue.key();

    storage.set_queue_concurrency(DynamicTenantBase, 5).await?;
    storage.set_queue_concurrency(child_queue, 5).await?;

    let configs = storage
        .queue_configs(std::slice::from_ref(&child_queue_key))
        .await?;
    assert!(configs.is_empty());

    let child_queue = DynamicTenantQueue {
        tenant: "acme".to_string(),
    };
    storage.set_queue_concurrency(child_queue, 9).await?;
    let configs = storage
        .queue_configs(std::slice::from_ref(&child_queue_key))
        .await?;
    assert_eq!(
        configs
            .get(&child_queue_key)
            .and_then(|config| config.concurrency),
        Some(9)
    );

    let child_queue = DynamicTenantQueue {
        tenant: "acme".to_string(),
    };
    storage.set_queue_concurrency(child_queue, 5).await?;
    let configs = storage
        .queue_configs(std::slice::from_ref(&child_queue_key))
        .await?;
    let config = configs
        .get(&child_queue_key)
        .expect("child runtime config should remain after clearing override");
    assert_eq!(config.concurrency, None);

    Ok(())
}
