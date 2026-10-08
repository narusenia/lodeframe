// SPDX-License-Identifier: Apache-2.0 OR MIT
//! MiniMessage: text with tags, `<red>Hello <bold>there</bold></red>`, read into a
//! [`Component`] and written back.
//!
//! Use it for text that does not come from the program: a config file, a database, a command. The
//! parse functions are for text you trust; **what a user typed goes in through a [`TagResolver`]
//! as [`unparsed`](TagResolver::unparsed) or [`component`](TagResolver::component)**, which are
//! put in as they are and never read as tags.
//!
//! ```
//! use lodeframe_text::mini::{self, TagResolver};
//!
//! let tags = TagResolver::new().unparsed("player", "<red>not a tag");
//! let line = mini::parse_with("<gold>Hello <bold><player></bold>!", &tags).unwrap();
//! assert_eq!(mini::serialize(&line).unwrap(), r"<gold>Hello <bold>\<red>not a tag</bold>!</gold>");
//! ```
//!
//! # Tags
//!
//! - colours: `<red>` (the sixteen names), `<#rrggbb>`, `<color:red>`
//! - decorations: `<bold>`, `<italic>`, `<underlined>`, `<strikethrough>`, `<obfuscated>` (and
//!   `<b>`, `<i>`, `<em>`, `<u>`, `<st>`, `<obf>`). `<!bold>` or `<bold:false>` turns one off
//! - `<reset>` closes everything opened so far
//! - `<gradient:red:blue>`, `<rainbow>`, `<transition:red:blue:0.5>`
//! - `<hover:show_text:'...'>`, `<hover:show_item:id[:count]>`,
//!   `<hover:show_entity:type:uuid[:name]>`
//! - `<click:open_url|run_command|suggest_command|change_page|copy_to_clipboard:value>`
//! - `<insertion:text>`, `<font:id>`, `<shadow:colour[:alpha]>` (`<!shadow>` removes it)
//! - `<key:key.jump>`, `<lang:key[:arg...]>`, `<lang_or:key:fallback[:arg...]>`, `<newline>`
//!
//! Arguments are split by `:`; put `'...'` or `"..."` around one that has a `:` or `>` in it
//! (`\'` and `\\` go inside). In text, `\<` is a `<` and `\\` is a `\`. A tag that is not closed
//! is closed where the text ends. `selector`, `score`, `nbt`, `sprite` and `head` are not tags
//! here.
//!
//! The nesting is at most [`MAX_DEPTH`](crate::MAX_DEPTH) deep; a `hover` text or a `parsed`
//! placeholder counts as one more level.

mod parse;
mod tags;
mod write;

use std::{fmt, sync::Arc};

use crate::{Component, Style};
pub use write::{SerializeError, serialize};

/// Reads MiniMessage, refusing what is wrong.
///
/// # Errors
///
/// The first tag that is unknown, has arguments it cannot use, is closed out of order or never
/// ends, with where it is. Use [`parse_lenient`] when something must be shown anyway.
pub fn parse(input: &str) -> Result<Component, Error> {
    parse_with(input, &TagResolver::new())
}

/// [`parse`], with tags and placeholders of your own.
///
/// # Errors
///
/// As [`parse`].
pub fn parse_with(input: &str, tags: &TagResolver) -> Result<Component, Error> {
    parse::parse(input, tags, false)
}

/// Reads MiniMessage and keeps what is wrong as plain text, so that a component always comes
/// back: a tag that is unknown or has bad arguments stays as it was written, a closing tag with
/// nothing to close stays too, and a closing tag that skips over others closes them.
pub fn parse_lenient(input: &str) -> Component {
    parse_lenient_with(input, &TagResolver::new())
}

/// [`parse_lenient`], with tags and placeholders of your own.
pub fn parse_lenient_with(input: &str, tags: &TagResolver) -> Component {
    parse::parse(input, tags, true).unwrap_or_else(|_| Component::text(input))
}

/// Why MiniMessage could not be read, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// Where in the input, in bytes: the `<` of the tag at fault. What went wrong inside the text
    /// of a `hover` or a `lang` argument is reported at the tag that has it.
    pub position: usize,
    /// What is wrong.
    pub kind: ErrorKind,
}

