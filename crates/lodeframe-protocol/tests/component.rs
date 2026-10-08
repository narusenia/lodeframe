// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Components as NBT, against what the game's own codec writes.
//!
//! `component_golden.txt` has, for each component of [`all_kinds`], the bytes the 26.3 server's
//! `ComponentSerialization.CODEC` writes for it on the network (`NbtIo.writeAnyTag`), as hex.
//! They were made by giving that codec the JSON `Component::to_json` writes for the component,
//! so that they check the JSON as well. To make them again: dump the JSON with
//! `COMPONENT_JSON_OUT=<file> cargo test -p lodeframe-protocol --test component dump_json -- --ignored`,
//! and run each line through the codec of the jar of `mise run datagen` (Java 25, after
//! `Bootstrap.bootStrap()`).

use lodeframe_protocol::{Compound, Decode, Encode, Nbt};
use lodeframe_text::{ClickEvent, Color, Component, Decoration, HoverEvent};

/// One of every kind, and every part of the style, with a name.
fn all_kinds() -> Vec<(&'static str, Component)> {
    let styled = Component::text("hi")
        .color(Color::Rgb(0xff, 0x00, 0xa0))
        .shadow_color(0x8011_2233)
        .bold()
        .decorate(Decoration::Italic, false)
        .underlined()
        .strikethrough()
        .obfuscated()
        .insertion("ins")
        .font("minecraft:uniform")
        .click_open_url("https://example.com")
        .hover_text(Component::text("tip").bold());
    vec![
        ("plain", Component::text("plain")),
        ("empty", Component::empty()),
        ("named_color", Component::text("x").color(Color::DarkBlue)),
        ("styled", styled),
        (
            "children_mixed",
            Component::text("a")
                .append("b")
                .append(Component::text("c").bold()),
        ),
        (
            "children_plain",
            Component::text("a").append("b").append("c"),
        ),
        (
            "children_styled",
            Component::text("a")
                .append(Component::text("b").bold())
                .append(Component::text("c").italic()),
        ),
        (
            "children_nested",
            Component::text("a").append(Component::text("b").append("c").bold()),
        ),
        (
            "translatable",
            Component::translatable("chat.type.text")
                .arg("a")
                .arg(Component::text("b").bold())
                .fallback("fb"),
        ),
        ("translatable_bare", Component::translatable("k")),
        ("keybind", Component::keybind("key.jump")),
        ("score", Component::score("@p", "obj")),
        ("selector", Component::selector("@a").separator(", ")),
        ("selector_bare", Component::selector("@a")),
        (
            "nbt_entity",
            Component::nbt_entity("Health", "@s")
                .interpret()
                .separator("-"),
        ),
        ("nbt_plain", Component::nbt_entity("Health", "@s").plain()),
        ("nbt_block", Component::nbt_block("a.b", "~ ~ ~")),
        ("nbt_storage", Component::nbt_storage("a", "minecraft:x")),
        ("sprite", Component::sprite("minecraft:block/stone")),
        (
            "sprite_atlas",
            Component::sprite("minecraft:item/apple")
                .atlas("minecraft:items")
                .sprite_fallback("[apple]"),
        ),
        (
            "click_run",
            Component::text("x").click_run_command("/say hi"),
        ),
        (
            "click_suggest",
            Component::text("x").click_suggest_command("/say"),
        ),
        (
            "click_page",
            Component::text("x").click(ClickEvent::ChangePage(3)),
        ),
        (
            "click_copy",
            Component::text("x").click(ClickEvent::CopyToClipboard("v".into())),
        ),
        (
            "hover_item",
            Component::text("x").hover(HoverEvent::ShowItem {
                id: "minecraft:stone".into(),
                count: 2,
            }),
        ),
        (
            "hover_item_one",
            Component::text("x").hover(HoverEvent::ShowItem {
                id: "minecraft:stone".into(),
                count: 1,
            }),
        ),
        (
            "hover_entity",
            Component::text("x").hover(HoverEvent::ShowEntity {
                entity_type: "minecraft:pig".into(),
                uuid: 0x0000_0001_0000_0002_ffff_ffff_8000_0000,
                name: Some(Component::text("n").bold()),
            }),
        ),
        (
            "hover_entity_bare",
            Component::text("x").hover(HoverEvent::ShowEntity {
                entity_type: "minecraft:pig".into(),
                uuid: 7,
                name: None,
            }),
        ),
        (
            "hover_nested",
            Component::text("x").hover_text(Component::text("y").hover_text("z")),
        ),
        (
            "tricky_text",
            Component::text("quote \" slash \\ newline \n tab \t nul \0 日本語 §c 🦀"),
        ),
    ]
}

#[test]
#[ignore = "writes the JSON for the golden file; see the module comment"]
fn dump_json() {
    let path = std::env::var("COMPONENT_JSON_OUT").expect("COMPONENT_JSON_OUT");
    let lines: String = all_kinds()
        .iter()
        .map(|(name, c)| format!("{name}\t{}\n", c.to_json()))
        .collect();
    std::fs::write(path, lines).unwrap();
}

/// The golden bytes, by name.
fn golden() -> Vec<(String, Vec<u8>)> {
    include_str!("component_golden.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| {
            let (name, hex) = l.split_once(' ').expect("name and hex");
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect();
            (name.to_owned(), bytes)
        })
        .collect()
}

