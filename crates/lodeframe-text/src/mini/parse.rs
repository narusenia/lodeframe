// SPDX-License-Identifier: Apache-2.0 OR MIT
//! From text to a tree of tags, and from the tree to a component.

use std::cell::Cell;

use super::{
    Error, ErrorKind, TagResolver,
    tags::{self, Fail, Paint, Resolved, Wrap},
};
use crate::{Component, Content, MAX_DEPTH};

/// How many `parsed` placeholders one text may put in, however they nest: one that contains
/// itself twice would otherwise grow with the square of the depth.
const MAX_EXPANSIONS: usize = 256;

/// What a parse runs with.
#[derive(Clone, Copy)]
pub(super) struct Cx<'a> {
    pub tags: &'a TagResolver,
    pub lenient: bool,
    /// How many levels the text being read is already inside.
    pub depth: usize,
    expansions: &'a Cell<usize>,
}

impl Cx<'_> {
    /// The context of a text read inside a tag that is `stack` levels deep in this one.
    pub fn inside(&self, stack: usize) -> Cx<'_> {
        Cx {
            depth: self.depth + stack + 1,
            ..*self
        }
    }

    /// Counts one more `parsed` placeholder put in; false when it is one too many.
    pub fn expand(&self) -> bool {
        let n = self.expansions.get() + 1;
        self.expansions.set(n);
        n <= MAX_EXPANSIONS
    }
}

/// Reads a whole text from the top.
pub(super) fn parse(input: &str, tags: &TagResolver, lenient: bool) -> Result<Component, Error> {
    let expansions = Cell::new(0);
    read(
        input,
        Cx {
            tags,
            lenient,
            depth: 0,
            expansions: &expansions,
        },
    )
}

/// Reads a text inside a context.
pub(super) fn read(input: &str, cx: Cx<'_>) -> Result<Component, Error> {
    if cx.depth > MAX_DEPTH {
        return Err(Error {
            position: 0,
            kind: ErrorKind::TooDeep,
        });
    }
    let mut tree = Tree::default();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => match bytes.get(i + 1) {
                Some(b'<') => {
                    tree.text("<");
                    i += 2;
                }
                Some(b'\\') => {
                    tree.text("\\");
                    i += 2;
                }
                _ => {
                    tree.text("\\");
                    i += 1;
                }
            },
            b'<' => i = tag(input, i, &mut tree, cx)?,
            _ => {
                let end = input[i..].find(['\\', '<']).map_or(input.len(), |n| i + n);
                tree.text(&input[i..end]);
                i = end;
            }
        }
    }
    tree.finish();
    Ok(render_root(tree.root))
}

/// Handles the tag at `at` and returns where the text goes on.
fn tag(input: &str, at: usize, tree: &mut Tree, cx: Cx<'_>) -> Result<usize, Error> {
    let fail = |kind| Error { position: at, kind };
    let (closing, self_closing, name, args, end) = match read_tag(input, at) {
        Scan::NotATag => {
            tree.text("<");
            return Ok(at + 1);
        }
        Scan::Malformed => {
            if cx.lenient {
                tree.text("<");
                return Ok(at + 1);
            }
            return Err(fail(ErrorKind::Malformed));
        }
        Scan::Tag {
            closing,
            self_closing,
            name,
            args,
            end,
        } => (closing, self_closing, name, args, end),
    };
    let raw = &input[at..end];
    let outcome = if closing {
        tree.close(&tags::canonical(&name), &name, cx.lenient)
    } else {
        open(tree, &name, &args, self_closing, cx)
    };
    match outcome {
        Ok(()) => {}
        Err(_) if cx.lenient => tree.text(raw),
        Err(kind) => return Err(fail(kind)),
    }
    Ok(end)
}

/// Opens or inserts what `<name:args>` is.
fn open(
    tree: &mut Tree,
    name: &str,
    args: &[String],
    self_closing: bool,
    cx: Cx<'_>,
) -> Result<(), ErrorKind> {
    let resolved = tags::resolve(name, args, cx, tree.stack.len()).map_err(|fail| match fail {
        Fail::Unknown => ErrorKind::UnknownTag(name.to_owned()),
        Fail::Bad(why) => ErrorKind::BadArgument {
            tag: name.to_owned(),
            why,
        },
        Fail::Nested(kind) => kind,
    })?;
    match resolved {
        Resolved::Text(text) => tree.text(&text),
        Resolved::Leaf(component) => tree.node(Node::Leaf(component)),
        Resolved::Reset => tree.reset(),
        // closed at once: it has nothing to draw
        Resolved::Wrap(_) if self_closing => {}
        Resolved::Wrap(wrap) => {
            if cx.depth + tree.stack.len() >= MAX_DEPTH {
                return Err(ErrorKind::TooDeep);
            }
            tree.stack.push(Frame {
                key: tags::canonical(name),
                wrap,
                children: Vec::new(),
            });
        }
    }
    Ok(())
}

