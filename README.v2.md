# Oxanus
[![Build Status](https://img.shields.io/github/actions/workflow/status/pragmaplatform/oxanus/test.yml?branch=main)](https://github.com/pragmaplatform/oxanus/actions)
[![Latest Version](https://img.shields.io/crates/v/oxanus.svg)](https://crates.io/crates/oxanus)
[![docs.rs](https://img.shields.io/static/v1?label=docs.rs&message=oxanus&color=blue&logo=data:image/svg+xml;base64,PHN2ZyByb2xlPSJpbWciIHhtbG5zPSJodHRwOi8vd3d3LnczLm9yZy8yMDAwL3N2ZyIgdmlld0JveD0iMCAwIDUxMiA1MTIiPjxwYXRoIGZpbGw9IiNmNWY1ZjUiIGQ9Ik00ODguNiAyNTAuMkwzOTIgMjE0VjEwNS41YzAtMTUtOS4zLTI4LjQtMjMuNC0zMy43bC0xMDAtMzcuNWMtOC4xLTMuMS0xNy4xLTMuMS0yNS4zIDBsLTEwMCAzNy41Yy0xNC4xIDUuMy0yMy40IDE4LjctMjMuNCAzMy43VjIxNGwtOTYuNiAzNi4yQzkuMyAyNTUuNSAwIDI2OC45IDAgMjgzLjlWMzk0YzAgMTMuNiA3LjcgMjYuMSAxOS45IDMyLjJsMTAwIDUwYzEwLjEgNS4xIDIyLjEgNS4xIDMyLjIgMGwxMDMuOS01MiAxMDMuOSA1MmMxMC4xIDUuMSAyMi4xIDUuMSAzMi4yIDBsMTAwLTUwYzEyLjItNi4xIDE5LjktMTguNiAxOS45LTMyLjJWMjgzLjljMC0xNS05LjMtMjguNC0yMy40LTMzLjd6TTM1OCAyMTQuOGwtODUgMzEuOXYtNjguMmw4NS0zN3Y3My4zek0xNTQgMTA0LjFsMTAyLTM4LjIgMTAyIDM4LjJ2LjZsLTEwMiA0MS40LTEwMi00MS40di0uNnptODQgMjkxLjFsLTg1IDQyLjV2LTc5LjFsODUtMzguOHY3NS40em0wLTExMmwtMTAyIDQxLjQtMTAyLTQxLjR2LS42bDEwMi0zOC4yIDEwMiAzOC4ydi42em0yNDAgMTEybC04NSA0Mi41di03OS4xbDg1LTM4Ljh2NzUuNHptMC0xMTJsLTEwMiA0MS40LTEwMi00MS40di0uNmwxMDItMzguMiAxMDIgMzguMnYuNnoiPjwvcGF0aD48L3N2Zz4K)](https://docs.rs/oxanus/latest)


<p align="center">
  <picture>
    <img alt="Oxanus logo" src="https://raw.githubusercontent.com/pragmaplatform/oxanus/refs/heads/main/logo.jpg" width="320">
  </picture>
</p>

Oxanus is job processing library written in Rust doesn't suck (or at least sucks in a completely different way than other options).

Oxanus goes for simplicity and depth over breadth. It only aims to support a single backend with a simple flow.

## Key Features

- **Isolated Queues**: Separate job processing queues with independent configurations
- **Retrying**: Automatic retry of failed jobs with configurable backoff
- **Scheduled Jobs**: Schedule jobs to run at specific times or after delays
- **Dynamic Queues**: Create and manage queues at runtime
- **Throttling**: Control job processing rates with queue-based throttling
- **Unique Jobs**: Ensure only one instance of a job runs at a time
- **Resilient Jobs**: Jobs that can survive worker crashes and restarts
- **Graceful Shutdown**: Clean shutdown of workers with in-progress job handling
- **Periodic Jobs**: Run jobs on a schedule using cron-like expressions
- **Resumable Jobs**: Jobs that can be resumed from where they left off when they are retried
- **Batch Processing**: Group multiple jobs together for efficient batch execution

## Quick Start

```rust
use oxanus::Storage;
use serde::{Serialize, Deserialize};

// Define your component registry
#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<MyContext, MyError>);

// Define your error type
#[derive(Debug, thiserror::Error)]
enum MyError {}

// Define your application context
#[derive(Debug, Clone)]
struct MyContext {}

// Define your job -- a plain serializable data struct
#[derive(Debug, Serialize, Deserialize)]
struct MyJob {
    data: String,
}

// Define your worker -- the processor for your job
#[derive(oxanus::Worker)]
#[oxanus(args = MyJob)]
struct MyWorker;

impl MyWorker {
    async fn process(&self, job: &MyJob, _ctx: &oxanus::JobContext) -> Result<(), MyError> {
        println!("Processing: {}", job.data);
        Ok(())
    }
}

// Define your queue using the derive macro
#[derive(Serialize, oxanus::Queue)]
#[oxanus(key = "my_queue", concurrency = 2)]
struct MyQueue;

// Run your worker
#[tokio::main]
async fn main() -> Result<(), oxanus::OxanusError> {
    let ctx = oxanus::ContextValue::new(MyContext {});
    let storage = Storage::builder().build_from_env()?;
    let config = ComponentRegistry::build_config(&storage)
        .with_graceful_shutdown(tokio::signal::ctrl_c());

    // Enqueue some jobs
    storage.enqueue(MyQueue, MyJob { data: "hello".into() }).await?;

    // Run the worker
    oxanus::run(config, ctx).await?;
    Ok(())
}
```

For more detailed usage examples, check out the [examples directory](https://github.com/pragmaplatform/oxanus/tree/main/oxanus/examples).


## Core Concepts

### Jobs and Workers

Oxanus separates **jobs** (data) from **workers** (processors):

- A **Job** is a plain serializable struct representing the work to be done. It carries the data needed for processing.
- A **Worker** is a struct that processes jobs. It holds application-level dependencies (database connections, API clients, configuration) and defines the processing logic.

```rust
// Job -- just data
#[derive(Debug, Serialize, Deserialize)]
struct SendEmailJob {
    to: String,
    subject: String,
    body: String,
}

// Worker -- the processor
#[derive(oxanus::Worker)]
#[oxanus(args = SendEmailJob)]
struct SendEmailWorker;

impl SendEmailWorker {
    async fn process(&self, job: &SendEmailJob, ctx: &oxanus::JobContext) -> Result<(), MyError> {
        // Send the email using job.to, job.subject, job.body
        Ok(())
    }
}
```

Workers can also hold application context for accessing shared resources:

```rust
#[derive(oxanus::Worker)]
#[oxanus(args = SendEmailJob)]
struct SendEmailWorker {
    ctx: MyContext, // application context is injected automatically
}

impl SendEmailWorker {
    async fn process(&self, job: &SendEmailJob, ctx: &oxanus::JobContext) -> Result<(), MyError> {
        // Access self.ctx for database connections, etc.
        // Access job for the job-specific data
        // Access ctx.meta for job metadata (retries, timestamps, etc.)
        // Access ctx.state for resumable job state
        Ok(())
    }
}
```

Worker attributes:
- `#[oxanus(args = MyJob)]` - Specify the job type this worker processes (required)
- `#[oxanus(max_retries = 3)]` - Set maximum retry attempts
- `#[oxanus(retry_delay = 5)]` - Set retry delay in seconds
- `#[oxanus(unique_id = "email_{to}")]` - Define unique job identifiers (references job struct fields)
- `#[oxanus(on_conflict = Skip)]` - Handle job conflicts (Skip or Replace)
- `#[oxanus(cron(schedule = "*/5 * * * * *", queue = MyQueue))]` - Schedule periodic jobs
- `#[oxanus(resurrect = false)]` - Disable job resurrection after crashes

### Queues

Queues are the channels through which jobs flow. They can be defined using the `#[derive(oxanus::Queue)]` macro or by implementing the [`Queue`] trait manually.

Queues can be:

- **Static**: Defined at compile time with a fixed key
- **Dynamic**: Created at runtime with each instance being a separate queue (requires struct fields)

```rust
// Static queue
#[derive(Serialize, oxanus::Queue)]
#[oxanus(key = "emails", concurrency = 5)]
struct EmailQueue;

// Dynamic queue -- each (region, priority) combination is a separate queue
#[derive(Serialize, oxanus::Queue)]
#[oxanus(prefix = "orders")]
struct OrderQueue(Region, u32);
```

Queue attributes:
- `#[oxanus(key = "my_queue")]` - Set static queue key
- `#[oxanus(prefix = "dynamic")]` - Set prefix for dynamic queues
- `#[oxanus(concurrency = 2)]` - Set concurrency limit
- `#[oxanus(throttle(window_ms = 2000, limit = 5))]` - Configure throttling

### Component Registry

The component registry automatically discovers and registers all workers and queues in your application. Use `#[derive(oxanus::Registry)]` to create a registry and `ComponentRegistry::build_config()` to build the configuration.

```rust
#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<MyContext, MyError>);

let config = ComponentRegistry::build_config(&storage);
```

### Storage

[`Storage`] provides the interface for job persistence. It handles:

- Job enqueueing
- Job scheduling
- Job state management
- Queue monitoring

Storage is built using `Storage::builder().build_from_env()` which reads the `REDIS_URL` environment variable.

```rust
let storage = oxanus::Storage::builder().build_from_env()?;

// Enqueue a job for immediate processing
storage.enqueue(MyQueue, MyJob { data: "hello".into() }).await?;

// Schedule a job to run in 5 minutes
storage.enqueue_in(MyQueue, MyJob { data: "delayed".into() }, 300).await?;

// Schedule a job for a specific time
storage.enqueue_at(MyQueue, MyJob { data: "scheduled".into() }, target_time).await?;
```

### JobContext

The `JobContext` is available in every worker's `process` method. It provides:

- `ctx.meta` -- Job metadata (id, retries, timestamps, latency)
- `ctx.state` -- Resumable job state (read and update persistent state across retries)

```rust
impl MyWorker {
    async fn process(&self, job: &MyJob, ctx: &oxanus::JobContext) -> Result<(), MyError> {
        println!("Job ID: {}", ctx.meta.id);
        println!("Retry #{}", ctx.meta.retries);
        println!("Latency: {}ms", ctx.meta.latency_millis());

        // Resumable state
        let progress: Option<u32> = ctx.state.get().await?;
        ctx.state.update(progress.unwrap_or(0) + 1).await?;

        Ok(())
    }
}
```

### Application Context (ContextValue)

Application-level dependencies are passed via `ContextValue` and automatically injected into workers that have fields:

```rust
#[derive(Debug, Clone)]
struct MyContext {
    db: DatabasePool,
    config: AppConfig,
}

let ctx = oxanus::ContextValue::new(MyContext {
    db: create_pool().await?,
    config: load_config()?,
});

oxanus::run(config, ctx).await?;
```

Workers with a single field receive the context automatically:

```rust
#[derive(oxanus::Worker)]
#[oxanus(args = MyJob)]
struct MyWorker {
    ctx: MyContext, // auto-populated from ContextValue
}
```

Workers without fields (unit structs) don't need any context:

```rust
#[derive(oxanus::Worker)]
#[oxanus(args = MyJob)]
struct StatelessWorker;
```

### Configuration

Configuration is done through the [`Config`] builder, which allows you to:

- Automatically register queues and workers via the component registry
- Set up graceful shutdown
- Configure exit conditions

```rust
let config = ComponentRegistry::build_config(&storage)
    .with_graceful_shutdown(tokio::signal::ctrl_c())
    .exit_when_processed(100);
```

### Error Handling

Oxanus uses a custom error type [`OxanusError`] that covers all possible error cases in the library.
Workers can define their own error type that implements `std::error::Error`.

```rust
#[derive(Debug, thiserror::Error)]
enum MyError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Validation error: {0}")]
    Validation(String),
}
```

### Unique Jobs

Ensure only one instance of a job runs at a time by defining a unique identifier:

```rust
#[derive(Debug, Serialize, Deserialize)]
struct DeduplicatedJob {
    user_id: u64,
}

#[derive(oxanus::Worker)]
#[oxanus(args = DeduplicatedJob)]
#[oxanus(unique_id = "sync_user:{user_id}", on_conflict = Skip)]
struct SyncUserWorker;
```

The `unique_id` format string references fields from the **job** struct. When a job with the same unique ID is already enqueued, the conflict strategy determines the behavior:
- `Skip` -- discard the new job
- `Replace` -- replace the existing job with the new one

### Cron / Periodic Jobs

Schedule workers to run periodically using 6-part cron expressions:

```rust
#[derive(Debug, Serialize, Deserialize)]
struct CleanupJob {}

#[derive(oxanus::Worker)]
#[oxanus(args = CleanupJob)]
#[oxanus(cron(schedule = "0 */5 * * * *", queue = MaintenanceQueue))]
struct CleanupWorker;
```

### Resumable Jobs

Jobs can persist state across retries, allowing them to resume from where they left off:

```rust
#[derive(Debug, Serialize, Deserialize)]
struct ImportJob {}

#[derive(oxanus::Worker)]
#[oxanus(args = ImportJob, max_retries = 10, retry_delay = 3)]
struct ImportWorker;

impl ImportWorker {
    async fn process(&self, _job: &ImportJob, ctx: &oxanus::JobContext) -> Result<(), MyError> {
        let offset: Option<u64> = ctx.state.get().await?;
        let start = offset.unwrap_or(0);

        for i in start..1000 {
            // Process item i...
            ctx.state.update(i + 1).await?;
        }

        Ok(())
    }
}
```

### Batch Processing

Group multiple jobs together for efficient batch execution:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexJob {
    document_id: u64,
}

#[derive(oxanus::Worker)]
#[oxanus(args = IndexJob)]
struct IndexWorker;

impl IndexWorker {
    async fn process(&self, _job: &IndexJob, _ctx: &oxanus::JobContext) -> Result<(), MyError> {
        Ok(())
    }
}

#[derive(oxanus::BatchProcessor)]
#[oxanus(args = IndexJob, batch_size = 10, batch_linger_ms = 500)]
struct IndexBatchProcessor;

impl IndexBatchProcessor {
    async fn process_batch(&self, jobs: &[IndexJob], ctx: &oxanus::JobContext) -> Result<(), MyError> {
        let ids: Vec<u64> = jobs.iter().map(|j| j.document_id).collect();
        // Bulk index all documents at once
        Ok(())
    }
}
```

Batch processors support context injection just like workers:

```rust
#[derive(oxanus::BatchProcessor)]
#[oxanus(args = IndexJob, batch_size = 10, batch_linger_ms = 500)]
struct IndexBatchProcessor {
    ctx: MyContext, // auto-populated from ContextValue
}
```

### Prometheus Metrics

Enable the `prometheus` feature to expose metrics:

```rust
let metrics = storage.metrics().await?;
let output = metrics.encode_to_string()?;
// Serve `output` on your metrics endpoint
```
