// SPDX-License-Identifier: Apache-2.0 OR MIT
//! What the client says it does: moving, pressing keys, changing its options. Each is an event,
//! and a move can be stopped or sent elsewhere.

use lodeframe::{
    chunk::FlatGenerator,
    player::{
        ArmSwingEvent, ChatMode, ClientSettings, ClientSettingsEvent, Input, MainHand,
        ParticleStatus, PlayerInputEvent, PlayerLoadedEvent, PlayerMoveEvent, PlayerSneakEvent,
        PlayerSprintEvent, PlayerTickEvent, SkinParts,
    },
    protocol::{
        VarInt, Vec3,
        ids::play::clientbound as out,
        packets::play::{
            AcceptTeleportation, ClientInformation, MovePlayerPos, MovePlayerPosRot, MovePlayerRot,
            MovePlayerStatusOnly, PlayerInput, PlayerLoaded, PlayerPosition, Punch,
        },
    },
    registry::Registries,
    test_util::{FakePlayer, Received, Recorder, TestEnv},
    text::Component,
    world::{Ctx, PlayerId, World},
};

fn env() -> TestEnv<World> {
    let mut world = World::new(&Registries::vanilla(), FlatGenerator::default());
    world.view_distance = 2;
    world.chunks_per_tick = usize::MAX;
    TestEnv::new(world)
}

fn join(env: &mut TestEnv<World>, name: &str) -> (FakePlayer, PlayerId) {
    let mut player = env.connect(name);
    player.drain();
    let id = env.instance().player_id(player.uuid()).unwrap();
    (player, id)
}

fn count(received: &[Received], id: i32) -> usize {
    received.iter().filter(|r| r.id == id).count()
}

fn walk_to(x: f64) -> MovePlayerPos {
    MovePlayerPos {
        position: Vec3::new(x, -60.0, 0.5),
        flags: 1,
    }
}

fn teleports(received: &[Received]) -> Vec<PlayerPosition> {
    received
        .iter()
        .filter(|r| r.is::<PlayerPosition>())
        .map(|r| r.decode().unwrap())
        .collect()
}

fn accept(env: &mut TestEnv<World>, player: &FakePlayer, teleport: &PlayerPosition) {
    env.send(
        player,
        &AcceptTeleportation {
            teleport_id: teleport.teleport_id,
        },
    );
}

#[test]
fn only_a_move_that_changes_something_is_an_event() {
    let mut env = env();
    let (steve, id) = join(&mut env, "Steve");
    let moves = Recorder::<PlayerMoveEvent>::attach(env.instance_mut().events_mut());

    // standing where they stood, or saying only whether they stand on something
    env.send(&steve, &walk_to(0.5));
    env.send(&steve, &MovePlayerStatusOnly { flags: 1 });
    assert!(moves.take().is_empty());
    assert!(env.instance().is_on_ground(id));

    env.send(&steve, &walk_to(3.5));
    let seen = moves.take();
    assert_eq!(seen.len(), 1);
    let event = &seen[0];
    assert_eq!(event.player, id);
    assert_eq!(event.from, Vec3::new(0.5, -60.0, 0.5));
    assert_eq!(event.position, Vec3::new(3.5, -60.0, 0.5));
    assert_eq!((event.yaw, event.pitch), (0.0, 0.0));
    assert!(event.on_ground);
    // nobody stopped it
    assert_eq!(env.instance().position(id).unwrap().x, 3.5);

    // turning alone is a move that stays where it is
    env.send(
        &steve,
        &MovePlayerRot {
            yaw: 90.0,
            pitch: 10.0,
            flags: 0,
        },
    );
    let seen = moves.take();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].from, seen[0].position);
    assert_eq!((seen[0].yaw, seen[0].pitch), (90.0, 10.0));
    assert!(!seen[0].on_ground);
    assert_eq!(env.instance().rotation(id), Some((90.0, 10.0)));
}

