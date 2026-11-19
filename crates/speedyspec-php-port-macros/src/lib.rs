//! `#[php_port(name = "PHP\\Interface\\Name")]` on a Rust port trait generates:
//!
//! - the trait itself, unchanged;
//! - `Php<Trait>`, the PHP adapter: it implements the trait by calling a PHP object that implements the twin;
//! - the twin's PHP source (an interface with the same methods, types mapped from Rust), submitted to
//!   `speedyspec_php_ext::twins()` so the twins area declares it at the start of every request.
//!
//! Every method must take `&self`. Arguments must implement `ToPhp` and the return type `FromPhp`. The twin is built
//! from the trait at compile time, so the two cannot drift.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{FnArg, ItemTrait, LitStr, Pat, ReturnType, TraitItem, Type, parse_macro_input};

#[proc_macro_attribute]
pub fn php_port(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut php_name: Option<String> = None;
    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("name") {
            let value: LitStr = meta.value()?.parse()?;
            php_name = Some(value.value());
            Ok(())
        } else {
            Err(meta.error("expected `name = \"PHP\\\\Interface\\\\Name\"`"))
        }
    });
    parse_macro_input!(attr with parser);
    let trait_item = parse_macro_input!(item as ItemTrait);
    let Some(php_name) = php_name else {
        return syn::Error::new_spanned(
            &trait_item.ident,
            "#[php_port] needs name = \"PHP\\\\Interface\\\\Name\"",
        )
        .to_compile_error()
        .into();
    };
    match expand(&php_name, &trait_item) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand(php_name: &str, item: &ItemTrait) -> syn::Result<proc_macro2::TokenStream> {
    let ident = &item.ident;
    let vis = &item.vis;
    let adapter = format_ident!("Php{}", ident);
    let mut methods = Vec::new();
    let mut php_methods = Vec::new();

    for entry in &item.items {
        let TraitItem::Fn(method) = entry else {
            continue;
        };
        let sig = &method.sig;
        let name = &sig.ident;
        let php_method = name.to_string();
        let mut receiver = false;
        let mut arg_names = Vec::new();
        let mut php_args = Vec::new();
        for input in &sig.inputs {
            match input {
                FnArg::Receiver(r) if r.reference.is_some() && r.mutability.is_none() => {
                    receiver = true
                }
                FnArg::Receiver(r) => {
                    return Err(syn::Error::new_spanned(r, "#[php_port] methods take &self"));
                }
                FnArg::Typed(typed) => {
                    let Pat::Ident(pat) = typed.pat.as_ref() else {
                        return Err(syn::Error::new_spanned(
                            &typed.pat,
                            "#[php_port] arguments need plain names",
                        ));
                    };
                    arg_names.push(pat.ident.clone());
                    php_args.push(format!("{}${}", php_type(&typed.ty, false), pat.ident));
                }
            }
        }
        if !receiver {
            return Err(syn::Error::new_spanned(
                sig,
                "#[php_port] methods take &self",
            ));
        }
        let (ret, php_ret) = match &sig.output {
            ReturnType::Default => (quote!(()), "void".to_string()),
            ReturnType::Type(_, ty) => (quote!(#ty), php_type(ty, true)),
        };
        let php_ret = php_ret.trim_end().to_string();
        let php_ret = if php_ret.is_empty() {
            String::new()
        } else {
            format!(": {php_ret}")
        };
        php_methods.push(format!(
            "    public function {php_method}({}){php_ret};",
            php_args.join(", ")
        ));
        let inputs = &sig.inputs;
        methods.push(quote! {
            fn #name(#inputs) -> #ret {
                let args = ::std::vec![#(::speedyspec_php_ext::ToPhp::to_php(#arg_names)),*];
                match self.0.call(#php_method, args) {
                    ::std::result::Result::Ok(value) => {
                        match <#ret as ::speedyspec_php_ext::FromPhp>::from_php(value) {
                            ::std::result::Result::Ok(result) => result,
                            ::std::result::Result::Err(why) => {
                                <#ret as ::speedyspec_php_ext::FromPhp>::from_failure(#php_name, #php_method, why)
                            }
                        }
                    }
                    ::std::result::Result::Err(err) => <#ret as ::speedyspec_php_ext::FromPhp>::from_failure(
                        #php_name,
                        #php_method,
                        ::std::string::ToString::to_string(&err),
                    ),
                }
            }
        });
    }

    let (namespace, short) = match php_name.rsplit_once('\\') {
        Some((namespace, short)) => (Some(namespace), short),
        None => (None, php_name),
    };
    let mut source = String::new();
    if let Some(namespace) = namespace {
        source.push_str(&format!("namespace {namespace};\n"));
    }
    source.push_str(&format!(
        "if (!\\interface_exists({short:?}, false)) {{\ninterface {short}\n{{\n"
    ));
    source.push_str(&php_methods.join("\n"));
    source.push_str("\n}\n}\n");

    Ok(quote! {
        #item

        /// The PHP adapter of this port: calls a PHP object implementing its twin.
        #vis struct #adapter(::speedyspec_php_ext::PhpObject);

        impl #adapter {
            pub fn new(object: ::speedyspec_php_ext::PhpObject) -> Self {
                Self(object)
            }

            pub fn object(&self) -> &::speedyspec_php_ext::PhpObject {
                &self.0
            }
        }

        impl #ident for #adapter {
            #(#methods)*
        }

        ::speedyspec_php_ext::inventory::submit! {
            ::speedyspec_php_ext::Twin { name: #php_name, source: #source }
        }
    })
}

/// The PHP type declaration for a Rust type, with a trailing space; empty when PHP should accept anything.
fn php_type(ty: &Type, ret: bool) -> String {
    let Type::Path(path) = ty else {
        return if matches!(ty, Type::Tuple(t) if t.elems.is_empty()) && ret {
            "void ".to_string()
        } else {
            String::new()
        };
    };
    let Some(last) = path.path.segments.last() else {
        return String::new();
    };
    let inner = || match &last.arguments {
        syn::PathArguments::AngleBracketed(args) => args.args.iter().find_map(|arg| match arg {
            syn::GenericArgument::Type(ty) => Some(ty.clone()),
            _ => None,
        }),
        _ => None,
    };
    let name = last.ident.to_string();
    match name.as_str() {
        "String" | "str" => "string ".to_string(),
        "i64" | "i32" | "u32" | "u64" | "usize" | "isize" => "int ".to_string(),
        "f64" | "f32" => "float ".to_string(),
        "bool" => "bool ".to_string(),
        "Vec" => "array ".to_string(),
        "Option" => match inner() {
            Some(inner) => {
                let t = php_type(&inner, ret);
                if t.is_empty() || t.trim() == "mixed" {
                    String::new()
                } else {
                    format!("?{t}")
                }
            }
            None => String::new(),
        },
        // A port that can fail returns Result<T, PortError>; PHP returns T (or throws).
        "Result" => inner().map(|t| php_type(&t, ret)).unwrap_or_default(),
        _ => String::new(),
    }
}
