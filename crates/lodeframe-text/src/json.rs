// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Components as JSON, the way the game's codec writes and reads them.

use std::fmt;

use serde_json::{Map, Value};

use crate::{ClickEvent, Color, Component, Content, HoverEvent, NbtMode, NbtSource, Style};

/// How many components may be inside one another when one is read. The game has no limit of its
/// own that is lower than the stack's; this keeps reading something a stranger sent from running
/// the stack out. It is well inside what `serde_json` takes (128 levels, and a component inside
/// another is two of them), so that this is the limit that is met.
pub const MAX_DEPTH: usize = 32;

/// Why a component could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParseError {
    /// The text is not JSON.
    Json(String),
    /// The JSON is not a component, for this reason.
    Invalid(&'static str),
    /// A component of a kind that is not supported yet.
    Unsupported(&'static str),
    /// More than [`MAX_DEPTH`] components inside one another.
    TooDeep,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(e) => write!(f, "not JSON: {e}"),
            Self::Invalid(why) => write!(f, "not a component: {why}"),
            Self::Unsupported(what) => write!(f, "not supported: {what}"),
            Self::TooDeep => write!(f, "components nested more than {MAX_DEPTH} deep"),
        }
    }
}

impl std::error::Error for ParseError {}

type Result<T> = std::result::Result<T, ParseError>;

// writing

impl Component {
    /// The component as JSON text, which is how a few packets (a refusal while logging in) carry
    /// it: `{"text":"..","color":"red","bold":true}`, with only what is set.
    pub fn to_json(&self) -> String {
        let mut json = String::new();
        write_component(&mut json, self);
        json
    }
}

/// An object being written: puts the commas where they go.
struct Object<'a> {
    out: &'a mut String,
    first: bool,
}

impl<'a> Object<'a> {
    fn new(out: &'a mut String) -> Self {
        out.push('{');
        Self { out, first: true }
    }

    fn key(&mut self, key: &str) -> &mut String {
        if !self.first {
            self.out.push(',');
        }
        self.first = false;
        push_string(self.out, key);
        self.out.push(':');
        self.out
    }

    fn string(&mut self, key: &str, value: &str) {
        let out = self.key(key);
        push_string(out, value);
    }

    fn raw(&mut self, key: &str, value: impl fmt::Display) {
        let out = self.key(key);
        out.push_str(&value.to_string());
    }

    fn component(&mut self, key: &str, value: &Component) {
        let out = self.key(key);
        write_component(out, value);
    }

    fn components(&mut self, key: &str, values: &[Component]) {
        let out = self.key(key);
        out.push('[');
        for (i, value) in values.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            write_component(out, value);
        }
        out.push(']');
    }

    fn finish(self) {
        self.out.push('}');
    }
}

fn write_component(out: &mut String, c: &Component) {
    let mut o = Object::new(out);
    match &c.content {
        Content::Text(text) => o.string("text", text),
        Content::Translatable {
            key,
            fallback,
            args,
        } => {
            o.string("translate", key);
            if let Some(fallback) = fallback {
                o.string("fallback", fallback);
            }
            if !args.is_empty() {
                o.components("with", args);
            }
        }
        Content::Score { name, objective } => {
            let out = o.key("score");
            let mut score = Object::new(out);
            score.string("name", name);
            score.string("objective", objective);
            score.finish();
        }
        Content::Selector {
            selector,
            separator,
        } => {
            o.string("selector", selector);
            if let Some(separator) = separator {
                o.component("separator", separator);
            }
        }
        Content::Keybind(key) => o.string("keybind", key),
        Content::Nbt {
            path,
            source,
            mode,
            separator,
        } => {
            o.string("nbt", path);
            match source {
                NbtSource::Entity(s) => o.string("entity", s),
                NbtSource::Block(s) => o.string("block", s),
                NbtSource::Storage(s) => o.string("storage", s),
            }
            match mode {
                NbtMode::Default => {}
                NbtMode::Interpret => o.raw("interpret", true),
                NbtMode::Plain => o.raw("plain", true),
            }
            if let Some(separator) = separator {
                o.component("separator", separator);
            }
        }
        Content::Sprite {
            atlas,
            sprite,
            fallback,
        } => {
            o.string("sprite", sprite);
            if let Some(atlas) = atlas {
                o.string("atlas", atlas);
            }
            if let Some(fallback) = fallback {
                o.component("fallback", fallback);
            }
        }
    }
    write_style(&mut o, &c.style);
    if !c.children.is_empty() {
        o.components("extra", &c.children);
    }
    o.finish();
}