#[test]
fn a_cancelled_move_puts_the_player_back_and_what_they_send_meanwhile_is_dropped() {
    let mut env = env();
    let (mut steve, id) = join(&mut env, "Steve");
    let moves = Recorder::<PlayerMoveEvent>::attach(env.instance_mut().events_mut());
    env.instance_mut()
        .events_mut()
        .on(|e: &mut PlayerMoveEvent, _: &mut Ctx| {
            if e.position.x > 10.0 {
                e.cancel();
            }
        });
    env.send(&steve, &walk_to(5.5));
    steve.drain();
    moves.take();

    env.send(&steve, &walk_to(20.5));
    assert_eq!(moves.take().len(), 1);
    assert_eq!(env.instance().position(id).unwrap().x, 5.5);
    let told = teleports(&steve.drain());
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].position, Vec3::new(5.5, -60.0, 0.5));
    // put back to what it was, the way they face too
    assert_eq!((told[0].yaw, told[0].pitch), (0.0, 0.0));

    // the client has not heard yet, and keeps sending: that is no event, and goes nowhere
    env.send(&steve, &walk_to(21.5));
    env.send(&steve, &walk_to(22.5));
    assert!(moves.take().is_empty());
    assert_eq!(env.instance().position(id).unwrap().x, 5.5);

    // once it has, it moves again, and the handler is asked again
    accept(&mut env, &steve, &told[0]);
    env.send(&steve, &walk_to(6.5));
    assert_eq!(moves.take().len(), 1);
    assert_eq!(env.instance().position(id).unwrap().x, 6.5);
}

#[test]
fn a_cancelled_turn_turns_the_player_back() {
    let mut env = env();
    let (mut steve, id) = join(&mut env, "Steve");
    env.instance_mut()
        .events_mut()
        .on(|e: &mut PlayerMoveEvent, _: &mut Ctx| e.cancel());
    steve.drain();

    env.send(
        &steve,
        &MovePlayerRot {
            yaw: 45.0,
            pitch: 5.0,
            flags: 1,
        },
    );
    assert_eq!(env.instance().rotation(id), Some((0.0, 0.0)));
    let told = teleports(&steve.drain());
    assert_eq!((told[0].yaw, told[0].pitch), (0.0, 0.0));
}

#[test]
fn a_handler_can_send_the_player_elsewhere() {
    let mut env = env();
    let (mut steve, id) = join(&mut env, "Steve");
    env.instance_mut()
        .events_mut()
        .on(|e: &mut PlayerMoveEvent, ctx: &mut Ctx| {
            if e.position.y < -70.0 {
                e.position = ctx.spawn;
                e.yaw = 180.0;
            }
        });
    steve.drain();

    env.send(
        &steve,
        &MovePlayerPosRot {
            position: Vec3::new(9.5, -100.0, 0.5),
            yaw: 30.0,
            pitch: 0.0,
            flags: 0,
        },
    );
    let spawn = env.instance().spawn;
    assert_eq!(env.instance().position(id), Some(spawn));
    assert_eq!(env.instance().rotation(id), Some((180.0, 0.0)));
    let told = teleports(&steve.drain());
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].position, spawn);
    assert_eq!((told[0].yaw, told[0].pitch), (180.0, 0.0));
    // that they said they were in the air was taken
    assert!(!env.instance().is_on_ground(id));

    // a place that is not a number puts them back instead
    env.send(&steve, &MovePlayerStatusOnly { flags: 0 });
    accept(&mut env, &steve, &told[0]);
    env.instance_mut()
        .events_mut()
        .on(|e: &mut PlayerMoveEvent, _: &mut Ctx| e.position = Vec3::new(f64::NAN, 0.0, 0.0));
    env.send(&steve, &walk_to(2.5));
    assert_eq!(env.instance().position(id), Some(spawn));
}

#[test]
fn a_client_that_says_it_is_nowhere_is_not_believed() {
    let mut env = env();
    let (steve, id) = join(&mut env, "Steve");
    let moves = Recorder::<PlayerMoveEvent>::attach(env.instance_mut().events_mut());

    env.send(
        &steve,
        &MovePlayerPos {
            position: Vec3::new(f64::NAN, -60.0, 0.5),
            flags: 1,
        },
    );
    env.send(
        &steve,
        &MovePlayerRot {
            yaw: f32::INFINITY,
            pitch: 0.0,
            flags: 1,
        },
    );
    assert!(moves.take().is_empty());
    assert_eq!(
        env.instance().position(id),
        Some(Vec3::new(0.5, -60.0, 0.5))
    );
}

#[test]
fn a_dead_player_and_one_who_has_not_confirmed_a_teleport_make_no_moves() {
    let mut env = env();
    let (steve, id) = join(&mut env, "Steve");
    let moves = Recorder::<PlayerMoveEvent>::attach(env.instance_mut().events_mut());

    env.instance_mut().kill(id, &Component::text("died"));
    env.send(&steve, &walk_to(4.5));
    assert!(moves.take().is_empty());
    env.instance_mut().respawn(id);
    env.send(&steve, &walk_to(4.5));
    assert!(moves.take().is_empty(), "the respawn is not confirmed yet");
}

