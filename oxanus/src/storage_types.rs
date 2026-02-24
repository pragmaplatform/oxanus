use serde::{Deserialize, Serialize};

/// Options for listing jobs in a queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueListOpts {
    /// Maximum number of jobs to return.
    pub count: usize,
    /// Number of jobs to skip from the start.
    pub offset: usize,
}
