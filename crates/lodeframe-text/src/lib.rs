// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Text components and the MiniMessage parser.
//!
//! A [`Component`] is what the game shows as text: chat, titles, item names, kick messages. It is
//! some [`Content`] (plain text, a translated key, a score, ...), a [`Style`], and children that
//! follow it and inherit its style. Build one with the methods on [`Component`]; [`to_json`]
//! (Component::to_json) and [`from_json`](Component::from_json) read and write it as JSON, and
//! the protocol crate writes it as NBT. The [`mini`] module reads and writes MiniMessage, the
//! tag syntax (`<red>Hello <bold>there`) for text that comes from a config file or a user.

mod json;
pub mod mini;

use std::{fmt, ops::Add};

pub use json::{MAX_DEPTH, ParseError, uuid_ints};
/// The JSON value [`Component::from_value`] reads, and the object it is made of.
pub use serde_json::{Map, Value};

/// A piece of styled text, as shown in chat, titles and item names.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Component {
    /// What is shown.
    pub content: Content,
    /// How it is drawn. Fields left unset inherit from the component this one is a child of.
    pub style: Style,
    /// What follows, each drawn on top of this component's style.
    pub children: Vec<Component>,
}

/// What a [`Component`] shows.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Content {
    /// Literal text.
    Text(String),
    /// A line of the client's language file, with `args` put into its `%s` places.
    Translatable {
        /// The key, such as `chat.type.text`.
        key: String,
        /// What is shown when the client does not know the key.
        fallback: Option<String>,
        /// What goes into the `%s` places, in order.
        args: Vec<Component>,
    },
    /// A scoreboard score.
    Score {
        /// Whose score: a player name, an entity selector, or `*` for whoever reads it.
        name: String,
        /// The objective the score is of.
        objective: String,
    },
    /// The names of the entities a selector such as `@a` matches.
    Selector {
        /// The selector.
        selector: String,
        /// What goes between the names; `, ` by default.
        separator: Option<Box<Component>>,
    },
    /// The key a game control is bound to, such as `key.jump`.
    Keybind(String),
    /// A value out of the NBT of an entity, a block or a command storage.
    Nbt {
        /// The NBT path.
        path: String,
        /// Where the NBT is.
        source: NbtSource,
        /// How the value is shown.
        mode: NbtMode,
        /// What goes between several values.
        separator: Option<Box<Component>>,
    },
    /// A picture out of a texture atlas, drawn in the text.
    Sprite {
        /// The atlas, such as `minecraft:blocks`; the blocks atlas when unset.
        atlas: Option<String>,
        /// The sprite in the atlas, such as `block/stone`.
        sprite: String,
        /// What is shown when the client cannot draw the sprite.
        fallback: Option<Box<Component>>,
    },
}

impl Default for Content {
    fn default() -> Self {
        Self::Text(String::new())
    }
}

/// How a [`Content::Nbt`] shows its value. The game takes at most one of the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NbtMode {
    /// As the text of the NBT, coloured by type.
    #[default]
    Default,
    /// The value is itself read as a component.
    Interpret,
    /// As the text of the NBT, without the colouring of its type.
    Plain,
}

/// Where a [`Content::Nbt`] reads from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NbtSource {
    /// The entities an entity selector matches.
    Entity(String),
    /// The block at a position such as `~ ~ ~`.
    Block(String),
    /// A command storage, by id.
    Storage(String),
}

/// How a [`Component`] is drawn. `None` inherits from the surrounding text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Style {
    /// The text colour.
    pub color: Option<Color>,
    /// The colour of the shadow, `0xAARRGGBB`.
    pub shadow_color: Option<u32>,
    /// Bold.
    pub bold: Option<bool>,
    /// Italic.
    pub italic: Option<bool>,
    /// Underlined.
    pub underlined: Option<bool>,
    /// Struck through.
    pub strikethrough: Option<bool>,
    /// Randomly changing characters.
    pub obfuscated: Option<bool>,
    /// What shift-clicking the text puts into the chat box.
    pub insertion: Option<String>,
    /// The font, by id, such as `minecraft:uniform`.
    pub font: Option<String>,
    /// What clicking the text does.
    pub click: Option<ClickEvent>,
    /// What hovering over the text shows.
    pub hover: Option<Box<HoverEvent>>,
}