#[test]
fn the_others_see_a_move_that_was_taken_and_not_one_that_was_stopped() {
    let mut env = env();
    let (steve, _) = join(&mut env, "Steve");
    let (mut alex, _) = join(&mut env, "Alex");
    env.tick(1);
    alex.drain();
    env.instance_mut()
        .events_mut()
        .on(|e: &mut PlayerMoveEvent, _: &mut Ctx| {
            if e.position.x > 10.0 {
                e.cancel();
            }
        });

    env.send(&steve, &walk_to(2.5));
    env.tick(1);
    let seen = alex.drain();
    assert_eq!(
        count(&seen, out::MOVE_ENTITY_POS) + count(&seen, out::ENTITY_POSITION_SYNC),
        1
    );

    // stopped: Steve is where he was here, so there is nothing to show
    env.send(&steve, &walk_to(30.5));
    env.tick(1);
    let seen = alex.drain();
    assert_eq!(
        count(&seen, out::MOVE_ENTITY_POS)
            + count(&seen, out::ENTITY_POSITION_SYNC)
            + count(&seen, out::MOVE_ENTITY_POS_ROT),
        0
    );
}

#[test]
fn keys_are_events_and_sneaking_and_sprinting_have_their_own() {
    let mut env = env();
    let (steve, id) = join(&mut env, "Steve");
    let (mut alex, _) = join(&mut env, "Alex");
    env.tick(1);
    alex.drain();
    let inputs = Recorder::<PlayerInputEvent>::attach(env.instance_mut().events_mut());
    let sneaks = Recorder::<PlayerSneakEvent>::attach(env.instance_mut().events_mut());
    let sprints = Recorder::<PlayerSprintEvent>::attach(env.instance_mut().events_mut());

    // forward and sprint
    env.send(&steve, &PlayerInput { flags: 0x41 });
    let forward_sprint = Input {
        forward: true,
        sprint: true,
        ..Input::default()
    };
    assert_eq!(
        inputs.take(),
        [PlayerInputEvent {
            player: id,
            input: forward_sprint,
            previous: Input::default()
        }]
    );
    assert_eq!(
        sprints.take(),
        [PlayerSprintEvent {
            player: id,
            sprinting: true
        }]
    );
    assert!(sneaks.take().is_empty());
    assert!(env.instance().is_sprinting(id));
    assert_eq!(env.instance().input(id), Some(forward_sprint));

    // the same keys again are not news
    env.send(&steve, &PlayerInput { flags: 0x41 });
    assert!(inputs.take().is_empty());

    // forward and sneak: the sprint ends, the sneak starts
    env.send(&steve, &PlayerInput { flags: 0x21 });
    let now = Input {
        forward: true,
        sneak: true,
        ..Input::default()
    };
    assert_eq!(inputs.take()[0].previous, forward_sprint);
    assert_eq!(env.instance().input(id), Some(now));
    assert_eq!(
        sneaks.take(),
        [PlayerSneakEvent {
            player: id,
            sneaking: true
        }]
    );
    assert_eq!(
        sprints.take(),
        [PlayerSprintEvent {
            player: id,
            sprinting: false
        }]
    );
    assert!(env.instance().is_sneaking(id));
    assert!(!env.instance().is_sprinting(id));
    // the others see them crouch
    assert_eq!(count(&alex.drain(), out::SET_ENTITY_DATA), 1);

    // every key
    env.send(&steve, &PlayerInput { flags: 0x7f });
    assert_eq!(env.instance().input(id), Some(Input::from_bits(0x7f)),);
    let all = Input::from_bits(0x7f);
    assert!(
        all.forward && all.backward && all.left && all.right && all.jump && all.sneak && all.sprint
    );
}

