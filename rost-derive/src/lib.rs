//! `#[derive(Rost)]` and `#[derive(Describe)]` for the `rost` crate.
//!
//! `Rost` implements `rost::Load`: the generated code reads the entries of the document and
//! converts each field directly, instead of going through serde's visitor protocol; every
//! value it does not recognise falls back to the serde reader, so `rost::load` and
//! `rost::from_str` agree on every document.
//!
//! * structs with named fields are read field by field;
//! * a newtype struct (`struct Port(u16)`) reads as its field;
//! * an enum with only unit variants reads as a word (`mode: dark`);
//! * any other enum, and tuple structs with several fields, are read through serde, so
//!   they must also derive `serde::Deserialize`.
//!
//! `Describe` implements `rost::Describe`, so the type can print a JSON Schema or a `.rost`
//! schema (`rost::Describe::describe().to_json_schema()`).
//!
//! Container attributes (`#[rost(...)]` on the struct or enum):
//!
//! * `rename_all = "snake_case"`: `lowercase`, `UPPERCASE`, `snake_case`, `kebab-case`,
//!   `camelCase`, `PascalCase` or `SCREAMING_SNAKE_CASE`, for the keys or the variants.
//!
//! Field attributes (`#[rost(...)]`; `rename` also works on enum variants):
//!
//! * `rename = "name"`: the key in the document;
//! * `default`: use `Default::default()` when the key is missing (an `Option` field
//!   already defaults to `None`); `optional` is the same;
//! * `serde`: read the field through its serde `Deserialize` implementation, for types
//!   that do not implement `Load` (lists, enums with data, dates…);
//! * `range(min = 1, max = 4096)`: either bound optional; numbers;
//! * `one_of("a", "b")`: the value must be one of these (strings, numbers, booleans);
//! * `regex = "^[a-z]+$"`: the text must match (needs the `regex` feature of `rost`);
//! * `flatten`: the entries of a struct-typed field sit next to the parent's own.
//!
//! A value outside its `range` or `one_of`, or not matching its `regex`, is an error with
//! the line and column of its key and the field path:
//! `3:3: field `window.width`: 5000 is out of range: expected 1 to 4096`.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::punctuated::Punctuated;
use syn::{
    parse_macro_input, spanned::Spanned, Data, DeriveInput, Expr, Fields, GenericParam, Lit, LitStr, Meta, Token, Type,
};

