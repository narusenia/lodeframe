// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Procedural macros: `Encode`/`Decode`, `Packet`, `text!`, and later `#[command]`, `Event`.
//!
//! The derives emit paths to `::lodeframe::protocol` by default. Inside the protocol
//! crate itself, or in a crate that depends on `lodeframe-protocol` directly, add
//! `#[lodeframe(crate = crate)]` (or the path to the crate) to the type.

mod codec;
mod packet;
mod text;

use proc_macro::TokenStream;
use syn::{DeriveInput, Path, parse_macro_input, parse_quote};

/// Derives `Encode`: fields in declaration order; enums as a `VarInt` tag then the fields.
///
/// Enum tags follow Rust's rules: an explicit integer literal (`A = 3`) or the previous
/// tag plus one, starting at 0.
#[proc_macro_derive(Encode, attributes(lodeframe))]
pub fn derive_encode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    codec::encode(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derives `Decode`, the inverse of `Encode`. An unknown enum tag is an error.
#[proc_macro_derive(Decode, attributes(lodeframe))]
pub fn derive_decode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    codec::decode(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Derives `Packet` from `#[packet(id = 0x26, state = Play, side = Clientbound)]`.
///
/// `id` is an integer expression, usually a generated constant such as
/// `ids::play::clientbound::KEEP_ALIVE`. `state` is one of `Handshake`, `Status`, `Login`, `Configuration`, `Play`;
/// `side` is `Clientbound` or `Serverbound`. Derive `Encode` / `Decode` separately.
#[proc_macro_derive(Packet, attributes(lodeframe, packet))]
pub fn derive_packet(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    packet::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// MiniMessage that is checked while compiling: `text!("<red>Hello {name}")` is a `Component`.
///
/// The text is read by the same parser as `lodeframe_text::mini::parse`, so a mistake in a tag is
/// a compile error that says where in the text it is, and what comes out is what the parser
/// gives at run time.
///
/// `{name}` puts in the variable `name` (or `name = expr` after the text), anything that is
/// `Into<Component>`: a string, a number, a `Component`. It goes in as it is, never read as
/// tags, so what a user typed is safe. `{{` and `}}` are braces. A placeholder has to be where
/// text goes: not in the arguments of a tag (`<click:run_command:/msg {name}>`), but it may be
/// in the quoted text of a hover.
///
/// ```ignore
/// let name = "Alice";
/// let line = text!("<green>{name}</green> joined, {n} online", n = 3);
/// ```
#[proc_macro]
pub fn text(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as text::Input);
    text::expand(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Reads `#[lodeframe(crate = path)]`; defaults to `::lodeframe::protocol`.
fn crate_path(input: &DeriveInput) -> syn::Result<Path> {
    let mut path = None;
    for attr in input
        .attrs
        .iter()
        .filter(|a| a.path().is_ident("lodeframe"))
    {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("crate") {
                path = Some(meta.value()?.parse()?);
                Ok(())
            } else {
                Err(meta.error("unknown lodeframe attribute, expected `crate = <path>`"))
            }
        })?;
    }
    Ok(path.unwrap_or_else(|| parse_quote!(::lodeframe::protocol)))
}
