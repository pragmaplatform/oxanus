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
    retry_delay: Option<RetryDelay>,
    unique_id: Option<UniqueIdSpec>,
    on_conflict: Option<Ident>,
    cron: Option<Cron>,
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

#[derive(Debug)]
enum RetryDelay {
    /// #[retry_delay = 3]
    Value(u64),
    /// #[retry_delay = mymod::func]
    CustomFunc(Path),
}

#[derive(Debug, FromMeta)]
struct Cron {
    schedule: String,
    queue: Option<Path>,
}

impl FromMeta for RetryDelay {
    fn from_meta(meta: &Meta) -> darling::Result<Self> {
        match meta {
            Meta::NameValue(nv) => match &nv.value {
                Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Int(lit),
                    ..
                }) => {
                    let value = lit.base10_parse::<u64>()?;
                    Ok(RetryDelay::Value(value))
                }
                Expr::Path(expr_path) => Ok(RetryDelay::CustomFunc(expr_path.path.clone())),
                other => Err(Error::custom(format!(
                    "unsupported retry_delay value: {:?}",
                    other
                ))),
            },
            _ => Err(Error::custom("retry_delay must be a name-value attribute")),
        }
    }
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
                            #[allow(clippy::collapsible_if)] // requires 1.88
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

    let retry_delay = match args.retry_delay {
        Some(retry_delay) => expand_retry_delay(retry_delay),
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

    let cron = match args.cron {
        Some(cron) => expand_cron(cron),
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

            #retry_delay

            #on_conflict

            #cron
        }
    }
}

fn expand_retry_delay(retry_delay: RetryDelay) -> TokenStream {
    match retry_delay {
        RetryDelay::Value(value) => {
            quote! {
                fn retry_delay(&self, _retries: u32) -> u64 {
                    #value
                }
            }
        }
        RetryDelay::CustomFunc(func) => {
            quote! {
                fn retry_delay(&self, retries: u32) -> u64 {
                    #func(self, retries)
                }
            }
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

        UniqueIdSpec::CustomFunc(func) => quote!(#func(self)),
    };

    quote! {
        fn unique_id(&self) -> Option<String> {
            #formatter
        }
    }
}

fn expand_cron(cron: Cron) -> TokenStream {
    let cron_schedule = cron.schedule;
    let cron_queue_config = match cron.queue {
        Some(queue) => quote! {
            fn cron_queue_config() -> Option<oxanus::QueueConfig>
            where
                Self: Sized,
            {
                use oxanus::Queue;
                Some(#queue::to_config())
            }
        },
        None => quote!(),
    };

    quote! {
        fn cron_schedule() -> Option<String>
        where
            Self: Sized,
        {
            Some(#cron_schedule.to_string())
        }

        #cron_queue_config
    }
}
