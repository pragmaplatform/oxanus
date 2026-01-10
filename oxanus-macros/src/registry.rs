use proc_macro_error2::abort;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, PathArguments, Type};

pub fn expand_derive_registry(input: DeriveInput) -> TokenStream {
    if !cfg!(feature = "registry") {
        return quote!();
    }

    let inner_type = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Unnamed(fields) if fields.unnamed.len() == 1 => &fields.unnamed[0].ty,
            _ => abort!(
                input.ident,
                "Expected a tuple struct with exactly one field",
            ),
        },
        _ => abort!(input.ident, "Expected a struct",),
    };

    let type_path = match inner_type {
        Type::Path(path) => path,
        _ => abort!(input.ident, "Expected a struct with inner type",),
    };

    let generics = match type_path.path.segments.last() {
        Some(segment) => match &segment.arguments {
            PathArguments::AngleBracketed(args) => args,
            _ => abort!(
                inner_type,
                "Expected generic arguments <WorkerContext, WorkerError>",
            ),
        },
        _ => abort!(input.ident, "Expected a struct with inner type",),
    };

    let struct_ident = &input.ident;

    quote! {
        oxanus::create_component_registry!(#struct_ident);

        impl #struct_ident {
            pub fn build_config(storage: &oxanus::Storage) -> oxanus::Config::#generics {
                oxanus::ComponentRegistry::#generics::build_config(
                    &storage,
                    oxanus::iterate_components::<#struct_ident>().map(|x| &x.0),
                )
            }
        }
    }
}
