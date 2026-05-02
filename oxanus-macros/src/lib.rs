mod job;
mod queue;
mod registry;
mod worker;

use job::*;
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

/// Generates impl for `oxanus::Job`.
///
/// Example usage:
/// ```ignore
/// #[derive(Serialize, oxanus::Job)]
/// #[oxanus(on_conflict = Replace)]
/// #[oxanus(unique_id = "foo_{id}")]
/// #[oxanus(throttle_cost = Self::throttle_cost)]
/// struct TestJob {
///     id: i32,
///     cost: u64,
/// }
///
/// impl TestJob {
///     fn throttle_cost(&self) -> Option<u64> {
///         Some(self.cost)
///     }
/// }
///
/// #[derive(oxanus::Worker)]
/// struct TestWorker;
/// ```
#[proc_macro_error]
#[proc_macro_derive(Job, attributes(oxanus))]
pub fn derive_job(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    expand_derive_job(input).into()
}

/// Generates impl for `oxanus::Worker`.
///
/// Example usage:
/// ```ignore
/// #[derive(oxanus::Worker)]
/// #[oxanus(max_retries = 3)]
/// #[oxanus(batch_size = 10, batch_timeout_ms = 500)]
/// struct TestWorkerUniqueId;
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
