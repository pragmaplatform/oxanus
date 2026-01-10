use darling::{FromDeriveInput, FromMeta};
use heck::ToSnakeCase;
use proc_macro_error2::abort;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

#[derive(Debug, FromDeriveInput)]
#[darling(attributes(oxanus), supports(struct_any))]
struct OxanusArgs {
    key: Option<String>,
    prefix: Option<String>,
    concurrency: Option<usize>,
    throttle: Option<ThrottleArgs>,
}

#[derive(Debug, FromMeta)]
struct ThrottleArgs {
    window_ms: i64,
    limit: u64,
}

pub fn expand_derive_queue(input: DeriveInput) -> TokenStream {
    let args = match OxanusArgs::from_derive_input(&input) {
        Ok(v) => v,
        Err(e) => {
            // darling::Error -> emit nice compile errors
            abort!(input.ident, "{}", e);
        }
    };

    let num_fields = match &input.data {
        Data::Struct(data_struct) => match &data_struct.fields {
            Fields::Named(named) => named.named.len(),
            Fields::Unnamed(unnamed) => unnamed.unnamed.len(),
            Fields::Unit => 0,
        },
        _ => 0,
    };

    let struct_ident = &input.ident;

    let kind = if args.prefix.is_some() {
        if num_fields == 0 {
            abort!(input.ident, "Dynamic queues must have struct fields.");
        }
        quote!(as_dynamic)
    } else {
        quote!(as_static)
    };

    let key = match (args.key, num_fields) {
        (Some(k), 0) => k,
        (Some(_), _) => abort!(input.ident, "Static queue cannot have struct fields."),
        (None, 0) => struct_ident.to_string().to_snake_case(),
        (None, _) => match args.prefix {
            Some(k) => k,
            None => abort!(
                input.ident,
                "`prefix` must be specified for dynamic queues."
            ),
        },
    };

    let concurrency = match args.concurrency {
        Some(v) => quote!(.concurrency(#v)),
        None => quote!(),
    };

    let throttle = match args.throttle {
        Some(ThrottleArgs { window_ms, limit }) => quote! {
            .throttle(oxanus::QueueThrottle {
                window_ms: #window_ms,
                limit: #limit,
            })
        },
        None => quote!(),
    };

    quote! {
        #[automatically_derived]
        impl oxanus::Queue for #struct_ident {
            fn to_config() -> oxanus::QueueConfig {
                oxanus::QueueConfig::#kind(#key)
                    #concurrency
                    #throttle
            }
        }
    }
}
