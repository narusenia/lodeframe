// SPDX-License-Identifier: Apache-2.0 OR MIT
//! `text!`: MiniMessage checked while compiling, with `{name}` placeholders.

use lodeframe_text::{
    Component, Content, HoverEvent,
    mini::{self, TagResolver},
};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Expr, Ident, LitStr, Token,
    parse::{Parse, ParseStream},
};

/// `"text" [, name = expr]* [,]`.
pub struct Input {
    lit: LitStr,
    args: Vec<(Ident, Expr)>,
}

impl Parse for Input {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let lit = input
            .parse::<LitStr>()
            .map_err(|e| syn::Error::new(e.span(), "text! expects a string literal first"))?;
        let mut args = Vec::new();
        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            let name = input.parse::<Ident>()?;
            input.parse::<Token![=]>()?;
            args.push((name, input.parse::<Expr>()?));
        }
        Ok(Self { lit, args })
    }
}

pub fn expand(input: &Input) -> syn::Result<TokenStream> {
    let span = input.lit.span();
    let src = input.lit.value();
    let prepared =
        prepare(&src).map_err(|e| syn::Error::new(span, describe(&src, e.at, &e.what)))?;

    // every named argument is used, and each name is given once
    for (i, (name, _)) in input.args.iter().enumerate() {
        if input.args[..i].iter().any(|(n, _)| n == name) {
            return Err(syn::Error::new(
                name.span(),
                format!("`{name}` is given twice"),
            ));
        }
        if !prepared.names.iter().any(|n| name == n) {
            return Err(syn::Error::new(
                name.span(),
                format!("`{name}` is not used in the text: write `{{{name}}}` where it goes"),
            ));
        }
    }
    check(&src, &prepared).map_err(|message| syn::Error::new(span, message))?;

    let text = &prepared.text;
    if prepared.names.is_empty() {
        return Ok(quote! {{
            static TEXT: ::std::sync::LazyLock<::lodeframe::text::Component> =
                ::std::sync::LazyLock::new(|| ::lodeframe::text::mini::parse_lenient(#text));
            ::std::clone::Clone::clone(&*TEXT)
        }});
    }
    let tags = prepared.names.iter().enumerate().map(|(i, name)| {
        let tag = prepared.tag(i);
        let value = match input.args.iter().find(|(n, _)| n == name) {
            Some((_, expr)) => quote!(#expr),
            // an implicit capture is looked up where the macro is written, as `format!` does
            None => {
                let ident = Ident::new(name, span);
                quote!(#ident)
            }
        };
        quote!(.component(#tag, ::lodeframe::text::Component::from(#value)))
    });
    Ok(quote! {{
        let __lodeframe_text_tags = ::lodeframe::text::mini::TagResolver::new() #(#tags)*;
        ::lodeframe::text::mini::parse_lenient_with(#text, &__lodeframe_text_tags)
    }})
}

/// The text with its placeholders turned into tags, and what is needed to tell where in the
/// original an error is.
#[derive(Debug)]
struct Prepared {
    /// The MiniMessage to give the parser: `{name}` is `<tag>`, `{{` is `{`.
    text: String,
    /// The placeholder names, in the order they are first used.
    names: Vec<String>,
    /// For each name: where it is first used in the original, and how many times it is.
    uses: Vec<(usize, usize)>,
    /// What the tags of the placeholders start with.
    prefix: String,
    /// Where the first sentinel character is.
    sentinel: u32,
    /// The stretches of `text` that stand for something longer or shorter in the original:
    /// `(start in text, end in text, start in the original, end in the original)`.
    regions: Vec<(usize, usize, usize, usize)>,
}

impl Prepared {
    fn tag(&self, index: usize) -> String {
        format!("{}{index}", self.prefix)
    }

    /// Where in the original a position in `text` is.
    fn original(&self, pos: usize) -> usize {
        let mut delta = 0_isize;
        for &(text_start, text_end, orig_start, orig_end) in &self.regions {
            if pos < text_start {
                break;
            }
            if pos < text_end {
                return orig_start;
            }
            delta = orig_end as isize - text_end as isize;
        }
        (pos as isize + delta) as usize
    }
}

/// What is wrong with the text, and where in it.
struct Fault {
    at: usize,
    what: String,
}

fn fault(at: usize, what: impl Into<String>) -> Fault {
    Fault {
        at,
        what: what.into(),
    }
}

/// Cuts `{name}` out of the text.
fn prepare(src: &str) -> Result<Prepared, Fault> {
    // a tag name and a private-use character the text does not have, to stand for placeholders
    let mut prefix = String::from("lodeframe-ph-");
    let mut k = 0;
    while src.contains(&prefix) {
        k += 1;
        prefix = format!("lodeframe-ph{k}-");
    }
    let mut p = Prepared {
        text: String::new(),
        names: Vec::new(),
        uses: Vec::new(),
        prefix,
        sentinel: 0,
        regions: Vec::new(),
    };
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' if bytes.get(i + 1) == Some(&b'{') => {
                let start = p.text.len();
                p.text.push('{');
                p.regions.push((start, p.text.len(), i, i + 2));
                i += 2;
            }
            b'}' if bytes.get(i + 1) == Some(&b'}') => {
                let start = p.text.len();
                p.text.push('}');
                p.regions.push((start, p.text.len(), i, i + 2));
                i += 2;
            }
            b'}' => return Err(fault(i, "a lone `}`: write `}}` for a brace")),
            b'{' => {
                let Some(len) = src[i + 1..].find('}') else {
                    return Err(fault(
                        i,
                        "a `{` that is never closed: write `{{` for a brace",
                    ));
                };
                let name = &src[i + 1..i + 1 + len];
                if !is_name(name) {
                    return Err(fault(
                        i,
                        format!(
                            "`{{{name}}}` is not a variable name: a placeholder is `{{name}}`, \
                             and `{{{{` is a brace"
                        ),
                    ));
                }
                let index = match p.names.iter().position(|n| n == name) {
                    Some(index) => index,
                    None => {
                        p.names.push(name.to_owned());
                        p.uses.push((i, 0));
                        p.names.len() - 1
                    }
                };
                p.uses[index].1 += 1;
                let start = p.text.len();
                p.text.push_str(&format!("<{}>", p.tag(index)));
                p.regions.push((start, p.text.len(), i, i + len + 2));
                i += len + 2;
            }
            _ => {
                let end = src[i..].find(['{', '}']).map_or(src.len(), |n| i + n);
                p.text.push_str(&src[i..end]);
                i = end;
            }
        }
    }
    p.sentinel = sentinel_base(src, p.names.len())
        .ok_or_else(|| fault(0, "the text uses all the private-use characters"))?;
    Ok(p)
}

fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    let plain = chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_');
    // not a keyword either
    plain && syn::parse_str::<Ident>(name).is_ok()
}

/// The first of `n` private-use characters the text does not have.
fn sentinel_base(src: &str, n: usize) -> Option<u32> {
    let n = n.max(1) as u32;
    let mut base = 0xE000;
    while base + n <= 0xF8FF {
        let free = (base..base + n).all(|c| char::from_u32(c).is_some_and(|c| !src.contains(c)));
        if free {
            return Some(base);
        }
        base += n;
    }
    None
}

/// Reads the text the way the program will, with a one-character component for each
/// placeholder, and checks that they all end up where text goes.
fn check(src: &str, p: &Prepared) -> Result<(), String> {
    let mut tags = TagResolver::new();
    for i in 0..p.names.len() {
        let mark = char::from_u32(p.sentinel + i as u32).expect("a private-use character");
        tags = tags.component(&p.tag(i), Component::text(mark.to_string()));
    }
    let read = mini::parse_with(&p.text, &tags).map_err(|e| {
        let at = p.original(e.position);
        let shown = mini::Error {
            position: at,
            kind: e.kind,
        };
        describe(src, at, &shown.to_string())
    })?;
    let mut found = vec![0; p.names.len()];
    count(&read, p.sentinel, &mut found);
    for (i, name) in p.names.iter().enumerate() {
        let (first, uses) = p.uses[i];
        if found[i] != uses {
            return Err(describe(
                src,
                first,
                &format!(
                    "`{{{name}}}` is used {uses} time(s) but only {} end up in the text: a \
                     placeholder cannot be inside the arguments of a tag (it can be in the \
                     quoted text of a hover)",
                    found[i]
                ),
            ));
        }
    }
    Ok(())
}

/// Counts the sentinel characters in the text of a component, its children, and what it shows
/// on hover.
fn count(c: &Component, base: u32, found: &mut [usize]) {
    match &c.content {
        Content::Text(text) => {
            for ch in text.chars() {
                let at = (ch as u32).wrapping_sub(base) as usize;
                if let Some(n) = found.get_mut(at) {
                    *n += 1;
                }
            }
        }
        Content::Translatable { args, .. } => args.iter().for_each(|a| count(a, base, found)),
        _ => {}
    }
    match c.style.hover.as_deref() {
        Some(HoverEvent::ShowText(text)) => count(text, base, found),
        Some(HoverEvent::ShowEntity {
            name: Some(name), ..
        }) => count(name, base, found),
        _ => {}
    }
    c.children
        .iter()
        .for_each(|child| count(child, base, found));
}

/// A message that says what is wrong and shows where in the text.
fn describe(src: &str, at: usize, what: &str) -> String {
    let mut at = at.min(src.len());
    while !src.is_char_boundary(at) {
        at -= 1;
    }
    let line_start = src[..at].rfind('\n').map_or(0, |n| n + 1);
    let line_end = src[at..].find('\n').map_or(src.len(), |n| at + n);
    let line = &src[line_start..line_end];
    let column = src[line_start..at].chars().count();
    // a long line is cut to what is around the place
    let (from, to) = (column.saturating_sub(40), column + 60);
    let shown: String = line.chars().skip(from).take(to - from).collect();
    let lead = if from > 0 { "…" } else { "" };
    let trail = if line.chars().count() > to { "…" } else { "" };
    let caret = " ".repeat(lead.chars().count() + column - from);
    let what = if what.starts_with("at byte") {
        what.to_owned()
    } else {
        format!("at byte {at}: {what}")
    };
    format!("invalid MiniMessage {what}\n  {lead}{shown}{trail}\n  {caret}^")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared(src: &str) -> Prepared {
        prepare(src).unwrap_or_else(|e| panic!("{src}: {}", e.what))
    }

    #[test]
    fn braces_and_placeholders_are_cut_out() {
        let p = prepared("a {{b}} {name} c {name} {other}");
        assert_eq!(
            p.text,
            "a {b} <lodeframe-ph-0> c <lodeframe-ph-0> <lodeframe-ph-1>"
        );
        assert_eq!(p.names, ["name", "other"]);
        assert_eq!(p.uses, [(8, 2), (24, 1)]);
        assert_eq!(prepared("plain <red>text</red>").names.len(), 0);
        assert_eq!(
            prepared("plain <red>text</red>").text,
            "plain <red>text</red>"
        );
    }

    #[test]
    fn a_bad_brace_is_an_error_at_the_brace() {
        for (src, at) in [
            ("a { b", 2),
            ("a } b", 2),
            ("x{}", 1),
            ("x{0}", 1),
            ("x{a.b}", 1),
            ("x{a:?}", 1),
            ("x{ a }", 1),
            ("x{fn}", 1),
            ("{{ok}} and }", 11),
        ] {
            let e = prepare(src).unwrap_err();
            assert_eq!(e.at, at, "{src}: {}", e.what);
        }
    }

    #[test]
    fn a_position_in_the_text_is_taken_back_to_the_original() {
        let src = "{{ab}} {name}<nope>";
        let p = prepared(src);
        let at = |tag: &str| p.original(p.text.find(tag).unwrap());
        assert_eq!(at("ab"), 2);
        assert_eq!(at("<nope>"), src.find("<nope>").unwrap());
        // inside what stands for a placeholder is the placeholder
        assert_eq!(p.original(p.text.find("<lodeframe").unwrap() + 3), 7);
        // a `{{` is two characters in the original
        assert_eq!(p.original(1), 2);
    }

    #[test]
    fn the_tag_name_and_the_marks_keep_clear_of_the_text() {
        let src = "<lodeframe-ph-0> and {x} \u{E000}";
        let p = prepared(src);
        assert!(p.prefix != "lodeframe-ph-");
        assert!(!src.contains(&p.prefix));
        assert_eq!(p.sentinel, 0xE001);
        assert_eq!(sentinel_base("", 0), Some(0xE000));
    }

    #[test]
    fn a_mistake_in_the_tags_is_shown_where_it_is() {
        let src = "<red>{name} <nope>";
        let p = prepared(src);
        let message = check(src, &p).unwrap_err();
        assert_eq!(
            message,
            "invalid MiniMessage at byte 12: unknown tag <nope>\n  <red>{name} <nope>\n              ^"
        );
        // a second line is shown on its own
        let src = "ok\n<red><nope>";
        let message = check(src, &prepared(src)).unwrap_err();
        assert_eq!(
            message,
            "invalid MiniMessage at byte 8: unknown tag <nope>\n  <red><nope>\n       ^"
        );
    }

    #[test]
    fn a_placeholder_that_does_not_end_up_in_the_text_is_refused() {
        // an argument of a tag is not text
        let src = "<click:run_command:/msg {name}>x";
        let message = check(src, &prepared(src)).unwrap_err();
        assert!(
            message.contains("`{name}` is used 1 time(s) but only 0"),
            "{message}"
        );
        assert!(message.contains("at byte 24"), "{message}");
        // one of two
        let src = "{a} <click:run_command:/msg {a}>x";
        let message = check(src, &prepared(src)).unwrap_err();
        assert!(message.contains("used 2 time(s) but only 1"), "{message}");
        // in text, inside a tag, in a hover, and in a gradient, where it is one place
        for src in [
            "{a}",
            "<red>{a}</red>",
            "<hover:show_text:'x {a}'>y",
            "<gradient:red:blue>ab{a}cd</gradient>",
            "<lang:chat.type.text:'{a}'>",
            "{a}{a}{a}",
        ] {
            check(src, &prepared(src)).unwrap_or_else(|m| panic!("{src}: {m}"));
        }
    }

    #[test]
    fn a_long_line_is_cut_around_the_place() {
        let src = format!("{}<nope>{}", "a".repeat(100), "b".repeat(100));
        let message = check(&src, &prepared(&src)).unwrap_err();
        let lines: Vec<_> = message.lines().collect();
        assert!(
            lines[1].starts_with("  …a") && lines[1].ends_with("b…"),
            "{message}"
        );
        assert!(lines[1].chars().count() < 120);
        let caret = lines[2].find('^').unwrap();
        assert_eq!(
            &lines[1].chars().skip(caret).take(6).collect::<String>(),
            "<nope>"
        );
    }

    #[test]
    fn the_input_is_a_string_then_named_arguments() {
        let parse = |tokens: TokenStream| syn::parse2::<Input>(tokens);
        let ok = parse(quote!("a {x}", x = 1 + 2, y = "z",)).unwrap();
        assert_eq!(ok.args.len(), 2);
        assert!(parse(quote!("a")).is_ok());
        assert!(parse(quote!(1)).is_err());
        assert!(parse(quote!("a", x)).is_err());
        assert!(parse(quote!("a" x = 1)).is_err());
    }
}
