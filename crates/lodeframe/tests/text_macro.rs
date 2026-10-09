// SPDX-License-Identifier: Apache-2.0 OR MIT
//! `text!` gives what the run-time parser gives, and keeps what a user typed out of the tags.

use lodeframe::text;
use lodeframe::text::{
    Color, Component, Content,
    mini::{self, TagResolver},
};

/// The text of a component and its children, and whether any of it is styled.
fn plain(c: &Component) -> (String, bool) {
    let mut text = String::new();
    let mut styled = false;
    fn walk(c: &Component, text: &mut String, styled: &mut bool) {
        if let Content::Text(t) = &c.content {
            text.push_str(t);
        }
        *styled |= !c.style.is_empty();
        c.children
            .iter()
            .for_each(|child| walk(child, text, styled));
    }
    walk(c, &mut text, &mut styled);
    (text, styled)
}

#[test]
fn a_text_without_placeholders_is_what_the_parser_gives() {
    let src = "<gradient:gold:red>Welcome</gradient> to <bold>lodeframe</bold>";
    let expected = mini::parse(src).unwrap();
    for _ in 0..3 {
        assert_eq!(
            text!("<gradient:gold:red>Welcome</gradient> to <bold>lodeframe</bold>"),
            expected
        );
    }
    assert_eq!(text!("plain"), Component::text("plain"));
    assert_eq!(text!(""), Component::empty());
}

#[test]
fn each_use_gets_a_value_of_its_own() {
    fn greeting() -> Component {
        text!("<red>hi</red>")
    }
    let mut first = greeting();
    first.children.push(Component::text("changed"));
    assert_eq!(greeting(), mini::parse("<red>hi</red>").unwrap());
}

#[test]
fn a_placeholder_is_the_variable_of_that_name() {
    let name = "Alice";
    let count = 3;
    let line = text!("<green>{name}</green> joined (<yellow>{count}</yellow> online)");
    let tags = TagResolver::new()
        .component("name", name)
        .component("count", count);
    let expected = mini::parse_with(
        "<green><name></green> joined (<yellow><count></yellow> online)",
        &tags,
    )
    .unwrap();
    assert_eq!(line, expected);
    assert_eq!(plain(&line).0, "Alice joined (3 online)");
}

#[test]
fn a_named_argument_and_braces() {
    let who = "not this one";
    let line = text!("{{hello}} {who} {n}", who = "Bob".to_uppercase(), n = 1 + 2);
    assert_eq!(plain(&line).0, "{hello} BOB 3");
    // the variable of the same name was not touched
    assert_eq!(who, "not this one");
    assert_eq!(plain(&text!("a }} b {{ c")).0, "a } b { c");
}

#[test]
fn a_name_can_be_used_more_than_once_even_for_a_string() {
    let name = String::from("Ann");
    let line = text!("{name}, {name}, {name}");
    assert_eq!(plain(&line).0, "Ann, Ann, Ann");
    // a name is evaluated once however often the text has it
    let again = String::from("x");
    let both = text!("{again}{again}");
    assert_eq!(plain(&both).0, "xx");
}

#[test]
fn numbers_bools_chars_strings_and_components_go_in() {
    let (i, f, b, c) = (-7_i64, 1.5_f64, true, 'é');
    let owned = String::from("owned");
    let styled = Component::text("styled").color(Color::Gold).bold();
    let line = text!(
        "{i} {f} {b} {c} {owned_ref} {styled_ref}",
        owned_ref = &owned,
        styled_ref = &styled
    );
    assert_eq!(plain(&line).0, "-7 1.5 true é owned styled");
    // a component keeps its own style
    let line = text!("<red>{styled}</red>", styled = &styled);
    let expected = mini::parse_with(
        "<red><styled></red>",
        &TagResolver::new().component("styled", styled.clone()),
    )
    .unwrap();
    assert_eq!(line, expected);
    assert!(plain(&line).1);
}

#[test]
fn what_a_user_typed_is_not_read_as_tags() {
    for typed in [
        "<red>x</red>",
        "</bold>",
        "<click:run_command:/op me>click</click>",
        r"\<",
        "<reset>",
        "{name}",
    ] {
        let line = text!("Hi {typed}!");
        assert_eq!(plain(&line), (format!("Hi {typed}!"), false), "{typed}");
    }
}

#[test]
fn a_placeholder_in_a_gradient_is_coloured_as_it_is_at_run_time() {
    let n = "XYZ";
    let line = text!("<gradient:#000000:#ffffff>a{n}b</gradient>");
    let expected = mini::parse_with(
        "<gradient:#000000:#ffffff>a<n>b</gradient>",
        &TagResolver::new().component("n", n),
    )
    .unwrap();
    assert_eq!(line, expected);
    assert_eq!(plain(&line).0, "aXYZb");
}

#[test]
fn a_placeholder_can_be_in_the_text_of_a_hover() {
    let who = "Bob";
    let line = text!("<hover:show_text:'tip for {who}'>x</hover>");
    let expected = mini::parse_with(
        "<hover:show_text:'tip for <who>'>x</hover>",
        &TagResolver::new().component("who", who),
    )
    .unwrap();
    assert_eq!(line, expected);
}

#[test]
fn raw_strings_and_escapes_are_read_as_the_parser_reads_them() {
    let x = "v";
    assert_eq!(plain(&text!(r"\<red> {x}")), ("<red> v".to_owned(), false));
    assert_eq!(plain(&text!("\\<red> {x}")), ("<red> v".to_owned(), false));
    assert_eq!(text!("line\none"), mini::parse("line\none").unwrap());
}

#[test]
fn it_is_an_expression_that_works_where_a_component_is_wanted() {
    fn takes(c: impl Into<Component>) -> Component {
        c.into()
    }
    let n = 2;
    assert_eq!(takes(text!("{n}")), text!("{n}"));
    let v: Vec<Component> = (1..=2).map(|n| text!("<b>#{n}</b>")).collect();
    assert_eq!(v.len(), 2);
    assert_ne!(v[0], v[1]);
    // the module of the same name is still there
    let _: text::Component = text!("x");
}