fn write_style(o: &mut Object<'_>, s: &Style) {
    if let Some(color) = s.color {
        o.string("color", &color.to_string());
    }
    if let Some(argb) = s.shadow_color {
        // the game writes the ARGB as a signed int
        o.raw("shadow_color", argb as i32);
    }
    for (key, value) in [
        ("bold", s.bold),
        ("italic", s.italic),
        ("underlined", s.underlined),
        ("strikethrough", s.strikethrough),
        ("obfuscated", s.obfuscated),
    ] {
        if let Some(value) = value {
            o.raw(key, value);
        }
    }
    if let Some(insertion) = &s.insertion {
        o.string("insertion", insertion);
    }
    if let Some(font) = &s.font {
        o.string("font", font);
    }
    if let Some(click) = &s.click {
        let out = o.key("click_event");
        let mut e = Object::new(out);
        match click {
            ClickEvent::OpenUrl(url) => {
                e.string("action", "open_url");
                e.string("url", url);
            }
            ClickEvent::RunCommand(command) => {
                e.string("action", "run_command");
                e.string("command", command);
            }
            ClickEvent::SuggestCommand(command) => {
                e.string("action", "suggest_command");
                e.string("command", command);
            }
            ClickEvent::ChangePage(page) => {
                e.string("action", "change_page");
                e.raw("page", page);
            }
            ClickEvent::CopyToClipboard(value) => {
                e.string("action", "copy_to_clipboard");
                e.string("value", value);
            }
        }
        e.finish();
    }
    if let Some(hover) = &s.hover {
        let out = o.key("hover_event");
        let mut e = Object::new(out);
        match &**hover {
            HoverEvent::ShowText(text) => {
                e.string("action", "show_text");
                e.component("value", text);
            }
            HoverEvent::ShowItem { id, count } => {
                e.string("action", "show_item");
                e.string("id", id);
                e.raw("count", count);
            }
            HoverEvent::ShowEntity {
                entity_type,
                uuid,
                name,
            } => {
                e.string("action", "show_entity");
                e.string("id", entity_type);
                let [a, b, c, d] = uuid_ints(*uuid);
                e.raw("uuid", format!("[{a},{b},{c},{d}]"));
                if let Some(name) = name {
                    e.component("name", name);
                }
            }
        }
        e.finish();
    }
}

/// A UUID as the four ints the game writes it as, most significant first.
pub fn uuid_ints(uuid: u128) -> [i32; 4] {
    [
        (uuid >> 96) as i32,
        (uuid >> 64) as i32,
        (uuid >> 32) as i32,
        uuid as i32,
    ]
}

/// Appends `s` as a JSON string: quoted, with `"`, `\` and control characters escaped.
fn push_string(json: &mut String, s: &str) {
    json.push('"');
    for c in s.chars() {
        match c {
            '"' => json.push_str("\\\""),
            '\\' => json.push_str("\\\\"),
            '\n' => json.push_str("\\n"),
            '\r' => json.push_str("\\r"),
            '\t' => json.push_str("\\t"),
            c if c < ' ' => json.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => json.push(c),
        }
    }
    json.push('"');
}

// reading

impl Component {
    /// Reads a component from JSON text.
    ///
    /// A string is plain text, an array is its first element with the rest as its children, and
    /// an object is the component with its `extra` as children. Anything else, such as a bare
    /// number, is not a component.
    pub fn from_json(json: &str) -> Result<Self> {
        let value: Value =
            serde_json::from_str(json).map_err(|e| ParseError::Json(e.to_string()))?;
        Self::from_value(&value)
    }