/// What is wrong with a MiniMessage text.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// A tag that never ends, or an argument whose quote is not closed or not followed by `:` or
    /// `>`.
    Malformed,
    /// A tag nobody knows.
    UnknownTag(String),
    /// A tag whose arguments are not usable.
    BadArgument {
        /// The tag, as written.
        tag: String,
        /// What is wrong with them.
        why: &'static str,
    },
    /// A closing tag that skips over a tag that is still open.
    Mismatched(String),
    /// A closing tag with no tag of that name open.
    UnmatchedClose(String),
    /// Tags, hover texts and placeholders inside one another more than
    /// [`MAX_DEPTH`](crate::MAX_DEPTH) deep.
    TooDeep,
    /// `parsed` placeholders that contain themselves have been put in too many times.
    TooManyExpansions,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "at byte {}: ", self.position)?;
        match &self.kind {
            ErrorKind::Malformed => f.write_str("a tag that is not finished"),
            ErrorKind::UnknownTag(tag) => write!(f, "unknown tag <{tag}>"),
            ErrorKind::BadArgument { tag, why } => write!(f, "<{tag}>: {why}"),
            ErrorKind::Mismatched(tag) => write!(f, "</{tag}> closes over a tag still open"),
            ErrorKind::UnmatchedClose(tag) => write!(f, "</{tag}> has nothing to close"),
            ErrorKind::TooDeep => write!(f, "nested more than {} deep", crate::MAX_DEPTH),
            ErrorKind::TooManyExpansions => f.write_str("placeholders expand into themselves"),
        }
    }
}

impl std::error::Error for Error {}

/// The tags and placeholders a MiniMessage text may use besides the standard ones. Built with the
/// methods below, one tag each; the first one with a name is used, and they are tried before the
/// standard tags.
///
/// A name is lower case letters, digits, `_` and `-`; it is matched without regard to case.
#[derive(Clone, Default)]
pub struct TagResolver {
    entries: Vec<(String, Entry)>,
}

type InsertTag = Arc<dyn Fn(&[&str]) -> Option<Component> + Send + Sync>;
type StyleTag = Arc<dyn Fn(&[&str]) -> Option<Style> + Send + Sync>;

#[derive(Clone)]
enum Entry {
    Unparsed(String),
    Component(Component),
    Parsed(String),
    Insert(InsertTag),
    Style(StyleTag),
}

impl TagResolver {
    /// A resolver with no tags: only the standard ones are known.
    pub fn new() -> Self {
        Self::default()
    }

    /// `<name>` is replaced by `text`, as it is. Tags in `text` are not read: use this for what a
    /// user typed.
    ///
    /// # Panics
    ///
    /// If `name` is not a valid name (see [`TagResolver`]).
    #[must_use]
    pub fn unparsed(self, name: &str, text: impl Into<String>) -> Self {
        self.with(name, Entry::Unparsed(text.into()))
    }

    /// `<name>` is replaced by `component`, which keeps its own style and is drawn on top of the
    /// style around the tag.
    ///
    /// # Panics
    ///
    /// If `name` is not a valid name (see [`TagResolver`]).
    #[must_use]
    pub fn component(self, name: &str, component: impl Into<Component>) -> Self {
        self.with(name, Entry::Component(component.into()))
    }

    /// `<name>` is replaced by `text` read as MiniMessage, with this resolver. **Tags in `text`
    /// work, `click` too: never give it what a user typed.**
    ///
    /// # Panics
    ///
    /// If `name` is not a valid name (see [`TagResolver`]).
    #[must_use]
    pub fn parsed(self, name: &str, text: impl Into<String>) -> Self {
        self.with(name, Entry::Parsed(text.into()))
    }

    /// `<name:arg:arg>` is replaced by what `tag` makes of the arguments (none when they are not
    /// usable, which is an error in the text).
    ///
    /// # Panics
    ///
    /// If `name` is not a valid name (see [`TagResolver`]).
    #[must_use]
    pub fn insert(
        self,
        name: &str,
        tag: impl Fn(&[&str]) -> Option<Component> + Send + Sync + 'static,
    ) -> Self {
        self.with(name, Entry::Insert(Arc::new(tag)))
    }

    /// `<name:arg:arg>...</name>` draws what is inside in the style `tag` makes of the arguments
    /// (none when they are not usable, which is an error in the text).
    ///
    /// # Panics
    ///
    /// If `name` is not a valid name (see [`TagResolver`]).
    #[must_use]
    pub fn style(
        self,
        name: &str,
        tag: impl Fn(&[&str]) -> Option<Style> + Send + Sync + 'static,
    ) -> Self {
        self.with(name, Entry::Style(Arc::new(tag)))
    }

    /// The tags of `other` after these: the ones here win over a tag of the same name there.
    #[must_use]
    pub fn and(mut self, other: TagResolver) -> Self {
        self.entries.extend(other.entries);
        self
    }

    fn with(mut self, name: &str, entry: Entry) -> Self {
        assert!(
            !name.is_empty()
                && name
                    .bytes()
                    .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-')),
            "a tag name is lower case letters, digits, `_` and `-`: {name:?}"
        );
        self.entries.push((name.to_owned(), entry));
        self
    }

    fn find(&self, name: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, e)| e)
    }
}

impl fmt::Debug for TagResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.entries.iter().map(|(name, _)| name))
            .finish()
    }
}

#[cfg(test)]
mod tests;
