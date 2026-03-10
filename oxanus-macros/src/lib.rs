mod batch_processor;
mod queue;
mod registry;
mod worker;

use batch_processor::*;
use queue::*;
use registry::*;
use worker::*;

use proc_macro::TokenStream;
use proc_macro_error2::proc_macro_error;
use syn::{DeriveInput, parse_macro_input};

/// Generates impl for `oxanus::Queue`.
///
/// Example usage:
/// ```ignore
/// #[derive(Serialize, oxanus::Queue)]
/// #[oxanus(key = "my_queue")]
/// #[oxanus(concurrency = 2)]
/// #[oxanus(throttle(window_ms = 3, limit = 4))]
/// pub struct MyQueue;
/// ```
#[proc_macro_error]
#[proc_macro_derive(Queue, attributes(oxanus))]
pub fn derive_queue(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    expand_derive_queue(input).into()
}

/// Generates impls for `oxanus::Worker<Args>`, `oxanus::Job`, and `oxanus::FromContext`.
///
/// Example usage:
/// ```ignore
/// #[derive(Debug, Serialize, Deserialize)]
/// struct MyJob { id: i32 }
///
/// #[derive(oxanus::Worker)]
/// #[oxanus(args = MyJob)]
/// #[oxanus(max_retries = 3, on_conflict = Replace)]
/// #[oxanus(unique_id = "my_job_{id}")]
/// struct MyWorker;
///
/// impl MyWorker {
///     async fn process(&self, job: &MyJob, ctx: &oxanus::JobContext) -> Result<(), WorkerError> {
///         Ok(())
///     }
/// }
/// ```
#[proc_macro_error]
#[proc_macro_derive(Worker, attributes(oxanus))]
pub fn derive_worker(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    expand_derive_worker(input).into()
}

/// Helper to define a component registry.
#[proc_macro_error]
#[proc_macro_derive(Registry, attributes(oxanus))]
pub fn derive_registry(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    expand_derive_registry(input).into()
}

/// Generates impl for `oxanus::BatchProcessor` and `oxanus::FromContext`.
///
/// Example usage:
/// ```ignore
/// #[derive(oxanus::BatchProcessor)]
/// #[oxanus(args = TestJob, batch_size = 3, batch_linger_ms = 1000)]
/// struct TestBatchProcessor;
///
/// impl TestBatchProcessor {
///     async fn process_batch(&self, jobs: &[TestJob], ctx: &oxanus::JobContext) -> Result<(), WorkerError> {
///         Ok(())
///     }
/// }
/// ```
#[proc_macro_error]
#[proc_macro_derive(BatchProcessor, attributes(oxanus))]
pub fn derive_batch_processor(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    expand_derive_batch_processor(input).into()
}
