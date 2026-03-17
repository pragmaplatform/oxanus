use darling::FromDeriveInput;
use proc_macro_error2::abort;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Path};

#[derive(Debug, FromDeriveInput)]
#[darling(attributes(oxanus), supports(struct_any))]
struct BatchProcessorArgs {
    args: Option<Path>,
    context: Option<Path>,
    error: Option<Path>,
    registry: Option<Path>,
    batch_size: usize,
    batch_linger_ms: u64,
}

pub fn expand_derive_batch_processor(input: DeriveInput) -> TokenStream {
    let args = match BatchProcessorArgs::from_derive_input(&input) {
        Ok(v) => v,
        Err(e) => {
            abort!(input.ident, "{}", e);
        }
    };

    let struct_ident = &input.ident;
    let batch_size = args.batch_size;
    let batch_linger_ms = args.batch_linger_ms;

    let item_type = match &args.args {
        Some(path) => quote!(#path),
        None => abort!(
            input.ident,
            "BatchProcessor must have #[oxanus(args = MyJob)] attribute specifying the item type"
        ),
    };

    let type_context = match &args.context {
        Some(context) => quote!(#context),
        None => quote!(WorkerContext),
    };

    let type_error = match &args.error {
        Some(error) => quote!(#error),
        None => quote!(WorkerError),
    };

    let batch_impl = expand_batch_impl(struct_ident, &item_type, &type_error, batch_size, batch_linger_ms);
    let from_context_impl = expand_from_context_impl(struct_ident, &type_context, &input);
    let registry_impl = expand_registry(struct_ident, &item_type, &type_context, &type_error, &args, batch_size, batch_linger_ms);

    quote! {
        #batch_impl
        #from_context_impl
        #registry_impl
    }
}

fn expand_batch_impl(
    struct_ident: &syn::Ident,
    item_type: &TokenStream,
    type_error: &TokenStream,
    batch_size: usize,
    batch_linger_ms: u64,
) -> TokenStream {
    quote! {
        #[automatically_derived]
        #[async_trait::async_trait]
        impl oxanus::BatchProcessor for #struct_ident {
            type Item = #item_type;
            type Error = #type_error;

            async fn process_batch(
                &self,
                items: &[Self::Item],
                ctx: &oxanus::JobContext,
            ) -> Result<(), Self::Error> {
                self.process_batch(items, ctx).await
            }

            fn batch_size() -> usize {
                #batch_size
            }

            fn batch_linger_ms() -> u64 {
                #batch_linger_ms
            }
        }
    }
}

fn expand_from_context_impl(
    struct_ident: &syn::Ident,
    type_context: &TokenStream,
    input: &DeriveInput,
) -> TokenStream {
    let fields = match &input.data {
        Data::Struct(data_struct) => &data_struct.fields,
        _ => abort!(input.ident, "BatchProcessor must be a struct."),
    };

    let constructor = match fields {
        Fields::Unit => quote!(Self),
        Fields::Named(named) if named.named.is_empty() => quote!(Self {}),
        Fields::Named(named) if named.named.len() == 1 => {
            let field = named.named.first().expect("checked len == 1");
            let field_name = field.ident.as_ref().expect("named field has ident");
            quote!(Self { #field_name: ctx.clone() })
        }
        Fields::Named(named) => {
            abort!(
                input.ident,
                "BatchProcessor structs with {} fields cannot auto-derive FromContext. \
                 Implement oxanus::FromContext<{}> manually.",
                named.named.len(),
                type_context
            );
        }
        Fields::Unnamed(_) => {
            abort!(
                input.ident,
                "Tuple batch processor structs are not supported. Use named fields or a unit struct."
            );
        }
    };

    quote! {
        #[automatically_derived]
        impl oxanus::FromContext<#type_context> for #struct_ident {
            fn from_context(ctx: &#type_context) -> Self {
                #constructor
            }
        }
    }
}

fn expand_registry(
    struct_ident: &syn::Ident,
    item_type: &TokenStream,
    type_context: &TokenStream,
    type_error: &TokenStream,
    args: &BatchProcessorArgs,
    batch_size: usize,
    batch_linger_ms: u64,
) -> TokenStream {
    let component_registry = match &args.registry {
        Some(registry) => quote!(#registry),
        None => quote!(ComponentRegistry),
    };

    if cfg!(feature = "registry") && component_registry.to_string() != "None" {
        quote! {
            oxanus::register_component! {
                #component_registry(oxanus::ComponentRegistry {
                    module_path: module_path!(),
                    type_name: stringify!(#struct_ident),
                    definition: || {
                        oxanus::ComponentDefinition::BatchProcessor(oxanus::BatchProcessorConfig {
                            worker_name: <#item_type as oxanus::Job>::worker_name().to_owned(),
                            batch_size: #batch_size,
                            batch_linger_ms: #batch_linger_ms,
                            factory: oxanus::batch_factory::<#struct_ident, #type_context, #type_error>,
                        })
                    }
                })
            }
        }
    } else {
        quote!()
    }
}
