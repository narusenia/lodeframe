// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A [`Component`] as NBT, the way the game's codec writes and reads it.
//!
//! Plain text with nothing else is a bare string tag; anything else is a compound whose keys are
//! what [`Component::to_json`] writes as a JSON object's.

use std::io::Write;

use lodeframe_text::{
    ClickEvent, Component, Content, HoverEvent, MAX_DEPTH, NbtMode, NbtSource, ParseError, Style,
    Value,
};

use crate::{Compound, Decode, Encode, Error, Nbt, Result};

/// A flag, which NBT stores as a byte.
fn flag(v: bool) -> Nbt {
    Nbt::Byte(i8::from(v))
}

fn string(s: &str) -> Nbt {
    Nbt::String(s.to_owned())
}

impl From<&Component> for Nbt {
    fn from(c: &Component) -> Self {
        if c.style.is_empty()
            && c.children.is_empty()
            && let Content::Text(text) = &c.content
        {
            return Self::String(text.clone());
        }
        let mut out = Compound::new();
        content(&mut out, &c.content);
        style(&mut out, &c.style);
        if !c.children.is_empty() {
            out.insert(
                "extra",
                Self::List(c.children.iter().map(Self::from).collect()),
            );
        }
        Self::Compound(out)
    }
}

fn content(out: &mut Compound, content: &Content) {
    match content {
        Content::Text(text) => {
            out.insert("text", string(text));
        }
        Content::Translatable {
            key,
            fallback,
            args,
        } => {
            out.insert("translate", string(key));
            if let Some(fallback) = fallback {
                out.insert("fallback", string(fallback));
            }
            if !args.is_empty() {
                out.insert("with", Nbt::List(args.iter().map(Nbt::from).collect()));
            }
        }
        Content::Score { name, objective } => {
            let mut score = Compound::new();
            score.insert("name", string(name));
            score.insert("objective", string(objective));
            out.insert("score", Nbt::Compound(score));
        }
        Content::Selector {
            selector,
            separator,
        } => {
            out.insert("selector", string(selector));
            if let Some(separator) = separator {
                out.insert("separator", Nbt::from(&**separator));
            }
        }
        Content::Keybind(key) => {
            out.insert("keybind", string(key));
        }
        Content::Nbt {
            path,
            source,
            mode,
            separator,
        } => {
            out.insert("nbt", string(path));
            let (key, value) = match source {
                NbtSource::Entity(s) => ("entity", s),
                NbtSource::Block(s) => ("block", s),
                NbtSource::Storage(s) => ("storage", s),
            };
            out.insert(key, string(value));
            match mode {
                NbtMode::Default => {}
                NbtMode::Interpret => {
                    out.insert("interpret", flag(true));
                }
                NbtMode::Plain => {
                    out.insert("plain", flag(true));
                }
            }
            if let Some(separator) = separator {
                out.insert("separator", Nbt::from(&**separator));
            }
        }
        Content::Sprite {
            atlas,
            sprite,
            fallback,
        } => {
            out.insert("sprite", string(sprite));
            if let Some(atlas) = atlas {
                out.insert("atlas", string(atlas));
            }
            if let Some(fallback) = fallback {
                out.insert("fallback", Nbt::from(&**fallback));
            }
        }
        // `Content` grows with the game; a kind this does not know is left out, and the tests
        // of whoever adds it say so
        _ => debug_assert!(false, "a component content this does not write"),
    }
}

