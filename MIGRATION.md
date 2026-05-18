# Migration Guide: 1.x -> 2.x

Oxana 2.x makes `Storage` a focused enqueueing and monitoring handle, and moves worker setup into a typed runtime built with `storage.runtime(ctx)`. It also changes persisted job identity from worker type names to job type names.

This guide is written for applications already using the 1.x job/worker split.

## Before You Upgrade

Plan the Redis migration first. In 1.x, queued job envelopes were registered by `Job::worker_name()`, usually the worker type name. In 2.x, new envelopes are registered by `Job::name()`, which defaults to the job type name.

For consumer compatibility, the 2.x runtime also registers each worker's type name as a temporary legacy alias for its job type. That lets existing enqueued, scheduled, retry, and dead jobs named `FooWorker` deserialize into `FooJob` and run through `FooWorker` after the upgrade.

This alias is read-side only. New jobs are still written with job-type identity, and unique job IDs are still prefixed with the new job type name. A unique 1.x job named `FooWorker/id` and a unique 2.x job named `FooJob/id` can therefore coexist during the migration window.

Recommended options:

- Stop 1.x producers and workers before starting 2.x if duplicate unique jobs would be harmful.
- Let the 2.x runtime consume old worker-named jobs through the legacy aliases, or drain queues first if you want a cleaner cutover.
- Use a new Oxana namespace or Redis database for the 2.x deployment.

## 1. Update Crate Names And Imports

If your 1.x app still uses the old Oxanus crate names or attributes, rename them to Oxana:

```toml
# Before
oxanus = "1"
oxanus-web = "1"

# After
oxana = "2"
oxana-web = "2"
```

Update Rust paths and macro attributes the same way:

```rust
// Before
#[oxanus(key = "emails")]

// After
#[oxana(key = "emails")]
```

## 2. Replace Config With RuntimeBuilder

In 1.x, `Config` carried worker registrations, queue registrations, shutdown settings, the global worker error type, and a storage handle.

In 2.x:

- `Storage` handles enqueueing, scheduling, metrics, and queue monitoring.
- `RuntimeBuilder<C>` handles context, worker/queue registration, runtime settings, running, draining, and catalogs.
- `ContextValue::new(...)` is no longer part of the public setup path. Pass your raw app context to `storage.runtime(ctx)`.

Before:

```rust
#[derive(oxana::Registry)]
struct ComponentRegistry(oxana::ComponentRegistry<AppContext, AppError>);

let ctx = oxana::ContextValue::new(AppContext { db, mailer });
let storage = oxana::Storage::builder().build_from_env()?;

let config = ComponentRegistry::build_config(&storage)
    .with_graceful_shutdown(tokio::signal::ctrl_c())
    .exit_when_processed(1);

storage.enqueue(EmailQueue, SendEmailJob { user_id }).await?;
oxana::run(config, ctx).await?;
```

After:

```rust
#[derive(oxana::Registry)]
struct ComponentRegistry(oxana::ComponentRegistry<AppContext>);

let storage = oxana::Storage::from_env()?;

let runtime = storage
    .runtime(AppContext { db, mailer })
    .register::<ComponentRegistry>()
    .shutdown_on_ctrl_c()
    .exit_when_processed(1);

storage.enqueue(EmailQueue, SendEmailJob { user_id }).await?;
runtime.run().await?;
```

Manual registration moves the same way:

```rust
// Before
let config = oxana::Config::<AppContext, AppError>::new(&storage)
    .register_queue::<EmailQueue>()
    .register_worker::<SendEmailWorker, SendEmailJob>();

// After
let runtime = storage
    .runtime(AppContext { db, mailer })
    .queue::<EmailQueue>()
    .worker::<SendEmailWorker, SendEmailJob>();
```

## 3. Move Runtime Settings

Configuration methods that used to live on `Config` now live on the runtime builder:

| 1.x | 2.x |
| --- | --- |
| `with_graceful_shutdown(fut)` | `shutdown_on(fut)` |
| `with_graceful_shutdown(tokio::signal::ctrl_c())` | `shutdown_on_ctrl_c()` |
| `with_retry_delay_override(...)` | `retry_delay_override(...)` |
| `exit_when_processed(n)` | `exit_when_processed(n)` |

2.x also exposes runtime tuning on the same builder:

```rust
let runtime = storage
    .runtime(ctx)
    .register::<ComponentRegistry>()
    .heartbeat_interval(std::time::Duration::from_millis(500))
    .dead_process_threshold(std::time::Duration::from_secs(5))
    .retry_poll_interval(std::time::Duration::from_millis(300))
    .schedule_poll_interval(std::time::Duration::from_millis(300))
    .shutdown_timeout(std::time::Duration::from_secs(180));
```

