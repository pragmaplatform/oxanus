# Migration Guide: 0.9 → 0.10

Oxanus 0.10 introduces a **Job/Worker separation** — job data (the serialized payload) is now defined in a separate struct from the worker (the processing logic). This gives workers access to application context at construction time, makes the processing signature more explicit, and removes the generic context parameter from `Context<T>`.

## Overview of changes

| 0.9 | 0.10 |
|-----|------|
| `Worker` trait combines data + processing | `Job` trait (data) + `Worker<Args>` trait (processing) |
| `Context<T>` (generic over app context) | `JobContext` (no generic) |
| `Context::value(x)` | `ContextValue::new(x)` |
| `#[derive(Serialize, Deserialize, oxanus::Worker)]` on one struct | `#[derive(Serialize, Deserialize)]` on job struct, `#[derive(oxanus::Worker)]` on worker struct |
| `config.register_worker::<W>()` | `config.register_worker::<W, J>()` |
| `storage.enqueue(queue, MyWorker { .. })` | `storage.enqueue(queue, MyJob { .. })` |
| `config.has_registered_worker::<W>()` | `config.has_registered_worker_type::<W>()` |
| `config.register_cron_worker::<W>(queue)` | `config.register_worker::<W, J>()` (cron config via derive) |

## Step-by-step migration

### 1. Split the worker struct into a job struct and a worker struct

**Before (0.9):**

```rust
#[derive(Debug, Serialize, Deserialize, oxanus::Worker)]
#[oxanus(unique_id = "send_email:{user_id}")]
#[oxanus(max_retries = 3)]
struct SendEmail {
    user_id: i64,
    subject: String,
    body: String,
}

impl SendEmail {
    async fn process(&self, ctx: &oxanus::Context<AppState>) -> Result<(), AppError> {
        let mailer = &ctx.ctx.mailer;
        mailer.send(self.user_id, &self.subject, &self.body).await?;
        Ok(())
    }
}
```

**After (0.10):**

```rust
// Job struct — holds the serialized data
#[derive(Debug, Serialize, Deserialize)]
struct SendEmailJob {
    user_id: i64,
    subject: String,
    body: String,
}

// Worker struct — holds processing logic (and optionally app context)
#[derive(oxanus::Worker)]
#[oxanus(args = SendEmailJob)]
#[oxanus(unique_id = "send_email:{user_id}")]
#[oxanus(max_retries = 3)]
struct SendEmail {
    state: AppState,  // single field → auto FromContext
}

impl SendEmail {
    async fn process(&self, job: &SendEmailJob, _ctx: &oxanus::JobContext) -> Result<(), AppError> {
        self.state.mailer.send(job.user_id, &job.subject, &job.body).await?;
        Ok(())
    }
}
```

Key points:
- The `#[oxanus(args = ...)]` attribute on the worker derive is **required** — it tells the macro which struct holds the job data.
- Job-specific attributes (`unique_id`, `on_conflict`, `resurrect`, `throttle_cost`) stay on `#[derive(oxanus::Worker)]` but generate impls on the **job** struct's `Job` trait.
- Worker-specific attributes (`max_retries`, `retry_delay`, `cron`, `error`, `context`, `registry`) generate impls on the **worker** struct's `Worker<Args>` trait.
- The process method signature changes from `process(&self, ctx: &Context<T>)` to `process(&self, job: &JobType, ctx: &JobContext)`.

### 2. Worker struct patterns

The worker struct can be:

**Unit struct** (no app context needed):
```rust
#[derive(oxanus::Worker)]
#[oxanus(args = MyJob)]
struct MyWorker;
```

**Single-field struct** (auto `FromContext` — field is cloned from app context):
```rust
#[derive(oxanus::Worker)]
#[oxanus(args = MyJob)]
struct MyWorker {
    state: AppState,
}
```

For more complex cases, implement `FromContext` manually:
```rust
impl oxanus::FromContext<AppState> for MyWorker {
    fn from_context(ctx: &AppState) -> Self {
        Self {
            db: ctx.db_pool.clone(),
            mailer: ctx.mailer.clone(),
        }
    }
}
```

### 3. Update context creation

**Before:**
```rust
let ctx = oxanus::Context::value(AppState { db, mailer });
```

**After:**
```rust
let ctx = oxanus::ContextValue::new(AppState { db, mailer });
```

### 4. Update enqueue calls

Enqueue now takes the **job** struct, not the worker struct:

**Before:**
```rust
storage.enqueue(MyQueue, SendEmail { user_id: 1, subject: "hi".into(), body: "hello".into() }).await?;
```