impl Style {
    /// Whether every field inherits.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Sets a decoration, or lets it inherit with `None`.
    pub fn set(&mut self, decoration: Decoration, value: Option<bool>) {
        match decoration {
            Decoration::Bold => self.bold = value,
            Decoration::Italic => self.italic = value,
            Decoration::Underlined => self.underlined = value,
            Decoration::Strikethrough => self.strikethrough = value,
            Decoration::Obfuscated => self.obfuscated = value,
        }
    }
}

/// One of the five on/off decorations of a [`Style`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs)] // the variant names are the decoration names
pub enum Decoration {
    Bold,
    Italic,
    Underlined,
    Strikethrough,
    Obfuscated,
}

/// What clicking text does.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClickEvent {
    /// Opens a web address, after the player confirms.
    OpenUrl(String),
    /// Runs a command as the player.
    RunCommand(String),
    /// Puts a command into the chat box.
    SuggestCommand(String),
    /// Turns a book to a page, counting from 1.
    ChangePage(i32),
    /// Copies text to the clipboard.
    CopyToClipboard(String),
}

/// What hovering over text shows.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum HoverEvent {
    /// Another component.
    ShowText(Component),
    /// An item's tooltip.
    ShowItem {
        /// The item, such as `minecraft:stone`.
        id: String,
        /// How many.
        count: i32,
    },
    /// An entity's tooltip.
    ShowEntity {
        /// The entity type, such as `minecraft:pig`.
        entity_type: String,
        /// The entity's UUID.
        uuid: u128,
        /// A name to show in place of the entity's own.
        name: Option<Component>,
    },
}

impl Component {
    /// Plain text that inherits its style.
    pub fn text(text: impl Into<String>) -> Self {
        Content::Text(text.into()).into()
    }

    /// Nothing at all. A place to put children under, with no style of its own.
    pub fn empty() -> Self {
        Self::default()
    }

    /// A line break.
    pub fn newline() -> Self {
        Self::text("\n")
    }

    /// A space.
    pub fn space() -> Self {
        Self::text(" ")
    }

    /// A line of the client's language file. Put what goes into its `%s` places with
    /// [`arg`](Self::arg).
    pub fn translatable(key: impl Into<String>) -> Self {
        Content::Translatable {
            key: key.into(),
            fallback: None,
            args: Vec::new(),
        }
        .into()
    }

    /// A scoreboard score of `name` on `objective`.
    pub fn score(name: impl Into<String>, objective: impl Into<String>) -> Self {
        Content::Score {
            name: name.into(),
            objective: objective.into(),
        }
        .into()
    }

    /// The names of the entities `selector` matches.
    pub fn selector(selector: impl Into<String>) -> Self {
        Content::Selector {
            selector: selector.into(),
            separator: None,
        }
        .into()
    }

    /// The key bound to the control `key`, such as `key.jump`.
    pub fn keybind(key: impl Into<String>) -> Self {
        Content::Keybind(key.into()).into()
    }

    /// The NBT at `path` of the entities `selector` matches.
    pub fn nbt_entity(path: impl Into<String>, selector: impl Into<String>) -> Self {
        Self::nbt(path, NbtSource::Entity(selector.into()))
    }

    /// The NBT at `path` of the block at `pos`, such as `~ ~ ~`.
    pub fn nbt_block(path: impl Into<String>, pos: impl Into<String>) -> Self {
        Self::nbt(path, NbtSource::Block(pos.into()))
    }

    /// The NBT at `path` of the command storage `id`.
    pub fn nbt_storage(path: impl Into<String>, id: impl Into<String>) -> Self {
        Self::nbt(path, NbtSource::Storage(id.into()))
    }

