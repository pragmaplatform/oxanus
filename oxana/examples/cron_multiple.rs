use serde::{Deserialize, Serialize};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(oxana::Registry)]
struct ComponentRegistry(oxana::ComponentRegistry<WorkerContext>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize, oxana::Job)]
#[oxana(resurrect = false)]
struct TickJob {}

#[derive(oxana::Worker)]
#[oxana(context = WorkerContext)]
#[oxana(cron(schedule = "* * * * * *", queue = QueueOne))]
struct TickWorker;

impl TickWorker {
    async fn process(&self, _job: TickJob, _ctx: &oxana::JobContext) -> Result<(), WorkerError> {
        println!("tick at {}", chrono::Utc::now().timestamp());
        Ok(())
    }
}

#[derive(Serialize, oxana::Queue)]
#[oxana(key = "cron_multiple")]
struct QueueOne;

#[tokio::main]
pub async fn main() -> Result<(), oxana::OxanaError> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let storage = oxana::Storage::builder().build_from_env()?;

    let (tx, _rx) = tokio::sync::broadcast::channel::<()>(1);

    let mut handles = Vec::new();

    for _ in 0..4 {
        let storage = storage.clone();
        let mut rx = tx.subscribe();

        handles.push(tokio::spawn(async move {
            let ctx = WorkerContext {};
            let runtime = storage
                .runtime(ctx)
                .register::<ComponentRegistry>()
                .shutdown_on(async move {
                    rx.recv().await.ok();
                    Ok(())
                });

            runtime.run().await
        }));
    }

    tokio::signal::ctrl_c().await.ok();
    tx.send(()).ok();

    for handle in handles {
        handle.await.ok();
    }

    Ok(())
}
