use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize, oxanus::Job)]
#[oxanus(resurrect = false)]
struct TickJob {}

#[derive(oxanus::Worker)]
#[oxanus(context = WorkerContext)]
#[oxanus(cron(schedule = "* * * * * *", queue = QueueOne))]
struct TickWorker;

impl TickWorker {
    async fn process(&self, _job: &TickJob, _ctx: &oxanus::JobContext) -> Result<(), WorkerError> {
        println!("tick at {}", chrono::Utc::now().timestamp());
        Ok(())
    }
}

#[derive(Serialize, oxanus::Queue)]
#[oxanus(key = "cron_multiple")]
struct QueueOne;

#[tokio::main]
pub async fn main() -> Result<(), oxanus::OxanusError> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let storage = oxanus::Storage::builder().build_from_env()?;

    let (tx, _rx) = tokio::sync::broadcast::channel::<()>(1);

    let mut handles = Vec::new();

    for _ in 0..4 {
        let storage = storage.clone();
        let mut rx = tx.subscribe();

        handles.push(tokio::spawn(async move {
            let ctx = oxanus::ContextValue::new(WorkerContext {});
            let config =
                ComponentRegistry::build_config(&storage).with_graceful_shutdown(async move {
                    rx.recv().await.ok();
                    Ok(())
                });

            oxanus::run(config, ctx).await
        }));
    }

    tokio::signal::ctrl_c().await.ok();
    tx.send(()).ok();

    for handle in handles {
        handle.await.ok();
    }

    Ok(())
}
