// SPDX-License-Identifier: Apache-2.0 OR MIT
//! From a component back to MiniMessage.

use std::fmt::{self, Write as _};

use crate::{ClickEvent, Component, Content, HoverEvent, MAX_DEPTH, Style};

/// Writes a component as MiniMessage, so that reading it gives a component that looks the same.
/// A gradient is not found again: it comes out as one colour tag per character.
///
/// # Errors
///
/// A component that has something MiniMessage here has no tag for (a score, a selector, NBT, a
/// sprite), or is nested deeper than [`MAX_DEPTH`].
pub fn serialize(component: &Component) -> Result<String, SerializeError> {
    let mut out = String::new();
    write(&mut out, component, 0)?;
    Ok(out)
}

/// Why a component cannot be written as MiniMessage.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SerializeError {
    /// It has something no tag stands for.
    Unsupported(&'static str),
    /// It is nested deeper than [`MAX_DEPTH`].
    TooDeep,
}

impl fmt::Display for SerializeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(what) => write!(f, "MiniMessage has no tag for {what}"),
            Self::TooDeep => write!(f, "nested more than {MAX_DEPTH} deep"),
        }
    }
}

impl std::error::Error for SerializeError {}

fn write(out: &mut String, c: &Component, depth: usize) -> Result<(), SerializeError> {
    if depth > MAX_DEPTH {
        return Err(SerializeError::TooDeep);
    }
    let closers = open(out, &c.style, depth)?;
    match &c.content {
        Content::Text(text) => escape(out, text),
        Content::Keybind(key) => {
            let _ = write!(out, "<key:{}>", quote(key));
        }
        Content::Translatable {
            key,
            fallback,
            args,
        } => {
            match fallback {
                Some(fallback) => {
                    let _ = write!(out, "<lang_or:{}:{}", quote(key), quote(fallback));
                }
                None => {
                    let _ = write!(out, "<lang:{}", quote(key));
                }
            }
            for arg in args {
                let mut inner = String::new();
                write(&mut inner, arg, depth + 1)?;
                let _ = write!(out, ":{}", quote(&inner));
            }
            out.push('>');
        }
        Content::Score { .. } => return Err(SerializeError::Unsupported("a score")),
        Content::Selector { .. } => return Err(SerializeError::Unsupported("a selector")),
        Content::Nbt { .. } => return Err(SerializeError::Unsupported("NBT")),
        Content::Sprite { .. } => return Err(SerializeError::Unsupported("a sprite")),
    }
    for child in &c.children {
        write(out, child, depth + 1)?;
    }
    for name in closers.iter().rev() {
        let _ = write!(out, "</{name}>");
    }
    Ok(())
}

/// Writes the opening tags of `style` and returns the names that close them.
fn open(out: &mut String, style: &Style, depth: usize) -> Result<Vec<String>, SerializeError> {
    let mut closers = Vec::new();
    if let Some(color) = style.color {
        let _ = write!(out, "<{color}>");
        closers.push(color.to_string());
    }
    if let Some(argb) = style.shadow_color {
        if argb == 0 {
            out.push_str("<!shadow>");
        } else {
            let _ = write!(out, "<shadow:#{:06x}{:02x}>", argb & 0xff_ffff, argb >> 24);
        }
        closers.push("shadow".into());
    }
    for (name, value) in [
        ("bold", style.bold),
        ("italic", style.italic),
        ("underlined", style.underlined),
        ("strikethrough", style.strikethrough),
        ("obfuscated", style.obfuscated),
    ] {
        if let Some(on) = value {
            let _ = write!(out, "<{}{name}>", if on { "" } else { "!" });
            closers.push(name.into());
        }
    }
    if let Some(insertion) = &style.insertion {
        let _ = write!(out, "<insert:{}>", quote(insertion));
        closers.push("insert".into());
    }
    if let Some(font) = &style.font {
        let _ = write!(out, "<font:{}>", quote(font));
        closers.push("font".into());
    }
    if let Some(click) = &style.click {
        let (action, value) = match click {
            ClickEvent::OpenUrl(v) => ("open_url", v.clone()),
            ClickEvent::RunCommand(v) => ("run_command", v.clone()),
            ClickEvent::SuggestCommand(v) => ("suggest_command", v.clone()),
            ClickEvent::ChangePage(page) => ("change_page", page.to_string()),
            ClickEvent::CopyToClipboard(v) => ("copy_to_clipboard", v.clone()),
        };
        let _ = write!(out, "<click:{action}:{}>", quote(&value));
        closers.push("click".into());
    }
    if let Some(hover) = &style.hover {
        match &**hover {
            HoverEvent::ShowText(text) => {
                let mut inner = String::new();
                write(&mut inner, text, depth + 1)?;
                let _ = write!(out, "<hover:show_text:{}>", quote(&inner));
            }
            HoverEvent::ShowItem { id, count } => {
                if *count < 1 {
                    return Err(SerializeError::Unsupported("an item count below 1"));
                }
                let _ = write!(out, "<hover:show_item:{}", quote(id));
                if *count != 1 {
                    let _ = write!(out, ":{count}");
                }
                out.push('>');
            }
            HoverEvent::ShowEntity {
                entity_type,
                uuid,
                name,
            } => {
                let hex = format!("{uuid:032x}");
                let _ = write!(
                    out,
                    "<hover:show_entity:{}:{}-{}-{}-{}-{}",
                    quote(entity_type),
                    &hex[..8],
                    &hex[8..12],
                    &hex[12..16],
                    &hex[16..20],
                    &hex[20..]
                );
                if let Some(name) = name {
                    let mut inner = String::new();
                    write(&mut inner, name, depth + 1)?;
                    let _ = write!(out, ":{}", quote(&inner));
                }
                out.push('>');
            }
        }
        closers.push("hover".into());
    }
    Ok(closers)
}

/// Text that is not read as tags.
fn escape(out: &mut String, text: &str) {
    for c in text.chars() {
        if matches!(c, '\\' | '<') {
            out.push('\\');
        }
        out.push(c);
    }
}

/// An argument in single quotes.
fn quote(arg: &str) -> String {
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('\'');
    for c in arg.chars() {
        if matches!(c, '\\' | '\'') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('\'');
    out
}
