// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A flat world that greets each player with one line per kind of component, to check how real
//! clients show them: `cargo run -p lodeframe --example components [addr]`.
//!
//! Look at each line, then hover over it and click it where it says so.
//!
//! `RUST_LOG=debug` (or `trace`, `warn`, ...) sets how much is logged; the default is `info`.

use lodeframe::{
    chunk::FlatGenerator,
    registry::Registries,
    server::Server,
    text::{Color, Component, HoverEvent},
    world::{Ctx, PlayerJoinEvent, World},
};
use tracing::Level;

/// A label in gray, then what it labels.
fn line(label: &str, shown: Component) -> Component {
    Component::text(format!("{label}: ")).color(Color::Gray) + shown
}

fn lines() -> Vec<Component> {
    vec![
        line(
            "colors",
            Component::join(
                " ",
                [
                    Color::Red,
                    Color::Gold,
                    Color::Yellow,
                    Color::Green,
                    Color::Aqua,
                    Color::Blue,
                    Color::LightPurple,
                ]
                .map(|c| Component::text("###").color(c)),
            ),
        ),
        line(
            "decorations",
            Component::text("bold ").bold()
                + Component::text("italic ").italic()
                + Component::text("underlined ").underlined()
                + Component::text("strikethrough ").strikethrough()
                + Component::text("obfuscated").obfuscated(),
        ),
        line(
            "children",
            Component::text("parent ")
                .color(Color::Green)
                .bold()
                .append(Component::text("child, bold and green "))
                .append(
                    Component::text("child, not bold")
                        .undecorate(lodeframe::text::Decoration::Bold),
                ),
        ),
        line(
            "translatable",
            Component::translatable("chat.type.text")
                .arg("an arg")
                .arg(Component::text("another").color(Color::Aqua)),
        ),
        line(
            "translatable fallback",
            Component::translatable("lodeframe.no.such.key")
                .fallback("the fallback text")
                .arg("unused"),
        ),
        line(
            "keybind",
            Component::keybind("key.jump").color(Color::Yellow),
        ),
        line(
            "hover text",
            Component::text("hover me")
                .color(Color::Gold)
                .hover_text(Component::text("hello from a hover").italic()),
        ),
        line(
            "hover item",
            Component::text("hover me")
                .color(Color::Gold)
                .hover(HoverEvent::ShowItem {
                    id: "minecraft:diamond_sword".into(),
                    count: 1,
                }),
        ),
        line(
            "hover entity",
            Component::text("hover me")
                .color(Color::Gold)
                .hover(HoverEvent::ShowEntity {
                    entity_type: "minecraft:pig".into(),
                    uuid: 0x1234_5678_9abc_def0_1234_5678_9abc_def0,
                    name: Some(Component::text("Piggy")),
                }),
        ),
        line(
            "click url",
            Component::text("click to open the repository")
                .color(Color::Aqua)
                .underlined()
                .click_open_url("https://github.com/narusenia/lodeframe"),
        ),
        line(
            "click suggest",
            Component::text("click to fill the chat box")
                .color(Color::Aqua)
                .click_suggest_command("hello"),
        ),
        line(
            "click copy",
            Component::text("click to copy").color(Color::Aqua).click(
                lodeframe::text::ClickEvent::CopyToClipboard("copied by lodeframe".into()),
            ),
        ),
        line(
            "insertion",
            Component::text("shift-click to insert").insertion("[inserted]"),
        ),
        line(
            "shadow",
            Component::text("shadow color").shadow_color(0xFFFF_0000),
        ),
        line(
            "font",
            Component::text("alt font").font("minecraft:uniform"),
        ),
        line(
            "selector / score",
            Component::selector("@p") + Component::space() + Component::score("Alice", "kills"),
        ),
    ]
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let level = std::env::var("RUST_LOG")
        .ok()
        .and_then(|v| v.parse::<Level>().ok())
        .unwrap_or(Level::INFO);
    tracing_subscriber::fmt().with_max_level(level).init();

    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:25565".into());
    Server::new(addr)
        .run(|registries: &Registries| {
            let mut world = World::new(registries, FlatGenerator::default());
            world
                .events_mut()
                .on(|e: &mut PlayerJoinEvent, ctx: &mut Ctx| {
                    for line in lines() {
                        ctx.send_message(e.player, &line);
                    }
                });
            world
        })
        .await
}
