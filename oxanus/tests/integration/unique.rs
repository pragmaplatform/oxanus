use deadpool_redis::redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use testresult::TestResult;

use crate::shared::*;

// --- Unique Skip ---

#[derive(Debug, Serialize, Deserialize)]
pub struct UniqueSkipJob {
    pub id: i32,
    pub key: String,
    pub value: i32,
}

pub struct UniqueSkipWorker {
    pub state: WorkerState,
}

impl oxanus::Job for UniqueSkipJob {
    fn worker_name() -> &'static str {
        std::any::type_name::<UniqueSkipWorker>()
    }

    fn unique_id(&self) -> Option<String> {
        Some(format!("unique:{}", self.id))
    }

    fn on_conflict(&self) -> oxanus::JobConflictStrategy {
        oxanus::JobConflictStrategy::Skip
    }
}

#[async_trait::async_trait]
impl oxanus::Worker<UniqueSkipJob> for UniqueSkipWorker {
    type Error = WorkerError;

    async fn process(
        &self,
        job: &UniqueSkipJob,
        _ctx: &oxanus::JobContext,
    ) -> Result<(), WorkerError> {
        let mut redis = self.state.redis.get().await?;
        let _: () = redis.set_ex(&job.key, job.value.to_string(), 3).await?;
        Ok(())
    }

    fn retry_delay(&self, _retries: u32) -> u64 {
        0
    }

    fn max_retries(&self) -> u32 {
        0
    }
}

impl oxanus::FromContext<WorkerState> for UniqueSkipWorker {
    fn from_context(ctx: &WorkerState) -> Self {
        Self { state: ctx.clone() }
    }
}

// --- Unique Replace ---

#[derive(Debug, Serialize, Deserialize)]
pub struct UniqueReplaceJob {
    pub id: i32,
    pub key: String,
    pub value: i32,
}

pub struct UniqueReplaceWorker {
    pub state: WorkerState,
}

impl oxanus::Job for UniqueReplaceJob {
    fn worker_name() -> &'static str {
        std::any::type_name::<UniqueReplaceWorker>()
    }

    fn unique_id(&self) -> Option<String> {
        Some(format!("unique:{}", self.id))
    }

    fn on_conflict(&self) -> oxanus::JobConflictStrategy {
        oxanus::JobConflictStrategy::Replace
    }
}

#[async_trait::async_trait]
impl oxanus::Worker<UniqueReplaceJob> for UniqueReplaceWorker {
    type Error = WorkerError;

    async fn process(
        &self,
        job: &UniqueReplaceJob,
        _ctx: &oxanus::JobContext,
    ) -> Result<(), WorkerError> {
        let mut redis = self.state.redis.get().await?;
        let _: () = redis.set_ex(&job.key, job.value.to_string(), 3).await?;
        Ok(())
    }

    fn retry_delay(&self, _retries: u32) -> u64 {
        0
    }

    fn max_retries(&self) -> u32 {
        0
    }
}

impl oxanus::FromContext<WorkerState> for UniqueReplaceWorker {
    fn from_context(ctx: &WorkerState) -> Self {
        Self { state: ctx.clone() }
    }
}

// --- Tests ---

#[tokio::test]
pub async fn test_unique_skip() -> TestResult {
    let redis_pool = setup();
    let mut redis_conn = redis_pool.get().await?;
    let ctx = oxanus::ContextValue::new(WorkerState {
        redis: redis_pool.clone(),
    });
    let storage = oxanus::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool.clone())?;
    let config = oxanus::Config::new(&storage)
        .register_queue::<QueueOne>()
        .register_worker::<UniqueSkipWorker, UniqueSkipJob>()
        .exit_when_processed(2);
    let key1 = random_string();
    let key2 = random_string();

    storage
        .enqueue(
            QueueOne,
            UniqueSkipJob {
                id: 1,
                key: key1.clone(),
                value: 1,
            },
        )
        .await?;
    storage
        .enqueue(
            QueueOne,
            UniqueSkipJob {
                id: 1,
                key: key1.clone(),
                value: 2,
            },
        )
        .await?;
    storage
        .enqueue(
            QueueOne,
            UniqueSkipJob {
                id: 2,
                key: key2.clone(),
                value: 3,
            },
        )
        .await?;
    storage
        .enqueue(
            QueueOne,
            UniqueSkipJob {
                id: 2,
                key: key2.clone(),
                value: 4,
            },
        )
        .await?;

    assert_eq!(storage.enqueued_count(QueueOne).await?, 2);

    oxanus::run(config, ctx).await?;

    assert_eq!(storage.dead_count().await?, 0);
    assert_eq!(storage.enqueued_count(QueueOne).await?, 0);
    assert_eq!(storage.jobs_count().await?, 0);

    let value: Option<i32> = redis_conn.get(key1).await?;
    assert_eq!(value, Some(1));
    let value: Option<i32> = redis_conn.get(key2).await?;
    assert_eq!(value, Some(3));

    Ok(())
}

#[tokio::test]
pub async fn test_unique_replace() -> TestResult {
    let redis_pool = setup();
    let mut redis_conn = redis_pool.get().await?;
    let ctx = oxanus::ContextValue::new(WorkerState {
        redis: redis_pool.clone(),
    });
    let storage = oxanus::Storage::builder()
        .namespace(random_string())
        .build_from_pool(redis_pool)?;
    let config = oxanus::Config::new(&storage)
        .register_queue::<QueueOne>()
        .register_worker::<UniqueReplaceWorker, UniqueReplaceJob>()
        .exit_when_processed(2);

    let key1 = random_string();
    let key2 = random_string();

    storage
        .enqueue(
            QueueOne,
            UniqueReplaceJob {
                id: 1,
                key: key1.clone(),
                value: 1,
            },
        )
        .await?;
    storage
        .enqueue(
            QueueOne,
            UniqueReplaceJob {
                id: 1,
                key: key1.clone(),
                value: 2,
            },
        )
        .await?;
    storage
        .enqueue(
            QueueOne,
            UniqueReplaceJob {
                id: 2,
                key: key2.clone(),
                value: 3,
            },
        )
        .await?;
    storage
        .enqueue(
            QueueOne,
            UniqueReplaceJob {
                id: 2,
                key: key2.clone(),
                value: 4,
            },
        )
        .await?;

    assert_eq!(storage.enqueued_count(QueueOne).await?, 2);

    oxanus::run(config, ctx).await?;

    assert_eq!(storage.dead_count().await?, 0);
    assert_eq!(storage.enqueued_count(QueueOne).await?, 0);
    assert_eq!(storage.jobs_count().await?, 0);

    let value: Option<i32> = redis_conn.get(key1).await?;
    assert_eq!(value, Some(2));
    let value: Option<i32> = redis_conn.get(key2).await?;
    assert_eq!(value, Some(4));

    Ok(())
}
