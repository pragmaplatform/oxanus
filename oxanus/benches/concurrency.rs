use deadpool_redis::PoolConfig;
use serde::{Deserialize, Serialize};

fn main() {
    divan::main();
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NoopJob {
    pub sleep_ms: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("Generic error: {0}")]
    GenericError(String),
}

#[derive(Debug, Clone)]
pub struct WorkerState {}

pub struct NoopWorker;

impl oxanus::Job for NoopJob {
    fn worker_name() -> &'static str {
        std::any::type_name::<NoopWorker>()
    }
}

#[async_trait::async_trait]
impl oxanus::Worker<NoopJob> for NoopWorker {
    type Error = ServiceError;

    async fn process(&self, job: &NoopJob, _ctx: &oxanus::JobContext) -> Result<(), ServiceError> {
        tokio::time::sleep(std::time::Duration::from_millis(job.sleep_ms)).await;
        Ok(())
    }
}

impl oxanus::FromContext<WorkerState> for NoopWorker {
    fn from_context(_: &WorkerState) -> Self {
        Self
    }
}

#[derive(Serialize)]
pub struct QueueOne;

impl oxanus::Queue for QueueOne {
    fn to_config() -> oxanus::QueueConfig {
        oxanus::QueueConfig {
            kind: oxanus::QueueKind::Static {
                key: "one".to_string(),
            },
            concurrency: 1,
            throttle: None,
        }
    }
}

const JOBS_COUNTS: &[u64] = &[1000, 10_000];
const CONCURRENCY: &[usize] = &[4, 8, 12, 16, 512];

#[divan::bench(consts = JOBS_COUNTS, args = CONCURRENCY, sample_size = 1, sample_count = 1)]
fn run_jobs_taking_0_ms<const JOBS_COUNT: u64>(bencher: divan::Bencher, n: usize) {
    let rt = &tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let sleep_ms = 0;
    let config = build_config(n);
    rt.block_on(async {
        setup(config, JOBS_COUNT, sleep_ms)
            .await
            .expect("setup failed")
    });

    bencher.bench(|| {
        rt.block_on(async {
            execute(n, JOBS_COUNT).await.expect("execute failed");
        })
    });
}

#[divan::bench(consts = JOBS_COUNTS, args = CONCURRENCY, sample_size = 1, sample_count = 1)]
fn run_jobs_taking_1_ms<const JOBS_COUNT: u64>(bencher: divan::Bencher, n: usize) {
    let rt = &tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let sleep_ms = 1;
    let config = build_config(n);
    rt.block_on(async {
        setup(config, JOBS_COUNT, sleep_ms)
            .await
            .expect("setup failed")
    });

    bencher.bench(|| {
        rt.block_on(async {
            execute(n, JOBS_COUNT).await.expect("execute failed");
        })
    });
}

#[divan::bench(consts = JOBS_COUNTS, args = CONCURRENCY, sample_size = 1, sample_count = 1)]
fn run_jobs_taking_2_ms<const JOBS_COUNT: u64>(bencher: divan::Bencher, n: usize) {
    let rt = &tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let sleep_ms = 2;
    let config = build_config(n);
    rt.block_on(async {
        setup(config, JOBS_COUNT, sleep_ms)
            .await
            .expect("setup failed")
    });

    bencher.bench(|| {
        rt.block_on(async {
            execute(n, JOBS_COUNT).await.expect("execute failed");
        })
    });
}

#[divan::bench(consts = JOBS_COUNTS, args = CONCURRENCY, sample_size = 1, sample_count = 1)]
fn run_jobs_taking_10_ms<const JOBS_COUNT: u64>(bencher: divan::Bencher, n: usize) {
    let rt = &tokio::runtime::Runtime::new().expect("Failed to create runtime");
    let sleep_ms = 10;
    let config = build_config(n);
    rt.block_on(async {
        setup(config, JOBS_COUNT, sleep_ms)
            .await
            .expect("setup failed")
    });

    bencher.bench(|| {
        rt.block_on(async {
            execute(n, JOBS_COUNT).await.expect("execute failed");
        })
    });
}

async fn setup(
    config: oxanus::Config<WorkerState, ServiceError>,
    jobs_count: u64,
    sleep_ms: u64,
) -> Result<(), oxanus::OxanusError> {
    for _ in 0..jobs_count {
        config
            .storage
            .enqueue(QueueOne, NoopJob { sleep_ms })
            .await?;
    }

    Ok(())
}

async fn execute(concurrency: usize, jobs_count: u64) -> Result<(), oxanus::OxanusError> {
    let config = build_config(concurrency).exit_when_processed(jobs_count);
    let ctx = oxanus::ContextValue::new(WorkerState {});

    let stats = oxanus::run(config, ctx).await?;

    assert_eq!(stats.processed, jobs_count);
    assert_eq!(stats.succeeded, jobs_count);
    assert_eq!(stats.failed, 0);

    Ok(())
}

fn redis_pool() -> deadpool_redis::Pool {
    let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL is not set");
    let mut cfg = deadpool_redis::Config::from_url(redis_url);
    cfg.pool = Some(PoolConfig {
        max_size: 512,
        ..Default::default()
    });
    cfg.create_pool(Some(deadpool_redis::Runtime::Tokio1))
        .expect("Failed to create Redis pool")
}

fn build_config(concurrency: usize) -> oxanus::Config<WorkerState, ServiceError> {
    dotenvy::from_filename(".env.test").ok();
    let storage = oxanus::Storage::builder()
        .build_from_pool(redis_pool())
        .expect("Failed to build storage");
    oxanus::Config::new(&storage)
        .register_queue_with_concurrency::<QueueOne>(concurrency)
        .register_worker::<NoopWorker, NoopJob>()
}
