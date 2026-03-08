use deadpool_redis::redis::AsyncCommands;
use rand::distr::{Alphanumeric, SampleString};
use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error("Generic error: {0}")]
    Generic(String),
    #[error("Redis error: {0}")]
    Redis(#[from] deadpool_redis::redis::RedisError),
    #[error("Redis error: {0}")]
    Pool(#[from] deadpool_redis::PoolError),
}

#[derive(Clone)]
pub struct WorkerState {
    pub redis: deadpool_redis::Pool,
}

// --- NoopJob + NoopWorker ---

#[derive(Serialize, Deserialize)]
pub struct NoopJob {}

pub struct NoopWorker;

impl oxanus::Job for NoopJob {
    fn worker_name() -> &'static str {
        std::any::type_name::<NoopWorker>()
    }
}

#[async_trait::async_trait]
impl oxanus::Worker<NoopJob> for NoopWorker {
    type Error = WorkerError;

    async fn process(&self, _: &NoopJob, _: &oxanus::JobContext) -> Result<(), WorkerError> {
        Ok(())
    }
}

impl oxanus::FromContext<()> for NoopWorker {
    fn from_context(_: &()) -> Self {
        Self
    }
}

// --- RedisSetJob + RedisSetWorker ---

#[derive(Debug, Serialize, Deserialize)]
pub struct RedisSetJob {
    pub key: String,
    pub value: String,
}

pub struct RedisSetWorker {
    pub state: WorkerState,
}

impl oxanus::Job for RedisSetJob {
    fn worker_name() -> &'static str {
        std::any::type_name::<RedisSetWorker>()
    }
}

#[async_trait::async_trait]
impl oxanus::Worker<RedisSetJob> for RedisSetWorker {
    type Error = WorkerError;

    async fn process(
        &self,
        job: &RedisSetJob,
        _ctx: &oxanus::JobContext,
    ) -> Result<(), WorkerError> {
        let mut redis = self.state.redis.get().await?;
        let _: () = redis.set_ex(&job.key, job.value.clone(), 3).await?;
        Ok(())
    }
}

impl oxanus::FromContext<WorkerState> for RedisSetWorker {
    fn from_context(ctx: &WorkerState) -> Self {
        Self { state: ctx.clone() }
    }
}

// --- Queue ---

#[derive(Serialize)]
pub struct QueueOne;

impl oxanus::Queue for QueueOne {
    fn to_config() -> oxanus::QueueConfig {
        oxanus::QueueConfig::as_static("one")
    }
}

// --- Setup ---

pub fn setup() -> deadpool_redis::Pool {
    dotenvy::from_filename(".env.test").ok();

    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .try_init()
        .ok();

    redis_pool()
}

pub fn redis_pool() -> deadpool_redis::Pool {
    let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL is not set");
    let mut cfg = deadpool_redis::Config::from_url(redis_url);
    cfg.pool = Some(deadpool_redis::PoolConfig {
        max_size: 10,
        timeouts: deadpool_redis::Timeouts {
            wait: Some(std::time::Duration::from_millis(50)),
            create: Some(std::time::Duration::from_millis(50)),
            recycle: Some(std::time::Duration::from_millis(50)),
        },
        ..Default::default()
    });
    cfg.create_pool(Some(deadpool_redis::Runtime::Tokio1))
        .expect("Failed to create Redis pool")
}

pub fn random_string() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), 16)
}