    /// Reads a component from a JSON value, like [`from_json`](Self::from_json). Flags may be
    /// `true`/`false` or `1`/`0`, which is how they arrive from NBT.
    pub fn from_value(value: &Value) -> Result<Self> {
        parse(value, 0)
    }
}

fn parse(value: &Value, depth: usize) -> Result<Component> {
    if depth > MAX_DEPTH {
        return Err(ParseError::TooDeep);
    }
    match value {
        Value::String(text) => Ok(Component::text(text.as_str())),
        Value::Array(items) => {
            let (first, rest) = items
                .split_first()
                .ok_or(ParseError::Invalid("an empty array"))?;
            let mut base = parse(first, depth + 1)?;
            for item in rest {
                base.children.push(parse(item, depth + 1)?);
            }
            Ok(base)
        }
        Value::Object(map) => parse_object(map, depth),
        _ => Err(ParseError::Invalid(
            "a component is a string, an array or an object",
        )),
    }
}

fn parse_object(map: &Map<String, Value>, depth: usize) -> Result<Component> {
    let content = parse_content(map, depth)?;
    let mut children = Vec::new();
    match map.get("extra") {
        None | Some(Value::Null) => {}
        Some(Value::Array(items)) => {
            for item in items {
                children.push(parse(item, depth + 1)?);
            }
        }
        Some(_) => return Err(ParseError::Invalid("extra is not an array")),
    }
    Ok(Component {
        content,
        style: parse_style(map, depth)?,
        children,
    })
}

fn parse_content(map: &Map<String, Value>, depth: usize) -> Result<Content> {
    let sub = |key: &str| -> Result<Option<Box<Component>>> {
        match map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => Ok(Some(Box::new(parse(v, depth + 1)?))),
        }
    };
    if let Some(text) = map.get("text") {
        return Ok(Content::Text(
            string(text, "text is not a string")?.to_owned(),
        ));
    }
    if let Some(key) = map.get("translate") {
        let args = match map.get("with") {
            None | Some(Value::Null) => Vec::new(),
            // a number or a flag is shown as the text it is written as
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| match item {
                    Value::Number(n) => Ok(Component::text(n.to_string())),
                    Value::Bool(b) => Ok(Component::text(b.to_string())),
                    item => parse(item, depth + 1),
                })
                .collect::<Result<_>>()?,
            Some(_) => return Err(ParseError::Invalid("with is not an array")),
        };
        return Ok(Content::Translatable {
            key: string(key, "translate is not a string")?.to_owned(),
            fallback: opt_string(map, "fallback")?,
            args,
        });
    }
    if let Some(score) = map.get("score") {
        let Value::Object(score) = score else {
            return Err(ParseError::Invalid("score is not an object"));
        };
        let field = |key: &str| {
            score
                .get(key)
                .ok_or(ParseError::Invalid("a score needs a name and an objective"))
                .and_then(|v| string(v, "a score's name and objective are strings"))
                .map(str::to_owned)
        };
        return Ok(Content::Score {
            name: field("name")?,
            objective: field("objective")?,
        });
    }
    if let Some(selector) = map.get("selector") {
        return Ok(Content::Selector {
            selector: string(selector, "selector is not a string")?.to_owned(),
            separator: sub("separator")?,
        });
    }
    if let Some(key) = map.get("keybind") {
        return Ok(Content::Keybind(
            string(key, "keybind is not a string")?.to_owned(),
        ));
    }
    if let Some(path) = map.get("nbt") {
        let sources = [("entity", 0), ("block", 1), ("storage", 2)]
            .into_iter()
            .filter_map(|(key, kind)| map.get(key).map(|v| (kind, v)))
            .collect::<Vec<_>>();
        let [(kind, source)] = sources[..] else {
            return Err(ParseError::Invalid(
                "an NBT component has exactly one of entity, block and storage",
            ));
        };
        let source = string(source, "the NBT source is not a string")?.to_owned();
        return Ok(Content::Nbt {
            path: string(path, "nbt is not a string")?.to_owned(),
            source: match kind {
                0 => NbtSource::Entity(source),
                1 => NbtSource::Block(source),
                _ => NbtSource::Storage(source),
            },
            mode: match (
                flag(map, "interpret")?.unwrap_or(false),
                flag(map, "plain")?.unwrap_or(false),
            ) {
                (false, false) => NbtMode::Default,
                (true, false) => NbtMode::Interpret,
                (false, true) => NbtMode::Plain,
                (true, true) => {
                    return Err(ParseError::Invalid("interpret and plain are exclusive"));
                }
            },
            separator: sub("separator")?,
        });
    }
    if let Some(sprite) = map.get("sprite") {
        return Ok(Content::Sprite {
            atlas: opt_string(map, "atlas")?,
            sprite: string(sprite, "sprite is not a string")?.to_owned(),
            fallback: sub("fallback")?,
        });
    }
    if map.contains_key("player") {
        return Err(ParseError::Unsupported("a player's head as a sprite"));
    }
    Err(ParseError::Invalid(
        "no text, translate, score, selector, keybind, nbt or sprite",
    ))
}

