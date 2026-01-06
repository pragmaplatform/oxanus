use darling::{Error, FromDeriveInput, FromMeta};
use proc_macro_error2::abort;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Data, DeriveInput, Expr, Fields, Ident, LitStr, Meta, Path, Token, punctuated::Punctuated,
};

#[derive(Debug, FromDeriveInput)]
#[darling(attributes(oxanus), supports(struct_any))]
struct OxanusArgs {
    context: Option<Path>,
    error: Option<Path>,
    max_retries: Option<u32>,
    unique_id: Option<UniqueIdSpec>,
    on_conflict: Option<Ident>,
}

#[derive(Debug)]
enum UniqueIdSpec {
    /// #[unique_id = "job_{id}"]
    Shorthand(LitStr),

    /// #[unique_id(fmt = "...", name = expr, ...)]
    NamedFormatter {
        fmt: LitStr,
        args: Vec<(syn::Ident, Expr)>,
    },

    /// #[unique_id = mymod::func]
    CustomFunc(Path),
}

impl FromMeta for UniqueIdSpec {
    fn from_meta(meta: &Meta) -> darling::Result<Self> {
        match meta {
            Meta::NameValue(nv) => match &nv.value {
                Expr::Lit(expr_lit) => {
                    if let syn::Lit::Str(s) = &expr_lit.lit {
                        Ok(UniqueIdSpec::Shorthand(s.clone()))
                    } else {
                        Err(Error::custom("unique_id must be a string literal"))
                    }
                }
                Expr::Path(expr_path) => Ok(UniqueIdSpec::CustomFunc(expr_path.path.clone())),
                _ => Err(Error::custom("Expected string literal or type path.")),
            },
            Meta::List(list) => {
                let mut fmt = None;
                let mut args = Vec::new();

                let metas =
                    list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;

                for meta in metas {
                    match meta {
                        Meta::NameValue(nv) if nv.path.is_ident("fmt") => {
                            if let syn::Expr::Lit(expr_lit) = nv.value {
                                if let syn::Lit::Str(s) = expr_lit.lit {
                                    fmt = Some(s);
                                    continue;
                                }
                            }
                            return Err(Error::custom("fmt must be a string literal"));
                        }

                        Meta::NameValue(nv) => {
                            let ident = nv
                                .path
                                .get_ident()
                                .ok_or_else(|| Error::custom("expected identifier"))?
                                .clone();
                            args.push((ident, nv.value));
                        }

                        _ => return Err(Error::custom("unsupported unique_id syntax")),
                    }
                }

                let fmt = fmt.ok_or_else(|| Error::custom("missing fmt = \"...\""))?;
                Ok(UniqueIdSpec::NamedFormatter { fmt, args })
            }
            _ => Err(Error::custom("invalid unique_id attribute")),
        }
    }
}

pub fn expand_derive_worker(input: DeriveInput) -> TokenStream {
    let args = match OxanusArgs::from_derive_input(&input) {
        Ok(v) => v,
        Err(e) => {
            // darling::Error -> emit nice compile errors
            abort!(input.ident, "{}", e);
        }
    };

    let struct_ident = &input.ident;

    let type_context = match args.context {
        Some(context) => quote!(#context),
        None => quote!(WorkerContext),
    };

    let type_error = match args.error {
        Some(error) => quote!(#error),
        None => quote!(WorkerError),
    };

    let max_retries = match args.max_retries {
        Some(max_retries) => quote! {
            fn max_retries(&self) -> u32 {
                #max_retries
            }
        },
        None => quote!(),
    };

    let on_conflict = match args.on_conflict {
        Some(on_conflict) => quote! {
            fn on_conflict(&self) -> oxanus::JobConflictStrategy {
                oxanus::JobConflictStrategy::#on_conflict
            }
        },
        None => quote!(),
    };

    let unique_id = match args.unique_id {
        Some(unique_id) => expand_unique_id(
            unique_id,
            match &input.data {
                Data::Struct(data_struct) => &data_struct.fields,
                _ => abort!(input.ident, "Worker must be a struct."),
            },
        ),
        None => quote!(),
    };

    quote! {
        #[automatically_derived]
        #[async_trait::async_trait]
        impl oxanus::Worker for #struct_ident {
            type Context = #type_context;
            type Error = #type_error;

            async fn process(&self, data: &oxanus::Context<Self::Context>) -> Result<(), Self::Error> {
                self.process(data).await
            }

            #unique_id

            #max_retries

            #on_conflict
        }
    }
}

fn expand_unique_id(spec: UniqueIdSpec, fields: &Fields) -> TokenStream {
    let formatter = match spec {
        UniqueIdSpec::Shorthand(fmt) => {
            let args = fields.iter().filter_map(|f| {
                let name = f.ident.as_ref()?;
                Some(quote!(#name = self.#name))
            });

            quote! {
                Some(format!(
                    #fmt,
                    #(#args),*
                ))
            }
        }

        UniqueIdSpec::NamedFormatter { fmt, args } => {
            let args = args.iter().map(|(name, expr)| quote!(#name = #expr));

            quote! {
                Some(format!(
                    #fmt,
                    #(#args),*
                ))
            }
        }

        UniqueIdSpec::CustomFunc(path) => quote!(#path(self)),
    };

    quote! {
        fn unique_id(&self) -> Option<String> {
            #formatter
        }
    }
}