    fn nbt(path: impl Into<String>, source: NbtSource) -> Self {
        Content::Nbt {
            path: path.into(),
            source,
            mode: NbtMode::Default,
            separator: None,
        }
        .into()
    }

    /// A picture `sprite` of the blocks atlas; change the atlas with [`atlas`](Self::atlas).
    pub fn sprite(sprite: impl Into<String>) -> Self {
        Content::Sprite {
            atlas: None,
            sprite: sprite.into(),
            fallback: None,
        }
        .into()
    }

    /// The text if this is plain text, which is `None` for any other content.
    pub fn as_text(&self) -> Option<&str> {
        match &self.content {
            Content::Text(text) => Some(text),
            _ => None,
        }
    }

    /// Adds `arg` to what goes into a [translatable](Self::translatable)'s `%s` places. Has no
    /// effect on any other content.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<Component>) -> Self {
        if let Content::Translatable { args, .. } = &mut self.content {
            args.push(arg.into());
        }
        self
    }

    /// Adds all of `args`, like [`arg`](Self::arg).
    #[must_use]
    pub fn args(mut self, new: impl IntoIterator<Item = Component>) -> Self {
        if let Content::Translatable { args, .. } = &mut self.content {
            args.extend(new);
        }
        self
    }

    /// Sets what a [translatable](Self::translatable) shows when the client does not know its
    /// key. Has no effect on any other content.
    #[must_use]
    pub fn fallback(mut self, text: impl Into<String>) -> Self {
        if let Content::Translatable { fallback, .. } = &mut self.content {
            *fallback = Some(text.into());
        }
        self
    }

    /// Sets what goes between the values of a [selector](Self::selector) or an NBT component.
    /// Has no effect on any other content.
    #[must_use]
    pub fn separator(mut self, separator: impl Into<Component>) -> Self {
        if let Content::Selector { separator: s, .. } | Content::Nbt { separator: s, .. } =
            &mut self.content
        {
            *s = Some(Box::new(separator.into()));
        }
        self
    }

    /// Makes an NBT component read its value as a component, in place of [`plain`](Self::plain).
    /// Has no effect on any other content.
    #[must_use]
    pub fn interpret(mut self) -> Self {
        if let Content::Nbt { mode, .. } = &mut self.content {
            *mode = NbtMode::Interpret;
        }
        self
    }

    /// Makes an NBT component show its value without the colouring of its type, in place of
    /// [`interpret`](Self::interpret). Has no effect on any other content.
    #[must_use]
    pub fn plain(mut self) -> Self {
        if let Content::Nbt { mode, .. } = &mut self.content {
            *mode = NbtMode::Plain;
        }
        self
    }

    /// Sets the atlas of a [sprite](Self::sprite). Has no effect on any other content.
    #[must_use]
    pub fn atlas(mut self, atlas: impl Into<String>) -> Self {
        if let Content::Sprite { atlas: a, .. } = &mut self.content {
            *a = Some(atlas.into());
        }
        self
    }

    /// Sets what a [sprite](Self::sprite) shows when it cannot be drawn. Has no effect on any
    /// other content.
    #[must_use]
    pub fn sprite_fallback(mut self, fallback: impl Into<Component>) -> Self {
        if let Content::Sprite { fallback: f, .. } = &mut self.content {
            *f = Some(Box::new(fallback.into()));
        }
        self
    }

    /// Replaces the whole style.
    #[must_use]
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the colour.
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.style.color = Some(color);
        self
    }

    /// Sets the colour of the shadow, `0xAARRGGBB`.
    #[must_use]
    pub fn shadow_color(mut self, argb: u32) -> Self {
        self.style.shadow_color = Some(argb);
        self
    }

    /// Makes the text bold.
    #[must_use]
    pub fn bold(self) -> Self {
        self.decorate(Decoration::Bold, true)
    }

    /// Makes the text italic.
    #[must_use]
    pub fn italic(self) -> Self {
        self.decorate(Decoration::Italic, true)
    }

    /// Underlines the text.
    #[must_use]
    pub fn underlined(self) -> Self {
        self.decorate(Decoration::Underlined, true)
    }

    /// Strikes the text through.
    #[must_use]
    pub fn strikethrough(self) -> Self {
        self.decorate(Decoration::Strikethrough, true)
    }

    /// Makes the characters change at random.
    #[must_use]
    pub fn obfuscated(self) -> Self {
        self.decorate(Decoration::Obfuscated, true)
    }

    /// Turns a decoration on or off, whatever the text this is a child of does.
    #[must_use]
    pub fn decorate(mut self, decoration: Decoration, on: bool) -> Self {
        self.style.set(decoration, Some(on));
        self
    }

    /// Lets a decoration inherit again.
    #[must_use]
    pub fn undecorate(mut self, decoration: Decoration) -> Self {
        self.style.set(decoration, None);
        self
    }

    /// Sets what shift-clicking the text puts into the chat box.
    #[must_use]
    pub fn insertion(mut self, insertion: impl Into<String>) -> Self {
        self.style.insertion = Some(insertion.into());
        self
    }

    /// Sets the font, by id.
    #[must_use]
    pub fn font(mut self, font: impl Into<String>) -> Self {
        self.style.font = Some(font.into());
        self
    }

    /// Sets what clicking the text does.
    #[must_use]
    pub fn click(mut self, click: ClickEvent) -> Self {
        self.style.click = Some(click);
        self
    }

    /// Opens `url` when the text is clicked.
    #[must_use]
    pub fn click_open_url(self, url: impl Into<String>) -> Self {
        self.click(ClickEvent::OpenUrl(url.into()))
    }

    /// Runs `command` as the player when the text is clicked.
    #[must_use]
    pub fn click_run_command(self, command: impl Into<String>) -> Self {
        self.click(ClickEvent::RunCommand(command.into()))
    }

    /// Puts `command` into the chat box when the text is clicked.
    #[must_use]
    pub fn click_suggest_command(self, command: impl Into<String>) -> Self {
        self.click(ClickEvent::SuggestCommand(command.into()))
    }

    /// Sets what hovering over the text shows.
    #[must_use]
    pub fn hover(mut self, hover: HoverEvent) -> Self {
        self.style.hover = Some(Box::new(hover));
        self
    }

    /// Shows `text` when the text is hovered over.
    #[must_use]
    pub fn hover_text(self, text: impl Into<Component>) -> Self {
        self.hover(HoverEvent::ShowText(text.into()))
    }

    /// Puts `child` after this component, drawn on top of its style.
    #[must_use]
    pub fn append(mut self, child: impl Into<Component>) -> Self {
        self.children.push(child.into());
        self
    }

    /// Puts all of `children` after this component, in order.
    #[must_use]
    pub fn append_all(mut self, children: impl IntoIterator<Item = Component>) -> Self {
        self.children.extend(children);
        self
    }

    /// Puts `separator` between `parts`: a component with no style of its own whose children are
    /// the parts, with a copy of the separator between each two.
    pub fn join(
        separator: impl Into<Component>,
        parts: impl IntoIterator<Item = Component>,
    ) -> Self {
        let separator = separator.into();
        let mut joined = Self::empty();
        for (i, part) in parts.into_iter().enumerate() {
            if i > 0 {
                joined.children.push(separator.clone());
            }
            joined.children.push(part);
        }
        joined
    }

    /// Whether this is nothing but a place for children: no content, no style.
    fn is_container(&self) -> bool {
        self.style.is_empty() && matches!(&self.content, Content::Text(text) if text.is_empty())
    }
}

