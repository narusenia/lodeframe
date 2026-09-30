// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Text components and the MiniMessage parser.
//!
//! So far: plain text with a colour and the five decorations. Children, events and
//! the rest of the component model follow in v0.2.

use std::fmt;

/// A piece of styled text, as shown in chat, titles and item names.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Component {
    /// The text itself.
    pub text: String,
    /// How the text is drawn.
    pub style: Style,
}

/// How a [`Component`] is drawn. `None` inherits from the surrounding text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    /// The text colour.
    pub color: Option<Color>,
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
}

impl Style {
    /// Whether every field inherits.
    pub const fn is_empty(&self) -> bool {
        self.color.is_none()
            && self.bold.is_none()
            && self.italic.is_none()
            && self.underlined.is_none()
            && self.strikethrough.is_none()
            && self.obfuscated.is_none()
    }
}

impl Component {
    /// Plain text that inherits its style.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: Style::default(),
        }
    }

    /// Sets the colour.
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.style.color = Some(color);
        self
    }

    /// Makes the text bold.
    #[must_use]
    pub fn bold(mut self) -> Self {
        self.style.bold = Some(true);
        self
    }

    /// Makes the text italic.
    #[must_use]
    pub fn italic(mut self) -> Self {
        self.style.italic = Some(true);
        self
    }

    /// Underlines the text.
    #[must_use]
    pub fn underlined(mut self) -> Self {
        self.style.underlined = Some(true);
        self
    }

    /// Strikes the text through.
    #[must_use]
    pub fn strikethrough(mut self) -> Self {
        self.style.strikethrough = Some(true);
        self
    }

    /// Makes the characters change randomly.
    #[must_use]
    pub fn obfuscated(mut self) -> Self {
        self.style.obfuscated = Some(true);
        self
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

/// The name the game uses: `red`, `dark_blue`, ... or `#rrggbb`.
impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Black => "black",
            Self::DarkBlue => "dark_blue",
            Self::DarkGreen => "dark_green",
            Self::DarkAqua => "dark_aqua",
            Self::DarkRed => "dark_red",
            Self::DarkPurple => "dark_purple",
            Self::Gold => "gold",
            Self::Gray => "gray",
            Self::DarkGray => "dark_gray",
            Self::Blue => "blue",
            Self::Green => "green",
            Self::Aqua => "aqua",
            Self::Red => "red",
            Self::LightPurple => "light_purple",
            Self::Yellow => "yellow",
            Self::White => "white",
            Self::Rgb(r, g, b) => return write!(f, "#{r:02x}{g:02x}{b:02x}"),
        };
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
    fn builders_set_only_what_they_name() {
        let plain = Component::text("hi");
        assert!(plain.style.is_empty());

        let c = Component::text("hi").color(Color::Gold).bold().underlined();
        assert_eq!(c.style.color, Some(Color::Gold));
        assert_eq!((c.style.bold, c.style.underlined), (Some(true), Some(true)));
        assert_eq!(
            (c.style.italic, c.style.strikethrough, c.style.obfuscated),
            (None, None, None)
        );
        assert!(!c.style.is_empty());
        assert_eq!(Component::from("hi"), plain);
    }
}
