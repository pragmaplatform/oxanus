use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct WorkerContext;

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Serialize, Deserialize, oxanus::Job)]
struct MissingTimeoutJob {
    value: String,
}

#[derive(oxanus::Worker)]
#[oxanus(registry = None)]
#[oxanus(batch_size = 2)]
struct MissingTimeoutWorker;

impl MissingTimeoutWorker {
    async fn process_batch(
        &self,
        _jobs: Vec<oxanus::BatchItem<MissingTimeoutJob>>,
    ) -> Result<(), WorkerError> {
        Ok(())
    }
}

fn main() {}
