// SPDX-License-Identifier: Apache-2.0 OR MIT
use super::*;
use crate::{ClickEvent, Color, Component, Content, Decoration, HoverEvent, MAX_DEPTH, Style};

// What a component looks like, whatever shape it is built in: its text and the style each
// stretch of it is drawn in, with the styles of the components around it put in.

#[derive(Debug, PartialEq)]
enum Item {
    Text(String),
    Other(String),
}

type Run = (Item, Style);

fn inherit(parent: &Style, own: &Style) -> Style {
    Style {
        color: own.color.or(parent.color),
        shadow_color: own.shadow_color.or(parent.shadow_color),
        bold: own.bold.or(parent.bold),
        italic: own.italic.or(parent.italic),
        underlined: own.underlined.or(parent.underlined),
        strikethrough: own.strikethrough.or(parent.strikethrough),
        obfuscated: own.obfuscated.or(parent.obfuscated),
        insertion: own.insertion.clone().or_else(|| parent.insertion.clone()),
        font: own.font.clone().or_else(|| parent.font.clone()),
        click: own.click.clone().or_else(|| parent.click.clone()),
        hover: own
            .hover
            .as_ref()
            .map(|h| {
                Box::new(match &**h {
                    HoverEvent::ShowText(c) => HoverEvent::ShowText(look(c)),
                    HoverEvent::ShowEntity {
                        entity_type,
                        uuid,
                        name,
                    } => HoverEvent::ShowEntity {
                        entity_type: entity_type.clone(),
                        uuid: *uuid,
                        name: name.as_ref().map(look),
                    },
                    other => other.clone(),
                })
            })
            .or_else(|| parent.hover.clone()),
    }
}

/// A component made the same way whatever shape the original was: one text to a style.
fn look(c: &Component) -> Component {
    Component::empty().append_all(runs(c).into_iter().map(|(item, style)| match item {
        Item::Text(t) => Component::text(t).style(style),
        Item::Other(t) => Component::text(t).style(style),
    }))
}

fn flatten(c: &Component, parent: &Style, out: &mut Vec<Run>) {
    let style = inherit(parent, &c.style);
    match &c.content {
        Content::Text(t) if t.is_empty() => {}
        Content::Text(t) => match out.last_mut() {
            Some((Item::Text(last), s)) if *s == style => last.push_str(t),
            _ => out.push((Item::Text(t.clone()), style.clone())),
        },
        other => out.push((Item::Other(format!("{other:?}")), style.clone())),
    }
    for child in &c.children {
        flatten(child, &style, out);
    }
}

fn runs(c: &Component) -> Vec<Run> {
    let mut out = Vec::new();
    flatten(c, &Style::default(), &mut out);
    out
}

/// The text of a component, without its style.
fn plain(c: &Component) -> String {
    runs(c)
        .into_iter()
        .map(|(item, _)| match item {
            Item::Text(t) | Item::Other(t) => t,
        })
        .collect()
}

fn text_run(text: &str, style: Style) -> Run {
    (Item::Text(text.into()), style)
}