fn parse_style(map: &Map<String, Value>, depth: usize) -> Result<Style> {
    let mut style = Style::default();
    if let Some(color) = opt_string(map, "color")? {
        style.color = Some(Color::parse(&color).ok_or(ParseError::Invalid("not a colour"))?);
    }
    match map.get("shadow_color") {
        None | Some(Value::Null) => {}
        Some(Value::Number(n)) => {
            let n = n
                .as_i64()
                .ok_or(ParseError::Invalid("shadow_color is not an integer"))?;
            // the game writes a signed int, and also reads the same bits as unsigned
            style.shadow_color = Some(
                u32::try_from(n)
                    .or_else(|_| i32::try_from(n).map(|n| n as u32))
                    .map_err(|_| ParseError::Invalid("shadow_color is out of range"))?,
            );
        }
        Some(Value::Array(parts)) => {
            let [r, g, b, a] = &parts[..] else {
                return Err(ParseError::Invalid("shadow_color is four numbers"));
            };
            let byte = |v: &Value| {
                v.as_f64()
                    .filter(|f| (0.0..=1.0).contains(f))
                    .map(|f| (f * 255.0).round() as u32)
                    .ok_or(ParseError::Invalid(
                        "shadow_color parts are numbers from 0 to 1",
                    ))
            };
            style.shadow_color = Some(byte(a)? << 24 | byte(r)? << 16 | byte(g)? << 8 | byte(b)?);
        }
        Some(_) => {
            return Err(ParseError::Invalid(
                "shadow_color is a number or four numbers",
            ));
        }
    }
    style.bold = flag(map, "bold")?;
    style.italic = flag(map, "italic")?;
    style.underlined = flag(map, "underlined")?;
    style.strikethrough = flag(map, "strikethrough")?;
    style.obfuscated = flag(map, "obfuscated")?;
    style.insertion = opt_string(map, "insertion")?;
    style.font = opt_string(map, "font")?;
    match map.get("click_event") {
        None | Some(Value::Null) => {}
        Some(Value::Object(e)) => style.click = Some(parse_click(e)?),
        Some(_) => return Err(ParseError::Invalid("click_event is not an object")),
    }
    match map.get("hover_event") {
        None | Some(Value::Null) => {}
        Some(Value::Object(e)) => style.hover = Some(Box::new(parse_hover(e, depth)?)),
        Some(_) => return Err(ParseError::Invalid("hover_event is not an object")),
    }
    Ok(style)
}

