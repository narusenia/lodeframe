// SPDX-License-Identifier: Apache-2.0 OR MIT
//! What each tag means: the standard ones, and the ones a [`TagResolver`](super::TagResolver)
//! adds.

use super::{
    ErrorKind,
    parse::{self, Cx},
};
use crate::{ClickEvent, Color, Component, Decoration, HoverEvent, Style};

/// What a tag turned out to be.
pub(super) enum Resolved {
    /// Text that goes in as it is, and is coloured by a gradient around it.
    Text(String),
    /// A component that goes in as it is.
    Leaf(Component),
    /// Everything up to the closing tag is drawn this way.
    Wrap(Wrap),
    /// `<reset>`.
    Reset,
}

pub(super) enum Wrap {
    Style(Style),
    Paint(PaintSpec),
}

/// Why a tag is not understood.
pub(super) enum Fail {
    Unknown,
    Bad(&'static str),
    Nested(ErrorKind),
}

type Res<T> = Result<T, Fail>;

/// The name a tag is closed by: lower case, without the `!` of `<!bold>`, one name for the
/// aliases.
pub(super) fn canonical(name: &str) -> String {
    let name = name.trim_start_matches('!').to_ascii_lowercase();
    match name.as_str() {
        "b" => "bold",
        "i" | "em" => "italic",
        "u" => "underlined",
        "st" => "strikethrough",
        "obf" => "obfuscated",
        "c" | "colour" => "color",
        "tr" | "translate" => "lang",
        "tr_or" | "translate_or" => "lang_or",
        "br" => "newline",
        "grey" => "gray",
        "dark_grey" => "dark_gray",
        other => other,
    }
    .to_owned()
}

/// What `<name:args>` is. `stack` is how many tags are open around it.
pub(super) fn resolve(name: &str, args: &[String], cx: Cx<'_>, stack: usize) -> Res<Resolved> {
    let negated = name.starts_with('!');
    if !negated && let Some(entry) = cx.tags.find(name) {
        return custom(entry, args, cx, stack);
    }
    let base = canonical(name);
    if negated {
        return match base.as_str() {
            "bold" | "italic" | "underlined" | "strikethrough" | "obfuscated" => {
                no_args(args)?;
                Ok(decoration(&base, false))
            }
            "shadow" => {
                no_args(args)?;
                Ok(style(Style {
                    shadow_color: Some(0),
                    ..Style::default()
                }))
            }
            _ => Err(Fail::Unknown),
        };
    }
    match base.as_str() {
        "bold" | "italic" | "underlined" | "strikethrough" | "obfuscated" => {
            let on = match args {
                [] => true,
                [v] if v.eq_ignore_ascii_case("true") => true,
                [v] if v.eq_ignore_ascii_case("false") => false,
                _ => return Err(Fail::Bad("expects true or false")),
            };
            Ok(decoration(&base, on))
        }
        "reset" => no_args(args).map(|()| Resolved::Reset),
        "color" => match args {
            [colour] => Ok(coloured(
                parse_color(colour).ok_or(Fail::Bad("unknown colour"))?,
            )),
            _ => Err(Fail::Bad("expects one colour")),
        },
        "gradient" => gradient(args),
        "rainbow" => rainbow(args),
        "transition" => transition(args),
        "hover" => hover(args, cx, stack),
        "click" => click(args),
        "insertion" => Ok(style(Style {
            insertion: Some(joined(args, "expects the text")?),
            ..Style::default()
        })),
        "font" => Ok(style(Style {
            font: Some(joined(args, "expects a font id")?),
            ..Style::default()
        })),
        "shadow" => shadow(args),
        "key" => match args {
            [key] if !key.is_empty() => Ok(Resolved::Leaf(Component::keybind(key.clone()))),
            _ => Err(Fail::Bad("expects one key")),
        },
        "lang" | "lang_or" => lang(&base, args, cx, stack),
        "newline" => no_args(args).map(|()| Resolved::Text("\n".into())),
        other => match Color::parse(other) {
            Some(color) => {
                no_args(args)?;
                Ok(coloured(color))
            }
            None => Err(Fail::Unknown),
        },
    }
}

fn custom(entry: &super::Entry, args: &[String], cx: Cx<'_>, stack: usize) -> Res<Resolved> {
    use super::Entry;
    let argv = || args.iter().map(String::as_str).collect::<Vec<_>>();
    match entry {
        Entry::Unparsed(text) => no_args(args).map(|()| Resolved::Text(text.clone())),
        Entry::Component(component) => no_args(args).map(|()| Resolved::Leaf(component.clone())),
        Entry::Parsed(text) => {
            no_args(args)?;
            if !cx.expand() {
                return Err(Fail::Nested(ErrorKind::TooManyExpansions));
            }
            Ok(Resolved::Leaf(minimessage(text, cx, stack)?))
        }
        Entry::Insert(tag) => tag(&argv())
            .map(Resolved::Leaf)
            .ok_or(Fail::Bad("arguments not accepted")),
        Entry::Style(tag) => tag(&argv())
            .map(style)
            .ok_or(Fail::Bad("arguments not accepted")),
    }
}

fn no_args(args: &[String]) -> Res<()> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(Fail::Bad("takes no arguments"))
    }
}

