//! `#[derive(Columns)]` for fitdata's row structs (see `engine::Columns`).

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Error, Expr, Fields, Lit, LitStr, Meta, parse_macro_input};

/// One column per field, named as serde names it, typed by the field's
/// `engine::ColumnType` and described by its doc comment. The struct is
/// also a `ColumnType` itself (a `Struct` column), so a row can nest it.
#[proc_macro_derive(Columns)]
pub fn derive_columns(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    columns(&input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

fn columns(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;
    if !input.generics.params.is_empty() {
        return Err(Error::new_spanned(&input.generics, "Columns: no generics"));
    }
    let Data::Struct(data) = &input.data else {
        return Err(Error::new_spanned(input, "Columns: structs only"));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(Error::new_spanned(input, "Columns: named fields only"));
    };
    let mut columns = Vec::new();
    for f in &fields.named {
        let serde = Serde::of(&f.attrs)?;
        if serde.skip {
            continue;
        }
        let ident = f.ident.as_ref().expect("named field");
        let column = serde
            .rename
            .unwrap_or_else(|| LitStr::new(&ident.to_string(), ident.span()));
        let doc = doc_lines(&f.attrs);
        let ty = &f.ty;
        columns.push(quote! { crate::engine::column::<#ty>(#column, &[#(#doc),*]) });
    }
    Ok(quote! {
        impl crate::engine::Columns for #name {
            fn columns() -> ::std::vec::Vec<::datafusion::arrow::datatypes::Field> {
                ::std::vec![#(#columns),*]
            }
        }

        impl crate::engine::ColumnType for #name {
            fn data_type() -> ::datafusion::arrow::datatypes::DataType {
                ::datafusion::arrow::datatypes::DataType::Struct(
                    <Self as crate::engine::Columns>::columns().into(),
                )
            }
        }
    })
}

/// The lines of a field's doc comment.
fn doc_lines(attrs: &[syn::Attribute]) -> Vec<LitStr> {
    attrs
        .iter()
        .filter(|a| a.path().is_ident("doc"))
        .filter_map(|a| match &a.meta {
            Meta::NameValue(nv) => match &nv.value {
                Expr::Lit(e) => match &e.lit {
                    Lit::Str(s) => Some(s.clone()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// The serde field attributes that change a column: `rename` and `skip`.
/// Others that would (`flatten`, `with`) are refused rather than ignored.
#[derive(Default)]
struct Serde {
    rename: Option<LitStr>,
    skip: bool,
}

impl Serde {
    fn of(attrs: &[syn::Attribute]) -> syn::Result<Self> {
        let mut out = Serde::default();
        for a in attrs.iter().filter(|a| a.path().is_ident("serde")) {
            a.parse_nested_meta(|m| {
                if m.path.is_ident("rename") {
                    out.rename = Some(m.value()?.parse()?);
                } else if m.path.is_ident("skip") || m.path.is_ident("skip_serializing") {
                    out.skip = true;
                } else if m.path.is_ident("flatten") || m.path.is_ident("with") {
                    return Err(m.error("Columns: not supported"));
                } else if m.input.peek(syn::Token![=]) {
                    // e.g. `default = "..."`, `skip_serializing_if = "..."`
                    m.value()?.parse::<Expr>()?;
                }
                Ok(())
            })?;
        }
        Ok(out)
    }
}
