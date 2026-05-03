use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct WorkerContext;

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Serialize, Deserialize, oxanus::Job)]
struct MissingProcessBatchJob {
    value: String,
}

#[derive(oxanus::Worker)]
#[oxanus(registry = None)]
#[oxanus(batch_size = 2, batch_timeout_ms = 100)]
struct MissingProcessBatchWorker;

fn main() {}