/// The arguments from the second `:` on put back together: a URL or a font id has `:` in it.
fn joined(args: &[String], why: &'static str) -> Res<String> {
    let text = args.join(":");
    if args.is_empty() || text.is_empty() {
        Err(Fail::Bad(why))
    } else {
        Ok(text)
    }
}

fn style(style: Style) -> Resolved {
    Resolved::Wrap(Wrap::Style(style))
}

fn coloured(color: Color) -> Resolved {
    style(Style {
        color: Some(color),
        ..Style::default()
    })
}

fn decoration(name: &str, on: bool) -> Resolved {
    let which = match name {
        "bold" => Decoration::Bold,
        "italic" => Decoration::Italic,
        "underlined" => Decoration::Underlined,
        "strikethrough" => Decoration::Strikethrough,
        _ => Decoration::Obfuscated,
    };
    let mut s = Style::default();
    s.set(which, Some(on));
    style(s)
}

/// A colour by name or `#rrggbb`.
fn parse_color(s: &str) -> Option<Color> {
    Color::parse(&canonical(s))
}

/// A text read as MiniMessage inside the tag that has it.
fn minimessage(text: &str, cx: Cx<'_>, stack: usize) -> Res<Component> {
    parse::read(text, cx.inside(stack)).map_err(|e| Fail::Nested(e.kind))
}

/// A number from `lo` to `hi`.
fn number(s: &str, lo: f64, hi: f64) -> Option<f64> {
    s.parse::<f64>()
        .ok()
        .filter(|n| n.is_finite() && (lo..=hi).contains(n))
}

// shadow

fn shadow(args: &[String]) -> Res<Resolved> {
    let (colour, alpha) = match args {
        [colour] => (colour, None),
        [colour, alpha] => (colour, Some(alpha)),
        _ => return Err(Fail::Bad("expects a colour and perhaps an alpha")),
    };
    // `#aarrggbb`
    if let Some(hex) = colour.strip_prefix('#')
        && hex.len() == 8
        && hex.bytes().all(|b| b.is_ascii_hexdigit())
    {
        if alpha.is_some() {
            return Err(Fail::Bad("an #aarrggbb colour has its alpha already"));
        }
        let argb = u32::from_str_radix(hex, 16).map_err(|_| Fail::Bad("unknown colour"))?;
        return Ok(style(Style {
            shadow_color: Some(argb),
            ..Style::default()
        }));
    }
    let (r, g, b) = parse_color(colour)
        .ok_or(Fail::Bad("unknown colour"))?
        .rgb();
    let alpha = match alpha {
        Some(a) => number(a, 0.0, 1.0).ok_or(Fail::Bad("alpha is between 0 and 1"))?,
        None => 1.0,
    };
    let a = (alpha * 255.0).round() as u32;
    Ok(style(Style {
        shadow_color: Some(a << 24 | u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)),
        ..Style::default()
    }))
}

// events

