use darling::{Error, FromDeriveInput, FromMeta};
use proc_macro_error2::{abort, emit_error};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Expr, Fields, Ident, Meta, Path};

#[derive(Debug, FromDeriveInput)]
#[darling(attributes(oxana), supports(struct_any))]
struct OxanaArgs {
    job: Option<Path>,
    context: Option<Path>,
    error: Option<Path>,
    registry: Option<Path>,
    max_retries: Option<MaxRetries>,
    retry_delay: Option<RetryDelay>,
    batch_size: Option<usize>,
    batch_timeout_ms: Option<u64>,
    cron: Option<Cron>,
}

#[derive(Debug)]
enum MaxRetries {
    /// #[max_retries = 3]
    Value(u32),
    /// #[max_retries = mymod::func]
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
    queue: Path,
}

macro_rules! impl_from_meta_for_num_or_path {
    ($ty:ty, $int:ty, $name:literal) => {
        impl FromMeta for $ty {
            fn from_meta(meta: &Meta) -> darling::Result<Self> {
                match meta {
                    Meta::NameValue(nv) => match &nv.value {
                        Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Int(lit),
                            ..
                        }) => {
                            let value = lit.base10_parse::<$int>()?;
                            Ok(Self::Value(value))
                        }
                        Expr::Path(expr_path) => Ok(Self::CustomFunc(expr_path.path.clone())),
                        other => Err(Error::custom(format!(
                            "Unsupported {} value: {other:?}",
                            $name
                        ))),
                    },
                    _ => Err(Error::custom(format!(
                        "{} must be a name-value attribute",
                        $name
                    ))),
                }
            }
        }
    };
}

impl_from_meta_for_num_or_path!(MaxRetries, u32, "max_retries");
impl_from_meta_for_num_or_path!(RetryDelay, u64, "retry_delay");

pub fn expand_derive_worker(input: DeriveInput) -> TokenStream {
    let args = match OxanaArgs::from_derive_input(&input) {
        Ok(v) => v,
        Err(e) => {
            abort!(input.ident, "{}", e);
        }
    };

    let struct_ident = &input.ident;

    let type_args = match &args.job {
        Some(path) => quote!(#path),
        None => {
            let name = struct_ident.to_string();
            let base = name.strip_suffix("Worker").unwrap_or(&name);
            let job_ident = Ident::new(&format!("{base}Job"), struct_ident.span());
            quote!(#job_ident)
        }
    };

    let type_context = match &args.context {
        Some(context) => quote!(#context),
        None => quote!(WorkerContext),
    };

    let type_error = match &args.error {
        Some(error) => quote!(#error),
        None => quote!(WorkerError),
    };

    let worker_impl = expand_worker_impl(struct_ident, &type_args, &type_error, &args);
    let from_context_impl = expand_from_context_impl(struct_ident, &type_context, &input);
    let registry_impl =
        expand_registry(struct_ident, &type_args, &type_context, &type_error, &args);

    quote! {
        #worker_impl
        #from_context_impl
        #registry_impl
    }
}

fn expand_worker_impl(
    struct_ident: &Ident,
    type_args: &TokenStream,
    type_error: &TokenStream,
    args: &OxanaArgs,
) -> TokenStream {
    let max_retries = match &args.max_retries {
        Some(max_retries) => expand_max_retries(max_retries, type_args),
        None => quote!(),
    };

    let retry_delay = match &args.retry_delay {
        Some(retry_delay) => expand_retry_delay(retry_delay, type_args),
        None => quote!(),
    };

    let cron = match &args.cron {
        Some(cron) => expand_cron(cron),
        None => quote!(),
    };

    let batch_config = expand_batch_config(struct_ident, args);
    let process = if batch_config.is_some() {
        quote! {
            async fn run_batch(&self, jobs: Vec<oxana::BatchItem<#type_args>>) -> Result<(), Self::Error> {
                self.process_batch(jobs).await
            }
        }
    } else {
        quote! {
            async fn run_batch(&self, jobs: Vec<oxana::BatchItem<#type_args>>) -> Result<(), Self::Error> {
                for oxana::BatchItem { job, ctx } in jobs {
                    self.process(job, &ctx).await?;
                }

                Ok(())
            }
        }
    };

    quote! {
        #[automatically_derived]
        #[async_trait::async_trait]
        impl oxana::Worker<#type_args> for #struct_ident {
            type Error = #type_error;

            #process

            #max_retries

            #retry_delay

            #cron

            #batch_config
        }
    }
}

fn expand_from_context_impl(
    struct_ident: &Ident,
    type_context: &TokenStream,
    input: &DeriveInput,
) -> TokenStream {
    let fields = match &input.data {
        Data::Struct(data_struct) => &data_struct.fields,
        _ => abort!(input.ident, "Worker must be a struct."),
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
            emit_error!(
                input.ident,
                "Worker structs with {} fields cannot auto-derive FromContext. \
                 Implement oxana::FromContext<{}> manually.",
                named.named.len(),
                type_context
            );
            return quote!();
        }
        Fields::Unnamed(_) => {
            emit_error!(
                input.ident,
                "Tuple worker structs are not supported. Use named fields or a unit struct."
            );
            return quote!();
        }
    };

    quote! {
        #[automatically_derived]
        impl oxana::FromContext<#type_context> for #struct_ident {
            fn from_context(ctx: &#type_context) -> Self {
                #constructor
            }
        }
    }
}

fn expand_registry(
    struct_ident: &Ident,
    type_args: &TokenStream,
    type_context: &TokenStream,
    type_error: &TokenStream,
    args: &OxanaArgs,
) -> TokenStream {
    let component_registry = match &args.registry {
        Some(registry) => quote!(#registry),
        None => quote!(ComponentRegistry),
    };

    if cfg!(feature = "registry") && component_registry.to_string() != "None" {
        quote! {
            oxana::register_component! {
                #component_registry(oxana::ComponentRegistry {
                    module_path: module_path!(),
                    type_name: stringify!(#struct_ident),
                    definition: || {
                        oxana::ComponentDefinition::Worker(oxana::WorkerConfig {
                            name: std::any::type_name::<#struct_ident>().to_owned(),
                            factory: oxana::job_factory::<#struct_ident, #type_args, #type_context, #type_error>,
                            batch_factory: oxana::job_batch_factory::<#struct_ident, #type_args, #type_context, #type_error>,
                            batch_config: <#struct_ident as oxana::Worker<#type_args>>::batch_config(),
                            on_demand: <#type_args as oxana::Job>::on_demand_args_template().map(|args_template| {
                                oxana::OnDemandJobRegistration {
                                    args_template,
                                    enqueue_factory: oxana::job_envelope_factory::<#type_args>,
                                }
                            }),
                            kind: <#struct_ident as oxana::Worker<#type_args>>::to_config(),
                        })
                    }
                })
            }
        }
    } else {
        quote!()
    }
}

