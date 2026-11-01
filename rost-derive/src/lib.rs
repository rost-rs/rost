//! `#[derive(Rost)]`: generates the code that loads a struct from a `.rost` document.
//!
//! The derive implements `rost::Load` for a struct with named fields. The generated code
//! reads the entries of the document and converts each field directly, instead of going
//! through serde's visitor protocol; every value it does not recognise falls back to the
//! serde reader, so `rost::load` and `rost::from_str` agree on every document.
//!
//! Field attributes (`#[rost(...)]`):
//!
//! * `rename = "name"`: the key in the document;
//! * `default`: use `Default::default()` when the key is missing (an `Option` field
//!   already defaults to `None`);
//! * `serde`: read the field through its serde `Deserialize` implementation, for types
//!   that do not implement `Load` (lists, enums, dates…).

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, spanned::Spanned, Data, DeriveInput, Fields, GenericParam, LitStr, Type};

#[proc_macro_derive(Rost, attributes(rost))]
pub fn derive_rost(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

struct FieldInfo {
    ident: syn::Ident,
    ty: Type,
    key: String,
    serde: bool,
    default: bool,
    optional: bool,
}

fn is_option(ty: &Type) -> bool {
    matches!(ty, Type::Path(p) if p.qself.is_none() && p.path.segments.last().is_some_and(|s| s.ident == "Option"))
}

fn fields_of(input: &DeriveInput) -> syn::Result<Vec<FieldInfo>> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new(input.span(), "#[derive(Rost)] supports structs with named fields only"));
    };
    let Fields::Named(named) = &data.fields else {
        return Err(syn::Error::new(input.span(), "#[derive(Rost)] supports structs with named fields only"));
    };
    named
        .named
        .iter()
        .map(|field| {
            let ident = field.ident.clone().expect("named field");
            let mut info = FieldInfo {
                key: ident.to_string().trim_start_matches("r#").to_owned(),
                ty: field.ty.clone(),
                optional: is_option(&field.ty),
                ident,
                serde: false,
                default: false,
            };
            for attr in field.attrs.iter().filter(|a| a.path().is_ident("rost")) {
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("rename") {
                        info.key = meta.value()?.parse::<LitStr>()?.value();
                    } else if meta.path.is_ident("serde") {
                        info.serde = true;
                    } else if meta.path.is_ident("default") {
                        info.default = true;
                    } else {
                        return Err(meta.error("unknown rost attribute; expected `rename`, `default` or `serde`"));
                    }
                    Ok(())
                })?;
            }
            Ok(info)
        })
        .collect()
}

fn expand(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;
    let fields = fields_of(input)?;

    // `'de` outlives every lifetime of the struct, so `&'a str` fields can borrow from the text.
    let mut generics = input.generics.clone();
    let lifetimes: Vec<_> = generics
        .params
        .iter()
        .filter_map(|p| if let GenericParam::Lifetime(l) = p { Some(l.lifetime.clone()) } else { None })
        .collect();
    let type_params: Vec<_> = generics
        .params
        .iter()
        .filter_map(|p| if let GenericParam::Type(t) = p { Some(t.ident.clone()) } else { None })
        .collect();
    {
        let wh = generics.make_where_clause();
        for l in &lifetimes {
            wh.predicates.push(syn::parse_quote!('de: #l));
        }
        for t in &type_params {
            wh.predicates.push(syn::parse_quote!(#t: ::rost::Load<'de>));
        }
    }
    let (_, ty_generics, where_clause) = generics.split_for_impl();
    let impl_params = input.generics.params.iter();
    let impl_generics = quote!(<'de #(, #impl_params)*>);

    let count = fields.len();
    let names = fields.iter().map(|f| &f.key);
    let slots: Vec<_> = (0..count).map(|i| format_ident!("__field{i}")).collect();
    let types = fields.iter().map(|f| &f.ty);
    let arms = fields.iter().zip(&slots).enumerate().map(|(i, (f, slot))| {
        let ty = &f.ty;
        let key = &f.key;
        let read = if f.serde {
            quote!(__f.value::<#ty>().map_err(|__e| __f.locate(__e))?)
        } else {
            quote!(<#ty as ::rost::Load<'de>>::load(__f).map_err(|__e| __f.locate(__e))?)
        };
        quote! {
            #i => {
                if #slot.is_some() {
                    return ::core::result::Result::Err(::rost::de::DeError::new(
                        ::std::format!("duplicate field `{}`", #key)));
                }
                #slot = ::core::option::Option::Some(#read);
            }
        }
    });
    let inits = fields.iter().zip(&slots).map(|(f, slot)| {
        let ident = &f.ident;
        let key = &f.key;
        let missing = if f.default {
            quote!(::core::default::Default::default())
        } else if f.optional {
            quote!(::core::option::Option::None)
        } else {
            quote!(return ::core::result::Result::Err(::rost::de::DeError::new(
                ::std::format!("missing field `{}`", #key))))
        };
        quote!(#ident: match #slot { ::core::option::Option::Some(v) => v, ::core::option::Option::None => #missing })
    });

    Ok(quote! {
        impl #impl_generics ::rost::Load<'de> for #name #ty_generics #where_clause {
            fn load(__f: &mut ::rost::Fast<'_, 'de>) -> ::core::result::Result<Self, ::rost::de::DeError> {
                __f.block(<Self as ::rost::Load<'de>>::load_body)
            }

            fn load_body(__f: &mut ::rost::Fast<'_, 'de>) -> ::core::result::Result<Self, ::rost::de::DeError> {
                const __NAMES: [&str; #count] = [#(#names),*];
                #(let mut #slots: ::core::option::Option<#types> = ::core::option::Option::None;)*
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
                ::core::result::Result::Ok(Self { #(#inits),* })
            }
        }
    })
}
