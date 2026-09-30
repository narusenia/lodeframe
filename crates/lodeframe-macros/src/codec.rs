use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Data, DataEnum, DeriveInput, Expr, ExprLit, ExprUnary, Fields, Ident, Lit, Path, UnOp, Variant,
    parse_quote, spanned::Spanned,
};

use crate::crate_path;

pub fn encode(input: &DeriveInput) -> syn::Result<TokenStream> {
    let krate = crate_path(input)?;
    let body = match &input.data {
        Data::Struct(s) => {
            let (pat, names) = bindings(&s.fields);
            quote! {
                let Self #pat = self;
                #( #krate::Encode::encode(#names, w)?; )*
                ::core::result::Result::Ok(())
            }
        }
        Data::Enum(e) => {
            let arms = tagged(e)?.into_iter().map(|(variant, tag)| {
                let name = &variant.ident;
                let (pat, names) = bindings(&variant.fields);
                quote! {
                    Self::#name #pat => {
                        #krate::Encode::encode(&#krate::VarInt(#tag), w)?;
                        #( #krate::Encode::encode(#names, w)?; )*
                        ::core::result::Result::Ok(())
                    }
                }
            });
            quote! { match self { #( #arms )* } }
        }
        Data::Union(u) => {
            return Err(syn::Error::new(
                u.union_token.span,
                "unions are not supported",
            ));
        }
    };
    Ok(impl_block(
        input,
        &parse_quote!(#krate::Encode),
        quote! {
            fn encode(&self, w: &mut impl ::std::io::Write) -> #krate::Result<()> { #body }
        },
    ))
}

pub fn decode(input: &DeriveInput) -> syn::Result<TokenStream> {
    let krate = crate_path(input)?;
    let body = match &input.data {
        Data::Struct(s) => {
            let value = construct(quote!(Self), &s.fields, &krate);
            quote! { ::core::result::Result::Ok(#value) }
        }
        Data::Enum(e) => {
            let arms = tagged(e)?.into_iter().map(|(variant, tag)| {
                let name = &variant.ident;
                let value = construct(quote!(Self::#name), &variant.fields, &krate);
                quote! { #tag => ::core::result::Result::Ok(#value), }
            });
            let what = format!("unknown tag for {}", input.ident);
            quote! {
                match <#krate::VarInt as #krate::Decode>::decode(r)?.0 {
                    #( #arms )*
                    _ => ::core::result::Result::Err(#krate::Error::InvalidValue(#what)),
                }
            }
        }
        Data::Union(u) => {
            return Err(syn::Error::new(
                u.union_token.span,
                "unions are not supported",
            ));
        }
    };
    Ok(impl_block(
        input,
        &parse_quote!(#krate::Decode),
        quote! {
            fn decode(r: &mut &[u8]) -> #krate::Result<Self> { #body }
        },
    ))
}

/// `impl<..> Trait for Type<..> where T: Trait, .. { items }`, bounding every type parameter.
fn impl_block(input: &DeriveInput, trait_path: &Path, items: TokenStream) -> TokenStream {
    let name = &input.ident;
    let mut generics = input.generics.clone();
    for param in input.generics.type_params() {
        let ident = &param.ident;
        generics
            .make_where_clause()
            .predicates
            .push(parse_quote!(#ident: #trait_path));
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    quote! {
        #[automatically_derived]
        impl #impl_generics #trait_path for #name #ty_generics #where_clause { #items }
    }
}

/// A destructuring pattern with every field bound to `__f<i>`, and those bindings.
/// Prefixed so a field called `w` or `r` cannot shadow the writer / reader.
fn bindings(fields: &Fields) -> (TokenStream, Vec<Ident>) {
    let names: Vec<_> = (0..fields.len()).map(|i| format_ident!("__f{i}")).collect();
    let pat = match fields {
        Fields::Named(f) => {
            let idents = f.named.iter().map(|f| &f.ident);
            quote! { { #( #idents: #names ),* } }
        }
        Fields::Unnamed(_) => quote! { ( #( #names ),* ) },
        Fields::Unit => quote! {},
    };
    (pat, names)
}

/// Builds `path { a: Decode::decode(r)?, .. }` (or the tuple / unit form), in field order.
fn construct(path: TokenStream, fields: &Fields, krate: &Path) -> TokenStream {
    let read = quote! { #krate::Decode::decode(r)? };
    match fields {
        Fields::Named(f) => {
            let idents = f.named.iter().map(|f| &f.ident);
            quote! { #path { #( #idents: #read ),* } }
        }
        Fields::Unnamed(f) => {
            let reads = f.unnamed.iter().map(|_| &read);
            quote! { #path( #( #reads ),* ) }
        }
        Fields::Unit => path,
    }
}

/// Pairs each variant with its tag, rejecting empty enums and non-literal tags.
/// Duplicate tags are left to rustc, which rejects duplicate discriminants itself.
fn tagged(e: &DataEnum) -> syn::Result<Vec<(&Variant, i32)>> {
    if e.variants.is_empty() {
        return Err(syn::Error::new(
            e.enum_token.span,
            "an enum needs at least one variant",
        ));
    }
    let mut next = 0_i32;
    let mut out = Vec::new();
    for v in &e.variants {
        let tag = match &v.discriminant {
            Some((_, expr)) => literal(expr)?,
            None => next,
        };
        next = tag.wrapping_add(1);
        out.push((v, tag));
    }
    Ok(out)
}

/// An integer literal, optionally negative.
fn literal(expr: &Expr) -> syn::Result<i32> {
    let (neg, lit) = match expr {
        Expr::Unary(ExprUnary {
            op: UnOp::Neg(_),
            expr,
            ..
        }) => (true, &**expr),
        e => (false, e),
    };
    if let Expr::Lit(ExprLit {
        lit: Lit::Int(int), ..
    }) = lit
    {
        let n: i64 = int.base10_parse()?;
        if let Ok(n) = i32::try_from(if neg { -n } else { n }) {
            return Ok(n);
        }
    }
    Err(syn::Error::new(
        expr.span(),
        "an enum tag must be an integer literal that fits in i32",
    ))
}
