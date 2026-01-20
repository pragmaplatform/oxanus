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

## Quick Start

```rust
use oxanus::{Context, Storage};
use serde::{Serialize, Deserialize};

// Define your component registry
#[derive(oxanus::Registry)]
struct ComponentRegistry(oxanus::ComponentRegistry<MyContext, MyError>);

// Define your error type
#[derive(Debug, thiserror::Error)]
enum MyError {}

// Define your context
#[derive(Debug, Clone)]
struct MyContext {}

// Define your worker using the derive macro
#[derive(Debug, Serialize, Deserialize, oxanus::Worker)]
struct MyWorker {
    data: String,
}

impl MyWorker {
    async fn process(&self, _ctx: &Context<MyContext>) -> Result<(), MyError> {
        // Process your job here
        println!("Processing: {}", self.data);
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
    let ctx = Context::value(MyContext {});
    let storage = Storage::builder().build_from_env()?;
    let config = ComponentRegistry::build_config(&storage)
        .with_graceful_shutdown(tokio::signal::ctrl_c());

    // Enqueue some jobs
    storage.enqueue(MyQueue, MyWorker { data: "hello".into() }).await?;

    // Run the worker
    oxanus::run(config, ctx).await?;
    Ok(())
}
```

For more detailed usage examples, check out the [examples directory](https://github.com/pragmaplatform/oxanus/tree/main/oxanus/examples).


## Core Concepts

### Workers

Workers are the units of work in Oxanus. They can be defined using the `#[derive(oxanus::Worker)]` macro or by implementing the [`Worker`] trait manually. Workers define the processing logic for jobs.

Worker attributes:
- `#[oxanus(max_retries = 3)]` - Set maximum retry attempts
- `#[oxanus(retry_delay = 5)]` - Set retry delay in seconds
- `#[oxanus(unique_id = "worker_{id}")]` - Define unique job identifiers
- `#[oxanus(on_conflict = Skip)]` - Handle job conflicts (Skip or Replace)
- `#[oxanus(cron(schedule = "*/5 * * * * *", queue = MyQueue))]` - Schedule periodic jobs

### Queues

Queues are the channels through which jobs flow. They can be defined using the `#[derive(oxanus::Queue)]` macro or by implementing the [`Queue`] trait manually.

Queues can be:

- **Static**: Defined at compile time with a fixed key
- **Dynamic**: Created at runtime with each instance being a separate queue (requires struct fields)

Queue attributes:
- `#[oxanus(key = "my_queue")]` - Set static queue key
- `#[oxanus(prefix = "dynamic")]` - Set prefix for dynamic queues
- `#[oxanus(concurrency = 2)]` - Set concurrency limit
- `#[oxanus(throttle(window_ms = 2000, limit = 5))]` - Configure throttling

### Component Registry

The component registry automatically discovers and registers all workers and queues in your application. Use `#[derive(oxanus::Registry)]` to create a registry and `ComponentRegistry::build_config()` to build the configuration.

### Storage

The [`Storage`] trait provides the interface for job persistence. It handles:

- Job enqueueing
- Job scheduling
- Job state management
- Queue monitoring

Storage is built using `Storage::builder().build_from_env()` which reads the `REDIS_URL` environment variable.

### Context

The context provides shared state and utilities to workers. It can include:

- Database connections
- Configuration
- Shared resources
- Job state (for resumable jobs)

### Configuration

Configuration is done through the [`Config`] builder, which allows you to:

- Automatically register queues and workers via the component registry
- Set up graceful shutdown
- Configure exit conditions

### Error Handling

Oxanus uses a custom error type [`OxanusError`] that covers all possible error cases in the library.
Workers can define their own error type that implements `std::error::Error`.

