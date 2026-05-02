// cargo run --package oxanus-web --example web

use serde::{Deserialize, Serialize};

#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<WorkerContext, WorkerError>);

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Clone)]
struct WorkerContext {}

#[derive(Debug, Serialize, Deserialize, oxanus::Job)]
struct PingJob {
    target: String,
}

#[derive(oxanus::Worker)]
struct PingWorker;

impl PingWorker {
    async fn process(&self, _job: PingJob, _ctx: &oxanus::JobContext) -> Result<(), WorkerError> {
        Ok(())
    }
}

#[derive(Serialize, oxanus::Queue)]
#[oxanus(key = "ping", concurrency = 5)]
struct PingQueue;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let storage = oxanus::Storage::builder().build_from_env()?;
    let config = ComponentRegistry::build_config(&storage);
    let catalog = config.catalog();

    let base_path = "/oxanus";
    let oxanus_state =
        oxanus_web::OxanusWebState::new(config.storage.clone(), catalog, base_path.to_string());

    let app = axum::Router::new().nest(base_path, oxanus_web::router(oxanus_state));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    println!("Oxanus web UI available at http://localhost:3000{base_path}");
    axum::serve(listener, app).await?;

    Ok(())
}
