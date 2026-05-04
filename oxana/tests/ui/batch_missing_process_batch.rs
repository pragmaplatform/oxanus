use serde::{Deserialize, Serialize};

#[derive(Clone)]
struct WorkerContext;

#[derive(Debug, thiserror::Error)]
enum WorkerError {}

#[derive(Debug, Serialize, Deserialize, oxana::Job)]
struct MissingProcessBatchJob {
    value: String,
}

#[derive(oxana::Worker)]
#[oxana(registry = None)]
#[oxana(batch_size = 2, batch_timeout_ms = 100)]
struct MissingProcessBatchWorker;

fn main() {}