fn click(args: &[String]) -> Res<Resolved> {
    let (action, rest) = args
        .split_first()
        .ok_or(Fail::Bad("expects an action and a value"))?;
    let value = joined(rest, "expects a value")?;
    let event = match action.to_ascii_lowercase().as_str() {
        "open_url" => ClickEvent::OpenUrl(value),
        "run_command" => ClickEvent::RunCommand(value),
        "suggest_command" => ClickEvent::SuggestCommand(value),
        "copy_to_clipboard" => ClickEvent::CopyToClipboard(value),
        "change_page" => match value.parse::<i32>() {
            Ok(page) if page >= 1 => ClickEvent::ChangePage(page),
            _ => return Err(Fail::Bad("a page is a number from 1")),
        },
        _ => return Err(Fail::Bad("unknown click action")),
    };
    Ok(style(Style {
        click: Some(event),
        ..Style::default()
    }))
}

fn hover(args: &[String], cx: Cx<'_>, stack: usize) -> Res<Resolved> {
    let (action, rest) = args
        .split_first()
        .ok_or(Fail::Bad("expects an action and a value"))?;
    let event = match action.to_ascii_lowercase().as_str() {
        "show_text" => {
            let text = joined(rest, "expects the text")?;
            HoverEvent::ShowText(minimessage(&text, cx, stack)?)
        }
        "show_item" => {
            let (id, count) = match rest {
                [id] => (id.clone(), None),
                [id, n] if n.parse::<i32>().is_ok() => (id.clone(), Some(n.as_str())),
                [namespace, path] => (format!("{namespace}:{path}"), None),
                [namespace, path, n] => (format!("{namespace}:{path}"), Some(n.as_str())),
                _ => return Err(Fail::Bad("expects an item id and perhaps a count")),
            };
            let count = match count {
                Some(n) => n
                    .parse::<i32>()
                    .ok()
                    .filter(|n| *n >= 1)
                    .ok_or(Fail::Bad("a count is a number from 1"))?,
                None => 1,
            };
            if id.is_empty() {
                return Err(Fail::Bad("expects an item id"));
            }
            HoverEvent::ShowItem { id, count }
        }
        "show_entity" => {
            let at = rest
                .iter()
                .enumerate()
                .skip(1)
                .find_map(|(i, a)| parse_uuid(a).map(|uuid| (i, uuid)));
            let Some((i, uuid)) = at else {
                return Err(Fail::Bad("expects an entity type and a uuid"));
            };
            let name = if rest.len() > i + 1 {
                Some(minimessage(&rest[i + 1..].join(":"), cx, stack)?)
            } else {
                None
            };
            HoverEvent::ShowEntity {
                entity_type: rest[..i].join(":"),
                uuid,
                name,
            }
        }
        _ => return Err(Fail::Bad("unknown hover action")),
    };
    Ok(style(Style {
        hover: Some(Box::new(event)),
        ..Style::default()
    }))
}

/// A UUID, with the hyphens or without.
fn parse_uuid(s: &str) -> Option<u128> {
    let hex: String = if s.len() == 36 {
        let b = s.as_bytes();
        if [8, 13, 18, 23].iter().any(|&i| b[i] != b'-') {
            return None;
        }
        s.chars().filter(|&c| c != '-').collect()
    } else {
        s.to_owned()
    };
    if hex.len() != 32 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u128::from_str_radix(&hex, 16).ok()
}

// translations

fn lang(tag: &str, args: &[String], cx: Cx<'_>, stack: usize) -> Res<Resolved> {
    let (key, fallback, rest) = match (tag, args) {
        ("lang", [key, rest @ ..]) => (key, None, rest),
        ("lang_or", [key, fallback, rest @ ..]) => (key, Some(fallback), rest),
        _ => return Err(Fail::Bad("expects a key")),
    };
    if key.is_empty() {
        return Err(Fail::Bad("expects a key"));
    }
    let mut component = Component::translatable(key.clone());
    if let Some(fallback) = fallback {
        component = component.fallback(fallback.clone());
    }
    for arg in rest {
        component = component.arg(minimessage(arg, cx, stack)?);
    }
    Ok(Resolved::Leaf(component))
}

// colours that change along the text

/// How a gradient or a rainbow colours the characters under it.
#[derive(Clone)]
pub(super) enum PaintSpec {
    Gradient {
        colors: Vec<(u8, u8, u8)>,
        phase: f64,
    },
    Rainbow {
        reverse: bool,
        phase: f64,
    },
}

impl PaintSpec {
    /// A painter for a text of `total` characters.
    pub fn over(&self, total: usize) -> Paint {
        Paint {
            spec: self.clone(),
            total,
            index: 0,
        }
    }
}