#[proc_macro_derive(Rost, attributes(rost))]
pub fn derive_rost(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_load(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[proc_macro_derive(Describe, attributes(rost))]
pub fn derive_describe(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_describe(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

struct FieldInfo {
    ident: syn::Ident,
    ty: Type,
    key: String,
    doc: String,
    serde: bool,
    default: bool,
    optional: bool,
    flatten: bool,
    min: Option<Expr>,
    max: Option<Expr>,
    one_of: Vec<String>,
    regex: Option<String>,
}

fn is_option(ty: &Type) -> bool {
    matches!(ty, Type::Path(p) if p.qself.is_none() && p.path.segments.last().is_some_and(|s| s.ident == "Option"))
}

/// The `///` documentation of an item, one line per comment line.
fn doc_of(attrs: &[syn::Attribute]) -> String {
    let lines: Vec<String> = attrs
        .iter()
        .filter(|a| a.path().is_ident("doc"))
        .filter_map(|a| match &a.meta {
            Meta::NameValue(nv) => match &nv.value {
                Expr::Lit(l) => match &l.lit {
                    Lit::Str(s) => Some(s.value().trim().to_string()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        })
        .collect();
    lines.join("\n")
}

/// Splits an identifier into lowercase words (`snake_case` and `PascalCase` both).
fn words(name: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    for c in name.chars() {
        if c == '_' {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
        } else if c.is_uppercase() && !current.is_empty() && current.chars().last().is_some_and(|p| p.is_lowercase() || p.is_ascii_digit()) {
            out.push(std::mem::take(&mut current));
            current.extend(c.to_lowercase());
        } else {
            current.extend(c.to_lowercase());
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn capitalize(w: &str) -> String {
    let mut chars = w.chars();
    chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
}

fn apply_case(name: &str, style: &str) -> Option<String> {
    let w = words(name);
    Some(match style {
        "lowercase" => w.concat(),
        "UPPERCASE" => w.concat().to_uppercase(),
        "snake_case" => w.join("_"),
        "SCREAMING_SNAKE_CASE" => w.join("_").to_uppercase(),
        "kebab-case" => w.join("-"),
        "PascalCase" => w.iter().map(|x| capitalize(x)).collect(),
        "camelCase" => w.iter().enumerate().map(|(i, x)| if i == 0 { x.clone() } else { capitalize(x) }).collect(),
        _ => return None,
    })
}

fn rename_all_of(attrs: &[syn::Attribute]) -> syn::Result<Option<String>> {
    let mut style = None;
    for attr in attrs.iter().filter(|a| a.path().is_ident("rost")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename_all") {
                let lit = meta.value()?.parse::<LitStr>()?;
                if apply_case("a_b", &lit.value()).is_none() {
                    return Err(syn::Error::new(lit.span(), "unknown case; expected `lowercase`, `UPPERCASE`, `snake_case`, `kebab-case`, `camelCase`, `PascalCase` or `SCREAMING_SNAKE_CASE`"));
                }
                style = Some(lit.value());
                Ok(())
            } else {
                Err(meta.error("unknown container attribute; expected `rename_all`"))
            }
        })?;
    }
    Ok(style)
}

fn lit_text(lit: &Lit) -> syn::Result<String> {
    match lit {
        Lit::Str(s) => Ok(s.value()),
        Lit::Int(i) => Ok(i.base10_digits().to_string()),
        Lit::Float(f) => Ok(f.base10_digits().to_string()),
        Lit::Bool(b) => Ok(b.value.to_string()),
        other => Err(syn::Error::new(other.span(), "expected a string, number or boolean literal")),
    }
}

fn parse_field(field: &syn::Field, rename_all: &Option<String>) -> syn::Result<FieldInfo> {
    let ident = field.ident.clone().expect("named field");
    let plain = ident.to_string().trim_start_matches("r#").to_owned();
    let mut info = FieldInfo {
        key: rename_all.as_deref().and_then(|s| apply_case(&plain, s)).unwrap_or(plain),
        ty: field.ty.clone(),
        doc: doc_of(&field.attrs),
        optional: is_option(&field.ty),
        ident,
        serde: false,
        default: false,
        flatten: false,
        min: None,
        max: None,
        one_of: Vec::new(),
        regex: None,
    };
    for attr in field.attrs.iter().filter(|a| a.path().is_ident("rost")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                info.key = meta.value()?.parse::<LitStr>()?.value();
            } else if meta.path.is_ident("serde") {
                info.serde = true;
            } else if meta.path.is_ident("default") || meta.path.is_ident("optional") {
                info.default = true;
            } else if meta.path.is_ident("flatten") {
                info.flatten = true;
            } else if meta.path.is_ident("range") {
                meta.parse_nested_meta(|m| {
                    let value: Expr = m.value()?.parse()?;
                    if m.path.is_ident("min") {
                        info.min = Some(value);
                    } else if m.path.is_ident("max") {
                        info.max = Some(value);
                    } else {
                        return Err(m.error("expected `min = …` or `max = …`"));
                    }
                    Ok(())
                })?;
            } else if meta.path.is_ident("one_of") {
                let content;
                syn::parenthesized!(content in meta.input);
                for lit in Punctuated::<Lit, Token![,]>::parse_terminated(&content)? {
                    info.one_of.push(lit_text(&lit)?);
                }
            } else if meta.path.is_ident("regex") {
                info.regex = Some(meta.value()?.parse::<LitStr>()?.value());
            } else {
                return Err(meta.error(
                    "unknown rost attribute; expected `rename`, `default`, `optional`, `serde`, `flatten`, `range`, `one_of` or `regex`",
                ));
            }
            Ok(())
        })?;
    }
    Ok(info)
}

fn named_fields(input: &DeriveInput) -> syn::Result<Option<Vec<FieldInfo>>> {
    let Data::Struct(data) = &input.data else { return Ok(None) };
    let Fields::Named(named) = &data.fields else { return Ok(None) };
    let rename_all = rename_all_of(&input.attrs)?;
    named.named.iter().map(|f| parse_field(f, &rename_all)).collect::<syn::Result<Vec<_>>>().map(Some)
}

/// `(variant ident, name in the document)` for an enum whose variants are all unit variants.
fn unit_variants(input: &DeriveInput) -> syn::Result<Option<Vec<(syn::Ident, String)>>> {
    let Data::Enum(data) = &input.data else { return Ok(None) };
    if !data.variants.iter().all(|v| matches!(v.fields, Fields::Unit)) {
        return Ok(None);
    }
    let rename_all = rename_all_of(&input.attrs)?;
    let mut out = Vec::new();
    for variant in &data.variants {
        let mut name = rename_all.as_deref().and_then(|s| apply_case(&variant.ident.to_string(), s)).unwrap_or_else(|| variant.ident.to_string());
        for attr in variant.attrs.iter().filter(|a| a.path().is_ident("rost")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("rename") {
                    name = meta.value()?.parse::<LitStr>()?.value();
                    Ok(())
                } else {
                    Err(meta.error("unknown variant attribute; expected `rename`"))
                }
            })?;
        }
        out.push((variant.ident.clone(), name));
    }
    Ok(Some(out))
}

/// The generics of the impl: `'de` outlives every lifetime of the type, and every type
/// parameter gets `bound`.
fn impl_parts(input: &DeriveInput, bound: proc_macro2::TokenStream, with_de: bool) -> (proc_macro2::TokenStream, proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let mut generics = input.generics.clone();
    {
        let lifetimes: Vec<_> = generics.params.iter().filter_map(|p| if let GenericParam::Lifetime(l) = p { Some(l.lifetime.clone()) } else { None }).collect();
        let type_params: Vec<_> = generics.params.iter().filter_map(|p| if let GenericParam::Type(t) = p { Some(t.ident.clone()) } else { None }).collect();
        let wh = generics.make_where_clause();
        if with_de {
            for l in &lifetimes {
                wh.predicates.push(syn::parse_quote!('de: #l));
            }
        }
        for t in &type_params {
            wh.predicates.push(syn::parse_quote!(#t: #bound));
        }
    }
    let (_, ty_generics, where_clause) = generics.split_for_impl();
    let params = input.generics.params.iter();
    let impl_generics = if with_de { quote!(<'de #(, #params)*>) } else { quote!(<#(#params),*>) };
    (impl_generics, quote!(#ty_generics), quote!(#where_clause))
}

fn bound(expr: &Option<Expr>) -> proc_macro2::TokenStream {
    match expr {
        Some(e) => quote!(::core::option::Option::Some((#e) as f64)),
        None => quote!(::core::option::Option::None),
    }
}

/// The validation statements for a field whose value is in `__v`.
fn checks(f: &FieldInfo) -> proc_macro2::TokenStream {
    let mut tests = Vec::new();
    if f.min.is_some() || f.max.is_some() {
        let (lo, hi) = (bound(&f.min), bound(&f.max));
        tests.push(quote!(::rost::validate::range(*__x, #lo, #hi)));
    }
    if !f.one_of.is_empty() {
        let allowed = &f.one_of;
        tests.push(quote!(::rost::validate::one_of(__x, &[#(#allowed),*])));
    }
    if let Some(pattern) = &f.regex {
        tests.push(quote!({
            static __PATTERN: ::rost::validate::Pattern = ::rost::validate::Pattern::new(#pattern);
            ::rost::validate::matches(__x, &__PATTERN)
        }));
    }
    if tests.is_empty() {
        return quote!();
    }
    let run = quote! {
        #(if let ::core::result::Result::Err(__m) = #tests {
            return ::core::result::Result::Err(__f.locate(::rost::de::DeError::new(__m)));
        })*
    };
    if f.optional {
        quote!(if let ::core::option::Option::Some(__x) = &__v { #run })
    } else {
        quote!({ let __x = &__v; #run })
    }
}

fn expand_load(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = impl_parts(input, quote!(::rost::Load<'de>), true);

    if let Some(fields) = named_fields(input)? {
        return Ok(expand_struct(name, &impl_generics, &ty_generics, &where_clause, &fields));
    }
    if let Some(variants) = unit_variants(input)? {
        let idents = variants.iter().map(|(i, _)| i);
        let names = variants.iter().map(|(_, n)| n);
        let list = variants.iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>().join(", ");
        return Ok(quote! {
            impl #impl_generics ::rost::Load<'de> for #name #ty_generics #where_clause {
                fn load(__f: &mut ::rost::Fast<'_, 'de>) -> ::core::result::Result<Self, ::rost::de::DeError> {
                    let __text = __f.text_value()?;
                    match &*__text {
                        #(#names => ::core::result::Result::Ok(Self::#idents),)*
                        __other => ::core::result::Result::Err(::rost::de::DeError::new(
                            ::std::format!("unknown variant `{}`: expected one of {}", __other, #list))),
                    }
                }
            }
        });
    }
    if let Data::Struct(data) = &input.data
        && let Fields::Unnamed(unnamed) = &data.fields
        && unnamed.unnamed.len() == 1
    {
        let ty = &unnamed.unnamed[0].ty;
        return Ok(quote! {
            impl #impl_generics ::rost::Load<'de> for #name #ty_generics #where_clause {
                fn load(__f: &mut ::rost::Fast<'_, 'de>) -> ::core::result::Result<Self, ::rost::de::DeError> {
                    <#ty as ::rost::Load<'de>>::load(__f).map(Self)
                }
                fn load_body(__f: &mut ::rost::Fast<'_, 'de>) -> ::core::result::Result<Self, ::rost::de::DeError> {
                    <#ty as ::rost::Load<'de>>::load_body(__f).map(Self)
                }
            }
        });
    }
    if matches!(&input.data, Data::Enum(_)) || matches!(&input.data, Data::Struct(d) if matches!(d.fields, Fields::Unnamed(_))) {
        // Data-carrying enums and multi-field tuple structs: the serde reader does the work.
        return Ok(quote! {
            impl #impl_generics ::rost::Load<'de> for #name #ty_generics #where_clause {
                fn load(__f: &mut ::rost::Fast<'_, 'de>) -> ::core::result::Result<Self, ::rost::de::DeError> {
                    __f.value::<Self>()
                }
            }
        });
    }
    Err(syn::Error::new(input.span(), "#[derive(Rost)] supports structs, newtype structs and enums"))
}

fn expand_struct(
    name: &syn::Ident,
    impl_generics: &proc_macro2::TokenStream,
    ty_generics: &proc_macro2::TokenStream,
    where_clause: &proc_macro2::TokenStream,
    fields: &[FieldInfo],
) -> proc_macro2::TokenStream {
    let slots: Vec<_> = (0..fields.len()).map(|i| format_ident!("__field{i}")).collect();
    let keyed: Vec<usize> = (0..fields.len()).filter(|&i| !fields[i].flatten).collect();
    let count = keyed.len();
    let names = keyed.iter().map(|&i| &fields[i].key);
    let types = fields.iter().map(|f| &f.ty);
    let arms = keyed.iter().enumerate().map(|(n, &i)| {
        let f = &fields[i];
        let (ty, key, slot) = (&f.ty, &f.key, &slots[i]);
        let read = if f.serde {
            quote!(__f.value::<#ty>().map_err(|__e| __f.locate(__e))?)
        } else {
            quote!(<#ty as ::rost::Load<'de>>::load(__f).map_err(|__e| __f.locate(__e))?)
        };
        let checks = checks(f);
        quote! {
            #n => {
                if #slot.is_some() {
                    return ::core::result::Result::Err(__f.locate(::rost::de::DeError::new(
                        ::std::format!("field `{}`: duplicate", #key))));
                }
                let __v = #read;
                #checks
                #slot = ::core::option::Option::Some(__v);
            }
        }
    });
    let flatten_ty = fields.iter().filter(|f| f.flatten).count();
    let flattened = fields.iter().zip(&slots).filter(|(f, _)| f.flatten).map(|(f, slot)| {
        let ty = &f.ty;
        quote! {
            __f.rewind(__mark);
            #slot = ::core::option::Option::Some(<#ty as ::rost::Load<'de>>::load_body(__f)?);
        }
    });
    let mark = if flatten_ty > 0 { quote!(let __mark = __f.mark();) } else { quote!() };
    let inits = fields.iter().zip(&slots).map(|(f, slot)| {
        let ident = &f.ident;
        let key = &f.key;
        let missing = if f.default {
            quote!(::core::default::Default::default())
        } else if f.optional {
            quote!(::core::option::Option::None)
        } else {
            quote!(return ::core::result::Result::Err(::rost::de::DeError::new(
                ::std::format!("field `{}`: missing", #key))))
        };
        quote!(#ident: match #slot { ::core::option::Option::Some(v) => v, ::core::option::Option::None => #missing })
    });

    quote! {
        impl #impl_generics ::rost::Load<'de> for #name #ty_generics #where_clause {
            fn load(__f: &mut ::rost::Fast<'_, 'de>) -> ::core::result::Result<Self, ::rost::de::DeError> {
                __f.block(<Self as ::rost::Load<'de>>::load_body)
            }

            fn load_body(__f: &mut ::rost::Fast<'_, 'de>) -> ::core::result::Result<Self, ::rost::de::DeError> {
                const __NAMES: [&str; #count] = [#(#names),*];
                #(let mut #slots: ::core::option::Option<#types> = ::core::option::Option::None;)*
                #mark
                let mut __hint = 0usize;
                loop {
                    match __f.field(&__NAMES, __hint)? {
                        ::rost::Field::End => break,
                        ::rost::Field::Unknown => __f.skip()?,
                        ::rost::Field::Known(__i) => {
                            __hint = __i + 1;
                            match __i {
                                #(#arms)*
                                _ => ::core::unreachable!(),
                            }
                        }
                    }
                }
                #(#flattened)*
                ::core::result::Result::Ok(Self { #(#inits),* })
            }
        }
    }
}

fn expand_describe(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = impl_parts(input, quote!(::rost::Describe), false);
    let body = if let Some(fields) = named_fields(input)? {
        let pushes = fields.iter().map(|f| {
            let (ty, key, doc) = (&f.ty, &f.key, &f.doc);
            if f.flatten {
                return quote! {
                    if let ::rost::TypeDesc::Object(__sub) = <#ty as ::rost::Describe>::describe() {
                        __fields.extend(__sub);
                    }
                };
            }
            let (lo, hi) = (bound(&f.min), bound(&f.max));
            let range = if f.min.is_some() || f.max.is_some() { quote!(.with_range(#lo, #hi)) } else { quote!() };
            let one_of = if f.one_of.is_empty() {
                quote!()
            } else {
                let allowed = &f.one_of;
                quote!(.with_one_of(::std::vec![#(::std::string::ToString::to_string(#allowed)),*]))
            };
            let regex = match &f.regex {
                Some(p) => quote!(.with_regex(#p)),
                None => quote!(),
            };
            let required = !(f.default || f.optional);
            quote! {
                __fields.push(::rost::FieldDesc {
                    name: ::std::string::ToString::to_string(#key),
                    doc: ::std::string::ToString::to_string(#doc),
                    ty: <#ty as ::rost::Describe>::describe() #range #one_of #regex,
                    required: #required,
                });
            }
        });
        quote! {
            let mut __fields = ::std::vec::Vec::new();
            #(#pushes)*
            ::rost::TypeDesc::Object(__fields)
        }
    } else if let Some(variants) = unit_variants(input)? {
        let names = variants.iter().map(|(_, n)| n);
        quote!(::rost::TypeDesc::Enum(::std::vec![#(::std::string::ToString::to_string(#names)),*]))
    } else if let Data::Struct(data) = &input.data
        && let Fields::Unnamed(unnamed) = &data.fields
        && unnamed.unnamed.len() == 1
    {
        let ty = &unnamed.unnamed[0].ty;
        quote!(<#ty as ::rost::Describe>::describe())
    } else {
        quote!(::rost::TypeDesc::Any)
    };
    Ok(quote! {
        impl #impl_generics ::rost::Describe for #name #ty_generics #where_clause {
            fn describe() -> ::rost::TypeDesc {
                #body
            }
        }
    })
}