#[test]
fn every_player_ticks_every_tick() {
    let mut env = env();
    let (_steve, steve_id) = join(&mut env, "Steve");
    let (alex, alex_id) = join(&mut env, "Alex");
    let ticks = Recorder::<PlayerTickEvent>::attach(env.instance_mut().events_mut());

    env.tick(3);
    let seen = ticks.take();
    assert_eq!(seen.len(), 6);
    assert_eq!(seen.iter().filter(|e| e.player == steve_id).count(), 3);
    assert_eq!(seen.iter().filter(|e| e.player == alex_id).count(), 3);

    // in the same order each time
    let first: Vec<PlayerId> = seen[..2].iter().map(|e| e.player).collect();
    let second: Vec<PlayerId> = seen[2..4].iter().map(|e| e.player).collect();
    assert_eq!(first, second);

    // a dead player ticks, one who left does not
    env.instance_mut().kill(steve_id, &Component::text("died"));
    env.tick(1);
    assert_eq!(ticks.take().len(), 2);
    env.disconnect(alex);
    env.tick(1);
    let seen = ticks.take();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].player, steve_id);
}

fn information(language: &str, view_distance: i8) -> ClientInformation {
    ClientInformation {
        language: language.into(),
        view_distance,
        chat_mode: VarInt(1),
        chat_colors: false,
        skin_parts: 0x41,
        main_hand: VarInt(0),
        text_filtering: true,
        allow_listing: false,
        particles: VarInt(2),
    }
}

#[test]
fn a_player_has_the_options_they_joined_with_or_the_games() {
    let mut env = env();
    let (_, plain) = join(&mut env, "Plain");
    assert_eq!(env.instance().locale(plain), Some("en_us"));
    assert_eq!(
        env.instance().client_settings(plain),
        Some(&ClientSettings::default())
    );

    let settings = ClientSettings::from(&information("ja_jp", 12));
    let mut japanese = env.connect_with_client_settings("Japanese", settings.clone());
    japanese.drain();
    let id = env.instance().player_id(japanese.uuid()).unwrap();
    assert_eq!(env.instance().locale(id), Some("ja_jp"));
    let got = env.instance().client_settings(id).unwrap();
    assert_eq!(got, &settings);
    assert_eq!(got.view_distance, 12);
    assert_eq!(got.chat_mode, ChatMode::CommandsOnly);
    assert!(!got.chat_colors);
    assert!(got.skin_parts.shows(SkinParts::CAPE) && got.skin_parts.shows(SkinParts::HAT));
    assert!(!got.skin_parts.shows(SkinParts::JACKET));
    assert_eq!(got.main_hand, MainHand::Left);
    assert!(got.text_filtering && !got.allow_listing);
    assert_eq!(got.particles, ParticleStatus::Minimal);
}

#[test]
fn changing_the_options_while_playing_is_an_event_with_what_they_were() {
    let mut env = env();
    let (steve, id) = join(&mut env, "Steve");
    let changes = Recorder::<ClientSettingsEvent>::attach(env.instance_mut().events_mut());

    env.send(&steve, &information("de_de", 10));
    let seen = changes.take();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].player, id);
    assert_eq!(seen[0].previous, ClientSettings::default());
    assert_eq!(seen[0].settings.language, "de_de");
    assert_eq!(env.instance().locale(id), Some("de_de"));

    // the same again is not a change
    env.send(&steve, &information("de_de", 10));
    assert!(changes.take().is_empty());

    // what the game does not take is brought into range
    env.send(&steve, &information(&"x".repeat(40), 100));
    let got = &changes.take()[0].settings;
    assert_eq!(got.language.chars().count(), 16);
    assert_eq!(got.view_distance, 32);
    env.send(&steve, &information("a", -5));
    assert_eq!(env.instance().client_settings(id).unwrap().view_distance, 2);
    let mut odd = information("de_de", 10);
    odd.chat_mode = VarInt(9);
    odd.particles = VarInt(9);
    odd.skin_parts = 0xff;
    env.send(&steve, &odd);
    let got = env.instance().client_settings(id).unwrap();
    assert_eq!(got.chat_mode, ChatMode::Full);
    assert_eq!(got.particles, ParticleStatus::All);
    assert_eq!(got.skin_parts, SkinParts::ALL);
}

#[test]
fn the_client_finishing_loading_and_swinging_its_arm_are_events() {
    let mut env = env();
    let (steve, id) = join(&mut env, "Steve");
    let loaded = Recorder::<PlayerLoadedEvent>::attach(env.instance_mut().events_mut());
    let swings = Recorder::<ArmSwingEvent>::attach(env.instance_mut().events_mut());

    env.send(&steve, &PlayerLoaded);
    assert_eq!(loaded.take(), [PlayerLoadedEvent { player: id }]);

    env.send(&steve, &Punch);
    let seen = swings.take();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].player, id);
}
