/// Centralizes the Redis key naming scheme used by `StorageInternal`.
#[derive(Clone)]
pub(crate) struct StorageKeys {
    /// Normalized namespace prefix applied to every Redis key
    /// (e.g. `oxanus` or `oxanus:<custom>`).
    pub(crate) namespace: String,
    /// Redis hash that stores serialized `JobEnvelope` values keyed by `JobId`.
    pub(crate) jobs: String,
    /// Redis list acting as the dead-letter queue containing JSON `JobEnvelope`s.
    pub(crate) dead: String,
    /// Redis sorted set (ZSET) of job IDs scheduled for future execution,
    /// scored by their target timestamp in microseconds.
    pub(crate) schedule: String,
    /// Redis sorted set (ZSET) of job IDs queued for retry,
    /// scored by the retry timestamp in microseconds.
    pub(crate) retry: String,
    /// Prefix for Redis list keys that hold enqueued job IDs
    /// (actual keys look like `{queue_prefix}:<queue>`).
    pub(crate) queue_prefix: String,
    /// Prefix for Redis list keys tracking jobs currently processed by a worker
    /// process (keys look like `{processing_queue_prefix}:<process_id>`).
    pub(crate) processing_queue_prefix: String,
    /// Redis sorted set (ZSET) of active process IDs scored by their last heartbeat.
    pub(crate) processes: String,
    /// Redis hash that stores serialized `Process` metadata keyed by process ID.
    pub(crate) processes_data: String,
    /// Redis hash that stores per-queue counters (processed, succeeded, panicked,
    /// failed) keyed as `<queue_full_key>:<metric>`.
    pub(crate) stats: String,
}

impl StorageKeys {
    /// Builds a namespaced collection of Redis keys, defaulting to the `oxanus`
    /// namespace when none is provided.
    pub(crate) fn new(namespace: impl Into<String>) -> Self {
        let namespace = namespace.into();
        let namespace = if namespace.is_empty() {
            "oxanus".to_string()
        } else {
            format!("oxanus:{namespace}")
        };

        Self {
            jobs: format!("{namespace}:jobs"),
            dead: format!("{namespace}:dead"),
            schedule: format!("{namespace}:schedule"),
            retry: format!("{namespace}:retry"),
            queue_prefix: format!("{namespace}:queue"),
            processing_queue_prefix: format!("{namespace}:processing"),
            processes: format!("{namespace}:processes"),
            processes_data: format!("{namespace}:processes_data"),
            stats: format!("{namespace}:stats"),
            namespace,
        }
    }
}
