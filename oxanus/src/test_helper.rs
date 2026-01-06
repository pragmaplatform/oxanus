use crate::{
    Context, Storage, Worker, context::JobState, job_envelope::JobEnvelope,
    storage_internal::StorageInternal,
};
use rand::distr::{Alphanumeric, SampleString};
use serde::Serialize;

pub fn random_string() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), 16)
}

pub async fn redis_pool() -> Result<deadpool_redis::Pool, deadpool_redis::PoolError> {
    dotenvy::from_filename(".env.test").ok();
    let redis_url = std::env::var("REDIS_URL").expect("REDIS_URL is not set");
    let cfg = deadpool_redis::Config::from_url(redis_url);
    let pool = cfg
        .create_pool(Some(deadpool_redis::Runtime::Tokio1))
        .unwrap();

    Ok(pool)
}

pub async fn create_worker_context<W>(ctx: W::Context, worker: W) -> Context<W::Context>
where
    W: Worker + Serialize + 'static,
{
    let internal = StorageInternal::new(redis_pool().await.unwrap(), Some(random_string()));
    let queue = random_string();
    let envelope = JobEnvelope::new(queue.clone(), worker).unwrap();
    let state = JobState::new(
        Storage { internal },
        envelope.id,
        envelope.meta.state.clone(),
    );
    Context {
        ctx: ctx.clone(),
        meta: envelope.meta,
        state,
    }
}
