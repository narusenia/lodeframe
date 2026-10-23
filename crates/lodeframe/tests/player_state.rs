// SPDX-License-Identifier: Apache-2.0 OR MIT
//! The game mode, abilities, permission level, teleports and kicks: what the client is told, and
//! what the world believes of what it says.

use lodeframe::{
    chunk::FlatGenerator,
    player::{
        Abilities, FlyingToggleEvent, GameMode, GameModeChangedEvent, GameModeRequestEvent,
        Relative, Teleport,
    },
    protocol::{
        BlockPos, Direction, Encode, VarInt, Vec3,
        block::AIR,
        ids::play::clientbound as out,
        packets::play::{
            ABILITY_ALLOW_FLYING, ABILITY_FLYING, ABILITY_INSTANT_BREAK, ABILITY_INVULNERABLE,
            AcceptTeleportation, BlockChangedAck, BlockUpdate, ChangeGameMode, Disconnect,
            ENTITY_EVENT_PERMISSION_LEVEL, EntityEvent, GAME_EVENT_CHANGE_GAME_MODE, GameEvent,
            INPUT_SNEAK, Login, MovePlayerPos, MovePlayerPosRot, PlayerAbilities, PlayerAction,
            PlayerInfoAdd, PlayerInfoGameMode, PlayerInput, PlayerPosition,
            ServerboundPlayerAbilities, UseItemOn,
        },
    },
    registry::Registries,
    schedule::Delay,
    test_util::{FakePlayer, Received, Recorder, TestEnv},
    text::Component,
    world::{Ctx, PlayerId, PlayerLeaveEvent, World},
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

fn abilities_of(received: &[Received]) -> Vec<PlayerAbilities> {
    received
        .iter()
        .filter(|r| r.is::<PlayerAbilities>())
        .map(|r| r.decode().unwrap())
        .collect()
}

fn walk(x: f64) -> MovePlayerPos {
    MovePlayerPos {
        position: Vec3::new(x, -60.0, 0.5),
        flags: 1,
    }
}

#[test]
fn players_join_in_creative_unless_the_world_says_otherwise() {
    let mut env = env();
    let mut steve = env.connect("Steve");
    let joined = steve.drain();

    // the order the first packets had before is the same; what is new comes after them
    assert_eq!(joined[0].id, out::LOGIN);
    assert_eq!(joined[1].id, out::PLAYER_POSITION);
    let login: Login = joined[0].decode().unwrap();
    assert_eq!(login.spawn.game_mode.0, 1);
    let abilities = abilities_of(&joined);
    assert_eq!(abilities.len(), 1);
    assert_eq!(
        abilities[0].flags,
        ABILITY_INVULNERABLE | ABILITY_ALLOW_FLYING | ABILITY_INSTANT_BREAK
    );
    assert_eq!(abilities[0].fly_speed, 0.05);
    assert_eq!(abilities[0].walk_speed, 0.1);
    let event: EntityEvent = joined
        .iter()
        .find(|r| r.is::<EntityEvent>())
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!(event.entity_id, login.entity_id);
    assert_eq!(event.event, ENTITY_EVENT_PERMISSION_LEVEL);
    let tab: PlayerInfoAdd = joined
        .iter()
        .find(|r| r.is::<PlayerInfoAdd>())
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!(tab.players[0].game_mode.0, 1);

    env.instance_mut().default_game_mode = GameMode::Adventure;
    let mut alex = env.connect("Alex");
    let joined = alex.drain();
    let login: Login = joined[0].decode().unwrap();
    assert_eq!(login.spawn.game_mode.0, 2);
    assert_eq!(abilities_of(&joined)[0].flags, 0);
    let tab: PlayerInfoAdd = joined
        .iter()
        .find(|r| r.is::<PlayerInfoAdd>())
        .unwrap()
        .decode()
        .unwrap();
    let alex_entry = tab.players.iter().find(|p| p.name == "Alex").unwrap();
    assert_eq!(alex_entry.game_mode.0, 2);
    // Steve is still in creative in Alex's list
    let steve_entry = tab.players.iter().find(|p| p.name == "Steve").unwrap();
    assert_eq!(steve_entry.game_mode.0, 1);
}

#[test]
fn changing_the_game_mode_tells_the_player_everyone_and_the_handlers() {
    let mut env = env();
    let (mut steve, steve_id) = join(&mut env, "Steve");
    let (mut alex, alex_id) = join(&mut env, "Alex");
    steve.drain();
    let changed = Recorder::<GameModeChangedEvent>::attach(env.instance_mut().events_mut());

    assert_eq!(env.instance().game_mode(steve_id), Some(GameMode::Creative));
    assert!(
        env.instance_mut()
            .set_game_mode(steve_id, GameMode::Spectator)
    );
    assert_eq!(
        env.instance().game_mode(steve_id),
        Some(GameMode::Spectator)
    );

    let got = steve.drain();
    let game_event: GameEvent = got
        .iter()
        .find(|r| r.is::<GameEvent>())
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!(game_event.event, GAME_EVENT_CHANGE_GAME_MODE);
    assert_eq!(game_event.value, 3.0);
    // spectators fly from the start, take no damage and break nothing at once
    assert_eq!(
        abilities_of(&got)[0].flags,
        ABILITY_INVULNERABLE | ABILITY_ALLOW_FLYING | ABILITY_FLYING
    );
    // everyone's tab list, the player's own too
    let info: PlayerInfoGameMode = got
        .iter()
        .find(|r| r.is::<PlayerInfoGameMode>())
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!(info.players[0].uuid, steve.uuid());
    assert_eq!(info.players[0].game_mode.0, 3);
    let to_alex = alex.drain();
    assert_eq!(count(&to_alex, out::PLAYER_INFO_UPDATE), 1);
    assert_eq!(count(&to_alex, out::GAME_EVENT), 0);
    assert_eq!(count(&to_alex, out::PLAYER_ABILITIES), 0);

    // the handlers hear of it once the call is over: at the end of a handler, or the next tick
    assert!(changed.take().is_empty());
    env.tick(1);
    assert_eq!(
        changed.take(),
        [GameModeChangedEvent {
            player: steve_id,
            from: GameMode::Creative,
            to: GameMode::Spectator
        }]
    );

    // the same mode again sends and says nothing
    assert!(
        !env.instance_mut()
            .set_game_mode(steve_id, GameMode::Spectator)
    );
    assert!(steve.drain().is_empty());
    assert!(changed.take().is_empty());
    let _ = alex_id;
}

#[test]
fn a_new_game_mode_resets_the_abilities_but_keeps_the_speeds() {
    let mut env = env();
    let (mut steve, id) = join(&mut env, "Steve");
    let fast = Abilities {
        fly_speed: 0.2,
        walk_speed: 0.3,
        ..env.instance().abilities(id).unwrap()
    };
    env.instance_mut().set_abilities(id, fast);
    assert_eq!(env.instance().abilities(id), Some(fast));
    let got = abilities_of(&steve.drain());
    assert_eq!(got[0].fly_speed, 0.2);
    assert_eq!(got[0].walk_speed, 0.3);

    env.instance_mut().set_game_mode(id, GameMode::Survival);
    let now = env.instance().abilities(id).unwrap();
    assert!(!now.allow_flying && !now.invulnerable && !now.instant_break && !now.flying);
    assert_eq!((now.fly_speed, now.walk_speed), (0.2, 0.3));
    assert_eq!(abilities_of(&steve.drain())[0].flags, 0);
}

#[test]
fn f3_f4_is_a_request_that_does_nothing_until_a_handler_acts() {
    let mut env = env();
    let (steve, id) = join(&mut env, "Steve");
    let requests = Recorder::<GameModeRequestEvent>::attach(env.instance_mut().events_mut());

    env.send(&steve, &ChangeGameMode { mode: VarInt(0) });
    assert_eq!(
        requests.take(),
        [GameModeRequestEvent {
            player: id,
            mode: GameMode::Survival
        }]
    );
    assert_eq!(env.instance().game_mode(id), Some(GameMode::Creative));

    // a mode the game does not have is not a request
    env.send(&steve, &ChangeGameMode { mode: VarInt(9) });
    assert!(requests.take().is_empty());

    // a handler that allows it
    env.instance_mut()
        .events_mut()
        .on(|e: &mut GameModeRequestEvent, ctx: &mut Ctx| {
            ctx.set_game_mode(e.player, e.mode);
        });
    env.send(&steve, &ChangeGameMode { mode: VarInt(2) });
    assert_eq!(env.instance().game_mode(id), Some(GameMode::Adventure));
}

#[test]
fn flying_is_allowed_to_those_who_may_and_taken_back_from_those_who_may_not() {
    let mut env = env();
    let (mut steve, id) = join(&mut env, "Steve");
    let toggles = Recorder::<FlyingToggleEvent>::attach(env.instance_mut().events_mut());
    let fly = ServerboundPlayerAbilities {
        flags: ABILITY_FLYING,
    };

    // creative may
    env.send(&steve, &fly);
    assert!(env.instance().abilities(id).unwrap().flying);
    assert!(steve.drain().is_empty());
    env.send(&steve, &ServerboundPlayerAbilities { flags: 0 });
    assert!(!env.instance().abilities(id).unwrap().flying);
    assert_eq!(toggles.take().len(), 2);

    // survival may not: the client is told it is not flying
    env.instance_mut().set_game_mode(id, GameMode::Survival);
    steve.drain();
    env.send(&steve, &fly);
    assert!(!env.instance().abilities(id).unwrap().flying);
    assert_eq!(abilities_of(&steve.drain())[0].flags, 0);

    // a handler can say no to one who may
    env.instance_mut().set_game_mode(id, GameMode::Creative);
    steve.drain();
    env.instance_mut()
        .events_mut()
        .on(|e: &mut FlyingToggleEvent, _: &mut Ctx| e.cancel());
    env.send(&steve, &fly);
    assert!(!env.instance().abilities(id).unwrap().flying);
    assert_eq!(count(&steve.drain(), out::PLAYER_ABILITIES), 1);
}

#[test]
fn the_permission_level_is_told_to_the_client() {
    let mut env = env();
    let (mut steve, id) = join(&mut env, "Steve");
    assert_eq!(env.instance().permission_level(id), Some(0));

    env.instance_mut().set_permission_level(id, 2);
    assert_eq!(env.instance().permission_level(id), Some(2));
    let event: EntityEvent = steve
        .drain()
        .iter()
        .find(|r| r.is::<EntityEvent>())
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!(event.event, ENTITY_EVENT_PERMISSION_LEVEL + 2);

    // more than 4 is 4, and the same level says nothing
    env.instance_mut().set_permission_level(id, 9);
    assert_eq!(env.instance().permission_level(id), Some(4));
    let event: EntityEvent = steve
        .drain()
        .iter()
        .find(|r| r.is::<EntityEvent>())
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!(event.event, ENTITY_EVENT_PERMISSION_LEVEL + 4);
    env.instance_mut().set_permission_level(id, 4);
    assert!(steve.drain().is_empty());
}

fn position_packets(received: &[Received]) -> Vec<PlayerPosition> {
    received
        .iter()
        .filter(|r| r.is::<PlayerPosition>())
        .map(|r| r.decode().unwrap())
        .collect()
}

#[test]
fn a_teleport_moves_the_player_here_at_once_and_tells_the_client() {
    let mut env = env();
    let (mut steve, id) = join(&mut env, "Steve");

    env.instance_mut()
        .teleport(id, Teleport::new(Vec3::new(10.5, -50.0, -3.5)));
    assert_eq!(
        env.instance().position(id),
        Some(Vec3::new(10.5, -50.0, -3.5))
    );
    let sent = position_packets(&steve.drain());
    assert_eq!(sent.len(), 1);
    // the join was 1
    assert_eq!(sent[0].teleport_id.0, 2);
    assert_eq!(sent[0].position, Vec3::new(10.5, -50.0, -3.5));
    assert_eq!(sent[0].velocity, Vec3::ZERO);
    // the way they face is kept: relative, by nothing
    assert_eq!(sent[0].relative, 8 | 16);

    // a way to face, absolute
    env.instance_mut()
        .teleport(id, Teleport::new(Vec3::ZERO).yaw_pitch(90.0, 10.0));
    let sent = position_packets(&steve.drain());
    assert_eq!(sent[0].teleport_id.0, 3);
    assert_eq!(
        (sent[0].yaw, sent[0].pitch, sent[0].relative),
        (90.0, 10.0, 0)
    );
    assert_eq!(env.instance().rotation(id), Some((90.0, 10.0)));
}

#[test]
fn a_relative_teleport_adds_to_the_parts_it_names() {
    let mut env = env();
    let (mut steve, id) = join(&mut env, "Steve");
    env.instance_mut().teleport(
        id,
        Teleport::new(Vec3::new(1.0, 2.0, 3.0)).yaw_pitch(30.0, 5.0),
    );
    steve.drain();

    // x and z are offsets, y is where to go; the yaw is an offset, the pitch is not
    env.instance_mut().teleport(
        id,
        Teleport::new(Vec3::new(10.0, 70.0, -1.0))
            .yaw_pitch(15.0, 45.0)
            .relative(Relative::X | Relative::Z | Relative::YAW),
    );
    assert_eq!(
        env.instance().position(id),
        Some(Vec3::new(11.0, 70.0, 2.0))
    );
    assert_eq!(env.instance().rotation(id), Some((45.0, 45.0)));
    let sent = position_packets(&steve.drain());
    // what the client adds is what was asked for, not the sum
    assert_eq!(sent[0].position, Vec3::new(10.0, 70.0, -1.0));
    assert_eq!(sent[0].relative, 1 | 4 | 8);
}

#[test]
fn positions_sent_before_the_client_confirms_a_teleport_are_dropped() {
    let mut env = env();
    let (steve, id) = join(&mut env, "Steve");
    env.send(&steve, &walk(1.5));
    assert_eq!(env.instance().position(id).unwrap().x, 1.5);

    env.instance_mut()
        .teleport(id, Teleport::new(Vec3::new(50.5, -60.0, 0.5)));
    // the client was still walking, and says where it was
    env.send(&steve, &walk(2.5));
    env.send(
        &steve,
        &MovePlayerPosRot {
            position: Vec3::new(3.5, -60.0, 0.5),
            yaw: 40.0,
            pitch: 0.0,
            flags: 1,
        },
    );
    assert_eq!(env.instance().position(id).unwrap().x, 50.5);
    assert_eq!(env.instance().rotation(id), Some((0.0, 0.0)));

    // an id that was not sent does not do
    for wrong in [1, 3, 99] {
        env.send(
            &steve,
            &AcceptTeleportation {
                teleport_id: VarInt(wrong),
            },
        );
    }
    env.send(&steve, &walk(4.5));
    assert_eq!(env.instance().position(id).unwrap().x, 50.5);

    env.send(
        &steve,
        &AcceptTeleportation {
            teleport_id: VarInt(2),
        },
    );
    env.send(&steve, &walk(51.5));
    assert_eq!(env.instance().position(id).unwrap().x, 51.5);
}

#[test]
fn after_two_teleports_only_the_answer_to_the_last_counts() {
    let mut env = env();
    let (steve, id) = join(&mut env, "Steve");
    env.instance_mut()
        .teleport(id, Teleport::new(Vec3::new(10.5, -60.0, 0.5)));
    env.instance_mut()
        .teleport(id, Teleport::new(Vec3::new(20.5, -60.0, 0.5)));

    env.send(
        &steve,
        &AcceptTeleportation {
            teleport_id: VarInt(2),
        },
    );
    env.send(&steve, &walk(11.5));
    assert_eq!(env.instance().position(id).unwrap().x, 20.5);

    env.send(
        &steve,
        &AcceptTeleportation {
            teleport_id: VarInt(3),
        },
    );
    env.send(&steve, &walk(21.5));
    assert_eq!(env.instance().position(id).unwrap().x, 21.5);
}

#[test]
fn a_far_teleport_swaps_the_chunks_and_the_others_see_the_player_go() {
    let mut env = env();
    let (mut steve, steve_id) = join(&mut env, "Steve");
    let (mut alex, _) = join(&mut env, "Alex");
    steve.drain();
    env.tick(1);
    alex.drain();

    env.instance_mut()
        .teleport(steve_id, Teleport::new(Vec3::new(400.5, -60.0, 0.5)));
    let got = steve.drain();
    assert_eq!(count(&got, out::SET_CHUNK_CACHE_CENTER), 1);
    assert!(count(&got, out::FORGET_LEVEL_CHUNK) > 0);
    assert!(count(&got, out::LEVEL_CHUNK_WITH_LIGHT) > 0);

    // Alex no longer sees them
    env.tick(1);
    assert_eq!(count(&alex.drain(), out::REMOVE_ENTITIES), 1);

    // and comes back into view when they are brought back
    env.instance_mut()
        .teleport(steve_id, Teleport::new(Vec3::new(0.5, -60.0, 0.5)));
    env.tick(1);
    assert_eq!(count(&alex.drain(), out::ADD_ENTITY), 1);
}

#[test]
fn a_teleport_inside_the_view_reaches_the_others_as_a_move() {
    let mut env = env();
    let (mut steve, steve_id) = join(&mut env, "Steve");
    let (mut alex, _) = join(&mut env, "Alex");
    steve.drain();
    env.tick(1);
    alex.drain();

    env.instance_mut()
        .teleport(steve_id, Teleport::new(Vec3::new(20.5, -60.0, 0.5)));
    env.tick(1);
    let moves = alex.drain();
    assert_eq!(
        count(&moves, out::ENTITY_POSITION_SYNC) + count(&moves, out::MOVE_ENTITY_POS),
        1
    );
}

#[test]
fn a_kicked_player_is_told_why_and_leaves() {
    let mut env = env();
    let (mut steve, steve_id) = join(&mut env, "Steve");
    let (mut alex, _) = join(&mut env, "Alex");
    steve.drain();
    let left = Recorder::<PlayerLeaveEvent>::attach(env.instance_mut().events_mut());

    env.instance_mut().kick(steve_id, &Component::text("bye"));
    let got = steve.drain();
    let sent = got.iter().find(|r| r.is::<Disconnect>()).unwrap();
    let mut expected = Vec::new();
    Disconnect {
        reason: Component::text("bye"),
    }
    .encode(&mut expected)
    .unwrap();
    assert_eq!(sent.payload(), expected);
    assert!(steve.is_disconnected());
    env.tick(1);
    assert_eq!(left.take().len(), 1);
    assert!(!env.instance().is_online(steve_id));
    assert_eq!(count(&alex.drain(), out::PLAYER_INFO_REMOVE), 1);

    // the old id finds nobody, and nobody else is hurt
    env.instance_mut().kick(steve_id, &Component::text("again"));
    assert!(left.take().is_empty());
    assert!(!alex.is_disconnected());
}

#[test]
fn what_the_client_says_can_be_read_back() {
    let mut env = env();
    let (steve, id) = join(&mut env, "Steve");
    assert_eq!(
        env.instance().position(id),
        Some(Vec3::new(0.5, -60.0, 0.5))
    );
    assert!(!env.instance().is_on_ground(id));
    assert!(!env.instance().is_sneaking(id));

    env.send(
        &steve,
        &MovePlayerPosRot {
            position: Vec3::new(7.5, -60.0, -2.5),
            yaw: 123.0,
            pitch: -20.0,
            flags: 1,
        },
    );
    env.send(&steve, &PlayerInput { flags: INPUT_SNEAK });
    assert_eq!(
        env.instance().position(id),
        Some(Vec3::new(7.5, -60.0, -2.5))
    );
    assert_eq!(env.instance().rotation(id), Some((123.0, -20.0)));
    assert!(env.instance().is_on_ground(id));
    assert!(env.instance().is_sneaking(id));

    // a player who is gone reads as nothing
    env.disconnect(steve);
    assert_eq!(env.instance().position(id), None);
    assert_eq!(env.instance().rotation(id), None);
    assert!(!env.instance().is_on_ground(id));
    assert!(!env.instance().is_sneaking(id));
    assert_eq!(env.instance().game_mode(id), None);
    assert_eq!(env.instance().abilities(id), None);
    assert_eq!(env.instance().permission_level(id), None);
    // and these do nothing
    assert!(!env.instance_mut().set_game_mode(id, GameMode::Survival));
    env.instance_mut().teleport(id, Teleport::new(Vec3::ZERO));
    env.instance_mut().set_permission_level(id, 3);
}

/// The top layer of the flat world: grass at y = -61, air above it.
const GROUND: BlockPos = BlockPos::new(0, -61, 0);

fn dig(sequence: i32) -> PlayerAction {
    PlayerAction {
        action: VarInt(0),
        pos: GROUND,
        face: Direction::Up.id(),
        sequence: VarInt(sequence),
    }
}

fn click(sequence: i32) -> UseItemOn {
    UseItemOn {
        hand: VarInt(0),
        pos: GROUND,
        face: VarInt(Direction::Up.id().into()),
        cursor_x: 0.5,
        cursor_y: 1.0,
        cursor_z: 0.5,
        inside: false,
        world_border_hit: false,
        sequence: VarInt(sequence),
    }
}

fn updates(received: &[Received]) -> Vec<BlockUpdate> {
    received
        .iter()
        .filter(|r| r.is::<BlockUpdate>())
        .map(|r| r.decode().unwrap())
        .collect()
}

#[test]
fn only_creative_and_survival_players_break_and_place_blocks() {
    use lodeframe::world::{BlockBreakEvent, BlockPlaceEvent};
    let mut env = env();
    let (mut steve, id) = join(&mut env, "Steve");
    let breaks = Recorder::<BlockBreakEvent>::attach(env.instance_mut().events_mut());
    let places = Recorder::<BlockPlaceEvent>::attach(env.instance_mut().events_mut());
    let above = GROUND.offset(Direction::Up);

    for mode in [GameMode::Adventure, GameMode::Spectator] {
        env.instance_mut().set_game_mode(id, mode);
        steve.drain();
        env.send(&steve, &dig(1));
        env.send(&steve, &click(2));
        // nothing happened, and the client is told so: the block it predicted is undone
        assert!(breaks.take().is_empty() && places.take().is_empty());
        assert_ne!(env.instance_mut().block(GROUND), Some(AIR.default_state()));
        assert_eq!(env.instance_mut().block(above), Some(AIR.default_state()));
        let got = steve.drain();
        assert_eq!(updates(&got).len(), 2, "{mode:?}");
        assert_eq!(count(&got, out::BLOCK_CHANGED_ACK), 2);
        let acks: Vec<BlockChangedAck> = got
            .iter()
            .filter(|r| r.is::<BlockChangedAck>())
            .map(|r| r.decode().unwrap())
            .collect();
        assert_eq!(acks[0].sequence.0, 1);
        assert_eq!(acks[1].sequence.0, 2);
    }

    for mode in [GameMode::Survival, GameMode::Creative] {
        env.instance_mut().set_game_mode(id, mode);
        steve.drain();
        env.send(&steve, &click(3));
        assert_eq!(places.take().len(), 1, "{mode:?}");
        env.send(&steve, &dig(4));
        assert_eq!(breaks.take().len(), 1, "{mode:?}");
        // put it back for the next round
        env.instance_mut().set_block(GROUND, AIR.default_state());
        env.instance_mut().set_block(above, AIR.default_state());
        let state = lodeframe::protocol::block::GRASS_BLOCK.default_state();
        env.instance_mut().set_block(GROUND, state);
    }
}

#[test]
fn a_player_who_falls_into_the_void_can_be_sent_back_to_the_spawn() {
    let mut env = env();
    let (steve, id) = join(&mut env, "Steve");
    let spawn = env.instance().spawn;
    env.instance_mut()
        .after(Delay::ticks(1))
        .every(Delay::ticks(1), move |ctx: &mut Ctx| {
            let fallen: Vec<PlayerId> = ctx
                .players()
                .filter(|p| ctx.position(*p).is_some_and(|at| at.y < -70.0))
                .collect();
            for player in fallen {
                ctx.teleport(player, Teleport::new(spawn));
            }
        });

    env.send(
        &steve,
        &MovePlayerPos {
            position: Vec3::new(3.5, -120.0, 0.5),
            flags: 0,
        },
    );
    env.tick(2);
    assert_eq!(env.instance().position(id), Some(spawn));
}