fn red() -> Style {
    Style {
        color: Some(Color::Red),
        ..Style::default()
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Style {
    Style {
        color: Some(Color::Rgb(r, g, b)),
        ..Style::default()
    }
}

fn kind(input: &str) -> (usize, ErrorKind) {
    let e = parse(input).unwrap_err();
    (e.position, e.kind)
}

// reading

#[test]
fn text_without_tags_is_text() {
    assert_eq!(parse("hello").unwrap(), Component::text("hello"));
    assert_eq!(parse("").unwrap(), Component::empty());
    assert_eq!(
        parse_lenient("héllo wörld ✓"),
        Component::text("héllo wörld ✓")
    );
}

#[test]
fn colours_by_name_by_hex_and_by_color_tag() {
    for (input, expected) in [
        ("<red>hi</red>", Color::Red),
        ("<RED>hi</Red>", Color::Red),
        ("<dark_blue>hi", Color::DarkBlue),
        ("<grey>hi", Color::Gray),
        ("<dark_grey>hi", Color::DarkGray),
        ("<#ff8000>hi", Color::Rgb(255, 128, 0)),
        ("<#FF8000>hi</#ff8000>", Color::Rgb(255, 128, 0)),
        ("<color:gold>hi</color>", Color::Gold),
        ("<c:#00ff00>hi</c>", Color::Rgb(0, 255, 0)),
        ("<colour:aqua>hi", Color::Aqua),
    ] {
        assert_eq!(
            parse(input).unwrap(),
            Component::text("hi").color(expected),
            "{input}"
        );
    }
}

#[test]
fn decorations_on_off_and_by_alias() {
    for (input, decoration) in [
        ("<bold>", Decoration::Bold),
        ("<b>", Decoration::Bold),
        ("<italic>", Decoration::Italic),
        ("<i>", Decoration::Italic),
        ("<em>", Decoration::Italic),
        ("<underlined>", Decoration::Underlined),
        ("<u>", Decoration::Underlined),
        ("<strikethrough>", Decoration::Strikethrough),
        ("<st>", Decoration::Strikethrough),
        ("<obfuscated>", Decoration::Obfuscated),
        ("<obf>", Decoration::Obfuscated),
    ] {
        let on = parse(&format!("{input}x")).unwrap();
        assert_eq!(
            on,
            Component::text("x").decorate(decoration, true),
            "{input}"
        );
        let name = input.trim_matches(['<', '>']);
        for off in [format!("<!{name}>x"), format!("<{name}:false>x")] {
            assert_eq!(
                parse(&off).unwrap(),
                Component::text("x").decorate(decoration, false),
                "{off}"
            );
        }
        assert_eq!(
            parse(&format!("<{name}:true>x")).unwrap(),
            Component::text("x").decorate(decoration, true)
        );
    }
}

#[test]
fn tags_nest_and_close_the_innermost_of_their_name() {
    assert_eq!(
        parse("<red>a<bold>b</bold>c</red>d").unwrap(),
        Component::empty()
            .append(Component::empty().color(Color::Red).append_all([
                Component::text("a"),
                Component::text("b").bold(),
                Component::text("c"),
            ]))
            .append("d")
    );
    // an alias closes what the other name opened
    assert_eq!(
        runs(&parse("<b>a</bold>b").unwrap()),
        [
            text_run(
                "a",
                Style {
                    bold: Some(true),
                    ..Style::default()
                }
            ),
            text_run("b", Style::default())
        ]
    );
    // the inner one first
    assert_eq!(
        runs(&parse("<red>a<red>b</red>c</red>").unwrap()),
        [text_run("abc", red())]
    );
}

#[test]
fn a_tag_that_is_not_closed_is_closed_where_the_text_ends() {
    for input in ["<red>Error", "<red>Error</red>"] {
        assert_eq!(
            parse(input).unwrap(),
            Component::text("Error").color(Color::Red)
        );
        assert_eq!(parse_lenient(input), parse(input).unwrap());
    }
}

#[test]
fn escapes_make_a_tag_text() {
    let c = parse(r"a \<red> b \\ c \x").unwrap();
    assert_eq!(c, Component::text(r"a <red> b \ c \x"));
    // the backslash is only special in front of `<` and `\`; at the end it stays
    assert_eq!(parse(r"end\").unwrap(), Component::text(r"end\"));
}

#[test]
fn a_lone_angle_bracket_is_text() {
    for input in ["a < b", "1 <3 you", "x<", "<>", "< red>", "a <  > b", "</>"] {
        assert_eq!(parse(input).unwrap(), Component::text(input), "{input}");
    }
}

#[test]
fn reset_closes_everything_open_and_its_closing_tags_are_forgiven() {
    let c = parse("<red><bold>a<reset>b</bold></red>c").unwrap();
    assert_eq!(
        runs(&c),
        [
            text_run(
                "a",
                Style {
                    color: Some(Color::Red),
                    bold: Some(true),
                    ..Style::default()
                }
            ),
            text_run("bc", Style::default()),
        ]
    );
    // a closing tag that nothing opened is still an error
    assert_eq!(
        kind("<reset>x</red>"),
        (8, ErrorKind::UnmatchedClose("red".into()))
    );
    assert_eq!(
        kind("<reset:1>"),
        (
            0,
            ErrorKind::BadArgument {
                tag: "reset".into(),
                why: "takes no arguments"
            }
        )
    );
}

#[test]
fn hover_and_click() {
    let c = parse("<hover:show_text:'<red>tip'><click:run_command:/say hi>x").unwrap();
    assert_eq!(
        runs(&c),
        [text_run(
            "x",
            Style {
                click: Some(ClickEvent::RunCommand("/say hi".into())),
                hover: Some(Box::new(HoverEvent::ShowText(look(
                    &Component::text("tip").color(Color::Red)
                )))),
                ..Style::default()
            }
        )]
    );

    for (input, event) in [
        (
            "<click:open_url:https://example.com/a?b=c>",
            ClickEvent::OpenUrl("https://example.com/a?b=c".into()),
        ),
        (
            "<click:suggest_command:'/msg a:b'>",
            ClickEvent::SuggestCommand("/msg a:b".into()),
        ),
        (
            "<click:copy_to_clipboard:text>",
            ClickEvent::CopyToClipboard("text".into()),
        ),
        ("<click:change_page:3>", ClickEvent::ChangePage(3)),
    ] {
        assert_eq!(
            parse(&format!("{input}x")).unwrap(),
            Component::text("x").click(event),
            "{input}"
        );
    }
    for bad in [
        "<click:change_page:0>",
        "<click:change_page:x>",
        "<click:dance:now>",
        "<click:run_command>",
        "<click>",
        "<hover:show_text>",
        "<hover:dance:x>",
    ] {
        assert!(
            matches!(kind(bad).1, ErrorKind::BadArgument { .. }),
            "{bad}"
        );
    }
}

#[test]
fn hover_shows_an_item_or_an_entity() {
    let hover = |input: &str| match parse(&format!("{input}x")).unwrap().style.hover {
        Some(h) => *h,
        None => panic!("no hover in {input}"),
    };
    assert_eq!(
        hover("<hover:show_item:diamond_sword>"),
        HoverEvent::ShowItem {
            id: "diamond_sword".into(),
            count: 1
        }
    );
    for input in [
        "<hover:show_item:diamond_sword:2>",
        "<hover:show_item:minecraft:diamond_sword:2>",
        "<hover:show_item:'minecraft:diamond_sword':2>",
    ] {
        let HoverEvent::ShowItem { id, count } = hover(input) else {
            panic!("{input}")
        };
        assert!(id.ends_with("diamond_sword") && count == 2, "{input}: {id}");
    }
    assert_eq!(
        hover("<hover:show_item:minecraft:stone>"),
        HoverEvent::ShowItem {
            id: "minecraft:stone".into(),
            count: 1
        }
    );
    let uuid = "00000000-0000-0000-0000-000000000001";
    assert_eq!(
        hover(&format!(
            "<hover:show_entity:minecraft:pig:{uuid}:'<gold>Piggy'>"
        )),
        HoverEvent::ShowEntity {
            entity_type: "minecraft:pig".into(),
            uuid: 1,
            name: Some(Component::text("Piggy").color(Color::Gold)),
        }
    );
    assert_eq!(
        hover("<hover:show_entity:'minecraft:pig':00000000000000000000000000000002>"),
        HoverEvent::ShowEntity {
            entity_type: "minecraft:pig".into(),
            uuid: 2,
            name: None,
        }
    );
    for bad in [
        "<hover:show_item>",
        "<hover:show_item:stone:0>",
        "<hover:show_item:stone:1:2:3>",
        "<hover:show_entity:pig>",
        "<hover:show_entity:pig:not-a-uuid>",
    ] {
        assert!(
            matches!(kind(bad).1, ErrorKind::BadArgument { .. }),
            "{bad}"
        );
    }
}

#[test]
fn insertion_font_and_shadow() {
    assert_eq!(
        parse("<insertion:'hello world'>x").unwrap(),
        Component::text("x").insertion("hello world")
    );
    assert_eq!(
        parse("<font:minecraft:uniform>x").unwrap(),
        Component::text("x").font("minecraft:uniform")
    );
    for (input, argb) in [
        ("<shadow:red>", 0x3FFF_5555),
        ("<shadow:red:0.5>", 0x7FFF_5555),
        ("<shadow:red:1>", 0xFFFF_5555),
        ("<shadow:#102030:0>", 0x0010_2030),
        ("<shadow:#ff000080>", 0x80FF_0000),
        ("<!shadow>", 0),
    ] {
        assert_eq!(
            parse(&format!("{input}x")).unwrap(),
            Component::text("x").shadow_color(argb),
            "{input}"
        );
    }
    for bad in [
        "<shadow>",
        "<shadow:red:2>",
        "<shadow:#ff000080:1>",
        "<shadow:nope>",
        "<font>",
        "<insertion>",
    ] {
        assert!(
            matches!(kind(bad).1, ErrorKind::BadArgument { .. }),
            "{bad}"
        );
    }
}

#[test]
fn key_lang_and_newline() {
    assert_eq!(
        parse("<key:key.jump>").unwrap(),
        Component::keybind("key.jump")
    );
    assert_eq!(
        parse("<lang:chat.type.text:'<red>a':b>").unwrap(),
        Component::translatable("chat.type.text")
            .arg(Component::text("a").color(Color::Red))
            .arg("b")
    );
    assert_eq!(
        parse("<tr:some.key>").unwrap(),
        Component::translatable("some.key")
    );
    assert_eq!(
        parse("<lang_or:some.key:'a fallback':x>").unwrap(),
        Component::translatable("some.key")
            .fallback("a fallback")
            .arg("x")
    );
    assert_eq!(
        parse("a<newline>b<br>c").unwrap(),
        Component::text("a\nb\nc")
    );
    for bad in ["<key>", "<key:a:b>", "<lang>", "<lang_or:k>", "<newline:2>"] {
        assert!(
            matches!(kind(bad).1, ErrorKind::BadArgument { .. }),
            "{bad}"
        );
    }
}

// colours along the text

#[test]
fn a_gradient_colours_each_character() {
    let c = parse("<gradient:#ff0000:#0000ff>abc").unwrap();
    assert_eq!(
        c,
        Component::empty().append_all([
            Component::text("a").color(Color::Rgb(255, 0, 0)),
            Component::text("b").color(Color::Rgb(128, 0, 128)),
            Component::text("c").color(Color::Rgb(0, 0, 255)),
        ])
    );
    assert_eq!(plain(&c), "abc");
    // one character gets the first colour, and there are no colours by default
    assert_eq!(
        runs(&parse("<gradient:red:blue>x").unwrap()),
        [text_run("x", rgb(255, 85, 85))]
    );
    assert_eq!(
        runs(&parse("<gradient>ab").unwrap()),
        [
            text_run("a", rgb(255, 255, 255)),
            text_run("b", rgb(0, 0, 0))
        ]
    );
    // characters are characters, not bytes
    assert_eq!(runs(&parse("<gradient>é✓").unwrap()).len(), 2);
}

#[test]
fn a_gradient_goes_through_all_its_colours_and_can_be_shifted() {
    let colours = |input: &str| -> Vec<Style> {
        runs(&parse(input).unwrap())
            .into_iter()
            .map(|(_, s)| s)
            .collect()
    };
    assert_eq!(
        colours("<gradient:#ff0000:#00ff00:#0000ff>abcde"),
        [
            rgb(255, 0, 0),
            rgb(128, 128, 0),
            rgb(0, 255, 0),
            rgb(0, 128, 128),
            rgb(0, 0, 255)
        ]
    );
    assert_eq!(
        colours("<gradient:#000000:#ffffff:0.5>abc"),
        [rgb(128, 128, 128), rgb(255, 255, 255), rgb(128, 128, 128)]
    );
}

#[test]
fn a_colour_inside_a_gradient_wins_and_the_gradient_goes_on() {
    let c = parse("<gradient:#000000:#ffffff><red>b</red>c").unwrap();
    assert_eq!(
        runs(&c),
        [text_run("b", red()), text_run("c", rgb(255, 255, 255))]
    );
    // decorations inside keep the gradient colour
    let c = parse("<gradient:red:blue>a<b>b</b>").unwrap();
    let looks = runs(&c);
    assert_eq!(looks.len(), 2);
    assert_eq!(looks[1].1.bold, Some(true));
    assert_eq!(looks[1].1.color, Some(Color::Rgb(85, 85, 255)));
    // text a placeholder puts in is coloured like the rest
    let tags = TagResolver::new().unparsed("p", "xy");
    let c = parse_with("<gradient:#000000:#ffffff>a<p>", &tags).unwrap();
    assert_eq!(runs(&c).len(), 3);
}

#[test]
fn a_rainbow_starts_at_red_and_can_run_backwards_and_shifted() {
    let colours = |input: &str| -> Vec<Option<Color>> {
        runs(&parse(input).unwrap())
            .into_iter()
            .map(|(_, s)| s.color)
            .collect()
    };
    let forward = colours("<rainbow>abcdef");
    let backward = colours("<rainbow:!>abcdef");
    assert_eq!(forward.len(), 6);
    assert_eq!(forward[0], Some(Color::Rgb(255, 0, 0)));
    // the same colours, from the last to the first
    for k in 0..6 {
        assert_eq!(backward[k], forward[5 - k], "{k}");
    }
    // a phase is in tenths of the way round, and goes with the `!`
    let shifted = colours("<rainbow:5>abcdef");
    assert_ne!(shifted, forward);
    assert_eq!(shifted[0], forward[3]);
    assert_eq!(colours("<rainbow:!5>abc").len(), 3);
    assert_eq!(colours("<rainbow>x"), [Some(Color::Rgb(255, 0, 0))]);
}

#[test]
fn a_transition_is_one_colour() {
    assert_eq!(
        runs(&parse("<transition:#000000:#ffffff:0.5>xy").unwrap()),
        [text_run("xy", rgb(128, 128, 128))]
    );
    assert_eq!(
        runs(&parse("<transition:red:blue:0>x").unwrap()),
        [text_run("x", rgb(255, 85, 85))]
    );
    assert_eq!(
        runs(&parse("<transition:red:blue:1>x").unwrap()),
        [text_run("x", rgb(85, 85, 255))]
    );
    // a negative phase counts from the end
    assert_eq!(
        runs(&parse("<transition:red:blue:-0.25>x").unwrap()),
        [text_run("x", rgb(213, 85, 128))]
    );
    // no colours: white to black
    assert_eq!(
        runs(&parse("<transition:0.5>x").unwrap()),
        [text_run("x", rgb(128, 128, 128))]
    );
    assert_eq!(
        runs(&parse("<transition>x").unwrap()),
        [text_run("x", rgb(255, 255, 255))]
    );
    for bad in [
        "<transition:red>",
        "<transition:red:blue:2>",
        "<gradient:red>",
        "<gradient:nope:red>",
        "<gradient:red:blue:2>",
        "<gradient:red:blue:nan>",
        "<rainbow:x>",
        "<rainbow:0.5>",
        "<rainbow:!:1>",
    ] {
        assert!(
            matches!(kind(bad).1, ErrorKind::BadArgument { .. }),
            "{bad}"
        );
    }
}

// what Adventure does with them, read from its source

#[test]
fn a_decoration_is_on_unless_it_says_false() {
    for input in ["<b>x", "<b:true>x", "<b:maybe>x", "<b:1>x"] {
        assert_eq!(
            parse(input).unwrap(),
            Component::text("x").bold(),
            "{input}"
        );
    }
    assert_eq!(
        parse("<b:FALSE>x").unwrap(),
        Component::text("x").decorate(Decoration::Bold, false)
    );
}

#[test]
fn a_tag_ending_in_a_slash_is_opened_and_closed_at_once() {
    assert_eq!(
        parse("a<newline/>b<br/>c").unwrap(),
        Component::text("a\nb\nc")
    );
    assert_eq!(
        parse("<key:key.jump/>").unwrap(),
        Component::keybind("key.jump")
    );
    // nothing is inside it to draw
    assert_eq!(parse("<red/>x").unwrap(), Component::text("x"));
    assert_eq!(
        runs(&parse("<red/>x<b:true/>y").unwrap()),
        [text_run("xy", Style::default())]
    );
    // a link that ends in a slash is such a tag too
    assert_eq!(
        parse("<click:open_url:https://example.com/>go").unwrap(),
        Component::text("go")
    );
    assert_eq!(
        parse("<click:open_url:'https://example.com/'>go").unwrap(),
        Component::text("go").click(ClickEvent::OpenUrl("https://example.com/".into()))
    );
    // a closing tag has no such form: it is text
    assert_eq!(parse("</red/>").unwrap(), Component::text("</red/>"));
}

#[test]
fn a_colon_before_two_slashes_does_not_end_an_argument() {
    assert_eq!(
        parse("<click:open_url:https://a.b:80/c>x").unwrap(),
        Component::text("x").click(ClickEvent::OpenUrl("https://a.b:80/c".into()))
    );
    // `insert` is the name; `insertion` is taken too
    for tag in ["insert", "insertion"] {
        assert_eq!(
            parse(&format!("<{tag}:hi>x")).unwrap(),
            Component::text("x").insertion("hi")
        );
    }
}

#[test]
fn a_gradient_takes_in_what_is_put_in_and_a_colour_of_its_own_is_kept() {
    let tags = TagResolver::new()
        .component("item", Component::text("XY"))
        .component("gold", Component::text("XY").color(Color::Gold))
        .component("key", Component::keybind("key.jump"));
    let colours = |input: &str| -> Vec<Run> { runs(&parse_with(input, &tags).unwrap()) };
    // XY are two of the four characters
    assert_eq!(
        colours("<gradient:#000000:#ffffff>a<item>b"),
        [
            text_run("a", rgb(0, 0, 0)),
            text_run("X", rgb(85, 85, 85)),
            text_run("Y", rgb(170, 170, 170)),
            text_run("b", rgb(255, 255, 255)),
        ]
    );
    // its own colour is kept, and the place it takes is kept too
    assert_eq!(
        colours("<gradient:#000000:#ffffff>a<gold>b"),
        [
            text_run("a", rgb(0, 0, 0)),
            text_run(
                "XY",
                Style {
                    color: Some(Color::Gold),
                    ..Style::default()
                }
            ),
            text_run("b", rgb(255, 255, 255)),
        ]
    );
    // what is not text is one place, and gets its colour whole
    let looks = colours("<gradient:#000000:#ffffff>a<key>b");
    assert_eq!(looks.len(), 3);
    assert!(matches!(looks[1].0, Item::Other(_)));
    assert_eq!(looks[1].1, rgb(128, 128, 128));
}

// errors

#[test]
fn a_strict_parse_says_what_and_where() {
    assert_eq!(
        kind("<red>hi</bold>"),
        (7, ErrorKind::UnmatchedClose("bold".into()))
    );
    assert_eq!(
        kind("<red><bold>a</red>"),
        (12, ErrorKind::Mismatched("red".into()))
    );
    assert_eq!(kind("ab<nope>"), (2, ErrorKind::UnknownTag("nope".into())));
    assert_eq!(kind("x<!red>"), (1, ErrorKind::UnknownTag("!red".into())));
    assert_eq!(kind("<red"), (0, ErrorKind::Malformed));
    assert_eq!(kind("a<hover:show_text:'abc>"), (1, ErrorKind::Malformed));
    assert_eq!(kind("<hover:show_text:'abc'x>"), (0, ErrorKind::Malformed));
    assert_eq!(
        kind("a <click:dance:x>"),
        (
            2,
            ErrorKind::BadArgument {
                tag: "click".into(),
                why: "unknown click action"
            }
        )
    );
    // what is wrong inside a hover is reported at the hover
    assert_eq!(
        kind("x<hover:show_text:'<nope>'>y"),
        (1, ErrorKind::UnknownTag("nope".into()))
    );
    let message = parse("<red>hi</bold>").unwrap_err().to_string();
    assert_eq!(message, "at byte 7: </bold> has nothing to close");
}

#[test]
fn a_lenient_parse_keeps_what_is_wrong_as_text() {
    for (input, expected) in [
        ("a <nope> b", "a <nope> b"),
        ("<red>hi</bold>", "hi</bold>"),
        ("x <red", "x <red"),
        ("<hover:show_text:'abc>", "<hover:show_text:'abc>"),
        ("<click:dance:x>go", "<click:dance:x>go"),
        ("<gradient:red:blue:9>ab", "<gradient:red:blue:9>ab"),
    ] {
        let c = parse_lenient(input);
        assert_eq!(plain(&c), expected, "{input}");
        assert!(
            runs(&c)
                .iter()
                .all(|(_, s)| s.is_empty() || s.color.is_some())
        );
    }
    // a closing tag that skips over others closes them
    let c = parse_lenient("<red><bold>a</red>b");
    assert_eq!(
        runs(&c),
        [
            text_run(
                "a",
                Style {
                    bold: Some(true),
                    ..red()
                }
            ),
            text_run("b", Style::default())
        ]
    );
    // the good part is still read
    assert_eq!(
        runs(&parse_lenient("<red>a<nope>b")),
        [text_run("a<nope>b", red())]
    );
}

// placeholders and tags of your own

#[test]
fn what_a_user_typed_is_never_read_as_tags() {
    let typed = [
        "<red>x</red>",
        "</bold>",
        "<click:run_command:/op me>click</click>",
        r"\<",
        "<hover:show_text:'x'>",
        "<reset>",
        "<name>",
        "<",
    ];
    for text in typed {
        let tags = TagResolver::new().unparsed("name", text);
        for c in [
            parse_with("Hi <name>!", &tags).unwrap(),
            parse_lenient_with("Hi <name>!", &tags),
            parse_with("<bold>Hi <NAME>!", &tags)
                .map(|c| Component::text(plain(&c)))
                .unwrap(),
        ] {
            assert_eq!(plain(&c), format!("Hi {text}!"), "{text}");
        }
        let c = parse_with("Hi <name>!", &tags).unwrap();
        assert_eq!(c, Component::text(format!("Hi {text}!")));
        // and the same for a component, which keeps its own style but gains no tags
        let c = parse_with(
            "<bold><item></bold>",
            &TagResolver::new().component("item", Component::text(text).color(Color::Gold)),
        )
        .unwrap();
        assert_eq!(
            runs(&c),
            [text_run(
                text,
                Style {
                    color: Some(Color::Gold),
                    bold: Some(true),
                    ..Style::default()
                }
            )]
        );
    }
}

#[test]
fn a_parsed_placeholder_is_read_as_tags_with_the_same_resolver() {
    let tags = TagResolver::new()
        .parsed("greeting", "<green>hello <who>")
        .unparsed("who", "<red>you");
    let c = parse_with("<bold><greeting>!", &tags).unwrap();
    assert_eq!(
        runs(&c),
        [
            text_run(
                "hello <red>you",
                Style {
                    color: Some(Color::Green),
                    bold: Some(true),
                    ..Style::default()
                }
            ),
            text_run(
                "!",
                Style {
                    bold: Some(true),
                    ..Style::default()
                }
            ),
        ]
    );
}

#[test]
fn a_parsed_placeholder_that_contains_itself_stops() {
    let tags = TagResolver::new().parsed("a", "x<a><a>");
    let e = parse_with("<a>", &tags).unwrap_err();
    assert!(matches!(
        e.kind,
        ErrorKind::TooDeep | ErrorKind::TooManyExpansions
    ));
    // and a lenient parse is done too, with the rest as text
    let c = parse_lenient_with("<a>", &tags);
    assert!(plain(&c).contains("<a>"));
}

#[test]
fn tags_of_your_own() {
    let tags = TagResolver::new()
        .insert("shout", |args| match args {
            [text] => Some(Component::text(text.to_uppercase()).bold()),
            _ => None,
        })
        .style("loud", |_| {
            Some(Style {
                bold: Some(true),
                color: Some(Color::Red),
                ..Style::default()
            })
        });
    assert_eq!(
        parse_with("<shout:hey>", &tags).unwrap(),
        Component::text("HEY").bold()
    );
    assert_eq!(
        runs(&parse_with("<loud>x</loud>y", &tags).unwrap()),
        [
            text_run(
                "x",
                Style {
                    bold: Some(true),
                    ..red()
                }
            ),
            text_run("y", Style::default())
        ]
    );
    assert!(matches!(
        parse_with("<shout>", &tags).unwrap_err().kind,
        ErrorKind::BadArgument { .. }
    ));
    assert_eq!(plain(&parse_lenient_with("<shout>x", &tags)), "<shout>x");
}

#[test]
fn the_first_tag_of_a_name_wins_and_yours_win_over_the_standard_ones() {
    let first = TagResolver::new().unparsed("a", "1").unparsed("a", "2");
    assert_eq!(parse_with("<a>", &first).unwrap(), Component::text("1"));
    let joined = TagResolver::new().unparsed("a", "mine").and(first);
    assert_eq!(parse_with("<A>", &joined).unwrap(), Component::text("mine"));
    let over = TagResolver::new().unparsed("red", "not a colour");
    assert_eq!(
        parse_with("<red>", &over).unwrap(),
        Component::text("not a colour")
    );
    assert_eq!(format!("{joined:?}"), r#"["a", "a", "a"]"#);
}

#[test]
fn a_placeholder_takes_no_arguments() {
    let tags = TagResolver::new().unparsed("a", "x");
    assert!(matches!(
        parse_with("<a:1>", &tags).unwrap_err().kind,
        ErrorKind::BadArgument { .. }
    ));
}

#[test]
#[should_panic(expected = "a tag name is lower case")]
fn a_name_that_is_not_a_name_is_a_mistake() {
    let _ = TagResolver::new().unparsed("Not A Name", "x");
}

// writing

fn round_trip(c: &Component) {
    let text = serialize(c).unwrap_or_else(|e| panic!("{c:?}: {e}"));
    let back = parse(&text).unwrap_or_else(|e| panic!("{text}: {e}"));
    assert_eq!(runs(&back), runs(c), "{text}");
}

fn samples() -> Vec<Component> {
    vec![
        Component::text("plain"),
        Component::text("a<b>c\\d\\<e"),
        Component::text("x").color(Color::Red).bold(),
        Component::text("x").color(Color::Rgb(1, 2, 255)),
        Component::text("x").decorate(Decoration::Italic, false),
        Component::text("x").shadow_color(0x80FF_0000),
        Component::text("x").shadow_color(0),
        Component::text("x").insertion("it's a \\ trap"),
        Component::text("x").font("minecraft:uniform"),
        Component::text("x").click(ClickEvent::OpenUrl("https://a.b/c?d=e".into())),
        Component::text("x").click(ClickEvent::RunCommand("/say 'hi' <there>".into())),
        Component::text("x").click(ClickEvent::SuggestCommand("a:b:c".into())),
        Component::text("x").click(ClickEvent::ChangePage(4)),
        Component::text("x").click(ClickEvent::CopyToClipboard("t".into())),
        Component::text("x").hover_text(Component::text("tip <1>").color(Color::Gold).bold()),
        Component::text("x").hover(HoverEvent::ShowItem {
            id: "minecraft:stone".into(),
            count: 1,
        }),
        Component::text("x").hover(HoverEvent::ShowItem {
            id: "minecraft:stone".into(),
            count: 12,
        }),
        Component::text("x").hover(HoverEvent::ShowEntity {
            entity_type: "minecraft:pig".into(),
            uuid: 0x1234_5678_9abc_def0_1234_5678_9abc_def0,
            name: Some(Component::text("Piggy").italic()),
        }),
        Component::text("x").hover(HoverEvent::ShowEntity {
            entity_type: "pig".into(),
            uuid: 7,
            name: None,
        }),
        // a hover in a hover
        Component::text("x").hover_text(
            Component::text("y")
                .hover_text(Component::text("it's z").color(Color::Aqua))
                .click_run_command("/c"),
        ),
        Component::text("[").color(Color::Gray)
            + Component::text("name")
                .bold()
                .click_suggest_command("/msg x")
            + "] "
            + Component::text("hi").italic(),
        Component::join(", ", ["a", "b", "c"].map(Component::text)),
        Component::keybind("key.jump").color(Color::Yellow),
        Component::translatable("chat.type.text")
            .arg("a'b")
            .arg(Component::text("c").bold())
            .fallback("fb")
            .color(Color::Red),
        Component::translatable("k").arg(Component::translatable("inner").arg("x")),
        Component::text("a\nb"),
        Component::empty(),
    ]
}

#[test]
fn what_is_written_reads_back_as_the_same_look() {
    for c in samples() {
        round_trip(&c);
    }
}

#[test]
fn what_is_read_can_be_written_and_read_again() {
    for input in [
        "<red>a<bold>b</bold>c</red>d",
        "<gradient:red:blue>abc",
        "<rainbow>text with <b>bold</b>",
        "<hover:show_text:'<red>tip'><click:run_command:/say hi>x",
        "<lang:k:'<b>x':y>",
        "a<reset>b",
        "<!italic>x<shadow:red:0.5>y",
    ] {
        let c = parse(input).unwrap();
        round_trip(&c);
    }
}

#[test]
fn the_written_text_is_what_one_would_write() {
    assert_eq!(
        serialize(&Component::text("Hi").color(Color::Red).bold()).unwrap(),
        "<red><bold>Hi</bold></red>"
    );
    assert_eq!(serialize(&Component::text(r"a<b\")).unwrap(), r"a\<b\\");
    assert_eq!(
        serialize(&Component::text("x").decorate(Decoration::Italic, false)).unwrap(),
        "<!italic>x</italic>"
    );
    assert_eq!(
        serialize(&Component::text("x").color(Color::Rgb(0, 128, 255))).unwrap(),
        "<#0080ff>x</#0080ff>"
    );
    assert_eq!(
        serialize(&Component::text("x").click_run_command("/a")).unwrap(),
        "<click:run_command:'/a'>x</click>"
    );
    assert_eq!(
        serialize(&(Component::text("a") + Component::text("b").bold())).unwrap(),
        "a<bold>b</bold>"
    );
}

#[test]
fn what_has_no_tag_is_refused() {
    for (c, what) in [
        (Component::score("@p", "o"), "a score"),
        (Component::selector("@a"), "a selector"),
        (Component::nbt_entity("Health", "@s"), "NBT"),
        (Component::sprite("block/stone"), "a sprite"),
    ] {
        assert_eq!(serialize(&c), Err(SerializeError::Unsupported(what)));
        // wherever it is
        assert_eq!(
            serialize(&Component::text("a").append(c.clone())),
            Err(SerializeError::Unsupported(what))
        );
        assert_eq!(
            serialize(&Component::text("a").hover_text(c)),
            Err(SerializeError::Unsupported(what))
        );
    }
    assert_eq!(
        serialize(&Component::text("a").hover(HoverEvent::ShowItem {
            id: "a".into(),
            count: 0
        })),
        Err(SerializeError::Unsupported("an item count below 1"))
    );
}

#[test]
fn a_component_nested_too_deep_is_refused_not_overflowed() {
    let mut c = Component::text("x");
    for _ in 0..MAX_DEPTH + 8 {
        c = Component::empty().append(c);
    }
    assert_eq!(serialize(&c), Err(SerializeError::TooDeep));
}

// the limits and the unexpected

fn on_a_small_stack(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn tags_nested_without_end_are_stopped_not_overflowed() {
    on_a_small_stack(|| {
        let input = "<b>".repeat(10_000) + "x";
        let e = parse(&input).unwrap_err();
        assert_eq!((e.position, e.kind), (3 * MAX_DEPTH, ErrorKind::TooDeep));
        let c = parse_lenient(&input);
        assert!(plain(&c).ends_with("<b>x"));
        // the same for the ones that have an end
        let input = "<b>".repeat(10_000) + "x" + &"</b>".repeat(10_000);
        assert!(parse(&input).is_err());
        let _ = parse_lenient(&input);
    });
}

#[test]
fn placeholders_nested_in_placeholders_are_stopped_not_overflowed() {
    on_a_small_stack(|| {
        // a chain: p1 holds p0, p2 holds p1, ...
        let chain = |links: usize| {
            (1..=links).fold(TagResolver::new().parsed("p0", "x"), |tags, i| {
                tags.parsed(&format!("p{i}"), format!("<p{}>", i - 1))
            })
        };
        assert_eq!(
            parse_with("<p10>", &chain(10)).unwrap(),
            Component::text("x")
        );
        let tags = chain(200);
        let e = parse_with("<p200>", &tags).unwrap_err();
        assert_eq!(e.kind, ErrorKind::TooDeep);
        // the link that went too deep is left as it was written
        let kept = plain(&parse_lenient_with("<p200>", &tags));
        assert!(
            kept.starts_with("<p") && kept.ends_with('>') && kept != "<p200>",
            "{kept}"
        );
    });
}

#[test]
fn hovers_inside_hovers_work_to_a_depth_and_do_not_overflow() {
    // each level has to escape the quotes of the ones inside it, so the text doubles in size
    // with each: a stranger cannot get far, and the stack is not what runs out
    on_a_small_stack(|| {
        let mut input = String::from("x");
        for _ in 0..12 {
            let quoted = input.replace('\\', "\\\\").replace('\'', "\\'");
            input = format!("<hover:show_text:'{quoted}'>y");
        }
        parse(&input).unwrap();
    });
}

#[test]
fn nothing_makes_the_parser_panic_and_what_it_reads_can_be_written() {
    const PARTS: [&str; 30] = [
        "<",
        ">",
        "/",
        ":",
        "'",
        "\"",
        "\\",
        "!",
        "#",
        "a",
        "b",
        "red",
        "bold",
        "gradient",
        "hover",
        "show_text",
        "click",
        "lang",
        "reset",
        "rainbow",
        "\n",
        "é",
        "0.5",
        " ",
        "</",
        "<red>",
        "</red>",
        "<b>",
        "<hover:show_text:'",
        "<click:run_command:",
    ];
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = move |bound: usize| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((state >> 33) as usize) % bound
    };
    for _ in 0..20_000 {
        let len = next(40);
        let input: String = (0..len).map(|_| PARTS[next(PARTS.len())]).collect();
        let _ = parse(&input);
        let c = parse_lenient(&input);
        // what was read is something that can be written, and read again the same way
        let text = serialize(&c).unwrap_or_else(|e| panic!("{input:?}: {e}"));
        let back = parse(&text).unwrap_or_else(|e| panic!("{input:?} -> {text:?}: {e}"));
        assert_eq!(runs(&back), runs(&c), "{input:?} -> {text:?}");
    }
}