fn style(out: &mut Compound, s: &Style) {
    if let Some(color) = s.color {
        out.insert("color", Nbt::String(color.to_string()));
    }
    if let Some(argb) = s.shadow_color {
        out.insert("shadow_color", Nbt::Int(argb as i32));
    }
    for (key, value) in [
        ("bold", s.bold),
        ("italic", s.italic),
        ("underlined", s.underlined),
        ("strikethrough", s.strikethrough),
        ("obfuscated", s.obfuscated),
    ] {
        if let Some(value) = value {
            out.insert(key, flag(value));
        }
    }
    if let Some(insertion) = &s.insertion {
        out.insert("insertion", string(insertion));
    }
    if let Some(font) = &s.font {
        out.insert("font", string(font));
    }
    if let Some(click) = &s.click {
        let mut e = Compound::new();
        let (action, key, value) = match click {
            ClickEvent::OpenUrl(url) => ("open_url", "url", string(url)),
            ClickEvent::RunCommand(command) => ("run_command", "command", string(command)),
            ClickEvent::SuggestCommand(command) => ("suggest_command", "command", string(command)),
            ClickEvent::ChangePage(page) => ("change_page", "page", Nbt::Int(*page)),
            ClickEvent::CopyToClipboard(value) => ("copy_to_clipboard", "value", string(value)),
            _ => {
                debug_assert!(false, "a click event this does not write");
                ("", "", Nbt::Byte(0))
            }
        };
        if !action.is_empty() {
            e.insert("action", string(action));
            e.insert(key, value);
            out.insert("click_event", Nbt::Compound(e));
        }
    }
    if let Some(hover) = &s.hover {
        let mut e = Compound::new();
        match &**hover {
            HoverEvent::ShowText(text) => {
                e.insert("action", string("show_text"));
                e.insert("value", Nbt::from(text));
            }
            HoverEvent::ShowItem { id, count } => {
                e.insert("action", string("show_item"));
                e.insert("id", string(id));
                // the game leaves out a count of one
                if *count != 1 {
                    e.insert("count", Nbt::Int(*count));
                }
            }
            HoverEvent::ShowEntity {
                entity_type,
                uuid,
                name,
            } => {
                e.insert("action", string("show_entity"));
                e.insert("id", string(entity_type));
                e.insert(
                    "uuid",
                    Nbt::IntArray(lodeframe_text::uuid_ints(*uuid).to_vec()),
                );
                if let Some(name) = name {
                    e.insert("name", Nbt::from(name));
                }
            }
            _ => debug_assert!(false, "a hover event this does not write"),
        }
        if !e.0.is_empty() {
            out.insert("hover_event", Nbt::Compound(e));
        }
    }
}

impl Encode for Component {
    fn encode(&self, w: &mut impl Write) -> Result<()> {
        Nbt::from(self).encode(w)
    }
}

/// How deep an NBT value may be to be turned into a component: a component inside another is a
/// compound and a list, so twice [`MAX_DEPTH`] with room for the hover events and the like.
const MAX_NBT_DEPTH: usize = MAX_DEPTH * 3 + 8;

/// Reads a component out of NBT: a string, a compound, or a list (its first element with the rest
/// as its children).
impl TryFrom<&Nbt> for Component {
    type Error = ParseError;

    fn try_from(nbt: &Nbt) -> std::result::Result<Self, ParseError> {
        Component::from_value(&to_value(nbt, 0)?)
    }
}

/// NBT as the JSON value the same component would be: bytes and the like become numbers, which
/// is how a flag is read from either.
fn to_value(nbt: &Nbt, depth: usize) -> std::result::Result<Value, ParseError> {
    if depth > MAX_NBT_DEPTH {
        return Err(ParseError::TooDeep);
    }
    let number = |n: i64| Value::from(n);
    Ok(match nbt {
        Nbt::Byte(n) => number(i64::from(*n)),
        Nbt::Short(n) => number(i64::from(*n)),
        Nbt::Int(n) => number(i64::from(*n)),
        Nbt::Long(n) => number(*n),
        Nbt::Float(n) => Value::from(f64::from(*n)),
        Nbt::Double(n) => Value::from(*n),
        Nbt::String(s) => Value::String(s.clone()),
        Nbt::ByteArray(v) => Value::Array(v.iter().map(|n| number(i64::from(*n))).collect()),
        Nbt::IntArray(v) => Value::Array(v.iter().map(|n| number(i64::from(*n))).collect()),
        Nbt::LongArray(v) => Value::Array(v.iter().map(|n| number(*n)).collect()),
        Nbt::List(items) => Value::Array(
            items
                .iter()
                .map(|i| to_value(i, depth + 1))
                .collect::<std::result::Result<_, _>>()?,
        ),
        Nbt::Compound(c) => {
            let mut map = lodeframe_text::Map::new();
            // the last of a repeated name stands, as with `Compound::get`
            for (key, value) in &c.0 {
                map.insert(key.clone(), to_value(value, depth + 1)?);
            }
            Value::Object(map)
        }
    })
}

impl Decode for Component {
    fn decode(r: &mut &[u8]) -> Result<Self> {
        let nbt = Nbt::decode(r)?;
        Component::try_from(&nbt).map_err(|e| {
            Error::InvalidValue(match e {
                ParseError::Invalid(why) | ParseError::Unsupported(why) => why,
                ParseError::TooDeep => "component nested too deeply",
                _ => "component malformed",
            })
        })
    }
}
