use proc_macro2::TokenStream;
use quote::quote;
use syn::{DeriveInput, Ident, LitInt, parse_quote};

use crate::crate_path;

const STATES: &[&str] = &["Handshake", "Status", "Login", "Configuration", "Play"];
const SIDES: &[&str] = &["Clientbound", "Serverbound"];

pub fn expand(input: &DeriveInput) -> syn::Result<TokenStream> {
    let krate = crate_path(input)?;
    let (mut id, mut state, mut side) = (None, None, None);
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("packet")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("id") {
                id = Some(meta.value()?.parse::<LitInt>()?);
            } else if meta.path.is_ident("state") {
                state = Some(one_of(meta.value()?.parse()?, STATES)?);
            } else if meta.path.is_ident("side") {
                side = Some(one_of(meta.value()?.parse()?, SIDES)?);
            } else {
                return Err(
                    meta.error("unknown packet attribute, expected `id`, `state` or `side`")
                );
            }
            Ok(())
        })?;
    }
    let missing = |what: &str| {
        syn::Error::new_spanned(
            &input.ident,
            format!(
                "missing `{what}`: write #[packet(id = 0x00, state = Play, side = Clientbound)]"
            ),
        )
    };
    let id = id.ok_or_else(|| missing("id"))?;
    let state = state.ok_or_else(|| missing("state"))?;
    let side = side.ok_or_else(|| missing("side"))?;

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let trait_path: syn::Path = parse_quote!(#krate::Packet);
    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics #trait_path for #name #ty_generics #where_clause {
            const ID: i32 = #id;
            const STATE: #krate::State = #krate::State::#state;
            const SIDE: #krate::Side = #krate::Side::#side;
        }
    })
}

fn one_of(ident: Ident, allowed: &[&str]) -> syn::Result<Ident> {
    if allowed.iter().any(|a| ident == a) {
        Ok(ident)
    } else {
        Err(syn::Error::new(
            ident.span(),
            format!("expected one of: {}", allowed.join(", ")),
        ))
    }
}
