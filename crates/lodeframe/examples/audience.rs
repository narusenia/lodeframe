// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A flat world that greets players with everything an audience can be sent, to check it against
//! real clients: `cargo run -p lodeframe --example audience [addr]`.
//!
//! When a player joins:
//!
//! - they get a title with a subtitle, a line above the hotbar and a header and footer on the
//!   player list
//! - everyone else is told in the chat and hears a sound
//! - a boss bar at the top of the screen counts the players, and shrinks as more come
//!
//! `RUST_LOG=debug` (or `trace`, `warn`, ...) sets how much is logged; the default is `info`.

use std::{cell::Cell, rc::Rc, time::Duration};

use lodeframe::{
    AudienceExt, BossBar, BossBarColor, BossBarId, BossBarOverlay, Everyone, Sound, Title,
    chunk::FlatGenerator,
    registry::Registries,
    server::Server,
    sound, text,
    world::{Ctx, PlayerJoinEvent, PlayerLeaveEvent, World},
};
use tracing::Level;

/// Room for this many players on the boss bar.
const FULL: f32 = 10.0;

/// Names the boss bar with the count, and fills it as far as the count goes.
fn count(ctx: &mut Ctx, bar: BossBarId) {
    let online = ctx.players().count();
    ctx.set_bossbar_title(bar, &text!("<red>{online} online"));
    ctx.set_bossbar_progress(bar, online as f32 / FULL);
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
            // made with the first player, kept for the rest
            let bar = Rc::new(Cell::new(None::<BossBarId>));

            let joined = Rc::clone(&bar);
            world
                .events_mut()
                .on(move |e: &mut PlayerJoinEvent, ctx: &mut Ctx| {
                    let name = &e.name;
                    ctx.show_title(
                        e.player,
                        &Title::new(text!("<gold><bold>Welcome"))
                            .subtitle(text!("<gray>{name}, this is an audience test"))
                            .times(
                                Duration::from_millis(500),
                                Duration::from_secs(3),
                                Duration::from_secs(1),
                            ),
                    );
                    ctx.send_action_bar(e.player, &text!("<green>have a look around"));
                    ctx.set_tab_list(
                        Everyone,
                        &text!("<gold>lodeframe"),
                        &text!("<gray>an audience example"),
                    );
                    // the others are told, and hear it; the newcomer does not
                    let others = Everyone.except(e.player);
                    ctx.send_message(others, &text!("<yellow>+ {name}"));
                    ctx.play_sound(others, &Sound::new(sound::ENTITY_PLAYER_LEVELUP));

                    let bar = joined.get().unwrap_or_else(|| {
                        let id = ctx.create_bossbar(
                            BossBar::new(text!("<red>players"))
                                .color(BossBarColor::Red)
                                .overlay(BossBarOverlay::Notched10),
                        );
                        joined.set(Some(id));
                        id
                    });
                    ctx.show_bossbar(e.player, bar);
                    count(ctx, bar);
                });

            let left = Rc::clone(&bar);
            world
                .events_mut()
                .on(move |e: &mut PlayerLeaveEvent, ctx: &mut Ctx| {
                    let name = &e.name;
                    ctx.send_message(Everyone, &text!("<yellow>- {name}"));
                    if let Some(bar) = left.get() {
                        count(ctx, bar);
                    }
                });
            world
        })
        .await
}
