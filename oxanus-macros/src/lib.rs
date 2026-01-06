mod queue;
mod worker;

use queue::*;
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

/// Generates impl for `oxanus::Worker`.
///
/// Example usage:
/// ```ignore
/// #[derive(Serialize, oxanus::Worker)]
/// #[oxanus(max_retries = 3, on_conflict = Replace)]
/// #[oxanus(unique_id = "test_worker_{id}")]
/// struct TestWorkerUniqueId {
///     id: i32,
/// }
/// ```
#[proc_macro_error]
#[proc_macro_derive(Worker, attributes(oxanus))]
pub fn derive_worker(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    expand_derive_worker(input).into()
}