impl From<Content> for Component {
    fn from(content: Content) -> Self {
        Self {
            content,
            style: Style::default(),
            children: Vec::new(),
        }
    }
}

impl From<&str> for Component {
    fn from(text: &str) -> Self {
        Self::text(text)
    }
}

impl From<String> for Component {
    fn from(text: String) -> Self {
        Self::text(text)
    }
}

/// Puts `rhs` after `self`, side by side. If `self` is only a place for children, `rhs` joins
/// them; otherwise both become the children of a new one, so that neither takes the style of
/// the other. `a + b + c` is one level of three.
impl<T: Into<Component>> Add<T> for Component {
    type Output = Component;

    fn add(self, rhs: T) -> Component {
        if self.is_container() {
            self.append(rhs)
        } else {
            Component::empty().append(self).append(rhs)
        }
    }
}

/// Collects components side by side, as children of a place with no style of its own.
impl FromIterator<Component> for Component {
    fn from_iter<I: IntoIterator<Item = Component>>(iter: I) -> Self {
        Self::empty().append_all(iter)
    }
}

/// A text colour: one of the sixteen named colours or any RGB value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(missing_docs)] // the variant names are the colour names
pub enum Color {
    Black,
    DarkBlue,
    DarkGreen,
    DarkAqua,
    DarkRed,
    DarkPurple,
    Gold,
    Gray,
    DarkGray,
    Blue,
    Green,
    Aqua,
    Red,
    LightPurple,
    Yellow,
    White,
    /// Red, green and blue.
    Rgb(u8, u8, u8),
}