Available runtime knobs include `heartbeat_interval`, `dead_process_threshold`,
`resurrect_scan_interval`, `redis_failure_tolerance`, `retry_poll_interval`,
`schedule_poll_interval`, `cron_initial_offset`, `cron_lookahead`,
`cron_tick_interval`, `dequeue_timeout`, `dispatcher_idle_sleep`,
`throttled_queue_fallback_wait`, and `shutdown_timeout`.

The retry-delay override now receives a type-erased worker error:

```rust
let runtime = storage.runtime(ctx).retry_delay_override(
    |error: &(dyn std::error::Error + Send + Sync), retries, default_delay| {
        if error.downcast_ref::<RateLimitError>().is_some() {
            Some(60)
        } else if retries > 3 {
            Some(default_delay * 2)
        } else {
            None
        }
    },
);
```

## 4. Update Registry And Error Typing

The component registry is now typed only by app context:

```rust
// Before
#[derive(oxana::Registry)]
struct ComponentRegistry(oxana::ComponentRegistry<AppContext, AppError>);

// After
#[derive(oxana::Registry)]
struct ComponentRegistry(oxana::ComponentRegistry<AppContext>);
```

Workers still choose their own error type. The derive default is `oxana::BoxError`, so most workers no longer need a registry-wide error type:

```rust
#[derive(oxana::Worker)]
struct SendEmailWorker;

impl SendEmailWorker {
    async fn process(
        &self,
        _job: SendEmailJob,
        _ctx: &oxana::JobContext,
    ) -> Result<(), AppError> {
        Ok(())
    }
}
```

Use `#[oxana(error = AppError)]` only when you need the generated `Worker` impl to expose that concrete error type.

## 5. Remove Job-To-Worker Binding From Jobs

Jobs no longer name their worker. The runtime maps each job type to a worker through registration.
The worker derive still infers `SendEmailWorker` -> `SendEmailJob`; keep using
`#[oxana(job = CustomJob)]` on the worker when the convention does not match.

Before:

```rust
#[derive(Debug, serde::Serialize, serde::Deserialize, oxana::Job)]
#[oxana(worker = SendEmailWorker)]
#[oxana(unique_id = "send_email:{user_id}")]
struct SendEmailJob {
    user_id: i64,
}
```

After:

```rust
#[derive(Debug, serde::Serialize, serde::Deserialize, oxana::Job)]
#[oxana(unique_id = "send_email:{user_id}")]
struct SendEmailJob {
    user_id: i64,
}

let runtime = storage
    .runtime(ctx)
    .worker::<SendEmailWorker, SendEmailJob>();
```

Manual `Job` implementations should remove `worker_name()`:

```rust
// Before
impl oxana::Job for SendEmailJob {
    fn worker_name() -> &'static str {
        std::any::type_name::<SendEmailWorker>()
    }

    fn unique_id(&self) -> Option<String> {
        Some(format!("send_email:{}", self.user_id))
    }
}

// After
impl oxana::Job for SendEmailJob {
    fn unique_id(&self) -> Option<String> {
        Some(format!("send_email:{}", self.user_id))
    }
}
```

Job attributes remain enqueue-time metadata:

- `#[oxana(unique_id = "...")]`
- `#[oxana(on_conflict = Skip)]` or `#[oxana(on_conflict = Replace)]`
- `#[oxana(resurrect = false)]`
- `#[oxana(resume = false)]`
- `#[oxana(throttle_cost = 2)]`
- `#[oxana(on_demand)]`

Worker attributes remain execution-time metadata:

- `#[oxana(job = SendEmailJob)]`
- `#[oxana(context = AppContext)]`
- `#[oxana(error = AppError)]`
- `#[oxana(registry = ComponentRegistry)]`
- `#[oxana(max_retries = 3)]`
- `#[oxana(retry_delay = 5)]`
- `#[oxana(cron(schedule = "*/5 * * * * *", queue = EmailQueue))]`
- `#[oxana(batch_size = 100, batch_timeout_ms = 500)]`

## 6. Update Worker Process Signatures

2.x owns job values during execution. If your handler still borrows the job, take it by value instead.

```rust
// Before
impl SendEmailWorker {
    async fn process(
        &self,
        _job: &SendEmailJob,
        _ctx: &oxana::JobContext,
    ) -> Result<(), AppError> {
        Ok(())
    }
}

// After
impl SendEmailWorker {
    async fn process(
        &self,
        _job: SendEmailJob,
        _ctx: &oxana::JobContext,
    ) -> Result<(), AppError> {
        Ok(())
    }
}
```

For derived batch workers, implement `process_batch`:

```rust
#[derive(oxana::Worker)]
#[oxana(batch_size = 100, batch_timeout_ms = 500)]
struct ImportUsersWorker;

impl ImportUsersWorker {
    async fn process_batch(
        &self,
        jobs: Vec<oxana::BatchItem<ImportUsersJob>>,
    ) -> Result<(), AppError> {
        for oxana::BatchItem { job: _job, ctx: _ctx } in jobs {
            // Process each job in the batch.
        }

        Ok(())
    }
}
```

