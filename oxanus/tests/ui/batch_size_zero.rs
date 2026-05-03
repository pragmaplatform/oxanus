use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct WorkerContext;

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Serialize, Deserialize, oxanus::Job)]
struct ZeroBatchSizeJob {
    value: String,
}

#[derive(oxanus::Worker)]
#[oxanus(registry = None)]
#[oxanus(batch_size = 0, batch_timeout_ms = 100)]
struct ZeroBatchSizeWorker;

impl ZeroBatchSizeWorker {
    async fn process_batch(
        &self,
        _jobs: Vec<oxanus::BatchItem<ZeroBatchSizeJob>>,
    ) -> Result<(), WorkerError> {
        Ok(())
    }
}

fn main() {}
