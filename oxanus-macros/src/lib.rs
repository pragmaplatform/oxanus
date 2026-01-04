mod queue;
use queue::*;

use proc_macro::TokenStream;
use proc_macro_error2::proc_macro_error;
use syn::{DeriveInput, parse_macro_input};

/// Generates impl for `oxanus::Queue`.
///
/// Example usage:
/// ```
/// #[derive(Serialize, oxanus::Queue)]
/// #[oxanus(key = "my_queue")]
/// #[oxanus(concurrency = 1)]
/// pub struct MyQueue;
/// ```
#[proc_macro_error]
#[proc_macro_derive(Queue, attributes(oxanus))]
pub fn derive_queue(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    expand_derive_queue(input).into()
}
