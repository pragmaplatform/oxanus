use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct WorkerContext;

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Serialize, Deserialize, oxana::Job)]
struct MissingTimeoutJob {
    value: String,
}

#[derive(oxana::Worker)]
#[oxana(registry = None)]
#[oxana(batch_size = 2)]
struct MissingTimeoutWorker;

impl MissingTimeoutWorker {
    async fn process_batch(
        &self,
        _jobs: Vec<oxana::BatchItem<MissingTimeoutJob>>,
    ) -> Result<(), WorkerError> {
        Ok(())
    }
}

fn main() {}
