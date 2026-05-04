use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct WorkerContext;

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Serialize, Deserialize, oxana::Job)]
struct MissingBatchSizeJob {
    value: String,
}

#[derive(oxana::Worker)]
#[oxana(registry = None)]
#[oxana(batch_timeout_ms = 100)]
struct MissingBatchSizeWorker;

impl MissingBatchSizeWorker {
    async fn process_batch(
        &self,
        _jobs: Vec<oxana::BatchItem<MissingBatchSizeJob>>,
    ) -> Result<(), WorkerError> {
        Ok(())
    }
}

fn main() {}
