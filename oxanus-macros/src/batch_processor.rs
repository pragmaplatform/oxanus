use darling::FromDeriveInput;
use proc_macro_error2::abort;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, GenericArgument, Path, PathArguments, Type};

#[derive(Debug, FromDeriveInput)]
#[darling(attributes(oxanus), supports(struct_named))]
struct BatchProcessorArgs {
    context: Option<Path>,
    error: Option<Path>,
    registry: Option<Path>,
    batch_size: usize,
    batch_linger_ms: u64,
}

fn extract_vec_field(fields: &Fields) -> Option<(syn::Ident, syn::Type)> {
    for field in fields.iter() {
        if let Type::Path(type_path) = &field.ty
            && let Some(segment) = type_path.path.segments.last()
            && segment.ident == "Vec"
            && let PathArguments::AngleBracketed(args) = &segment.arguments
            && let Some(GenericArgument::Type(inner_ty)) = args.args.first()
        {
            return Some((field.ident.clone()?, inner_ty.clone()));
        }
    }
    None
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

    let type_context = match args.context {
        Some(context) => quote!(#context),
        None => quote!(WorkerContext),
    };

    let type_error = match args.error {
        Some(error) => quote!(#error),
        None => quote!(WorkerError),
    };

    let fields = match &input.data {
        Data::Struct(data_struct) => &data_struct.fields,
        _ => abort!(input.ident, "BatchProcessor must be a struct."),
    };

    let (field_name, item_type) = match extract_vec_field(fields) {
        Some(v) => v,
        None => abort!(
            input.ident,
            "BatchProcessor must have a Vec<T> field for batch items."
        ),
    };

    let component_registry = match args.registry {
        Some(registry) => quote!(#registry),
        None => quote!(ComponentRegistry),
    };

    let registry = if cfg!(feature = "registry") && component_registry.to_string() != "None" {
        quote! {
            oxanus::register_component! {
                #component_registry(oxanus::ComponentRegistry {
                    module_path: module_path!(),
                    type_name: stringify!(#struct_ident),
                    definition: || {
                        oxanus::ComponentDefinition::BatchProcessor(oxanus::BatchProcessorConfig {
                            worker_name: std::any::type_name::<#item_type>().to_owned(),
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
    };

    quote! {
        #[automatically_derived]
        #[async_trait::async_trait]
        impl oxanus::Worker for #struct_ident {
            type Context = #type_context;
            type Error = #type_error;

            async fn process(&self, data: &oxanus::Context<Self::Context>) -> Result<(), Self::Error> {
                self.process_batch(data).await
            }
        }

        #[automatically_derived]
        impl oxanus::BatchProcessor for #struct_ident {
            type Item = #item_type;

            fn from_args(args: Vec<serde_json::Value>) -> Result<Self, oxanus::OxanusError> {
                let items: Vec<#item_type> = args
                    .into_iter()
                    .map(|a| serde_json::from_value(a))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Self { #field_name: items })
            }

            fn batch_size() -> usize {
                #batch_size
            }

            fn batch_linger_ms() -> u64 {
                #batch_linger_ms
            }
        }

        impl #item_type {
            async fn process(&self, data: &oxanus::Context<#type_context>) -> Result<(), #type_error> {
                #struct_ident { #field_name: vec![self.clone()] }.process_batch(data).await
            }
        }

        #registry
    }
}