/// Hands out the colour of each character in turn.
pub(super) struct Paint {
    spec: PaintSpec,
    total: usize,
    index: usize,
}

impl Paint {
    /// The colour of the next character.
    pub fn next(&mut self) -> Color {
        let i = self.index as f64;
        let n = self.total as f64;
        self.index += 1;
        match &self.spec {
            PaintSpec::Gradient { colors, phase } => {
                let mut at = if self.total > 1 { i / (n - 1.0) } else { 0.0 };
                if *phase != 0.0 {
                    at = (at + phase).rem_euclid(1.0);
                }
                blend(colors, at)
            }
            PaintSpec::Rainbow { reverse, phase } => {
                let mut hue = i / n;
                if *reverse {
                    hue = 1.0 - hue;
                }
                hsv((hue + phase).rem_euclid(1.0))
            }
        }
    }

    /// Passes `n` characters without colouring them.
    pub fn skip(&mut self, n: usize) {
        self.index += n;
    }
}

/// The colour `at` (0 to 1) of the way along `colors`, mixed in RGB.
fn blend(colors: &[(u8, u8, u8)], at: f64) -> Color {
    let last = colors.len() - 1;
    if last == 0 {
        let (r, g, b) = colors[0];
        return Color::Rgb(r, g, b);
    }
    let along = at.clamp(0.0, 1.0) * last as f64;
    let from = (along.floor() as usize).min(last - 1);
    let t = along - from as f64;
    let mix = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * t).round() as u8;
    let ((r1, g1, b1), (r2, g2, b2)) = (colors[from], colors[from + 1]);
    Color::Rgb(mix(r1, r2), mix(g1, g2), mix(b1, b2))
}

/// The colour of hue `h` (0 to 1) at full saturation and value.
fn hsv(h: f64) -> Color {
    let sector = h * 6.0;
    let f = sector - sector.floor();
    let (r, g, b) = match sector.floor() as i32 % 6 {
        0 => (1.0, f, 0.0),
        1 => (1.0 - f, 1.0, 0.0),
        2 => (0.0, 1.0, f),
        3 => (0.0, 1.0 - f, 1.0),
        4 => (f, 0.0, 1.0),
        _ => (1.0, 0.0, 1.0 - f),
    };
    let byte = |v: f64| (v * 255.0).round() as u8;
    Color::Rgb(byte(r), byte(g), byte(b))
}

/// Colours from arguments.
fn colors(args: &[String]) -> Res<Vec<(u8, u8, u8)>> {
    args.iter()
        .map(|a| {
            parse_color(a)
                .map(Color::rgb)
                .ok_or(Fail::Bad("unknown colour"))
        })
        .collect()
}

fn gradient(args: &[String]) -> Res<Resolved> {
    let (colours, phase) = match args.split_last() {
        Some((last, rest)) if last.parse::<f64>().is_ok() => (
            rest,
            number(last, -1.0, 1.0).ok_or(Fail::Bad("phase is between -1 and 1"))?,
        ),
        _ => (args, 0.0),
    };
    let mut colors = colors(colours)?;
    if colors.is_empty() {
        colors = vec![(0, 0, 0), (255, 255, 255)];
    }
    Ok(Resolved::Wrap(Wrap::Paint(PaintSpec::Gradient {
        colors,
        phase,
    })))
}

fn rainbow(args: &[String]) -> Res<Resolved> {
    let (reverse, rest) = match args.split_first() {
        Some((first, rest)) if first == "!" => (true, rest),
        _ => (false, args),
    };
    let phase = match rest {
        [] => 0.0,
        [phase] => number(phase, -1.0, 1.0).ok_or(Fail::Bad("phase is between -1 and 1"))?,
        _ => return Err(Fail::Bad("expects `!` and a phase at most")),
    };
    Ok(Resolved::Wrap(Wrap::Paint(PaintSpec::Rainbow {
        reverse,
        phase,
    })))
}

fn transition(args: &[String]) -> Res<Resolved> {
    let Some((phase, colours)) = args.split_last().filter(|(_, c)| !c.is_empty()) else {
        return Err(Fail::Bad("expects colours and a phase"));
    };
    let phase = number(phase, 0.0, 1.0).ok_or(Fail::Bad("phase is between 0 and 1"))?;
    Ok(coloured(blend(&colors(colours)?, phase)))
}
