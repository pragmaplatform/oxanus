use darling::{FromDeriveInput, FromMeta};
use heck::ToSnakeCase;
use proc_macro_error2::abort;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Expr, ExprCall, ExprLit, ExprPath, Fields, Lit, Path};

#[derive(Debug, FromDeriveInput)]
#[darling(attributes(oxana), supports(struct_any))]
struct OxanaArgs {
    registry: Option<Path>,
    key: Option<String>,
    prefix: Option<String>,
    concurrency: Option<ConcurrencyArg>,
    discovery_interval_ms: Option<u64>,
    throttle: Option<ThrottleArgs>,
}

#[derive(Debug)]
enum ConcurrencyArg {
    Fixed(usize),
    Dynamic(usize),
}

impl FromMeta for ConcurrencyArg {
    fn from_expr(expr: &Expr) -> darling::Result<Self> {
        match expr {
            Expr::Lit(expr_lit) => parse_concurrency_value(expr_lit).map(Self::Fixed),
            Expr::Call(expr_call) => parse_concurrency_call(expr_call),
            Expr::Group(group) => Self::from_expr(&group.expr),
            _ => Err(darling::Error::custom(
                "expected integer literal, Fixed(<integer>), or Dynamic(<integer>)",
            )
            .with_span(expr)),
        }
    }
}

fn parse_concurrency_call(expr_call: &ExprCall) -> darling::Result<ConcurrencyArg> {
    let Expr::Path(ExprPath { path, .. }) = expr_call.func.as_ref() else {
        return Err(
            darling::Error::custom("expected Fixed(<integer>) or Dynamic(<integer>)")
                .with_span(expr_call),
        );
    };
    let Some(ident) = path.get_ident() else {
        return Err(
            darling::Error::custom("expected Fixed(<integer>) or Dynamic(<integer>)")
                .with_span(path),
        );
    };
    let mut args = expr_call.args.iter();
    let Some(arg) = args.next() else {
        return Err(
            darling::Error::custom("expected exactly one integer argument").with_span(expr_call),
        );
    };
    if args.next().is_some() {
        return Err(
            darling::Error::custom("expected exactly one integer argument").with_span(expr_call),
        );
    }

    let value = parse_concurrency_expr(arg)?;
    match ident.to_string().as_str() {
        "Fixed" => Ok(ConcurrencyArg::Fixed(value)),
        "Dynamic" => Ok(ConcurrencyArg::Dynamic(value)),
        _ => Err(
            darling::Error::custom("expected Fixed(<integer>) or Dynamic(<integer>)")
                .with_span(ident),
        ),
    }
}

fn parse_concurrency_expr(expr: &Expr) -> darling::Result<usize> {
    match expr {
        Expr::Lit(expr_lit) => parse_concurrency_value(expr_lit),
        Expr::Group(group) => parse_concurrency_expr(&group.expr),
        _ => Err(darling::Error::custom("expected integer literal").with_span(expr)),
    }
}

fn parse_concurrency_value(expr_lit: &ExprLit) -> darling::Result<usize> {
    let Lit::Int(value) = &expr_lit.lit else {
        return Err(darling::Error::custom("expected integer literal").with_span(expr_lit));
    };

    value
        .base10_parse()
        .map_err(|err| darling::Error::custom(err.to_string()).with_span(value))
}

#[derive(Debug, FromMeta)]
struct ThrottleArgs {
    window_ms: i64,
    limit: u64,
}

pub fn expand_derive_queue(input: DeriveInput) -> TokenStream {
    let args = match OxanaArgs::from_derive_input(&input) {
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
        Some(ConcurrencyArg::Fixed(v)) => quote!(.concurrency(#v)),
        Some(ConcurrencyArg::Dynamic(v)) => quote!(.dynamic_concurrency(#v)),
        None => quote!(),
    };

    let discovery_interval = match args.discovery_interval_ms {
        Some(v) => quote!(.discovery_interval(std::time::Duration::from_millis(#v))),
        None => quote!(),
    };

    let throttle = match args.throttle {
        Some(ThrottleArgs { window_ms, limit }) => quote! {
            .throttle(oxana::QueueThrottle {
                window_ms: #window_ms,
                limit: #limit,
            })
        },
        None => quote!(),
    };

    let component_registry = match args.registry {
        Some(registry) => quote!(#registry),
        None => quote!(ComponentRegistry),
    };

    let registry = if cfg!(feature = "registry") && component_registry.to_string() != "None" {
        quote! {
            oxana::register_component! {
                #component_registry(oxana::ComponentRegistry {
                    module_path: module_path!(),
                    type_name: stringify!(#struct_ident),
                    definition: || {
                        oxana::ComponentDefinition::Queue(
                            <#struct_ident as oxana::Queue>::to_config()
                        )
                    }
                })
            }
        }
    } else {
        quote!()
    };

    quote! {
        #[automatically_derived]
        impl oxana::Queue for #struct_ident {
            fn to_config() -> oxana::QueueConfig {
                oxana::QueueConfig::#kind(#key)
                    #concurrency
                    #discovery_interval
                    #throttle
            }
        }

        #registry
    }
}
