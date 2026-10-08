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
type Rgb = (u8, u8, u8);

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
        "insertion" => "insert",
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
            // as Adventure does: only `false` turns it off
            let on = !matches!(args.first(), Some(v) if v.eq_ignore_ascii_case("false"));
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
        "insert" => Ok(style(Style {
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
    // `#rrggbbaa`, the alpha last as Adventure has it
    if let Some(hex) = colour.strip_prefix('#')
        && hex.len() == 8
        && hex.bytes().all(|b| b.is_ascii_hexdigit())
    {
        if alpha.is_some() {
            return Err(Fail::Bad("an #rrggbbaa colour has its alpha already"));
        }
        let rgba = u32::from_str_radix(hex, 16).map_err(|_| Fail::Bad("unknown colour"))?;
        return Ok(style(Style {
            shadow_color: Some(rgba.rotate_right(8)),
            ..Style::default()
        }));
    }
    let (r, g, b) = parse_color(colour)
        .ok_or(Fail::Bad("unknown colour"))?
        .rgb();
    // a quarter as opaque unless told, and cut, not rounded, to a byte: as Adventure does
    let alpha = match alpha {
        Some(a) => number(a, 0.0, 1.0).ok_or(Fail::Bad("alpha is between 0 and 1"))?,
        None => 0.25,
    };
    let a = (alpha as f32 * 255.0) as u32;
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

// colours that change along the text. The formulas are Adventure's (`GradientTag`, `RainbowTag`,
// `TransitionTag`), in `f32` where it is, so that the same text gets the same colours.

/// How a gradient or a rainbow colours the characters under it.
#[derive(Clone)]
pub(super) enum PaintSpec {
    Gradient {
        /// Already turned round for a negative phase.
        colors: Vec<Rgb>,
        /// From 0 up to 1.
        phase: f64,
    },
    Rainbow {
        reverse: bool,
        /// The phase in tenths, divided.
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
        let k = self.index;
        self.index += 1;
        let total = self.total.max(1);
        match &self.spec {
            PaintSpec::Gradient { colors, phase } => {
                let n = colors.len();
                // the colours spread over the text, the last at its last character
                let multiplier = if total == 1 {
                    0.0
                } else {
                    (n - 1) as f64 / (total - 1) as f64
                };
                let position = k as f64 * multiplier + phase * (n - 1) as f64;
                let low_unclamped = position.floor();
                // past the last colour it goes round to the first, so that a phase is a cycle
                let high = (position.ceil() as usize) % n;
                let low = (low_unclamped as usize) % n;
                lerp(
                    position as f32 - low_unclamped as f32,
                    colors[low],
                    colors[high],
                )
            }
            PaintSpec::Rainbow { reverse, phase } => {
                let at = if *reverse { total - 1 - k % total } else { k };
                let hue = (f64::from(at as f32 / total as f32) + phase).rem_euclid(1.0);
                hsv(hue as f32)
            }
        }
    }

    /// Passes `n` characters without colouring them.
    pub fn skip(&mut self, n: usize) {
        self.index += n;
    }
}

/// The colour `t` (0 to 1) of the way from `a` to `b`, mixed in RGB and rounded.
fn lerp(t: f32, a: Rgb, b: Rgb) -> Color {
    let t = t.clamp(0.0, 1.0);
    let mix = |a: u8, b: u8| (f32::from(a) + t * (f32::from(b) - f32::from(a)) + 0.5).floor() as u8;
    Color::Rgb(mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}

/// The colour of hue `h` (0 to 1) at full saturation and value; each part cut to a byte.
fn hsv(h: f32) -> Color {
    let sector = h * 6.0;
    let i = sector.floor() as i32;
    let f = sector - i as f32;
    let (q, t) = (1.0 - f, f);
    let (r, g, b) = match i {
        0 => (1.0, t, 0.0),
        1 => (q, 1.0, 0.0),
        2 => (0.0, 1.0, t),
        3 => (0.0, q, 1.0),
        4 => (t, 0.0, 1.0),
        _ => (1.0, 0.0, q),
    };
    let byte = |v: f32| (v * 255.0) as u8;
    Color::Rgb(byte(r), byte(g), byte(b))
}

/// Colours, and a phase in `-1..=1` as the last argument if it is a number.
fn colors_and_phase(args: &[String]) -> Res<(Vec<Rgb>, f64)> {
    let mut colors = Vec::new();
    for (i, arg) in args.iter().enumerate() {
        if let Some(color) = parse_color(arg) {
            colors.push(color.rgb());
        } else if i + 1 == args.len() && arg.parse::<f64>().is_ok() {
            let phase = number(arg, -1.0, 1.0).ok_or(Fail::Bad("phase is between -1 and 1"))?;
            return Ok((colors, phase));
        } else {
            return Err(Fail::Bad("unknown colour"));
        }
    }
    Ok((colors, 0.0))
}

const WHITE_TO_BLACK: [Rgb; 2] = [(255, 255, 255), (0, 0, 0)];

fn gradient(args: &[String]) -> Res<Resolved> {
    let (mut colors, phase) = colors_and_phase(args)?;
    match colors.len() {
        0 => colors = WHITE_TO_BLACK.to_vec(),
        1 => return Err(Fail::Bad("a gradient needs two colours at least")),
        _ => {}
    }
    // a negative phase runs the colours the other way
    let phase = if phase < 0.0 {
        colors.reverse();
        1.0 + phase
    } else {
        phase
    };
    Ok(Resolved::Wrap(Wrap::Paint(PaintSpec::Gradient {
        colors,
        phase,
    })))
}

fn rainbow(args: &[String]) -> Res<Resolved> {
    let (reverse, phase) = match args {
        [] => (false, 0),
        [arg] => {
            let (reverse, number) = match arg.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, arg.as_str()),
            };
            let phase = if number.is_empty() {
                0
            } else {
                number
                    .parse::<i32>()
                    .map_err(|_| Fail::Bad("expects `!` and a whole-number phase"))?
            };
            (reverse, phase)
        }
        _ => return Err(Fail::Bad("expects `!` and a whole-number phase at most")),
    };
    Ok(Resolved::Wrap(Wrap::Paint(PaintSpec::Rainbow {
        reverse,
        phase: f64::from(phase) / 10.0,
    })))
}

fn transition(args: &[String]) -> Res<Resolved> {
    let (mut colors, phase) = colors_and_phase(args)?;
    if colors.len() == 1 {
        return Err(Fail::Bad("a transition needs two colours at least"));
    }
    let negative = phase < 0.0;
    let phase = if negative {
        colors.reverse();
        1.0 + phase
    } else {
        phase
    };
    if colors.is_empty() {
        colors = WHITE_TO_BLACK.to_vec();
    }
    let phase = phase as f32;
    let last = colors.len() - 1;
    let steps = 1.0 / last as f32;
    for at in 1..=last {
        let val = at as f32 * steps;
        if val >= phase {
            let factor = 1.0 + (phase - val) * last as f32;
            // the segment is turned round for a negative phase, as Adventure does
            let color = if negative {
                lerp(1.0 - factor, colors[at], colors[at - 1])
            } else {
                lerp(factor, colors[at - 1], colors[at])
            };
            return Ok(coloured(color));
        }
    }
    let (r, g, b) = colors[last];
    Ok(coloured(Color::Rgb(r, g, b)))
}