fn expand_max_retries(max_retries: &MaxRetries, type_args: &TokenStream) -> TokenStream {
    match max_retries {
        MaxRetries::Value(value) => {
            quote! {
                fn max_retries(&self, _job: &#type_args) -> u32 {
                    #value
                }
            }
        }
        MaxRetries::CustomFunc(func) => {
            quote! {
                fn max_retries(&self, job: &#type_args) -> u32 {
                    #func(self, job)
                }
            }
        }
    }
}

fn expand_retry_delay(retry_delay: &RetryDelay, type_args: &TokenStream) -> TokenStream {
    match retry_delay {
        RetryDelay::Value(value) => {
            quote! {
                fn retry_delay(&self, _job: &#type_args, _retries: u32) -> u64 {
                    #value
                }
            }
        }
        RetryDelay::CustomFunc(func) => {
            quote! {
                fn retry_delay(&self, job: &#type_args, retries: u32) -> u64 {
                    #func(self, job, retries)
                }
            }
        }
    }
}

fn expand_batch_config(struct_ident: &Ident, args: &OxanaArgs) -> Option<TokenStream> {
    match (args.batch_size, args.batch_timeout_ms) {
        (Some(size), Some(timeout_ms)) => {
            if size == 0 {
                abort!(struct_ident, "batch_size must be greater than zero");
            }

            Some(quote! {
                fn batch_config() -> Option<oxana::WorkerBatchConfig>
                where
                    Self: Sized,
                {
                    Some(oxana::WorkerBatchConfig::new(
                        #size,
                        std::time::Duration::from_millis(#timeout_ms),
                    ))
                }
            })
        }
        (None, None) => None,
        (Some(_), None) => abort!(struct_ident, "batch_size requires batch_timeout_ms"),
        (None, Some(_)) => abort!(struct_ident, "batch_timeout_ms requires batch_size"),
    }
}

fn expand_cron(cron: &Cron) -> TokenStream {
    let cron_schedule = &cron.schedule;
    let queue = &cron.queue;

    quote! {
        fn cron_schedule() -> Option<String>
        where
            Self: Sized,
        {
            Some(#cron_schedule.to_string())
        }

        fn cron_queue_config() -> Option<oxana::QueueConfig>
        where
            Self: Sized,
        {
            use oxana::Queue;
            Some(#queue::to_config())
        }
    }
}
