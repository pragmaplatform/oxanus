use darling::{Error, FromDeriveInput, FromMeta};
use proc_macro_error2::abort;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, Expr, Ident, LitStr, Meta, Path, Token, punctuated::Punctuated};

#[derive(Debug, FromDeriveInput)]
#[darling(attributes(oxanus), supports(struct_any))]
struct OxanusJobArgs {
    worker: Option<Path>,
    unique_id: Option<UniqueIdSpec>,
    on_conflict: Option<Ident>,
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

    /// #[unique_id = Self::unique_id]
    CustomFunc(Path),
}

#[derive(Debug)]
enum ThrottleCost {
    /// #[throttle_cost = 2]
    Value(u64),
    /// #[throttle_cost = Self::throttle_cost]
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

impl FromMeta for ThrottleCost {
    fn from_meta(meta: &Meta) -> darling::Result<Self> {
        match meta {
            Meta::NameValue(nv) => match &nv.value {
                Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Int(lit),
                    ..
                }) => {
                    let value = lit.base10_parse::<u64>()?;
                    Ok(Self::Value(value))
                }
                Expr::Path(expr_path) => Ok(Self::CustomFunc(expr_path.path.clone())),
                other => Err(Error::custom(format!(
                    "Unsupported throttle_cost value: {other:?}",
                ))),
            },
            _ => Err(Error::custom(
                "throttle_cost must be a name-value attribute",
            )),
        }
    }
}

pub fn expand_derive_job(input: DeriveInput) -> TokenStream {
    let args = match OxanusJobArgs::from_derive_input(&input) {
        Ok(v) => v,
        Err(e) => {
            abort!(input.ident, "{}", e);
        }
    };

    let struct_ident = &input.ident;
    let worker = match &args.worker {
        Some(worker) => quote!(#worker),
        None => {
            let name = struct_ident.to_string();
            let base = name.strip_suffix("Job").unwrap_or(&name);
            let worker_ident = Ident::new(&format!("{base}Worker"), struct_ident.span());
            quote!(#worker_ident)
        }
    };

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

    let resurrect = match args.resurrect {
        Some(value) => quote! {
            fn should_resurrect() -> bool
            where
                Self: Sized,
            {
                #value
            }
        },
        None => quote!(),
    };

    let throttle_cost = match &args.throttle_cost {
        Some(throttle_cost) => expand_throttle_cost(throttle_cost),
        None => quote!(),
    };

    quote! {
        #[automatically_derived]
        impl oxanus::Job for #struct_ident {
            fn worker_name() -> &'static str
            where
                Self: Sized,
            {
                std::any::type_name::<#worker>()
            }

            #unique_id

            #on_conflict

            #resurrect

            #throttle_cost
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
                && !name.starts_with(|c: char| c.is_ascii_digit())
                && seen.insert(name.clone())
            {
                result.push(syn::Ident::new(&name, proc_macro2::Span::call_site()));
            }
        }
    }
    result
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