/// An NBT value with the order of a compound's keys and the case of a hex colour taken out, which
/// are the two things in which this writes the same thing differently.
fn normalized(nbt: &Nbt) -> Nbt {
    match nbt {
        Nbt::Compound(c) => {
            let mut entries: Vec<(String, Nbt)> =
                c.0.iter()
                    .map(|(k, v)| {
                        let v = match v {
                            Nbt::String(s) if k == "color" && s.starts_with('#') => {
                                Nbt::String(s.to_lowercase())
                            }
                            v => normalized(v),
                        };
                        (k.clone(), v)
                    })
                    .collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Nbt::Compound(Compound(entries))
        }
        Nbt::List(items) => Nbt::List(items.iter().map(normalized).collect()),
        other => other.clone(),
    }
}

fn bytes_of(c: &Component) -> Vec<u8> {
    let mut bytes = Vec::new();
    c.encode(&mut bytes).unwrap();
    bytes
}

#[test]
fn every_kind_has_golden_bytes() {
    let names: Vec<_> = all_kinds().into_iter().map(|(n, _)| n.to_owned()).collect();
    let golden: Vec<_> = golden().into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, golden, "make the golden file again");
}

#[test]
fn what_is_written_is_what_the_game_writes() {
    for ((name, component), (_, bytes)) in all_kinds().iter().zip(golden()) {
        let want = Nbt::decode(&mut bytes.as_slice()).unwrap();
        let ours = Nbt::decode(&mut bytes_of(component).as_slice()).unwrap();
        assert_eq!(normalized(&ours), normalized(&want), "{name}");
    }
}

#[test]
fn what_the_game_writes_is_read_back_as_the_component() {
    for ((name, component), (_, bytes)) in all_kinds().iter().zip(golden()) {
        let mut r = bytes.as_slice();
        assert_eq!(&Component::decode(&mut r).unwrap(), component, "{name}");
        assert!(r.is_empty(), "{name}");
    }
}

#[test]
fn writing_then_reading_gives_the_same_component() {
    for (name, component) in all_kinds() {
        let bytes = bytes_of(&component);
        assert_eq!(
            Component::decode(&mut bytes.as_slice()).unwrap(),
            component,
            "{name}"
        );
    }
}

#[test]
fn json_and_nbt_say_the_same_thing() {
    for (name, component) in all_kinds() {
        let from_json = Component::from_json(&component.to_json()).unwrap();
        assert_eq!(Nbt::from(&from_json), Nbt::from(&component), "{name}");
    }
}

#[test]
fn plain_text_is_a_string_tag_and_anything_more_is_a_compound() {
    assert_eq!(Nbt::from(&Component::text("hi")), Nbt::String("hi".into()));
    assert_eq!(Nbt::from(&Component::empty()), Nbt::String(String::new()));
    assert!(matches!(
        Nbt::from(&Component::text("hi").bold()),
        Nbt::Compound(_)
    ));
    assert!(matches!(
        Nbt::from(&Component::text("hi").append("x")),
        Nbt::Compound(_)
    ));
    assert!(matches!(
        Nbt::from(&Component::keybind("key.jump")),
        Nbt::Compound(_)
    ));
}

#[test]
fn a_list_is_the_first_component_with_the_rest_as_its_children() {
    let nbt = Nbt::List(vec![
        Nbt::String("a".into()),
        Nbt::String("b".into()),
        Nbt::Compound(Compound(vec![
            ("text".into(), Nbt::String("c".into())),
            ("bold".into(), Nbt::Byte(1)),
        ])),
    ]);
    assert_eq!(
        Component::try_from(&nbt).unwrap(),
        Component::text("a")
            .append("b")
            .append(Component::text("c").bold())
    );
}

#[test]
fn what_is_not_a_component_is_refused() {
    let compound = |entries: Vec<(&str, Nbt)>| {
        Nbt::Compound(Compound(
            entries
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v))
                .collect(),
        ))
    };
    for bad in [
        Nbt::Int(5),
        Nbt::Byte(1),
        Nbt::List(Vec::new()),
        compound(vec![]),
        compound(vec![("bold", Nbt::Byte(1))]),
        compound(vec![("text", Nbt::Int(5))]),
        compound(vec![
            ("text", Nbt::String("a".into())),
            ("extra", Nbt::Int(1)),
        ]),
        compound(vec![
            ("text", Nbt::String("a".into())),
            ("bold", Nbt::String("yes".into())),
        ]),
        compound(vec![
            ("text", Nbt::String("a".into())),
            ("color", Nbt::String("nope".into())),
        ]),
        compound(vec![
            ("text", Nbt::String("a".into())),
            (
                "click_event",
                compound(vec![("action", Nbt::String("show_dialog".into()))]),
            ),
        ]),
    ] {
        assert!(Component::try_from(&bad).is_err(), "{bad:?}");
        let mut bytes = Vec::new();
        bad.encode(&mut bytes).unwrap();
        assert!(Component::decode(&mut bytes.as_slice()).is_err(), "{bad:?}");
    }
}

#[test]
fn nesting_is_limited_and_the_stack_survives_it() {
    let deep = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            let mut nbt = Nbt::String("x".into());
            for _ in 0..60 {
                nbt = Nbt::Compound(Compound(vec![
                    ("text".into(), Nbt::String("x".into())),
                    ("extra".into(), Nbt::List(vec![nbt])),
                ]));
            }
            Component::try_from(&nbt).is_err()
        })
        .unwrap()
        .join()
        .unwrap();
    assert!(deep);
}

#[test]
#[ignore = "writes what this writes, for the game's codec to read; see the module comment"]
fn dump_hex() {
    let path = std::env::var("COMPONENT_HEX_OUT").expect("COMPONENT_HEX_OUT");
    let lines: String = all_kinds()
        .iter()
        .map(|(name, c)| {
            let hex: String = bytes_of(c).iter().map(|b| format!("{b:02x}")).collect();
            format!("{name} {hex}\n")
        })
        .collect();
    std::fs::write(path, lines).unwrap();
}