fn parse_click(e: &Map<String, Value>) -> Result<ClickEvent> {
    let text = |key: &str| {
        e.get(key)
            .ok_or(ParseError::Invalid("a click event is missing a field"))
            .and_then(|v| string(v, "a click event's field is not a string"))
            .map(str::to_owned)
    };
    let action = e
        .get("action")
        .and_then(Value::as_str)
        .ok_or(ParseError::Invalid("a click event has no action"))?;
    Ok(match action {
        "open_url" => ClickEvent::OpenUrl(text("url")?),
        "run_command" => ClickEvent::RunCommand(text("command")?),
        "suggest_command" => ClickEvent::SuggestCommand(text("command")?),
        "copy_to_clipboard" => ClickEvent::CopyToClipboard(text("value")?),
        "change_page" => {
            let page = int(e.get("page"), "change_page needs a page")?;
            if page < 1 {
                return Err(ParseError::Invalid("a page counts from 1"));
            }
            ClickEvent::ChangePage(page)
        }
        "open_file" | "show_dialog" | "custom" => {
            return Err(ParseError::Unsupported("this click action"));
        }
        _ => return Err(ParseError::Invalid("unknown click action")),
    })
}

fn parse_hover(e: &Map<String, Value>, depth: usize) -> Result<HoverEvent> {
    let action = e
        .get("action")
        .and_then(Value::as_str)
        .ok_or(ParseError::Invalid("a hover event has no action"))?;
    Ok(match action {
        "show_text" => {
            let value = e
                .get("value")
                .ok_or(ParseError::Invalid("show_text needs a value"))?;
            HoverEvent::ShowText(parse(value, depth + 1)?)
        }
        "show_item" => HoverEvent::ShowItem {
            id: e
                .get("id")
                .ok_or(ParseError::Invalid("show_item needs an id"))
                .and_then(|v| string(v, "the item id is not a string"))?
                .to_owned(),
            count: match e.get("count") {
                None | Some(Value::Null) => 1,
                count => int(count, "count is not an integer")?,
            },
        },
        "show_entity" => HoverEvent::ShowEntity {
            entity_type: e
                .get("id")
                .ok_or(ParseError::Invalid("show_entity needs an id"))
                .and_then(|v| string(v, "the entity type is not a string"))?
                .to_owned(),
            uuid: parse_uuid(
                e.get("uuid")
                    .ok_or(ParseError::Invalid("show_entity needs a uuid"))?,
            )?,
            name: match e.get("name") {
                None | Some(Value::Null) => None,
                Some(v) => Some(parse(v, depth + 1)?),
            },
        },
        _ => return Err(ParseError::Invalid("unknown hover action")),
    })
}

/// A UUID as four ints, or as the text with hyphens.
fn parse_uuid(value: &Value) -> Result<u128> {
    let bad = ParseError::Invalid("not a UUID");
    match value {
        Value::Array(parts) => {
            let [a, b, c, d] = &parts[..] else {
                return Err(bad);
            };
            let part = |v: &Value| {
                v.as_i64()
                    .and_then(|n| i32::try_from(n).ok())
                    .map(|n| u128::from(n as u32))
            };
            match (part(a), part(b), part(c), part(d)) {
                (Some(a), Some(b), Some(c), Some(d)) => Ok(a << 96 | b << 64 | c << 32 | d),
                _ => Err(bad),
            }
        }
        Value::String(text) => {
            let hex: String = text.chars().filter(|c| *c != '-').collect();
            let hyphens_ok =
                text.len() == 36 && [8, 13, 18, 23].iter().all(|&i| text.as_bytes()[i] == b'-');
            if hex.len() != 32 || !hyphens_ok || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(bad);
            }
            u128::from_str_radix(&hex, 16).map_err(|_| bad)
        }
        _ => Err(bad),
    }
}

fn string<'a>(value: &'a Value, why: &'static str) -> Result<&'a str> {
    value.as_str().ok_or(ParseError::Invalid(why))
}

fn opt_string(map: &Map<String, Value>, key: &'static str) -> Result<Option<String>> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(ParseError::Invalid("a field that is text is not")),
    }
}

fn int(value: Option<&Value>, why: &'static str) -> Result<i32> {
    value
        .and_then(Value::as_i64)
        .and_then(|n| i32::try_from(n).ok())
        .ok_or(ParseError::Invalid(why))
}

