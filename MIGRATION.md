# Migration Guide

This guide covers migrating from the old API (where worker structs contained both job data and processing logic) to the new API (where jobs and workers are separate).

## Overview of Changes

The core change is **separating job data from worker logic**:

| Concept | Old API | New API |
|---------|---------|---------|
| Job data | Fields on the worker struct | Separate `Job` struct |
| Worker | `#[derive(Serialize, Deserialize, oxanus::Worker)]` on data struct | `#[derive(oxanus::Worker)]` with `#[oxanus(args = MyJob)]` |
| Process signature | `process(&self, ctx: &oxanus::Context<T>)` | `process(&self, job: &MyJob, ctx: &oxanus::JobContext)` |
| Context constructor | `oxanus::Context::value(ctx)` | `oxanus::ContextValue::new(ctx)` |
| Enqueue | `storage.enqueue(Queue, worker_instance)` | `storage.enqueue(Queue, job_instance)` |

## Step-by-Step Migration

### 1. Split worker struct into Job + Worker

**Before:**

```rust
#[derive(Debug, Serialize, Deserialize, oxanus::Worker)]
struct SendEmail {
    to: String,
    subject: String,
}

impl SendEmail {
    async fn process(&self, ctx: &oxanus::Context<MyCtx>) -> Result<(), MyError> {
        send_email(&self.to, &self.subject).await
    }
}
```

**After:**

```rust
#[derive(Debug, Serialize, Deserialize)]
struct SendEmailJob {
    to: String,
    subject: String,
}

#[derive(oxanus::Worker)]
#[oxanus(args = SendEmailJob)]
struct SendEmailWorker;

impl SendEmailWorker {
    async fn process(&self, job: &SendEmailJob, _ctx: &oxanus::JobContext) -> Result<(), MyError> {
        send_email(&job.to, &job.subject).await
    }
}
```

Key changes:
- Job struct keeps `Serialize, Deserialize` but drops `oxanus::Worker`
- Worker struct gets `#[derive(oxanus::Worker)]` and `#[oxanus(args = JobType)]`
- Worker struct does **not** need `Serialize` or `Deserialize`
- `process` now takes `job: &JobType` as the first argument and `ctx: &oxanus::JobContext` as the second
- Job data accessed via `self.field` becomes `job.field`

### 2. Update the process method signature

**Before:**

```rust
async fn process(&self, ctx: &oxanus::Context<MyCtx>) -> Result<(), MyError> {
    let app_ctx = &ctx.value;     // application context
    let meta = &ctx.meta;         // job metadata
    let state = &ctx.state;       // resumable state
    let data = &self.some_field;  // job data from self
    // ...
}
```

**After:**

```rust
async fn process(&self, job: &MyJob, ctx: &oxanus::JobContext) -> Result<(), MyError> {
    let meta = &ctx.meta;         // job metadata (unchanged)
    let state = &ctx.state;       // resumable state (unchanged)
    let data = &job.some_field;   // job data now from `job` parameter
    // application context lives on `self` if the worker has fields (see step 5)
}
```

The new `JobContext` contains only `meta` and `state` -- no application context. Application context is now injected into the worker struct itself (see step 5).

### 3. Update enqueue calls

**Before:**

```rust
storage.enqueue(MyQueue, SendEmail { to: "a@b.com".into(), subject: "hi".into() }).await?;
```

**After:**

```rust
storage.enqueue(MyQueue, SendEmailJob { to: "a@b.com".into(), subject: "hi".into() }).await?;
```

You now pass the **job** instance, not the worker. The same applies to `enqueue_in` and `enqueue_at`.

### 4. Update ContextValue construction

**Before:**

```rust
let ctx = oxanus::Context::value(MyCtx { db: pool });
```

**After:**

```rust
let ctx = oxanus::ContextValue::new(MyCtx { db: pool });
```

### 5. Inject application context into workers (if needed)

If your worker needs access to application context (database pools, config, etc.), add a field to the worker struct. The derive macro generates the necessary `FromContext` implementation.

**Before:**

```rust
#[derive(Debug, Serialize, Deserialize, oxanus::Worker)]
struct MyWorker { id: u64 }

impl MyWorker {
    async fn process(&self, ctx: &oxanus::Context<MyCtx>) -> Result<(), MyError> {
        ctx.value.db.query(&self.id).await  // context accessed via ctx.value
    }
}
```

**After:**

```rust
#[derive(Debug, Serialize, Deserialize)]
struct MyJob { id: u64 }

#[derive(oxanus::Worker)]
#[oxanus(args = MyJob)]
struct MyWorker {
    ctx: MyCtx,  // automatically populated from ContextValue
}

impl MyWorker {
    async fn process(&self, job: &MyJob, _ctx: &oxanus::JobContext) -> Result<(), MyError> {
        self.ctx.db.query(&job.id).await  // context accessed via self
    }
}
```

Unit struct workers (no fields) don't need any context injection and work as-is:

```rust
#[derive(oxanus::Worker)]
#[oxanus(args = MyJob)]
struct StatelessWorker;
```

### 6. Update worker attributes

All `#[oxanus(...)]` attributes remain on the **worker** struct, with the addition of the required `args` attribute:

```rust
#[derive(oxanus::Worker)]
#[oxanus(args = MyJob)]                                           // NEW: required
#[oxanus(max_retries = 5, retry_delay = 10)]                      // unchanged
#[oxanus(unique_id = "sync:{user_id}", on_conflict = Skip)]       // unchanged (references job fields)
#[oxanus(cron(schedule = "*/5 * * * * *", queue = MyQueue))]       // unchanged
#[oxanus(resurrect = false)]                                      // unchanged
#[oxanus(context = MyCtx)]                                        // unchanged
#[oxanus(error = MyError, registry = MyRegistry)]                 // unchanged
struct MyWorker;
```

Note: `unique_id` format placeholders like `{user_id}` reference fields on the **job** struct (the `args` type), not the worker.

### 7. Update batch processors

**Before:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, oxanus::Worker)]
struct IndexWorker { document_id: u64 }

#[derive(Debug, Serialize, Deserialize, oxanus::BatchProcessor)]
#[oxanus(batch_size = 10, batch_linger_ms = 500)]
struct IndexBatch {
    workers: Vec<IndexWorker>,
}

impl IndexBatch {
    async fn process_batch(&self, _ctx: &oxanus::Context<MyCtx>) -> Result<(), MyError> {
        let ids: Vec<u64> = self.workers.iter().map(|w| w.document_id).collect();
        Ok(())
    }
}
```

**After:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexJob { document_id: u64 }

#[derive(oxanus::Worker)]
#[oxanus(args = IndexJob)]
struct IndexWorker;

impl IndexWorker {
    async fn process(&self, _job: &IndexJob, _ctx: &oxanus::JobContext) -> Result<(), MyError> {
        Ok(())
    }
}

#[derive(oxanus::BatchProcessor)]
#[oxanus(batch_size = 10, batch_linger_ms = 500)]
struct IndexBatch {
    jobs: Vec<IndexJob>,   // Vec of job types, not workers
}

impl IndexBatch {
    async fn process_batch(&self, _ctx: &oxanus::JobContext) -> Result<(), MyError> {
        let ids: Vec<u64> = self.jobs.iter().map(|j| j.document_id).collect();
        Ok(())
    }
}
```

Key changes:
- The `Vec` field holds **job** types, not worker types
- The batch processor no longer derives `Serialize, Deserialize`
- `process_batch` takes `&oxanus::JobContext` instead of `&oxanus::Context<T>`
- A corresponding `Worker` derive is needed for the job's worker

### 8. Update resumable jobs

The state API itself is unchanged, only the access path differs.

**Before:**

```rust
#[derive(Debug, Serialize, Deserialize, oxanus::Worker)]
#[oxanus(max_retries = 10, retry_delay = 3)]
struct ImportWorker {}

impl ImportWorker {
    async fn process(&self, ctx: &oxanus::Context<MyCtx>) -> Result<(), MyError> {
        let progress = ctx.state.get::<u64>().await?;
        ctx.state.update(progress.unwrap_or(0) + 1).await?;
        Ok(())
    }
}
```

**After:**

```rust
#[derive(Debug, Serialize, Deserialize)]
struct ImportJob {}

#[derive(oxanus::Worker)]
#[oxanus(args = ImportJob)]
#[oxanus(max_retries = 10, retry_delay = 3)]
struct ImportWorker;

impl ImportWorker {
    async fn process(&self, _job: &ImportJob, ctx: &oxanus::JobContext) -> Result<(), MyError> {
        let progress = ctx.state.get::<u64>().await?;
        ctx.state.update(progress.unwrap_or(0) + 1).await?;
        Ok(())
    }
}
```

### 9. WorkerContext no longer needs Serialize/Deserialize

In the old API, `WorkerContext` often derived `Serialize` and `Deserialize`. This is no longer required:

**Before:**

```rust
#[derive(Debug, Serialize, Deserialize, Clone)]
struct WorkerContext { db: DatabasePool }
```

**After:**

```rust
#[derive(Debug, Clone)]
struct WorkerContext { db: DatabasePool }
```

## Quick Reference Checklist

- [ ] Split each `#[derive(oxanus::Worker)]` struct into a Job (data) + Worker (logic)
- [ ] Add `#[oxanus(args = JobType)]` to every worker
- [ ] Remove `Serialize, Deserialize` from worker structs
- [ ] Update `process` signatures: add `job: &JobType` parameter, change `ctx` to `&oxanus::JobContext`
- [ ] Replace `self.field` with `job.field` for job data access inside `process`
- [ ] Move application context access from `ctx.value` to a field on the worker struct
- [ ] Update `oxanus::Context::value(...)` to `oxanus::ContextValue::new(...)`
- [ ] Update enqueue calls to pass job instances instead of worker instances
- [ ] Update batch processor `Vec` fields from worker types to job types
- [ ] Update `process_batch` signature to take `&oxanus::JobContext`
- [ ] Remove unnecessary `Serialize, Deserialize` from `WorkerContext`

## Unchanged

The following remain the same:

- Queue definitions (`#[derive(oxanus::Queue)]` and all queue attributes)
- Registry definitions (`#[derive(oxanus::Registry)]`)
- `Storage` builder and all storage methods
- `oxanus::run(config, ctx)` call
- `ComponentRegistry::build_config(&storage)` and config builder methods
- Cron schedule syntax and queue attribute
- Throttling configuration
- Dynamic queue usage