/// What the scanner found at a `<`.
enum Scan {
    /// Something that is not a tag at all, such as `a < b`.
    NotATag,
    /// The start of a tag that does not end the way a tag does.
    Malformed,
    Tag {
        closing: bool,
        /// `<tag/>`: opened and closed at once.
        self_closing: bool,
        /// Lower case, with the `!` of `<!bold>`.
        name: String,
        args: Vec<String>,
        /// After the `>`.
        end: usize,
    },
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'#' | b'!' | b'?' | b'-')
}

/// Scans the tag that starts at the `<` at `start`.
fn read_tag(s: &str, start: usize) -> Scan {
    let bytes = s.as_bytes();
    let mut i = start + 1;
    let closing = bytes.get(i) == Some(&b'/');
    if closing {
        i += 1;
    }
    let name_start = i;
    while bytes.get(i).is_some_and(|&b| is_name_byte(b)) {
        i += 1;
    }
    if i == name_start {
        return Scan::NotATag;
    }
    let name = s[name_start..i].to_ascii_lowercase();
    let mut args = Vec::new();
    let mut self_closing = false;
    loop {
        match bytes.get(i) {
            None => return Scan::Malformed,
            Some(b'>') => {
                return Scan::Tag {
                    closing,
                    self_closing,
                    name,
                    args,
                    end: i + 1,
                };
            }
            Some(b'/') if !closing && bytes.get(i + 1) == Some(&b'>') => {
                self_closing = true;
                i += 1;
            }
            Some(b':') => {
                i += 1;
                match bytes.get(i) {
                    Some(&quote @ (b'\'' | b'"')) => {
                        i += 1;
                        let mut arg = String::new();
                        loop {
                            let Some(&b) = bytes.get(i) else {
                                return Scan::Malformed;
                            };
                            if b == quote {
                                i += 1;
                                break;
                            }
                            if b == b'\\'
                                && matches!(bytes.get(i + 1), Some(&n) if n == quote || n == b'\\')
                            {
                                arg.push(bytes[i + 1] as char);
                                i += 2;
                                continue;
                            }
                            // a quote and a backslash are ASCII, so this is on a boundary
                            let next = s[i..]
                                .find(['\\', quote as char])
                                .map_or(s.len(), |n| i + n.max(1));
                            arg.push_str(&s[i..next]);
                            i = next;
                        }
                        let slash = !closing
                            && bytes.get(i) == Some(&b'/')
                            && bytes.get(i + 1) == Some(&b'>');
                        if !slash && !matches!(bytes.get(i), Some(b':' | b'>')) {
                            return Scan::Malformed;
                        }
                        args.push(arg);
                    }
                    _ => {
                        // a `:` is not the end of the argument in `https://`
                        let mut end = i;
                        loop {
                            match s[end..].find([':', '>']) {
                                None => {
                                    end = s.len();
                                    break;
                                }
                                Some(n)
                                    if bytes[end + n] == b':'
                                        && s[end + n + 1..].starts_with("//") =>
                                {
                                    end += n + 1;
                                }
                                Some(n) => {
                                    end += n;
                                    break;
                                }
                            }
                        }
                        let mut arg = s[i..end].to_owned();
                        // `<a:b/>` ends the tag, it is not part of the argument
                        if !closing && bytes.get(end) == Some(&b'>') && arg.ends_with('/') {
                            arg.pop();
                            self_closing = true;
                        }
                        args.push(arg);
                        i = end;
                    }
                }
            }
            // `<red hello>`, `<3 you`: not a tag
            Some(_) => return Scan::NotATag,
        }
    }
}

// the tree

enum Node {
    Text(String),
    Leaf(Component),
    Wrap(Wrap, Vec<Node>),
}

struct Frame {
    key: String,
    wrap: Wrap,
    children: Vec<Node>,
}

#[derive(Default)]
struct Tree {
    root: Vec<Node>,
    stack: Vec<Frame>,
    /// Tags a `<reset>` closed, whose closing tags may still come.
    reset: Vec<String>,
}

impl Tree {
    fn current(&mut self) -> &mut Vec<Node> {
        match self.stack.last_mut() {
            Some(frame) => &mut frame.children,
            None => &mut self.root,
        }
    }

    fn node(&mut self, node: Node) {
        self.current().push(node);
    }

