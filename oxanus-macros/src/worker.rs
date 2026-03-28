use darling::{Error, FromDeriveInput, FromMeta};
use proc_macro_error2::{abort, emit_error};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Data, DeriveInput, Expr, Fields, Ident, LitStr, Meta, Path, Token, punctuated::Punctuated,
};

#[derive(Debug, FromDeriveInput)]
#[darling(attributes(oxanus), supports(struct_any))]
struct OxanusArgs {
    args: Option<Path>,
    context: Option<Path>,
    error: Option<Path>,
    registry: Option<Path>,
    max_retries: Option<MaxRetries>,
    retry_delay: Option<RetryDelay>,
    unique_id: Option<UniqueIdSpec>,
    on_conflict: Option<Ident>,
    cron: Option<Cron>,
    resurrect: Option<bool>,
    throttle_cost: Option<ThrottleCost>,
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

#[derive(Debug)]
enum ThrottleCost {
    /// #[throttle_cost = 2]
    Value(u64),
    /// #[throttle_cost = Self::throttle_cost]
    CustomFunc(Path),
}

#[derive(Debug, FromMeta)]
struct Cron {
    schedule: String,
    queue: Option<Path>,
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
impl_from_meta_for_num_or_path!(ThrottleCost, u64, "throttle_cost");

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

                        _ => return Err(Error::custom("Unsupported unique_id syntax")),
                    }
                }

                let fmt = fmt.ok_or_else(|| Error::custom("missing fmt = \"...\""))?;
                Ok(UniqueIdSpec::NamedFormatter { fmt, args })
            }
            _ => Err(Error::custom("Invalid unique_id attribute")),
        }
    }
}

fn extract_format_placeholders(fmt_str: &str) -> Vec<syn::Ident> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    let mut chars = fmt_str.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut name = String::new();
            for inner in chars.by_ref() {
                if inner == '}' {
                    break;
                }
                name.push(inner);
            }
            if !name.is_empty()
                && name.chars().all(|c| c.is_alphanumeric() || c == '_')
                && !name.chars().next().unwrap().is_ascii_digit()
                && seen.insert(name.clone())
            {
                result.push(syn::Ident::new(&name, proc_macro2::Span::call_site()));
            }
        }
    }
    result
}

pub fn expand_derive_worker(input: DeriveInput) -> TokenStream {
    let args = match OxanusArgs::from_derive_input(&input) {
        Ok(v) => v,
        Err(e) => {
            abort!(input.ident, "{}", e);
        }
    };

    let struct_ident = &input.ident;

    let type_args = match &args.args {
        Some(path) => quote!(#path),
        None => abort!(
            input.ident,
            "Worker must have #[oxanus(args = MyJob)] attribute specifying the job type"
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

    let worker_impl = expand_worker_impl(struct_ident, &type_args, &type_error, &args);
    let job_impl = expand_job_impl(struct_ident, &type_args, &args);
    let from_context_impl = expand_from_context_impl(struct_ident, &type_context, &input);
    let registry_impl =
        expand_registry(struct_ident, &type_args, &type_context, &type_error, &args);

    quote! {
        #worker_impl
        #job_impl
        #from_context_impl
        #registry_impl
    }
}

fn expand_worker_impl(
    struct_ident: &Ident,
    type_args: &TokenStream,
    type_error: &TokenStream,
    args: &OxanusArgs,
) -> TokenStream {
    let max_retries = match &args.max_retries {
        Some(max_retries) => expand_max_retries(max_retries),
        None => quote!(),
    };

    let retry_delay = match &args.retry_delay {
        Some(retry_delay) => expand_retry_delay(retry_delay),
        None => quote!(),
    };

    let cron = match &args.cron {
        Some(cron) => expand_cron(cron),
        None => quote!(),
    };

    quote! {
        #[automatically_derived]
        #[async_trait::async_trait]
        impl oxanus::Worker<#type_args> for #struct_ident {
            type Error = #type_error;

            async fn process(&self, job: &#type_args, ctx: &oxanus::JobContext) -> Result<(), Self::Error> {
                self.process(job, ctx).await
            }

            #max_retries

            #retry_delay

            #cron
        }
    }
}

fn expand_job_impl(
    struct_ident: &Ident,
    type_args: &TokenStream,
    args: &OxanusArgs,
) -> TokenStream {
    let unique_id = match &args.unique_id {
        Some(unique_id) => expand_unique_id(unique_id),
        None => quote!(),
    };

    let on_conflict = match &args.on_conflict {
        Some(on_conflict) => quote! {
            fn on_conflict(&self) -> oxanus::JobConflictStrategy {
                oxanus::JobConflictStrategy::#on_conflict
            }
        },
        None => quote!(),
    };

    let resurrect = expand_resurrect(args.resurrect);

    let throttle_cost = match &args.throttle_cost {
        Some(throttle_cost) => expand_throttle_cost(throttle_cost),
        None => quote!(),
    };

    quote! {
        #[automatically_derived]
        impl oxanus::Job for #type_args {
            fn worker_name() -> &'static str
            where
                Self: Sized,
            {
                std::any::type_name::<#struct_ident>()
            }

            #unique_id

            #on_conflict

            #resurrect

            #throttle_cost
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
                 Implement oxanus::FromContext<{}> manually.",
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
        impl oxanus::FromContext<#type_context> for #struct_ident {
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
    args: &OxanusArgs,
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
                        oxanus::ComponentDefinition::Worker(oxanus::WorkerConfig {
                            name: std::any::type_name::<#struct_ident>().to_owned(),
                            factory: oxanus::job_factory::<#struct_ident, #type_args, #type_context, #type_error>,
                            kind: <#struct_ident as oxanus::Worker<#type_args>>::to_config(),
                        })
                    }
                })
            }
        }
    } else {
        quote!()
    }
}

fn expand_resurrect(resurrect: Option<bool>) -> TokenStream {
    match resurrect {
        Some(value) => quote! {
            fn should_resurrect() -> bool
            where
                Self: Sized,
            {
                #value
            }
        },
        None => quote!(),
    }
}

fn expand_max_retries(max_retries: &MaxRetries) -> TokenStream {
    match max_retries {
        MaxRetries::Value(value) => {
            quote! {
                fn max_retries(&self) -> u32 {
                    #value
                }
            }
        }
        MaxRetries::CustomFunc(func) => {
            quote! {
                fn max_retries(&self) -> u32 {
                    #func(self)
                }
            }
        }
    }
}

fn expand_retry_delay(retry_delay: &RetryDelay) -> TokenStream {
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

fn expand_unique_id(spec: &UniqueIdSpec) -> TokenStream {
    let formatter = match spec {
        UniqueIdSpec::Shorthand(fmt) => {
            let fmt_str = fmt.value();
            let placeholders = extract_format_placeholders(&fmt_str);
            let args = placeholders.iter().map(|name| quote!(#name = self.#name));

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

fn expand_throttle_cost(throttle_cost: &ThrottleCost) -> TokenStream {
    match throttle_cost {
        ThrottleCost::Value(value) => {
            quote! {
                fn throttle_cost(&self) -> Option<u64> {
                    Some(#value)
                }
            }
        }
        ThrottleCost::CustomFunc(func) => {
            quote! {
                fn throttle_cost(&self) -> Option<u64> {
                    #func(self)
                }
            }
        }
    }
}

fn expand_cron(cron: &Cron) -> TokenStream {
    let cron_schedule = &cron.schedule;
    let cron_queue_config = match &cron.queue {
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