/// A flag: `true`/`false`, or `1`/`0` as NBT carries one.
fn flag(map: &Map<String, Value>, key: &'static str) -> Result<Option<bool>> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(Value::Number(n)) if n.as_i64().is_some_and(|n| n == 0 || n == 1) => {
            Ok(Some(n.as_i64() == Some(1)))
        }
        Some(_) => Err(ParseError::Invalid("a flag is not true or false")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One of every kind, with all of the style set somewhere.
    fn everything() -> Vec<Component> {
        let styled = Component::text("hi")
            .color(Color::Rgb(1, 2, 3))
            .shadow_color(0x8011_2233)
            .bold()
            .decorate(crate::Decoration::Italic, false)
            .underlined()
            .strikethrough()
            .obfuscated()
            .insertion("ins")
            .font("minecraft:uniform")
            .click_open_url("https://example.com")
            .hover_text(Component::text("tip").bold());
        vec![
            Component::text("plain"),
            Component::empty(),
            styled,
            Component::text("a")
                .append("b")
                .append(Component::text("c").bold()),
            Component::translatable("chat.type.text")
                .arg("a")
                .arg(Component::text("b").bold())
                .fallback("fb"),
            Component::keybind("key.jump"),
            Component::score("@p", "obj"),
            Component::selector("@a").separator(", "),
            Component::nbt_entity("Health", "@s")
                .interpret()
                .plain()
                .separator("-"),
            Component::nbt_block("a.b", "~ ~ ~"),
            Component::nbt_storage("a", "minecraft:x"),
            Component::sprite("block/stone"),
            Component::sprite("item/apple")
                .atlas("minecraft:items")
                .sprite_fallback("[apple]"),
            Component::text("x").click_run_command("/say hi"),
            Component::text("x").click_suggest_command("/say"),
            Component::text("x").click(ClickEvent::ChangePage(3)),
            Component::text("x").click(ClickEvent::CopyToClipboard("v".into())),
            Component::text("x").hover(HoverEvent::ShowItem {
                id: "minecraft:stone".into(),
                count: 2,
            }),
            Component::text("x").hover(HoverEvent::ShowEntity {
                entity_type: "minecraft:pig".into(),
                uuid: 0x0000_0000_0000_0001_0000_0000_0000_0002,
                name: Some(Component::text("n")),
            }),
            Component::text("quote \" slash \\ newline \n tab \t nul \0 日本語 §c"),
        ]
    }

    fn json(c: &Component) -> Value {
        serde_json::from_str(&c.to_json()).expect("to_json writes JSON")
    }

    #[test]
    fn writing_then_reading_gives_the_same_component() {
        for c in everything() {
            assert_eq!(
                Component::from_json(&c.to_json()),
                Ok(c.clone()),
                "{}",
                c.to_json()
            );
        }
    }

    #[test]
    fn to_json_writes_the_text_and_only_what_is_set_in_a_steady_order() {
        assert_eq!(Component::text("hi").to_json(), r#"{"text":"hi"}"#);
        assert_eq!(
            Component::text("hi")
                .color(Color::Red)
                .bold()
                .underlined()
                .to_json(),
            r#"{"text":"hi","color":"red","bold":true,"underlined":true}"#
        );
        assert_eq!(
            Component::text("x").color(Color::Rgb(1, 2, 3)).to_json(),
            r##"{"text":"x","color":"#010203"}"##
        );
        assert_eq!(
            Component::text("a").append("b").to_json(),
            r#"{"text":"a","extra":[{"text":"b"}]}"#
        );
        assert_eq!(
            Component::translatable("k")
                .arg("a")
                .fallback("f")
                .to_json(),
            r#"{"translate":"k","fallback":"f","with":[{"text":"a"}]}"#
        );
        assert_eq!(
            Component::score("@p", "o").to_json(),
            r#"{"score":{"name":"@p","objective":"o"}}"#
        );
        assert_eq!(
            Component::text("x")
                .click_open_url("u")
                .hover_text("t")
                .to_json(),
            r#"{"text":"x","click_event":{"action":"open_url","url":"u"},"hover_event":{"action":"show_text","value":{"text":"t"}}}"#
        );
    }

    #[test]
    fn to_json_escapes_what_json_needs() {
        assert_eq!(
            Component::text("a\"b\\c\nd\te\r\u{1}\u{1f}").to_json(),
            r#"{"text":"a\"b\\c\nd\te\r\u0001\u001f"}"#
        );
        // everything else stays as it is
        assert_eq!(
            Component::text("日本語 §c").to_json(),
            r#"{"text":"日本語 §c"}"#
        );
    }

    #[test]
    fn the_shadow_is_written_as_the_signed_int_the_game_writes() {
        let c = Component::text("x").shadow_color(0xffff_ffff);
        assert_eq!(json(&c)["shadow_color"], -1);
        assert_eq!(
            Component::from_json(r#"{"text":"x","shadow_color":4294967295}"#)
                .unwrap()
                .style
                .shadow_color,
            Some(0xffff_ffff)
        );
        assert_eq!(
            Component::from_json(r#"{"text":"x","shadow_color":-1}"#)
                .unwrap()
                .style
                .shadow_color,
            Some(0xffff_ffff)
        );
        // four numbers from 0 to 1: red, green, blue, alpha
        assert_eq!(
            Component::from_json(r#"{"text":"x","shadow_color":[1.0,0.0,0.0,1.0]}"#)
                .unwrap()
                .style
                .shadow_color,
            Some(0xffff_0000)
        );
    }

    #[test]
    fn a_string_is_text_and_an_array_is_its_first_with_the_rest_as_children() {
        assert_eq!(Component::from_json(r#""hi""#), Ok(Component::text("hi")));
        assert_eq!(
            Component::from_json(r#"["a",{"text":"b","bold":true},"c"]"#),
            Ok(Component::text("a")
                .append(Component::text("b").bold())
                .append("c"))
        );
        assert_eq!(
            Component::from_json(r#"["a",["b","c"]]"#),
            Ok(Component::text("a").append(Component::text("b").append("c")))
        );
        assert_eq!(Component::from_json(r#"["a"]"#), Ok(Component::text("a")));
    }

    #[test]
    fn what_is_not_a_component_is_refused() {
        for bad in [
            "5",
            "true",
            "null",
            "[]",
            "{}",
            r#"{"bold":true}"#,
            r#"{"text":5}"#,
            r#"{"text":"a","extra":"b"}"#,
            r#"{"text":"a","extra":[5]}"#,
            r#"{"text":"a","color":"RED"}"#,
            r#"{"text":"a","color":"nope"}"#,
            r##"{"text":"a","color":"#fff"}"##,
            r#"{"text":"a","bold":"true"}"#,
            r#"{"text":"a","bold":2}"#,
            r#"{"text":"a","shadow_color":[1.0,0.0,0.0]}"#,
            r#"{"text":"a","shadow_color":[2.0,0.0,0.0,1.0]}"#,
            r#"{"text":"a","shadow_color":9999999999}"#,
            r#"{"text":"a","font":{"type":"atlas"}}"#,
            r#"{"text":"a","click_event":{"action":"open_url"}}"#,
            r#"{"text":"a","click_event":{"action":"nothing"}}"#,
            r#"{"text":"a","click_event":{"action":"change_page","page":0}}"#,
            r#"{"text":"a","hover_event":{"action":"show_text"}}"#,
            r#"{"text":"a","hover_event":{"action":"show_entity","id":"x","uuid":"not a uuid"}}"#,
            r#"{"text":"a","hover_event":{"action":"show_entity","id":"x","uuid":[1,2,3]}}"#,
            r#"{"nbt":"a"}"#,
            r#"{"nbt":"a","entity":"@s","interpret":true,"plain":true}"#,
            r#"{"nbt":"a","entity":"@s","block":"~ ~ ~"}"#,
            r#"{"score":{"name":"@p"}}"#,
            r#"{"translate":"k","with":"a"}"#,
            "not json",
        ] {
            assert!(Component::from_json(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn kinds_that_are_not_supported_say_so() {
        for unsupported in [
            r#"{"text":"a","click_event":{"action":"show_dialog","dialog":"x"}}"#,
            r#"{"text":"a","click_event":{"action":"custom","id":"a:b"}}"#,
            r#"{"text":"a","click_event":{"action":"open_file","path":"x"}}"#,
            r#"{"player":{"name":"Steve"}}"#,
        ] {
            assert!(
                matches!(
                    Component::from_json(unsupported),
                    Err(ParseError::Unsupported(_))
                ),
                "{unsupported}"
            );
        }
    }

    #[test]
    fn the_text_wins_when_there_is_more_than_one_content() {
        assert_eq!(
            Component::from_json(r#"{"text":"a","translate":"k"}"#),
            Ok(Component::text("a"))
        );
    }

    #[test]
    fn translation_arguments_may_be_numbers_and_flags() {
        let c =
            Component::from_json(r#"{"translate":"k","with":[1,true,"s",{"text":"t"}]}"#).unwrap();
        assert_eq!(
            c,
            Component::translatable("k")
                .arg("1")
                .arg("true")
                .arg("s")
                .arg("t")
        );
    }

    #[test]
    fn flags_may_be_numbers_as_nbt_carries_them() {
        let c =
            Component::from_value(&serde_json::json!({"text":"a","bold":1,"italic":0})).unwrap();
        assert_eq!((c.style.bold, c.style.italic), (Some(true), Some(false)));
    }

    #[test]
    fn an_entity_uuid_is_four_ints_or_text() {
        let want = 0x0000_0000_0000_0001_0000_0000_0000_0002;
        for uuid in [r#"[0,1,0,2]"#, r#""00000000-0000-0001-0000-000000000002""#] {
            let json = format!(
                r#"{{"text":"a","hover_event":{{"action":"show_entity","id":"x","uuid":{uuid}}}}}"#
            );
            let c = Component::from_json(&json).unwrap();
            let Some(hover) = c.style.hover else { panic!() };
            assert!(matches!(*hover, HoverEvent::ShowEntity { uuid, .. } if uuid == want));
        }
        assert_eq!(uuid_ints(want), [0, 1, 0, 2]);
        assert_eq!(uuid_ints(u128::MAX), [-1; 4]);
    }

    #[test]
    fn a_missing_count_is_one() {
        let c = Component::from_json(
            r#"{"text":"a","hover_event":{"action":"show_item","id":"minecraft:stone"}}"#,
        )
        .unwrap();
        assert_eq!(
            c.style.hover.as_deref(),
            Some(&HoverEvent::ShowItem {
                id: "minecraft:stone".into(),
                count: 1
            })
        );
    }

    /// `depth` components inside one another.
    fn nested(depth: usize) -> String {
        let mut json = String::from(r#"{"text":"x"}"#);
        for _ in 0..depth {
            json = format!(r#"{{"text":"x","extra":[{json}]}}"#);
        }
        json
    }

    #[test]
    fn nesting_has_a_limit_that_the_stack_survives() {
        // run in a thread with a small stack: the limit must hold before the stack runs out
        let outcome = std::thread::Builder::new()
            .stack_size(512 * 1024)
            .spawn(|| {
                // a value nested far past the limit, built without the parser's own limit
                let mut deep = serde_json::json!({"text": "x"});
                for _ in 0..200 {
                    deep = serde_json::json!({"text": "x", "extra": [deep]});
                }
                (
                    Component::from_json(&nested(MAX_DEPTH)).is_ok(),
                    Component::from_json(&nested(MAX_DEPTH + 1)),
                    Component::from_value(&deep),
                    Component::from_json(&nested(10_000)).is_err(),
                )
            })
            .unwrap()
            .join()
            .unwrap();
        assert!(outcome.0);
        assert_eq!(outcome.1, Err(ParseError::TooDeep));
        assert_eq!(outcome.2, Err(ParseError::TooDeep));
        assert!(outcome.3);
    }
}