/// The sixteen named colours with the names the game uses.
const NAMED: [(&str, Color); 16] = [
    ("black", Color::Black),
    ("dark_blue", Color::DarkBlue),
    ("dark_green", Color::DarkGreen),
    ("dark_aqua", Color::DarkAqua),
    ("dark_red", Color::DarkRed),
    ("dark_purple", Color::DarkPurple),
    ("gold", Color::Gold),
    ("gray", Color::Gray),
    ("dark_gray", Color::DarkGray),
    ("blue", Color::Blue),
    ("green", Color::Green),
    ("aqua", Color::Aqua),
    ("red", Color::Red),
    ("light_purple", Color::LightPurple),
    ("yellow", Color::Yellow),
    ("white", Color::White),
];

impl Color {
    /// Reads a colour the way the game writes it: a name (`dark_blue`, in lower case) or
    /// `#rrggbb` (in either case).
    pub fn parse(s: &str) -> Option<Self> {
        if let Some(hex) = s.strip_prefix('#') {
            // `from_str_radix` would take a sign
            if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            let rgb = u32::from_str_radix(hex, 16).ok()?;
            return Some(Self::Rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8));
        }
        NAMED
            .iter()
            .find(|(name, _)| *name == s)
            .map(|&(_, color)| color)
    }

    /// The red, green and blue the game draws the colour with.
    pub fn rgb(self) -> (u8, u8, u8) {
        let hex = |rgb: u32| ((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
        match self {
            Self::Black => hex(0x000000),
            Self::DarkBlue => hex(0x0000AA),
            Self::DarkGreen => hex(0x00AA00),
            Self::DarkAqua => hex(0x00AAAA),
            Self::DarkRed => hex(0xAA0000),
            Self::DarkPurple => hex(0xAA00AA),
            Self::Gold => hex(0xFFAA00),
            Self::Gray => hex(0xAAAAAA),
            Self::DarkGray => hex(0x555555),
            Self::Blue => hex(0x5555FF),
            Self::Green => hex(0x55FF55),
            Self::Aqua => hex(0x55FFFF),
            Self::Red => hex(0xFF5555),
            Self::LightPurple => hex(0xFF55FF),
            Self::Yellow => hex(0xFFFF55),
            Self::White => hex(0xFFFFFF),
            Self::Rgb(r, g, b) => (r, g, b),
        }
    }
}

/// The name the game uses: `red`, `dark_blue`, ... or `#rrggbb`.
impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Self::Rgb(r, g, b) = self {
            return write!(f, "#{r:02x}{g:02x}{b:02x}");
        }
        let (name, _) = NAMED
            .iter()
            .find(|(_, color)| color == self)
            .expect("every named colour is in the table");
        f.write_str(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_displays_the_game_name() {
        assert_eq!(Color::Red.to_string(), "red");
        assert_eq!(Color::DarkBlue.to_string(), "dark_blue");
        assert_eq!(Color::LightPurple.to_string(), "light_purple");
        assert_eq!(Color::Rgb(0xff, 0x00, 0xa0).to_string(), "#ff00a0");
        assert_eq!(Color::Rgb(1, 2, 3).to_string(), "#010203");
    }

    #[test]
    fn color_parses_what_it_displays_and_hex_in_either_case() {
        for (name, color) in NAMED {
            assert_eq!(Color::parse(name), Some(color));
            assert_eq!(color.to_string(), name);
        }
        // the game takes a name as it is written, and a hex number in either case
        assert_eq!(Color::parse("RED"), None);
        assert_eq!(Color::parse("#FF00a0"), Some(Color::Rgb(0xff, 0, 0xa0)));
        for bad in [
            "",
            "#",
            "#fff",
            "#+f00a0",
            "#ff00a0ff",
            "ff00a0",
            "reddish",
            "#gg0000",
        ] {
            assert_eq!(Color::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn builders_set_only_what_they_name() {
        let c = Component::text("x").color(Color::Red).bold();
        assert_eq!(c.style.color, Some(Color::Red));
        assert_eq!(c.style.bold, Some(true));
        assert_eq!(c.style.italic, None);
        assert_eq!(c.as_text(), Some("x"));
        assert!(Component::text("x").style.is_empty());
        assert_eq!(Component::keybind("key.jump").as_text(), None);
    }

    #[test]
    fn decorations_can_be_turned_off_and_back_to_inheriting() {
        let c = Component::text("x").decorate(Decoration::Italic, false);
        assert_eq!(c.style.italic, Some(false));
        assert_eq!(c.undecorate(Decoration::Italic).style.italic, None);
    }

    #[test]
    fn builders_for_a_kind_leave_other_kinds_alone() {
        let text = Component::text("x")
            .arg("a")
            .fallback("f")
            .interpret()
            .atlas("a");
        assert_eq!(text, Component::text("x"));
        let t = Component::translatable("k")
            .arg("a")
            .args([Component::text("b")]);
        assert_eq!(
            t.content,
            Content::Translatable {
                key: "k".into(),
                fallback: None,
                args: vec![Component::text("a"), Component::text("b")],
            }
        );
    }

    #[test]
    fn append_puts_children_after_in_order() {
        let c = Component::text("a")
            .append("b")
            .append(Component::text("c").bold())
            .append_all([Component::text("d")]);
        let names: Vec<_> = c.children.iter().map(|c| c.as_text().unwrap()).collect();
        assert_eq!(names, ["b", "c", "d"]);
    }

    #[test]
    fn plus_puts_components_side_by_side_without_nesting() {
        let a = Component::text("a").color(Color::Red);
        let sum = a.clone() + "b" + Component::text("c").bold();
        // one level: a, b, c
        assert_eq!(sum.children.len(), 3);
        assert_eq!(sum.children[0], a);
        assert!(sum.is_container());
        // the style of the left side stays on the left side
        assert_eq!(sum.children[1].style, Style::default());
    }

    #[test]
    fn plus_after_a_component_with_children_does_not_adopt_them() {
        let parent = Component::text("p").bold().append("child");
        let sum = parent.clone() + "next";
        assert_eq!(sum.children, [parent, Component::text("next")]);
    }

    #[test]
    fn collect_and_join_make_a_place_with_no_style() {
        let parts = || ["a", "b", "c"].map(Component::text);
        let collected: Component = parts().into_iter().collect();
        assert_eq!(collected.children.len(), 3);
        assert!(collected.is_container());

        let joined = Component::join(", ", parts());
        let texts: Vec<_> = joined
            .children
            .iter()
            .map(|c| c.as_text().unwrap())
            .collect();
        assert_eq!(texts, ["a", ", ", "b", ", ", "c"]);
        assert_eq!(Component::join(", ", []), Component::empty());
        assert_eq!(
            Component::join(", ", [Component::text("a")]).children.len(),
            1
        );
    }
}
