use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct WorkerContext;

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Serialize, Deserialize, oxanus::Job)]
struct MissingBatchSizeJob {
    value: String,
}

#[derive(oxanus::Worker)]
#[oxanus(registry = None)]
#[oxanus(batch_timeout_ms = 100)]
struct MissingBatchSizeWorker;

impl MissingBatchSizeWorker {
    async fn process_batch(
        &self,
        _jobs: Vec<oxanus::BatchItem<MissingBatchSizeJob>>,
    ) -> Result<(), WorkerError> {
        Ok(())
    }
}

fn main() {}