**After:**
```rust
storage.enqueue(MyQueue, SendEmailJob { user_id: 1, subject: "hi".into(), body: "hello".into() }).await?;
```

### 5. Update config registration

**Before:**
```rust
let config = oxanus::Config::new(&storage)
    .register_queue::<MyQueue>()
    .register_worker::<SendEmail>();
```

**After:**
```rust
let config = oxanus::Config::new(&storage)
    .register_queue::<MyQueue>()
    .register_worker::<SendEmail, SendEmailJob>();
```

### 6. Update `has_registered_worker` / `has_registered_cron_worker`

These methods now take a string name or use `_type` variants:

**Before:**
```rust
config.has_registered_worker::<SendEmail>()
config.has_registered_cron_worker::<MyCronWorker>()
```

**After:**
```rust
config.has_registered_worker_type::<SendEmail>()
config.has_registered_cron_worker_type::<MyCronWorker>()
```

### 7. Update cron worker registration

`register_cron_worker` has been removed. Cron workers are now registered with the standard `register_worker` — the cron config is derived from attributes:

**Before:**
```rust
let config = oxanus::Config::new(&storage)
    .register_cron_worker::<MyCronWorker>(MyDynamicQueue(1));
```

**After:**
```rust
// Cron schedule and queue are defined on the derive
#[derive(oxanus::Worker)]
#[oxanus(args = MyCronJob)]
#[oxanus(cron(schedule = "*/5 * * * * *", queue = MyQueue))]
struct MyCronWorker;

let config = oxanus::Config::new(&storage)
    .register_worker::<MyCronWorker, MyCronJob>();
```

### 8. Update manual Worker trait implementations

If you implement `Worker` manually (without the derive macro), you now need to implement three traits:

```rust
// 1. Job trait on the job data struct
impl oxanus::Job for MyJob {
    fn worker_name() -> &'static str {
        std::any::type_name::<MyWorker>()
    }

    // Optional: unique_id, on_conflict, should_resurrect, throttle_cost
}

// 2. Worker trait on the worker struct (now generic over job type)
#[async_trait::async_trait]
impl oxanus::Worker<MyJob> for MyWorker {
    type Error = MyError;

    async fn process(&self, job: &MyJob, ctx: &oxanus::JobContext) -> Result<(), MyError> {
        // ...
        Ok(())
    }

    // Optional: max_retries, retry_delay, cron_schedule, cron_queue_config
}

// 3. FromContext trait for constructing the worker from app context
impl oxanus::FromContext<AppState> for MyWorker {
    fn from_context(ctx: &AppState) -> Self {
        Self { state: ctx.clone() }
    }
}
```

### 9. Update process method body

In the process method:
- Job fields that were `self.field` are now `job.field`
- App context that was `ctx.ctx.field` is now `self.field` (via `FromContext`)
- Job metadata `ctx.meta` is now `ctx.meta` (unchanged)
- Job state `ctx.state` is now `ctx.state` (unchanged)

### 10. Accessing `JobContext` in process

`JobContext` still provides:
- `ctx.meta` — job metadata (id, retries, timestamps, etc.)
- `ctx.state` — persistent job state across retries (`ctx.state.get()`, `ctx.state.update()`)

The app context (`ctx.ctx` in 0.9) is no longer on `JobContext`. Instead, it lives on the worker struct itself, populated via `FromContext`.

## New public types

| Type | Description |
|------|-------------|
| `Job` | Trait for job data structs (serializable payloads) |
| `JobContext` | Replaces `Context<T>` — metadata + state, no generic |
| `ContextValue<T>` | Wrapper for app context passed to `oxanus::run()` |
| `FromContext<T>` | Trait for constructing workers from app context |
| `Processable` | Type-erased trait for job execution (internal, but public) |
| `BoxedProcessable<ET>` | Boxed `Processable` (internal, but public) |

## Removed types

| Type | Replacement |
|------|-------------|
| `Context<T>` | `JobContext` |
| `BoxedWorker<DT, ET>` | `BoxedProcessable<ET>` |

## Removed methods

| Method | Replacement |
|--------|-------------|
| `Config::register_cron_worker()` | `Config::register_worker::<W, J>()` with cron attributes |
| `Config::has_registered_worker::<W>()` | `Config::has_registered_worker_type::<W>()` |
| `Config::has_registered_cron_worker::<W>()` | `Config::has_registered_cron_worker_type::<W>()` |
| `Context::value()` | `ContextValue::new()` |
