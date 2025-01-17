use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, quote_spanned, ToTokens};
use syn2::{
    parenthesized, parse::ParseStream, spanned::Spanned, Attribute, Ident, ItemEnum, Variant,
};

struct StackErrorArgs {
    path: syn2::Path,
}

impl syn2::parse::Parse for StackErrorArgs {
    fn parse(input: ParseStream) -> syn2::Result<Self> {
        if input.is_empty() {
            return Ok(StackErrorArgs {
                path: syn2::parse_quote!(libs::StackError),
            });
        }

        let ident: Ident = input.parse()?;
        if ident != "path" {
            return Err(syn2::Error::new(ident.span(), "expected `path`"));
        }

        let content;
        parenthesized!(content in input);
        let path: syn2::Path = content.parse()?;

        Ok(StackErrorArgs { path })
    }
}

pub fn stack_trace_style_impl(args: TokenStream2, input: TokenStream2) -> TokenStream2 {
    let trait_path = match syn2::parse2::<StackErrorArgs>(args) {
        Ok(args) => args.path,
        Err(_) => syn2::parse_quote!(libs::StackError),
    };

    let item_enum = match syn2::parse2::<ItemEnum>(input.clone()) {
        Ok(item) => item,
        Err(e) => return e.to_compile_error(),
    };

    let enum_name = item_enum.ident;
    let mut variants = vec![];

    for error_variant in item_enum.variants {
        let variant = ErrorVariant::from_enum_variant(error_variant);
        variants.push(variant);
    }

    let debug_fmt_fn = build_debug_fmt_impl(enum_name.clone(), variants.clone());
    let next_fn = build_next_impl(enum_name.clone(), variants, &trait_path);
    let debug_impl = build_debug_impl(enum_name.clone(), &trait_path);

    quote! {
        #input

        impl #trait_path for #enum_name {
            #debug_fmt_fn
            #next_fn
        }

        #debug_impl
    }
}

fn build_debug_fmt_impl(enum_name: Ident, variants: Vec<ErrorVariant>) -> TokenStream2 {
    let match_arms = variants
        .iter()
        .map(|v| v.to_debug_match_arm())
        .collect::<Vec<_>>();

    quote! {
        fn debug_fmt(&self, layer: usize, buf: &mut Vec<String>) {
            use #enum_name::*;
            match self {
                #(#match_arms)*
            }
        }
    }
}

fn build_next_impl(
    enum_name: Ident,
    variants: Vec<ErrorVariant>,
    path: &syn2::Path,
) -> TokenStream2 {
    let match_arms = variants
        .iter()
        .map(|v| v.to_next_match_arm())
        .collect::<Vec<_>>();

    quote! {
        fn next(&self) -> Option<&dyn #path> {
            use #enum_name::*;
            match self {
                #(#match_arms)*
            }
        }
    }
}

fn build_debug_impl(enum_name: Ident, path: &syn2::Path) -> TokenStream2 {
    quote! {
        impl std::fmt::Debug for #enum_name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                use #path;
                let mut buf = vec![];
                self.debug_fmt(0, &mut buf);
                write!(f, "{}", buf.join("\n"))
            }
        }
    }
}

#[derive(Clone, Debug)]
struct ErrorVariant {
    name: Ident,
    fields: Vec<Ident>,
    has_location: bool,
    has_source: bool,
    has_external_cause: bool,
    display: TokenStream2,
    span: Span,
    cfg_attr: Option<Attribute>,
}

impl ErrorVariant {
    fn from_enum_variant(variant: Variant) -> Self {
        let span = variant.span();
        let mut has_location = false;
        let mut has_source = false;
        let mut has_external_cause = false;

        for field in &variant.fields {
            if let Some(ident) = &field.ident {
                if ident == "location" {
                    has_location = true;
                } else if ident == "source" {
                    has_source = true;
                } else if ident == "error" {
                    has_external_cause = true;
                }
            }
        }

        let mut display = None;
        let mut cfg_attr = None;
        for attr in variant.attrs {
            if attr.path().is_ident("snafu") {
                // roughly implementation to allow other attrs use on enum variant
                let stream = attr.to_token_stream();
                let attr_str = stream.to_string();
                if !attr_str.contains("display") {
                    continue;
                }

                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("display") {
                        let content;
                        parenthesized!(content in meta.input);
                        let display_ts: TokenStream2 = content.parse()?;
                        display = Some(display_ts);
                        Ok(())
                    } else {
                        Err(meta.error("unrecognized repr"))
                    }
                })
                .expect("Each error should contains a display attribute");
            }

            if attr.path().is_ident("cfg") {
                cfg_attr = Some(attr);
            }
        }

        let field_ident = variant
            .fields
            .iter()
            .map(|f| f.ident.clone().unwrap_or_else(|| Ident::new("_", f.span())))
            .collect();

        Self {
            name: variant.ident,
            fields: field_ident,
            has_location,
            has_source,
            has_external_cause,
            display: display.unwrap(),
            span,
            cfg_attr,
        }
    }

    fn to_debug_match_arm(&self) -> TokenStream2 {
        let name = &self.name;
        let fields = &self.fields;
        let display = &self.display;
        let cfg = if let Some(cfg) = &self.cfg_attr {
            quote_spanned!(cfg.span() => #cfg)
        } else {
            quote! {}
        };

        match (self.has_location, self.has_source, self.has_external_cause) {
            (true, true, _) => quote_spanned! {
               self.span => #cfg #[allow(unused_variables)] #name { #(#fields),*, } => {
                    buf.push(format!("{layer}: {}, at {}", format!(#display), location));
                    source.debug_fmt(layer + 1, buf);
                },
            },
            (true, false, true) => quote_spanned! {
                self.span => #cfg #[allow(unused_variables)] #name { #(#fields),* } => {
                    buf.push(format!("{layer}: {}, at {}", format!(#display), location));
                    buf.push(format!("{}: {:?}", layer + 1, error));
                },
            },
            (true, false, false) => quote_spanned! {
                self.span => #cfg #[allow(unused_variables)] #name { #(#fields),* } => {
                    buf.push(format!("{layer}: {}, at {}", format!(#display), location));
                },
            },
            (false, true, _) => quote_spanned! {
                self.span => #cfg #[allow(unused_variables)] #name { #(#fields),* } => {
                    buf.push(format!("{layer}: {}", format!(#display)));
                    source.debug_fmt(layer + 1, buf);
                },
            },
            (false, false, true) => quote_spanned! {
                self.span => #cfg #[allow(unused_variables)] #name { #(#fields),* } => {
                    buf.push(format!("{layer}: {}", format!(#display)));
                    buf.push(format!("{}: {:?}", layer + 1, error));
                },
            },
            (false, false, false) => quote_spanned! {
                self.span => #cfg #[allow(unused_variables)] #name { #(#fields),* } => {
                    buf.push(format!("{layer}: {}", format!(#display)));
                },
            },
        }
    }

    fn to_next_match_arm(&self) -> TokenStream2 {
        let name = &self.name;
        let fields = &self.fields;
        let cfg = if let Some(cfg) = &self.cfg_attr {
            quote_spanned!(cfg.span() => #cfg)
        } else {
            quote! {}
        };

        if self.has_source {
            quote_spanned! {
                self.span => #cfg #[allow(unused_variables)] #name { #(#fields),* } => {
                    Some(source)
                },
            }
        } else {
            quote_spanned! {
                self.span => #cfg #[allow(unused_variables)] #name { #(#fields),* } =>{
                    None
                }
            }
        }
    }
}