    fn text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let nodes = self.current();
        match nodes.last_mut() {
            Some(Node::Text(last)) => last.push_str(text),
            _ => nodes.push(Node::Text(text.to_owned())),
        }
    }

    /// Closes the innermost tag.
    fn pop(&mut self) {
        let Some(frame) = self.stack.pop() else {
            return;
        };
        self.node(Node::Wrap(frame.wrap, frame.children));
    }

    fn close(&mut self, key: &str, written: &str, lenient: bool) -> Result<(), ErrorKind> {
        let Some(found) = self.stack.iter().rposition(|f| f.key == key) else {
            if let Some(at) = self.reset.iter().position(|k| k == key) {
                self.reset.remove(at);
                return Ok(());
            }
            return Err(ErrorKind::UnmatchedClose(written.to_owned()));
        };
        if found + 1 != self.stack.len() && !lenient {
            return Err(ErrorKind::Mismatched(written.to_owned()));
        }
        while self.stack.len() > found {
            self.pop();
        }
        Ok(())
    }

    fn reset(&mut self) {
        while let Some(frame) = self.stack.last() {
            let key = frame.key.clone();
            self.reset.push(key);
            self.pop();
        }
    }

    fn finish(&mut self) {
        while !self.stack.is_empty() {
            self.pop();
        }
    }
}

// from the tree to a component

fn render_root(root: Vec<Node>) -> Component {
    let mut comps = render(&root, &mut None, false);
    match comps.len() {
        1 => comps.remove(0),
        _ => Component::empty().append_all(comps),
    }
}

/// The characters a gradient over `nodes` is spread across.
fn count(nodes: &[Node]) -> usize {
    nodes
        .iter()
        .map(|n| match n {
            Node::Text(t) => t.chars().count(),
            Node::Leaf(c) => length(c),
            Node::Wrap(_, children) => count(children),
        })
        .sum()
}

/// The characters of a component and its children; what is not text counts as one.
fn length(c: &Component) -> usize {
    let own = match &c.content {
        Content::Text(t) => t.chars().count(),
        _ => 1,
    };
    own + c.children.iter().map(length).sum::<usize>()
}

/// Colours a component that was put in: the characters of its text one by one, anything else
/// as a whole, unless it (or what it is in) has a colour already; it still takes its place in
/// the line.
fn paint_component(c: &Component, paint: &mut Paint, coloured: bool) -> Component {
    let coloured = coloured || c.style.color.is_some();
    // this component without its children, and the characters it is drawn as in front of them
    let mut out = Component {
        children: Vec::new(),
        ..c.clone()
    };
    match &c.content {
        Content::Text(t) if t.is_empty() => {}
        Content::Text(t) if coloured => paint.skip(t.chars().count()),
        Content::Text(t) => {
            out.content = Content::default();
            out.children = t
                .chars()
                .map(|ch| Component::text(ch.to_string()).color(paint.next()))
                .collect();
        }
        _ if coloured => {}
        _ => out.style.color = Some(paint.next()),
    }
    out.children.extend(
        c.children
            .iter()
            .map(|child| paint_component(child, paint, coloured)),
    );
    out
}

/// `paint` colours the text it reaches, unless `coloured` says something inside has a colour of
/// its own; it moves along either way, so that the colours after it are where they would be.
fn render(nodes: &[Node], paint: &mut Option<Paint>, coloured: bool) -> Vec<Component> {
    let mut out = Vec::new();
    for node in nodes {
        match node {
            Node::Text(text) => match paint {
                Some(paint) if !coloured => {
                    out.extend(
                        text.chars()
                            .map(|c| Component::text(c.to_string()).color(paint.next())),
                    );
                }
                Some(paint) => {
                    paint.skip(text.chars().count());
                    out.push(Component::text(text.clone()));
                }
                None => out.push(Component::text(text.clone())),
            },
            Node::Leaf(component) => out.push(match paint {
                Some(paint) => paint_component(component, paint, coloured),
                None => component.clone(),
            }),
            Node::Wrap(Wrap::Style(style), children) => {
                let coloured = coloured || style.color.is_some();
                let inner = render(children, paint, coloured);
                out.push(wrap(style.clone(), inner));
            }
            Node::Wrap(Wrap::Paint(painter), children) => {
                let total = count(children);
                let mut inner_paint = Some(painter.over(total));
                let inner = render(children, &mut inner_paint, false);
                if let Some(paint) = paint {
                    paint.skip(total);
                }
                out.push(wrap(crate::Style::default(), inner));
            }
        }
    }
    out
}

/// A component in `style` with `children` in it; a single bare text is the component itself.
fn wrap(style: crate::Style, mut children: Vec<Component>) -> Component {
    if children.len() == 1 && children[0].style.is_empty() && children[0].children.is_empty() {
        let only = children.remove(0);
        return Component {
            content: only.content,
            style,
            children: Vec::new(),
        };
    }
    Component::empty().style(style).append_all(children)
}