For manual worker implementations, implement `process` for single-job workers or `run_batch` for all-at-once batch workers:

```rust
#[async_trait::async_trait]
impl oxana::Worker<SendEmailJob> for SendEmailWorker {
    type Error = AppError;

    async fn process(
        &self,
        _job: SendEmailJob,
        _ctx: &oxana::JobContext,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}
```

Job payload types no longer need `Sync`; they still need `Send`, `Serialize`, and `DeserializeOwned` when registered for execution.

## 7. Update Queue Derives And Manual Queues

Static queues no longer need `serde::Serialize`:

```rust
// Before
#[derive(serde::Serialize, oxana::Queue)]
#[oxana(key = "emails", concurrency = 4)]
struct EmailQueue;

// After
#[derive(oxana::Queue)]
#[oxana(key = "emails", concurrency = 4)]
struct EmailQueue;
```

Dynamic queues still need serializable fields so Oxana can build a stable runtime key:

```rust
#[derive(serde::Serialize, oxana::Queue)]
#[oxana(prefix = "tenant", concurrency = Dynamic(2))]
struct TenantQueue {
    tenant_id: String,
}
```

Manual dynamic queues must implement `key()`:

```rust
impl oxana::Queue for TenantQueue {
    fn key(&self) -> String {
        format!("tenant#{}", self.tenant_id)
    }

    fn to_config() -> oxana::QueueConfig {
        oxana::QueueConfig::as_dynamic("tenant").dynamic_concurrency(2)
    }
}
```

Manual static queues can use the default `key()` as long as `to_config()` returns a static queue config.

## 8. Update Drain, Catalog, And Web Dashboard Setup

Drain now runs through a runtime:

```rust
// Before
let stats = oxana::drain(&config, ctx, EmailQueue).await?;

// After
let stats = runtime.drain(EmailQueue).await?;
```

Catalogs also come from the runtime:

```rust
// Before
let catalog = config.catalog();

// After
let catalog = runtime.catalog();
```

For `oxana-web`, build the catalog before consuming the runtime with `run()`:

```rust
let storage = oxana::Storage::from_env()?;
let runtime = storage
    .runtime(ctx)
    .register::<ComponentRegistry>()
    .shutdown_on_ctrl_c();

let catalog = runtime.catalog();
let oxana_state = oxana_web::OxanaWebState::new(
    storage.clone(),
    catalog,
    "/oxana".to_string(),
);

let oxana_router = oxana_web::router(oxana_state);
let app = your_app_router().nest("/oxana", oxana_router);

tokio::spawn(async move {
    if let Err(error) = runtime.run().await {
        tracing::error!(%error, "Oxana runtime stopped");
    }
});
```

Metrics and dashboard labels are keyed by job identity in 2.x. If you used worker type names to query metrics, update those callers to use the registered job name.

## 9. Update Progress State

`JobProgress` no longer exposes a separate `processed` field. Structured progress is cursor/total based:

```rust
ctx.state.update_progress((cursor, total)).await?;
ctx.state
    .update_progress((cursor, total, Some("importing users".to_string())))
    .await?;
```

If your application displayed `processed`, derive it from your cursor semantics or store that value in custom job state.

## 10. Storage Builder Notes

The existing builder still works:

```rust
let storage = oxana::Storage::builder().build_from_env()?;
```

2.x also adds shorter constructors:

```rust
let storage = oxana::Storage::from_env()?;
let storage = oxana::Storage::from_url("redis://127.0.0.1/0")?;
```

`build_from_env_var(...)` now returns `OxanaError::ConfigError` when the environment variable is missing instead of panicking.

## Checklist

- Let the 2.x runtime consume old worker-named Redis jobs through legacy aliases, or drain/clear/re-enqueue them for a cleaner cutover.
- Avoid overlapping 1.x and 2.x producers for unique jobs unless duplicate unique IDs are acceptable during the migration window.
- Rename any remaining Oxanus crate paths, imports, and macro attributes to Oxana.
- Replace `ContextValue::new(ctx)`, `Config`, and `oxana::run(config, ctx)` with `storage.runtime(ctx)` and `runtime.run()`.
- Change `ComponentRegistry<AppContext, AppError>` to `ComponentRegistry<AppContext>`.
- Move queue and worker registration to the runtime builder.
- Remove `#[oxana(worker = ...)]` and manual `Job::worker_name()` implementations.
- Ensure worker handlers take owned job values.
- Remove `Serialize` derives from static queues when they are no longer needed.
- Move drain, catalog, and web dashboard integration to the runtime API.
- Replace any `JobProgress::processed` usage with cursor/total progress.
