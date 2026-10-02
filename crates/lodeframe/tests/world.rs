// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A player joins a world, walks over a chunk border, and gets the chunks they need.

use lodeframe::{
    chunk::{ChunkLoader, ChunkPos, FlatGenerator},
    cooldown::Cooldown,
    data::Key,
    event::{Event, EventNode, Listener},
    instance::PluginMessage,
    login::Profile,
    protocol::{
        BlockPos, Direction, Encode, Uuid, VarInt, Vec3,
        block::{AIR, COBBLESTONE, STONE},
        chunk::LevelChunkWithLight,
        ids::play::{clientbound as out, serverbound},
        packets::{
            login::ProfileProperty,
            play::{
                AddEntity, BlockChangedAck, BlockUpdate, Chat, ChunkBatchFinished, DisguisedChat,
                EntityPositionSync, FLAG_SNEAKING, ForgetLevelChunk, INPUT_SNEAK, Login,
                MoveEntityPos, MoveEntityPosRot, MovePlayerPos, MovePlayerPosRot, MovePlayerRot,
                PlayerAction, PlayerInfoAdd, PlayerInfoRemove, PlayerInput, PlayerPosition,
                RemoveEntities, RotateHead, SetChunkCacheCenter, SetEntityFlagsAndPose, SystemChat,
                UseItemOn,
            },
        },
    },
    registry::Registries,
    schedule::{Delay, Next},
    test_util::{FakePlayer, Received, Recorder, TestEnv},
    text::{Color, Component},
    world::{
        BlockBreakEvent, BlockPlaceEvent, ChatEvent, Ctx, PlayerEvent, PlayerJoinEvent,
        PlayerLeaveEvent, PluginMessageEvent, ShutdownEvent, World,
    },
};

fn count(received: &[Received], id: i32) -> usize {
    received.iter().filter(|r| r.id == id).count()
}

fn walk(x: f64) -> MovePlayerPos {
    MovePlayerPos {
        position: Vec3::new(x, -60.0, 0.5),
        flags: 1,
    }
}

fn env() -> TestEnv<World> {
    let mut world = World::new(&Registries::vanilla(), FlatGenerator::default());
    world.view_distance = 2;
    // all the chunks at once, so that a test sees them right after the join
    world.chunks_per_tick = usize::MAX;
    TestEnv::new(world)
}

#[test]
fn a_player_gets_the_world_on_join() {
    let mut env = env();
    let mut steve = env.connect("Steve");

    let joined = steve.drain();
    assert_eq!(joined[0].id, out::LOGIN);
    assert_eq!(joined[1].id, out::PLAYER_POSITION);
    assert_eq!(count(&joined, out::LEVEL_CHUNK_WITH_LIGHT), 25);
    assert_eq!(count(&joined, out::CHUNK_BATCH_FINISHED), 1);

    let login: Login = joined[0].decode().unwrap();
    assert_eq!(login.view_distance.0, 2);
    let at: PlayerPosition = joined[1].decode().unwrap();
    assert_eq!(at.position, Vec3::new(0.5, -60.0, 0.5));
}

#[test]
fn crossing_a_chunk_border_swaps_a_column_of_chunks() {
    let mut env = env();
    let mut steve = env.connect("Steve");
    steve.drain();

    // inside the same chunk: nothing to send
    env.send(&steve, &walk(5.5));
    assert!(steve.drain().is_empty());

    // one chunk east: a new column of 5 in, the far column of 5 out
    env.send(&steve, &walk(16.5));
    let moved = steve.drain();
    assert_eq!(count(&moved, out::LEVEL_CHUNK_WITH_LIGHT), 5);
    let center: SetChunkCacheCenter = moved
        .iter()
        .find(|r| r.is::<SetChunkCacheCenter>())
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!((center.x.0, center.z.0), (1, 0));
    let mut forgotten = Vec::new();
    for r in moved.iter().filter(|r| r.is::<ForgetLevelChunk>()) {
        let f: ForgetLevelChunk = r.decode().unwrap();
        forgotten.push((f.x, f.z));
    }
    assert_eq!(forgotten, [(-2, -2), (-2, -1), (-2, 0), (-2, 1), (-2, 2)]);
}

#[test]
fn a_broken_packet_drops_the_player() {
    let mut env = env();
    let mut steve = env.connect("Steve");
    steve.drain();

    // a move packet with no payload
    env.send_raw(&steve, vec![serverbound::MOVE_PLAYER_POS as u8]);
    assert!(steve.is_disconnected());
}

fn names(packets: &[PlayerInfoAdd]) -> Vec<&str> {
    packets
        .iter()
        .flat_map(|p| &p.players)
        .map(|p| p.name.as_str())
        .collect()
}

#[test]
fn players_see_each_other_and_the_tab_list() {
    let mut env = env();
    let mut steve = env.connect("Steve");
    // Steve is in his own list, and nobody else is there to show
    assert_eq!(names(&steve.drain_as::<PlayerInfoAdd>()), ["Steve"]);

    let mut alex = env.connect("Alex");
    let steve_id = steve.drain_as::<AddEntity>();
    assert_eq!(steve_id.len(), 1);
    assert_eq!(steve.drain_as::<PlayerInfoAdd>().len(), 0);

    // Alex sees the list with both, and Steve's body
    assert_eq!(names(&alex.drain_as::<PlayerInfoAdd>()), ["Steve", "Alex"]);
    let seen = steve_id[0].clone();
    assert_eq!(seen.uuid, alex.uuid());

    env.disconnect(alex);
    let gone: Vec<RemoveEntities> = steve.drain_as();
    assert_eq!(gone.len(), 1);
    assert_eq!(gone[0].entity_ids[0], seen.entity_id);
}

#[test]
fn tab_list_entries_are_removed_on_leave() {
    let mut env = env();
    let mut steve = env.connect("Steve");
    let alex = env.connect("Alex");
    steve.drain();
    let alex_uuid = alex.uuid();
    env.disconnect(alex);
    let packets = steve.drain();
    let removed = packets
        .iter()
        .find(|r| r.is::<PlayerInfoRemove>())
        .expect("the list entry is removed")
        .decode::<PlayerInfoRemove>()
        .unwrap();
    assert_eq!(removed.uuids, [alex_uuid]);
    assert!(packets.iter().any(|r| r.is::<RemoveEntities>()));
}

#[test]
fn a_move_reaches_the_others_at_the_next_tick_as_an_offset() {
    let mut env = env();
    let mut steve = env.connect("Steve");
    let mut alex = env.connect("Alex");
    steve.drain();
    alex.drain();

    env.send(
        &alex,
        &MovePlayerPosRot {
            position: Vec3::new(3.5, -60.0, 0.5),
            yaw: 90.0,
            pitch: 10.0,
            flags: 1,
        },
    );
    assert!(steve.drain().is_empty(), "told at the tick, not on arrival");
    env.tick(1);

    let told = steve.drain();
    let moved: MoveEntityPosRot = told
        .iter()
        .find(|r| r.is::<MoveEntityPosRot>())
        .unwrap()
        .decode()
        .unwrap();
    // 3 blocks along x from the spawn at 4096 to a block; 90 degrees is 64 and 10 is 7
    let expected = MoveEntityPosRot {
        entity_id: VarInt(2),
        on_ground: true,
        dx: 12288,
        dy: 0,
        dz: 0,
        yaw: 64,
        pitch: 7,
    };
    assert_eq!(moved, expected);
    let head: RotateHead = told
        .iter()
        .find(|r| r.is::<RotateHead>())
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!(head.head_yaw, 64);
    assert!(
        alex.drain().is_empty(),
        "the mover is not told about themselves"
    );
}

#[test]
fn moving_and_turning_have_their_own_packets_and_the_head_follows_only_the_yaw() {
    let (mut env, mut steve, alex) = two_players();
    let mut told = |env: &mut TestEnv<World>| {
        env.tick(1);
        steve.drain()
    };

    env.send(&alex, &walk(4.5));
    let moved = told(&mut env);
    assert_eq!(count(&moved, out::MOVE_ENTITY_POS), 1);
    assert_eq!(count(&moved, out::ROTATE_HEAD), 0, "the yaw did not change");

    env.send(
        &alex,
        &MovePlayerRot {
            yaw: 180.0,
            pitch: 0.0,
            flags: 1,
        },
    );
    let turned = told(&mut env);
    assert_eq!(count(&turned, out::MOVE_ENTITY_ROT), 1);
    assert_eq!(count(&turned, out::MOVE_ENTITY_POS), 0);
    let head: RotateHead = turned
        .iter()
        .find(|r| r.is::<RotateHead>())
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!(head.head_yaw, 128);

    // looking up or down turns the body, not the head
    env.send(
        &alex,
        &MovePlayerRot {
            yaw: 180.0,
            pitch: 45.0,
            flags: 1,
        },
    );
    let nodded = told(&mut env);
    assert_eq!(count(&nodded, out::MOVE_ENTITY_ROT), 1);
    assert_eq!(count(&nodded, out::ROTATE_HEAD), 0);

    // a move that changes nothing is not repeated
    env.send(
        &alex,
        &MovePlayerRot {
            yaw: 180.0,
            pitch: 45.0,
            flags: 1,
        },
    );
    assert!(told(&mut env).is_empty());
    // and a tick with nobody having moved sends nothing
    assert!(told(&mut env).is_empty());
}

#[test]
fn sneaking_is_told_at_once() {
    let (mut env, mut steve, alex) = two_players();

    env.send(&alex, &PlayerInput { flags: INPUT_SNEAK });
    let crouch: Vec<SetEntityFlagsAndPose> = steve.drain_as();
    assert_eq!((crouch[0].flags, crouch[0].pose.0), (2, 5));
    env.send(&alex, &PlayerInput { flags: INPUT_SNEAK });
    assert!(steve.drain().is_empty(), "still sneaking: nothing new");
    env.send(&alex, &PlayerInput { flags: 0 });
    let stand: Vec<SetEntityFlagsAndPose> = steve.drain_as();
    assert_eq!((stand[0].flags, stand[0].pose.0), (0, 0));
}

#[test]
fn several_moves_in_a_tick_are_one_offset_to_the_last_place() {
    let (mut env, mut steve, alex) = two_players();

    for x in [1.5, 2.5, 3.5] {
        env.send(&alex, &walk(x));
    }
    env.tick(1);

    let moves: Vec<MoveEntityPos> = steve.drain_as();
    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0].dx, 3 * 4096);
}

#[test]
fn a_move_of_eight_blocks_or_more_is_said_as_a_place() {
    let (mut env, mut steve, alex) = two_players();

    env.send(&alex, &walk(20.5));
    env.tick(1);

    let told = steve.drain();
    let sync: EntityPositionSync = told
        .iter()
        .find(|r| r.is::<EntityPositionSync>())
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!(sync.position, Vec3::new(20.5, -60.0, 0.5));
    assert_eq!(count(&told, out::ROTATE_HEAD), 1);
    assert_eq!(count(&told, out::MOVE_ENTITY_POS), 0);
    // the next small step is an offset from there
    env.send(&alex, &walk(21.5));
    env.tick(1);
    let step: Vec<MoveEntityPos> = steve.drain_as();
    assert_eq!(step.len(), 1);
    assert_eq!(step[0].dx, 4096);
}

#[test]
fn offsets_do_not_add_up_to_a_drift() {
    let (mut env, mut steve, alex) = two_players();
    // 0.001 of a block is 4.096 units: rounding each step alone would lose 0.096 of a unit a time
    let mut seen = 0.5;
    for i in 1..=300 {
        env.send(&alex, &walk(0.5 + 0.001 * f64::from(i)));
        env.tick(1);
        for step in steve.drain_as::<MoveEntityPos>() {
            seen += f64::from(step.dx) / 4096.0;
        }
    }
    assert!((seen - 0.8).abs() <= 1.0 / 4096.0, "seen {seen}");
}

#[test]
fn a_player_who_comes_into_view_is_spawned_where_the_others_have_them_and_catches_up() {
    let (mut env, _steve, alex) = two_players();
    // Alex has moved, but the others have not been told yet
    env.send(&alex, &walk(5.5));

    let mut dave = env.connect("Dave");
    let spawned: AddEntity = dave
        .drain()
        .iter()
        .find(|r| r.is::<AddEntity>())
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!(spawned.position, Vec3::new(0.5, -60.0, 0.5));

    env.tick(1);

    let steps: Vec<MoveEntityPos> = dave.drain_as();
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].dx, 5 * 4096);
}

#[test]
fn the_moves_of_a_tick_arrive_as_one_message_for_each_viewer() {
    let mut env = env();
    let mut steve = env.connect("Steve");
    let mut alex = env.connect("Alex");
    let mut dave = env.connect("Dave");
    steve.drain();
    alex.drain();
    dave.drain();

    env.send(&alex, &walk(2.5));
    env.send(&dave, &walk(3.5));
    env.tick(1);

    // Steve sees both of them move, in one message; each of the two sees the other
    let messages = steve.drain_messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(count(&messages[0], out::MOVE_ENTITY_POS), 2);
    for mover in [&mut alex, &mut dave] {
        let messages = mover.drain_messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(count(&messages[0], out::MOVE_ENTITY_POS), 1);
    }
}

fn say(env: &mut TestEnv<World>, who: &FakePlayer, text: &str) {
    env.send(
        who,
        &Chat {
            message: text.into(),
        },
    );
}

/// The payload of the `DisguisedChat` a vanilla player gets for `message` from `name`.
fn chat_line(name: &str, message: Component) -> Vec<u8> {
    let chat_type = Registries::vanilla()
        .network_id("minecraft:chat_type", "minecraft:chat")
        .unwrap();
    let mut out = Vec::new();
    DisguisedChat {
        message,
        // the holder id is the registry id plus one
        chat_type: lodeframe::protocol::VarInt(chat_type as i32 + 1),
        name: Component::text(name),
        target_name: None,
    }
    .encode(&mut out)
    .unwrap();
    out
}

fn chat_lines(player: &mut FakePlayer) -> Vec<Received> {
    player
        .drain()
        .into_iter()
        .filter(|r| r.id == out::DISGUISED_CHAT)
        .collect()
}

fn two_players() -> (TestEnv<World>, FakePlayer, FakePlayer) {
    let mut env = env();
    let mut steve = env.connect("Steve");
    let mut alex = env.connect("Alex");
    steve.drain();
    alex.drain();
    (env, steve, alex)
}

#[test]
fn a_chat_line_reaches_everyone_including_the_sender() {
    let (mut env, mut steve, mut alex) = two_players();
    let seen = Recorder::<ChatEvent>::attach(env.instance_mut().events_mut());

    say(&mut env, &steve, "hello");

    for player in [&mut steve, &mut alex] {
        let lines = chat_lines(player);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0].payload(),
            chat_line("Steve", Component::text("hello"))
        );
    }
    let events = seen.take();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].player.uuid(), steve.uuid());
    assert_eq!(events[0].name, "Steve");
    assert_eq!(events[0].message, Component::text("hello"));
}

#[test]
fn a_cancelled_chat_line_reaches_nobody() {
    let (mut env, mut steve, mut alex) = two_players();
    env.instance_mut()
        .events_mut()
        .on(|e: &mut ChatEvent, _| e.cancel());

    say(&mut env, &steve, "hello");

    assert!(chat_lines(&mut steve).is_empty());
    assert!(chat_lines(&mut alex).is_empty());
}

#[test]
fn a_handler_can_replace_the_message_with_a_styled_one() {
    let (mut env, mut steve, mut alex) = two_players();
    let styled = Component::text("[vip] hello").color(Color::Red).bold();
    let replacement = styled.clone();
    env.instance_mut()
        .events_mut()
        .on(move |e: &mut ChatEvent, _| e.message = replacement.clone());

    say(&mut env, &steve, "hello");

    for player in [&mut steve, &mut alex] {
        let lines = chat_lines(player);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].payload(), chat_line("Steve", styled.clone()));
    }
}

#[test]
fn a_handler_can_act_on_the_world() {
    let (mut env, mut steve, mut alex) = two_players();
    env.instance_mut()
        .events_mut()
        .on(|e: &mut ChatEvent, ctx: &mut Ctx| {
            e.cancel();
            ctx.send_message(e.player, &Component::text("chat is closed"));
        });

    say(&mut env, &steve, "hello");

    let told = steve.drain();
    assert_eq!(count(&told, out::SYSTEM_CHAT), 1);
    assert_eq!(count(&told, out::DISGUISED_CHAT), 0);
    assert!(alex.drain().is_empty());
}

#[test]
fn lines_the_game_would_not_send_are_ignored() {
    let (mut env, mut steve, mut alex) = two_players();

    for line in ["", &"a".repeat(257), "a\u{7}b", "\u{a7}cred", "a\u{7f}"] {
        say(&mut env, &steve, line);
    }
    // the longest line is fine
    say(&mut env, &steve, &"a".repeat(256));

    assert_eq!(chat_lines(&mut alex).len(), 1);
    assert!(!steve.is_disconnected());
}

#[test]
fn a_command_is_not_chat() {
    let (mut env, steve, mut alex) = two_players();
    let mut body = vec![serverbound::CHAT_COMMAND as u8, 6];
    body.extend(b"say hi");

    env.send_raw(&steve, body);

    assert!(alex.drain().is_empty());
}

#[test]
fn server_messages_go_to_one_player_or_everyone() {
    let (mut env, mut steve, mut alex) = two_players();
    let note = Component::text("welcome").color(Color::Gold);

    let steve_id = env.instance().player_id(steve.uuid()).unwrap();
    env.instance_mut().send_message(steve_id, &note);
    assert_eq!(count(&steve.drain(), out::SYSTEM_CHAT), 1);
    assert!(alex.drain().is_empty());

    env.instance_mut().broadcast(&note);
    let sent = steve.drain();
    assert_eq!(count(&sent, out::SYSTEM_CHAT), 1);
    assert_eq!(count(&alex.drain(), out::SYSTEM_CHAT), 1);

    let mut expected = Vec::new();
    SystemChat {
        content: note,
        overlay: false,
    }
    .encode(&mut expected)
    .unwrap();
    assert_eq!(sent[0].payload(), expected);
}

/// The top layer of the flat world: grass at y = -61, air above it.
const GROUND: BlockPos = BlockPos::new(0, -61, 0);

fn dig(pos: BlockPos, sequence: i32) -> PlayerAction {
    PlayerAction {
        action: VarInt(0),
        pos,
        face: Direction::Up.id(),
        sequence: VarInt(sequence),
    }
}

fn click(pos: BlockPos, face: i32, sequence: i32) -> UseItemOn {
    UseItemOn {
        hand: VarInt(0),
        pos,
        face: VarInt(face),
        cursor_x: 0.5,
        cursor_y: 1.0,
        cursor_z: 0.5,
        inside: false,
        world_border_hit: false,
        sequence: VarInt(sequence),
    }
}

fn block_updates(received: &[Received]) -> Vec<BlockUpdate> {
    received
        .iter()
        .filter(|r| r.is::<BlockUpdate>())
        .map(|r| r.decode().unwrap())
        .collect()
}

/// The sequence of the one `BlockChangedAck` in `received`, which must come last.
fn ack(received: &[Received]) -> i32 {
    let acks: Vec<BlockChangedAck> = received
        .iter()
        .filter(|r| r.is::<BlockChangedAck>())
        .map(|r| r.decode().unwrap())
        .collect();
    assert_eq!(acks.len(), 1);
    assert!(received.last().unwrap().is::<BlockChangedAck>());
    acks[0].sequence.0
}

#[test]
fn breaking_a_block_shows_air_to_everyone() {
    let (mut env, mut steve, mut alex) = two_players();
    let seen = Recorder::<BlockBreakEvent>::attach(env.instance_mut().events_mut());
    let grass = env.instance_mut().block(GROUND).unwrap();
    assert_ne!(grass, AIR.default_state());

    env.send(&steve, &dig(GROUND, 7));

    let air = BlockUpdate {
        pos: GROUND,
        state: AIR.default_state(),
    };
    let told = steve.drain();
    assert_eq!(block_updates(&told), std::slice::from_ref(&air));
    assert_eq!(ack(&told), 7);
    assert_eq!(block_updates(&alex.drain()), [air]);
    assert_eq!(env.instance_mut().block(GROUND), Some(AIR.default_state()));
    let events = seen.take();
    assert_eq!(events.len(), 1);
    assert_eq!(
        (events[0].player.uuid(), events[0].pos),
        (steve.uuid(), GROUND)
    );
    assert_eq!(events[0].block, grass);
}

#[test]
fn placing_a_block_goes_on_the_clicked_face() {
    let faces = [
        (Direction::Down, (0, -1, 0)),
        (Direction::Up, (0, 1, 0)),
        (Direction::North, (0, 0, -1)),
        (Direction::South, (0, 0, 1)),
        (Direction::West, (-1, 0, 0)),
        (Direction::East, (1, 0, 0)),
    ];
    for (n, (face, (dx, dy, dz))) in faces.into_iter().enumerate() {
        let (mut env, mut steve, mut alex) = two_players();
        let seen = Recorder::<BlockPlaceEvent>::attach(env.instance_mut().events_mut());
        let at = BlockPos::new(dx, -61 + dy, dz);

        env.send(&steve, &click(GROUND, i32::from(face.id()), n as i32));

        let stone = BlockUpdate {
            pos: at,
            state: STONE.default_state(),
        };
        let told = steve.drain();
        assert_eq!(
            block_updates(&told),
            std::slice::from_ref(&stone),
            "{face:?}"
        );
        assert_eq!(ack(&told), n as i32);
        assert_eq!(block_updates(&alex.drain()), [stone], "{face:?}");
        assert_eq!(env.instance_mut().block(at), Some(STONE.default_state()));
        let events = seen.take();
        assert_eq!(events.len(), 1);
        assert_eq!((events[0].pos, events[0].face), (at, face));
    }
}

#[test]
fn a_cancelled_break_puts_the_block_back_for_the_player_only() {
    let (mut env, mut steve, mut alex) = two_players();
    env.instance_mut()
        .events_mut()
        .on(|e: &mut BlockBreakEvent, _| e.cancel());
    let grass = env.instance_mut().block(GROUND).unwrap();

    env.send(&steve, &dig(GROUND, 3));

    let told = steve.drain();
    assert_eq!(
        block_updates(&told),
        [BlockUpdate {
            pos: GROUND,
            state: grass
        }]
    );
    assert_eq!(ack(&told), 3);
    assert!(alex.drain().is_empty());
    assert_eq!(env.instance_mut().block(GROUND), Some(grass));
}

#[test]
fn a_cancelled_place_takes_the_predicted_block_away_from_the_player_only() {
    let (mut env, mut steve, mut alex) = two_players();
    env.instance_mut()
        .events_mut()
        .on(|e: &mut BlockPlaceEvent, _| e.cancel());
    let above = BlockPos::new(0, -60, 0);

    env.send(&steve, &click(GROUND, 1, 4));

    let told = steve.drain();
    assert_eq!(
        block_updates(&told),
        [BlockUpdate {
            pos: above,
            state: AIR.default_state()
        }]
    );
    assert_eq!(ack(&told), 4);
    assert!(alex.drain().is_empty());
    assert_eq!(env.instance_mut().block(above), Some(AIR.default_state()));
}

#[test]
fn a_handler_can_change_what_is_placed() {
    let (mut env, mut steve, mut alex) = two_players();
    env.instance_mut()
        .events_mut()
        .on(|e: &mut BlockPlaceEvent, _| e.block = COBBLESTONE.default_state());
    let above = BlockPos::new(0, -60, 0);

    env.send(&steve, &click(GROUND, 1, 1));

    let cobble = BlockUpdate {
        pos: above,
        state: COBBLESTONE.default_state(),
    };
    assert_eq!(block_updates(&steve.drain()), std::slice::from_ref(&cobble));
    assert_eq!(block_updates(&alex.drain()), [cobble]);
}

#[test]
fn edits_the_world_has_no_place_for_are_answered_and_change_nothing() {
    let (mut env, mut steve, mut alex) = two_players();
    let top = BlockPos::new(0, 319, 0);
    let bottom = BlockPos::new(0, -64, 0);

    // digging air, placing above the highest block, placing below the lowest, a face that
    // does not exist
    env.send(&steve, &dig(BlockPos::new(0, -50, 0), 1));
    env.send(&steve, &click(top, 1, 2));
    env.send(&steve, &click(bottom, 0, 3));
    env.send(&steve, &click(GROUND, 9, 4));

    let told = steve.drain();
    assert_eq!(count(&told, out::BLOCK_CHANGED_ACK), 4);
    // each answer undoes what the player predicted, if there is a block there at all
    let resync = block_updates(&told);
    assert_eq!(resync.len(), 2);
    assert_eq!(resync[0].state, AIR.default_state());
    assert_eq!(resync[1].pos, GROUND);
    assert!(alex.drain().is_empty());
    assert!(!steve.is_disconnected());
}

#[test]
fn a_chunk_the_loader_does_not_have_is_not_edited() {
    let flat = FlatGenerator::default();
    let mut world = World::new(&Registries::vanilla(), move |pos: ChunkPos| {
        if pos.x >= 100 { None } else { flat.load(pos) }
    });
    world.view_distance = 2;
    let mut env = TestEnv::new(world);
    let mut steve = env.connect("Steve");
    steve.drain();
    let far = BlockPos::new(100 * 16, -61, 0);

    env.send(&steve, &click(far, 1, 5));

    let told = steve.drain();
    assert_eq!(ack(&told), 5);
    assert!(block_updates(&told).is_empty());
    assert_eq!(env.instance_mut().block(far), None);
    assert!(!env.instance_mut().set_block(far, STONE.default_state()));
}

#[test]
fn the_other_player_actions_are_not_edits() {
    let (mut env, mut steve, mut alex) = two_players();
    // 5 is dropping one item
    env.send(
        &steve,
        &PlayerAction {
            action: VarInt(5),
            ..dig(GROUND, 1)
        },
    );

    assert!(steve.drain().is_empty());
    assert!(alex.drain().is_empty());
    assert_ne!(env.instance_mut().block(GROUND), Some(AIR.default_state()));
}

#[test]
fn set_block_shows_the_change_to_everyone() {
    let (mut env, mut steve, mut alex) = two_players();
    let at = BlockPos::new(3, -60, 3);

    assert!(env.instance_mut().set_block(at, STONE.default_state()));

    let stone = BlockUpdate {
        pos: at,
        state: STONE.default_state(),
    };
    assert_eq!(block_updates(&steve.drain()), std::slice::from_ref(&stone));
    assert_eq!(block_updates(&alex.drain()), [stone]);
    assert_eq!(env.instance_mut().block(at), Some(STONE.default_state()));
    // above the highest block: nothing changes and nothing is sent
    let sky = BlockPos::new(0, 320, 0);
    assert!(!env.instance_mut().set_block(sky, STONE.default_state()));
    assert!(steve.drain().is_empty());
}

fn position(received: &[Received], id: i32) -> usize {
    received
        .iter()
        .position(|r| r.id == id)
        .unwrap_or_else(|| panic!("no packet {id}"))
}

#[test]
fn a_message_from_the_join_handler_reaches_the_newcomer_before_their_chunks() {
    let mut env = env();
    env.instance_mut()
        .events_mut()
        .on(|e: &mut PlayerJoinEvent, ctx: &mut Ctx| {
            ctx.send_message(e.player, &Component::text("welcome"));
        });

    let got = env.connect("Steve").drain();

    let welcome = position(&got, out::SYSTEM_CHAT);
    assert!(position(&got, out::LOGIN) < welcome);
    assert!(welcome < position(&got, out::LEVEL_CHUNK_WITH_LIGHT));
}

#[test]
fn the_others_have_heard_of_a_join_by_the_time_the_handler_runs() {
    let mut env = env();
    let seen = Recorder::<PlayerJoinEvent>::attach(env.instance_mut().events_mut());
    env.instance_mut()
        .events_mut()
        .on(|e: &mut PlayerJoinEvent, ctx: &mut Ctx| {
            ctx.broadcast(&Component::text(format!("+ {}", e.name)));
        });
    let mut steve = env.connect("Steve");
    steve.drain();

    let alex = env.connect("Alex");

    let told = steve.drain();
    let shown = position(&told, out::ADD_ENTITY);
    assert!(position(&told, out::PLAYER_INFO_UPDATE) < shown);
    assert!(shown < position(&told, out::SYSTEM_CHAT));
    let events = seen.take();
    assert_eq!(
        events.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        ["Steve", "Alex"]
    );
    assert_eq!(events[1].player.uuid(), alex.uuid());
}

#[test]
fn a_leave_is_announced_after_the_others_were_told() {
    let (mut env, steve, mut alex) = two_players();
    let seen = Recorder::<PlayerLeaveEvent>::attach(env.instance_mut().events_mut());
    env.instance_mut()
        .events_mut()
        .on(|e: &mut PlayerLeaveEvent, ctx: &mut Ctx| {
            ctx.broadcast(&Component::text(format!("- {}", e.name)));
        });
    let steve_uuid = steve.uuid();

    env.disconnect(steve);

    let told = alex.drain();
    let removed = position(&told, out::REMOVE_ENTITIES);
    assert!(removed < position(&told, out::PLAYER_INFO_REMOVE));
    assert!(position(&told, out::PLAYER_INFO_REMOVE) < position(&told, out::SYSTEM_CHAT));
    let events = seen.take();
    assert_eq!(events.len(), 1);
    assert_eq!(
        (events[0].player.uuid(), events[0].name.as_str()),
        (steve_uuid, "Steve")
    );
}

fn throttled(per_tick: usize) -> TestEnv<World> {
    let mut world = World::new(&Registries::vanilla(), FlatGenerator::default());
    world.view_distance = 2;
    world.chunks_per_tick = per_tick;
    TestEnv::new(world)
}

/// The positions of the chunks in `received`, in the order they came.
fn chunks_in(received: &[Received]) -> Vec<(i32, i32)> {
    received
        .iter()
        .filter(|r| r.is::<LevelChunkWithLight>())
        .map(|r| {
            let chunk: LevelChunkWithLight = r.decode().unwrap();
            (chunk.x, chunk.z)
        })
        .collect()
}

/// The `count` of every `ChunkBatchFinished` in `received`.
fn batches_in(received: &[Received]) -> Vec<i32> {
    received
        .iter()
        .filter(|r| r.is::<ChunkBatchFinished>())
        .map(|r| r.decode::<ChunkBatchFinished>().unwrap().count.0)
        .collect()
}

#[test]
fn chunks_arrive_a_batch_at_a_time_nearest_first() {
    let mut env = throttled(4);
    let mut steve = env.connect("Steve");

    // the first batch comes with the join, and starts with the chunk the player stands in
    let first = steve.drain();
    assert_eq!(batches_in(&first), [4]);
    let mut seen = chunks_in(&first);
    assert_eq!(seen.len(), 4);
    assert_eq!(seen[0], (0, 0));

    // then one batch per tick until the 25 chunks of the view are there
    for _ in 0..6 {
        env.tick(1);
        let batch = steve.drain();
        assert_eq!(batches_in(&batch).len(), 1);
        seen.extend(chunks_in(&batch));
    }
    assert_eq!(seen.len(), 25);
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), 25);
    // nothing more is sent
    env.tick(3);
    assert!(steve.drain().is_empty());
}

#[test]
fn crossing_a_border_sends_a_batch_at_once_and_the_rest_over_the_ticks() {
    let mut env = throttled(2);
    let mut steve = env.connect("Steve");
    env.tick(20);
    steve.drain();

    // one column of 5 chunks is new
    env.send(&steve, &walk(16.5));
    let at_once = steve.drain();
    assert_eq!(chunks_in(&at_once).len(), 2);
    assert_eq!(count(&at_once, out::FORGET_LEVEL_CHUNK), 5);

    env.tick(1);
    assert_eq!(chunks_in(&steve.drain()).len(), 2);
    env.tick(1);
    assert_eq!(chunks_in(&steve.drain()).len(), 1);
    env.tick(1);
    assert!(steve.drain().is_empty());
}

#[test]
fn chunks_that_are_no_longer_in_view_are_not_sent() {
    let mut env = throttled(2);
    let mut steve = env.connect("Steve");
    env.tick(20);
    steve.drain();

    // two jumps in a row: the second leaves the view of the first behind before it was sent
    env.send(&steve, &walk(3.0 * 16.0 + 0.5));
    let first = chunks_in(&steve.drain());
    env.send(&steve, &walk(8.0 * 16.0 + 0.5));
    let mut after = chunks_in(&steve.drain());
    env.tick(30);
    after.extend(chunks_in(&steve.drain()));

    assert_eq!(first.len(), 2);
    // exactly the view around chunk x = 8: what the first jump had left to send is gone
    let in_view = |(x, z): (i32, i32)| (6..=10).contains(&x) && (-2..=2).contains(&z);
    assert_eq!(after.len(), 25);
    assert!(after.iter().all(|c| in_view(*c)));
}

#[test]
fn a_player_who_leaves_with_chunks_waiting_is_forgotten() {
    let mut env = throttled(1);
    let steve = env.connect("Steve");
    env.disconnect(steve);

    // the queue went with the player: ticking is harmless and the next player is served
    env.tick(5);
    let mut alex = env.connect("Alex");
    assert_eq!(chunks_in(&alex.drain()), [(0, 0)]);
}

#[test]
fn a_chunk_is_encoded_once_and_again_after_a_block_changes() {
    let at_origin = |received: &[Received]| {
        received
            .iter()
            .filter(|r| r.is::<LevelChunkWithLight>())
            .find(|r| {
                let chunk: LevelChunkWithLight = r.decode().unwrap();
                (chunk.x, chunk.z) == (0, 0)
            })
            .map(|r| r.payload().to_vec())
            .unwrap()
    };
    let mut env = env();
    let first = at_origin(&env.connect("Steve").drain());
    let second = at_origin(&env.connect("Alex").drain());
    assert_eq!(first, second);

    // a change in the chunk: the next player gets the chunk as it is now
    assert!(
        env.instance_mut()
            .set_block(BlockPos::new(3, -60, 3), STONE.default_state())
    );
    let changed = at_origin(&env.connect("Dave").drain());
    assert_ne!(changed, first);
    let again = at_origin(&env.connect("Eve").drain());
    assert_eq!(again, changed);

    // and it is the chunk a world that had the block from the start would send
    let mut fresh = env_with_block();
    assert_eq!(at_origin(&fresh.connect("Zed").drain()), changed);
}

fn env_with_block() -> TestEnv<World> {
    let mut env = env();
    assert!(
        env.instance_mut()
            .set_block(BlockPos::new(3, -60, 3), STONE.default_state())
    );
    env
}

/// Far enough from the spawn, with a view distance of 2, to be out of sight: chunk 5.
const FAR: f64 = 5.0 * 16.0 + 0.5;

fn sneak(env: &mut TestEnv<World>, who: &FakePlayer, on: bool) {
    env.send(
        who,
        &PlayerInput {
            flags: if on { INPUT_SNEAK } else { 0 },
        },
    );
}

fn removed_entities(received: &[Received]) -> Vec<i32> {
    received
        .iter()
        .filter(|r| r.is::<RemoveEntities>())
        .flat_map(|r| r.decode::<RemoveEntities>().unwrap().entity_ids)
        .map(|e| e.0)
        .collect()
}

#[test]
fn players_out_of_sight_do_not_get_each_others_moves_or_sneaking() {
    let (mut env, mut steve, mut alex) = two_players();
    env.send(&steve, &walk(FAR));
    steve.drain();
    alex.drain();

    env.send(&alex, &walk(3.0));
    sneak(&mut env, &alex, true);
    env.send(&steve, &walk(FAR + 1.0));

    assert!(steve.drain().is_empty());
    assert!(alex.drain().is_empty());
}

#[test]
fn walking_out_of_sight_removes_the_entity_and_coming_back_spawns_it_where_it_stands() {
    let (mut env, mut steve, mut alex) = two_players();
    // the entity ids are given out in the order of joining
    let (steve_entity, alex_entity) = (1, 2);

    env.send(&steve, &walk(FAR));
    let steve_told = steve.drain();
    let alex_told = alex.drain();
    assert_eq!(removed_entities(&steve_told), [alex_entity]);
    assert_eq!(removed_entities(&alex_told), [steve_entity]);
    // the list is for everyone, in sight or not
    assert_eq!(count(&steve_told, out::PLAYER_INFO_REMOVE), 0);

    // Alex moves within the chunk they are in; then Steve comes back
    env.send(&alex, &walk(10.5));
    env.tick(1);
    assert!(steve.drain().is_empty());
    env.send(&steve, &walk(0.5));

    let back = steve.drain();
    let spawned: Vec<AddEntity> = back
        .iter()
        .filter(|r| r.is::<AddEntity>())
        .map(|r| r.decode().unwrap())
        .collect();
    assert_eq!(spawned.len(), 1);
    assert_eq!(spawned[0].entity_id.0, alex_entity);
    assert_eq!(spawned[0].position, Vec3::new(10.5, -60.0, 0.5));
    let seen_by_alex = alex.drain();
    assert_eq!(count(&seen_by_alex, out::ADD_ENTITY), 1);
    assert_eq!(count(&back, out::PLAYER_INFO_UPDATE), 0);
}

#[test]
fn a_sneaking_player_comes_into_view_sitting_down() {
    let (mut env, mut steve, mut alex) = two_players();
    env.send(&steve, &walk(FAR));
    sneak(&mut env, &alex, true);
    steve.drain();
    alex.drain();

    env.send(&steve, &walk(0.5));

    let back = steve.drain();
    assert!(position(&back, out::ADD_ENTITY) < position(&back, out::SET_ENTITY_DATA));
    let sitting: SetEntityFlagsAndPose = back
        .iter()
        .find(|r| r.is::<SetEntityFlagsAndPose>())
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!(sitting.entity_id.0, 2);
    assert_eq!(sitting.flags, FLAG_SNEAKING);
}

#[test]
fn a_player_who_joins_out_of_sight_of_the_others_gets_the_list_but_no_entities() {
    let mut env = env();
    let mut steve = env.connect("Steve");
    env.send(&steve, &walk(FAR));
    steve.drain();

    let mut alex = env.connect("Alex");

    for told in [steve.drain(), alex.drain()] {
        assert_eq!(count(&told, out::PLAYER_INFO_UPDATE), 1);
        assert_eq!(count(&told, out::ADD_ENTITY), 0);
    }
    // and nothing either does reaches the other
    env.send(&alex, &walk(3.0));
    assert!(steve.drain().is_empty());
}

#[test]
fn a_leave_removes_the_entity_for_those_in_sight_and_the_list_entry_for_everyone() {
    let mut env = env();
    let steve = env.connect("Steve");
    let mut alex = env.connect("Alex");
    let mut dave = env.connect("Dave");
    env.send(&dave, &walk(FAR));
    alex.drain();
    dave.drain();

    env.disconnect(steve);

    let near = alex.drain();
    assert_eq!(removed_entities(&near), [1]);
    assert_eq!(count(&near, out::PLAYER_INFO_REMOVE), 1);
    let far = dave.drain();
    assert!(removed_entities(&far).is_empty());
    assert_eq!(count(&far, out::PLAYER_INFO_REMOVE), 1);
}

#[test]
fn block_changes_reach_only_players_who_have_the_chunk_in_view() {
    let (mut env, mut steve, mut alex) = two_players();
    env.send(&steve, &walk(FAR));
    steve.drain();
    alex.drain();

    assert!(
        env.instance_mut()
            .set_block(BlockPos::new(3, -60, 3), STONE.default_state())
    );

    assert_eq!(count(&alex.drain(), out::BLOCK_UPDATE), 1);
    assert!(steve.drain().is_empty());
    // chat is not a matter of sight
    say(&mut env, &alex, "hello");
    assert_eq!(chat_lines(&mut steve).len(), 1);
    // the block changed at Steve's end of the world reaches Steve and not Alex
    assert!(
        env.instance_mut()
            .set_block(BlockPos::new(88, -60, 3), STONE.default_state())
    );
    assert_eq!(count(&steve.drain(), out::BLOCK_UPDATE), 1);
    assert!(block_updates(&alex.drain()).is_empty());
}

#[test]
fn the_entity_view_distance_sets_how_far_players_see_each_other() {
    let mut world = World::new(&Registries::vanilla(), FlatGenerator::default());
    world.view_distance = 4;
    world.chunks_per_tick = usize::MAX;
    world.entity_view_distance = 1;
    let mut env = TestEnv::new(world);
    let mut steve = env.connect("Steve");
    let mut alex = env.connect("Alex");
    steve.drain();
    alex.drain();

    // two chunks away: out of sight, though the chunks are still in view
    env.send(&steve, &walk(2.0 * 16.0 + 0.5));
    assert_eq!(removed_entities(&steve.drain()), [2]);
    // one chunk away: in sight again
    env.send(&steve, &walk(16.5));
    assert_eq!(count(&steve.drain(), out::ADD_ENTITY), 1);
}

#[test]
fn a_crowd_in_view_is_spawned_in_one_message() {
    let mut env = env();
    let _crowd: Vec<FakePlayer> = ["A", "B", "C", "D"].map(|n| env.connect(n)).into();
    let mut newcomer = env.connect("Newcomer");

    let messages = newcomer.drain_messages();

    // the four entities arrive together, so a crowd cannot fill the queue of a connection
    let with_entities: Vec<&Vec<Received>> = messages
        .iter()
        .filter(|m| m.iter().any(|r| r.is::<AddEntity>()))
        .collect();
    assert_eq!(with_entities.len(), 1);
    assert_eq!(count(with_entities[0], out::ADD_ENTITY), 4);
}

#[test]
fn walking_away_from_and_back_to_a_crowd_is_one_message_each_way() {
    let mut env = env();
    let mut steve = env.connect("Steve");
    let _crowd: Vec<FakePlayer> = ["A", "B", "C"].map(|n| env.connect(n)).into();
    steve.drain();

    env.send(&steve, &walk(FAR));
    let away = steve.drain_messages();
    let removals: Vec<&Received> = away
        .iter()
        .flatten()
        .filter(|r| r.is::<RemoveEntities>())
        .collect();
    assert_eq!(removals.len(), 1);
    assert_eq!(removed_entities(&[removals[0].clone()]).len(), 3);

    env.send(&steve, &walk(0.5));
    let back = steve.drain_messages();
    let with_entities: Vec<&Vec<Received>> = back
        .iter()
        .filter(|m| m.iter().any(|r| r.is::<AddEntity>()))
        .collect();
    assert_eq!(with_entities.len(), 1);
    assert_eq!(count(with_entities[0], out::ADD_ENTITY), 3);
}

#[test]
fn a_second_login_of_the_same_name_cuts_off_the_first_and_leaves_the_second_alone() {
    let mut env = env();
    let joins = Recorder::<PlayerJoinEvent>::attach(env.instance_mut().events_mut());
    let leaves = Recorder::<PlayerLeaveEvent>::attach(env.instance_mut().events_mut());
    let mut first = env.connect("Steve");
    let mut alex = env.connect("Alex");
    first.drain();
    alex.drain();

    let mut second = env.connect("Steve");

    // the first is told why and then cut off
    let told = first.drain();
    let mut reason = Vec::new();
    lodeframe::protocol::packets::play::Disconnect {
        reason: Component::text("You logged in from another location"),
    }
    .encode(&mut reason)
    .unwrap();
    assert_eq!(count(&told, out::DISCONNECT), 1);
    assert_eq!(
        told.iter()
            .find(|r| r.id == out::DISCONNECT)
            .unwrap()
            .payload(),
        reason
    );
    assert!(first.is_disconnected());
    // the second gets the world as a new player does
    assert_eq!(count(&second.drain(), out::LOGIN), 1);
    // the others see one Steve go and one come, and the old entity is not left behind
    let seen = alex.drain();
    assert_eq!(removed_entities(&seen), [1]);
    assert!(position(&seen, out::REMOVE_ENTITIES) < position(&seen, out::ADD_ENTITY));
    assert_eq!(count(&seen, out::ADD_ENTITY), 1);
    assert_eq!(leaves.take().len(), 1);
    assert_eq!(joins.take().len(), 3);

    // the old connection ends: that must not take the new one with it
    env.disconnect(first);
    assert!(alex.drain().is_empty(), "nobody left");
    env.send(&second, &walk(2.5));
    env.tick(1);
    assert_eq!(alex.drain_as::<MoveEntityPos>().len(), 1);
}

#[test]
fn a_player_id_stops_naming_a_player_who_left() {
    let mut env = env();
    let joins = Recorder::<PlayerJoinEvent>::attach(env.instance_mut().events_mut());
    let steve = env.connect("Steve");
    let mut alex = env.connect("Alex");
    let id = joins.take()[0].player;
    assert_eq!(id.uuid(), steve.uuid());
    assert!(env.instance().is_online(id));
    assert_eq!(env.instance().name(id), Some("Steve"));
    assert_eq!(env.instance().player_id(steve.uuid()), Some(id));
    alex.drain();

    env.disconnect(steve);

    // nothing to find, and nothing breaks: the calls do nothing
    assert!(!env.instance().is_online(id));
    assert_eq!(env.instance().name(id), None);
    assert_eq!(env.instance().player_id(id.uuid()), None);
    env.instance_mut()
        .send_message(id, &Component::text("anyone there?"));
    assert_eq!(count(&alex.drain(), out::SYSTEM_CHAT), 0);
}

#[test]
fn an_old_player_id_does_not_name_the_player_who_came_after() {
    let mut env = env();
    let joins = Recorder::<PlayerJoinEvent>::attach(env.instance_mut().events_mut());

    // the player leaves and comes back
    let first = env.connect("Steve");
    let first_id = joins.take()[0].player;
    env.disconnect(first);
    let mut second = env.connect("Steve");
    let second_id = joins.take()[0].player;
    second.drain();

    assert_eq!(first_id.uuid(), second_id.uuid());
    assert_ne!(first_id, second_id);
    assert!(!env.instance().is_online(first_id));
    assert!(env.instance().is_online(second_id));
    env.instance_mut()
        .send_message(first_id, &Component::text("for the first"));
    assert_eq!(count(&second.drain(), out::SYSTEM_CHAT), 0);
    env.instance_mut()
        .send_message(second_id, &Component::text("for the second"));
    assert_eq!(count(&second.drain(), out::SYSTEM_CHAT), 1);

    // the same when the second login cuts off the first
    let third = env.connect("Steve");
    let third_id = joins.take()[0].player;
    assert!(!env.instance().is_online(second_id));
    assert!(env.instance().is_online(third_id));
    drop(third);
}

#[test]
fn a_player_dropped_while_a_handler_runs_leaves_after_it() {
    let mut env = env();
    let leaves = Recorder::<PlayerLeaveEvent>::attach(env.instance_mut().events_mut());
    env.instance_mut()
        .events_mut()
        .on(|_: &mut ChatEvent, ctx: &mut Ctx| ctx.broadcast(&Component::text("heard")));
    let steve = env.connect("Steve");
    let alex = env.connect("Alex");
    // a client that no longer reads: the message to it fails and it is dropped
    drop(alex);

    say(&mut env, &steve, "hello");

    let left = leaves.take();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].name, "Alex");
    assert!(!env.instance().is_online(left[0].player));
}

#[test]
fn handlers_do_not_name_the_loader_of_the_world() {
    // a closure is a loader, and the world and its handlers are the same types as with the
    // flat generator
    let flat = FlatGenerator::default();
    let mut world = World::new(&Registries::vanilla(), move |pos| flat.load(pos));
    world.view_distance = 2;
    world
        .events_mut()
        .on(|_: &mut PlayerJoinEvent, _: &mut Ctx| {});
    let mut env = TestEnv::new(world);

    let mut steve = env.connect("Steve");

    assert_eq!(count(&steve.drain(), out::LOGIN), 1);
}

/// An event of the test's own that a handler emits from inside another handler.
struct Ping(u32);
impl Event for Ping {}

type Log = std::rc::Rc<std::cell::RefCell<Vec<String>>>;

fn log() -> Log {
    Log::default()
}

fn note(log: &Log, line: impl Into<String>) {
    log.borrow_mut().push(line.into());
}

#[test]
fn an_event_a_handler_emits_arrives_after_it_in_the_same_call() {
    let mut env = env();
    let seen = log();
    let events = env.instance_mut().events_mut();
    let l = seen.clone();
    events.on(move |_: &mut ChatEvent, ctx: &mut Ctx| {
        ctx.emit(Ping(1));
        note(&l, "chat");
    });
    let l = seen.clone();
    events.on(move |e: &mut Ping, ctx: &mut Ctx| {
        note(&l, format!("ping {}", e.0));
        if e.0 < 3 {
            ctx.emit(Ping(e.0 + 1));
        }
    });
    let steve = env.connect("Steve");

    // no tick runs in between: the events come with the packet that caused them
    say(&mut env, &steve, "hello");

    assert_eq!(*seen.borrow(), ["chat", "ping 1", "ping 2", "ping 3"]);
}

#[test]
fn a_handler_that_keeps_emitting_is_stopped() {
    let mut env = env();
    let count = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let events = env.instance_mut().events_mut();
    let n = count.clone();
    events.on(|_: &mut ChatEvent, ctx: &mut Ctx| ctx.emit(Ping(0)));
    events.on(move |_: &mut Ping, ctx: &mut Ctx| {
        n.set(n.get() + 1);
        ctx.emit(Ping(0));
    });
    let steve = env.connect("Steve");

    say(&mut env, &steve, "hello");

    // it ran many times, but the world went on
    assert!(count.get() >= 100, "ran {} times", count.get());
    assert!(count.get() <= 1000);
    assert!(
        env.instance()
            .is_online(env.instance().player_id(steve.uuid()).unwrap())
    );
}

#[test]
fn a_listener_added_by_a_handler_hears_the_next_event_not_this_one() {
    let mut env = env();
    let seen = log();
    let l = seen.clone();
    env.instance_mut().events_mut().add_listener(
        Listener::new(move |_: &mut ChatEvent, ctx: &mut Ctx| {
            note(&l, "first");
            let l = l.clone();
            ctx.add_listener(
                Listener::new(move |_: &mut ChatEvent, _: &mut Ctx| note(&l, "added")).times(1),
            );
        })
        .times(1),
    );
    let steve = env.connect("Steve");

    say(&mut env, &steve, "one");
    assert_eq!(*seen.borrow(), ["first"]);
    say(&mut env, &steve, "two");
    say(&mut env, &steve, "three");

    // the added listener ran once, then took itself off
    assert_eq!(*seen.borrow(), ["first", "added"]);
}

#[test]
fn a_handler_can_take_a_listener_off_and_a_node_on_and_off() {
    let mut env = env();
    let seen = log();
    let l = seen.clone();
    let doomed = env.instance_mut().events_mut().add_listener(Listener::new(
        move |_: &mut Ping, _: &mut Ctx| note(&l, "doomed"),
    ));
    let l = seen.clone();
    env.instance_mut()
        .events_mut()
        .on(move |_: &mut ChatEvent, ctx: &mut Ctx| {
            ctx.remove_listener(doomed);
            let mut node = EventNode::<Ctx>::new();
            let n = l.clone();
            node.on(move |_: &mut Ping, _: &mut Ctx| note(&n, "node"));
            ctx.add_node(node);
            ctx.emit(Ping(0));
        });
    let steve = env.connect("Steve");

    say(&mut env, &steve, "hello");

    // the listener was taken off before the ping, and the node was there for it
    assert_eq!(*seen.borrow(), ["node"]);
}

#[test]
fn a_parent_listener_hears_events_about_players() {
    let mut env = env();
    let seen = log();
    let l = seen.clone();
    env.instance_mut()
        .events_mut()
        .on::<dyn PlayerEvent>(move |e, ctx| {
            note(&l, ctx.name(e.player()).unwrap_or("gone"));
        });
    let steve = env.connect("Steve");
    say(&mut env, &steve, "hello");
    env.disconnect(steve);

    // join, chat, and leave (when the name can no longer be looked up)
    assert_eq!(*seen.borrow(), ["Steve", "Steve", "gone"]);
}

#[test]
fn two_worlds_do_not_hear_each_others_handlers() {
    let mut a = env();
    let mut b = env();
    let seen = log();
    let l = seen.clone();
    a.instance_mut()
        .events_mut()
        .on(move |_: &mut PlayerJoinEvent, _: &mut Ctx| note(&l, "a"));
    let l = seen.clone();
    b.instance_mut()
        .events_mut()
        .on(move |_: &mut PlayerJoinEvent, _: &mut Ctx| note(&l, "b"));

    let _steve = b.connect("Steve");

    assert_eq!(*seen.borrow(), ["b"]);
}

async fn sleeping<T>(seconds: u64, value: T) -> T {
    tokio::time::sleep(std::time::Duration::from_secs(seconds)).await;
    value
}

#[test]
fn a_spawned_result_comes_back_at_the_start_of_the_next_tick() {
    let mut env = env();
    let seen = log();
    let l = seen.clone();
    env.instance_mut()
        .events_mut()
        .on(move |_: &mut ChatEvent, ctx: &mut Ctx| {
            let l = l.clone();
            ctx.spawn(async { 21 * 2 })
                .then(move |n, _| note(&l, format!("got {n}")));
        });
    let steve = env.connect("Steve");
    say(&mut env, &steve, "hello");
    assert!(seen.borrow().is_empty());

    // the work is done, but the world has not been ticked since
    env.run_until_idle();
    assert!(seen.borrow().is_empty());
    env.tick(1);

    assert_eq!(*seen.borrow(), ["got 42"]);
}

#[test]
fn work_that_never_finishes_does_not_hold_up_the_ticks() {
    let mut env = env();
    let seen = log();
    let l = seen.clone();
    env.instance_mut()
        .events_mut()
        .on(move |_: &mut ChatEvent, ctx: &mut Ctx| {
            let l = l.clone();
            ctx.spawn(std::future::pending::<()>())
                .then(move |_, _| note(&l, "never"));
        });
    let steve = env.connect("Steve");
    say(&mut env, &steve, "hello");

    let started = std::time::Instant::now();
    env.tick(100);

    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    assert!(seen.borrow().is_empty());
}

#[test]
fn a_long_wait_costs_no_real_time_in_the_harness() {
    let mut env = env();
    let seen = log();
    let l = seen.clone();
    env.instance_mut()
        .events_mut()
        .on(move |_: &mut ChatEvent, ctx: &mut Ctx| {
            let l = l.clone();
            ctx.spawn(sleeping(3600, "an hour"))
                .then(move |s, _| note(&l, s));
        });
    let steve = env.connect("Steve");
    say(&mut env, &steve, "hello");

    let started = std::time::Instant::now();
    env.run_until_idle();
    env.tick(1);

    assert!(started.elapsed() < std::time::Duration::from_secs(2));
    assert_eq!(*seen.borrow(), ["an hour"]);
}

#[test]
fn results_arrive_in_the_order_the_futures_finished() {
    let mut env = env();
    let seen = log();
    let l = seen.clone();
    env.instance_mut()
        .events_mut()
        .on(move |_: &mut ChatEvent, ctx: &mut Ctx| {
            for (name, seconds) in [("slow", 5), ("fast", 1), ("middle", 3)] {
                let l = l.clone();
                ctx.spawn(sleeping(seconds, name))
                    .then(move |name, _| note(&l, name));
            }
        });
    let steve = env.connect("Steve");
    say(&mut env, &steve, "hello");

    env.run_until_idle();
    env.tick(1);

    assert_eq!(*seen.borrow(), ["fast", "middle", "slow"]);
}

#[test]
fn a_result_for_a_player_who_left_can_be_skipped() {
    let mut env = env();
    let seen = log();
    let l = seen.clone();
    env.instance_mut()
        .events_mut()
        .on(move |e: &mut ChatEvent, ctx: &mut Ctx| {
            let (id, name) = (e.player, e.name.clone());
            let (a, b) = (l.clone(), l.clone());
            ctx.spawn(sleeping(1, ()))
                .then_for(id, move |_, _| note(&a, format!("for {name}")));
            ctx.spawn(sleeping(1, ())).then(move |_, ctx| {
                // the id names nobody once the player left: no panic, no name
                note(&b, format!("then: {:?}", ctx.name(id)));
            });
        });
    let steve = env.connect("Steve");
    let alex = env.connect("Alex");
    say(&mut env, &steve, "one");
    say(&mut env, &alex, "two");
    env.disconnect(steve);

    env.run_until_idle();
    env.tick(1);

    let seen = seen.borrow();
    // Alex stayed; Steve left. `then` ran for both, `then_for` only for Alex
    assert_eq!(seen.iter().filter(|s| s.as_str() == "for Alex").count(), 1);
    assert_eq!(seen.iter().filter(|s| s.starts_with("for ")).count(), 1);
    assert!(seen.contains(&"then: None".to_string()));
    assert!(seen.contains(&"then: Some(\"Alex\")".to_string()));
}

#[test]
fn a_result_does_not_go_to_a_player_who_came_back_under_the_same_name() {
    let mut env = env();
    let seen = log();
    let l = seen.clone();
    env.instance_mut()
        .events_mut()
        .on(move |e: &mut ChatEvent, ctx: &mut Ctx| {
            let l = l.clone();
            ctx.spawn(sleeping(1, ()))
                .then_for(e.player, move |_, _| note(&l, "called"));
        });
    let first = env.connect("Steve");
    say(&mut env, &first, "hello");
    env.disconnect(first);
    let _second = env.connect("Steve");

    env.run_until_idle();
    env.tick(1);

    assert!(seen.borrow().is_empty());
}

#[test]
fn a_callback_can_emit_and_spawn_and_the_new_work_waits_for_the_next_tick() {
    let mut env = env();
    let seen = log();
    let events = env.instance_mut().events_mut();
    let l = seen.clone();
    events.on(move |_: &mut ChatEvent, ctx: &mut Ctx| {
        let l = l.clone();
        ctx.spawn(async { 1 }).then(move |n, ctx| {
            note(&l, format!("first {n}"));
            ctx.emit(Ping(n));
            let l = l.clone();
            ctx.spawn(async { 2 })
                .then(move |n, _| note(&l, format!("second {n}")));
        });
    });
    let l = seen.clone();
    events.on(move |e: &mut Ping, _: &mut Ctx| note(&l, format!("ping {}", e.0)));
    let steve = env.connect("Steve");
    say(&mut env, &steve, "hello");

    env.run_until_idle();
    env.tick(1);
    // the event came in the same tick; the second result has not
    assert_eq!(*seen.borrow(), ["first 1", "ping 1"]);

    env.run_until_idle();
    env.tick(1);
    assert_eq!(*seen.borrow(), ["first 1", "ping 1", "second 2"]);
}

#[test]
fn a_future_that_panics_gives_no_call_and_does_not_hold_the_harness() {
    let mut env = env();
    let seen = log();
    let l = seen.clone();
    env.instance_mut()
        .events_mut()
        .on(move |_: &mut ChatEvent, ctx: &mut Ctx| {
            let l = l.clone();
            ctx.spawn(async {
                if true {
                    panic!("a failing query (expected in this test)");
                }
            })
            .then(move |_, _| note(&l, "called"));
        });
    let steve = env.connect("Steve");
    say(&mut env, &steve, "hello");

    env.run_until_idle();
    env.tick(1);

    assert!(seen.borrow().is_empty());
    assert!(
        env.instance()
            .is_online(env.instance().player_id(steve.uuid()).unwrap())
    );
}

#[test]
#[should_panic(expected = "ctx.spawn needs a runtime")]
fn spawning_in_a_world_that_was_never_attached_says_why() {
    let flat = FlatGenerator::default();
    let mut world = World::new(&Registries::vanilla(), move |pos| flat.load(pos));
    let _ = world.spawn(async {});
}

fn id_of(env: &TestEnv<World>, player: &FakePlayer) -> lodeframe::world::PlayerId {
    env.instance().player_id(player.uuid()).unwrap()
}

#[test]
fn a_task_runs_once_when_its_delay_is_over() {
    let mut env = env();
    let seen = log();
    let l = seen.clone();
    env.instance_mut()
        .after(Delay::ticks(3))
        .once(move |_| note(&l, "ran"));

    env.tick(2);
    assert!(seen.borrow().is_empty());
    env.tick(1);
    assert_eq!(*seen.borrow(), ["ran"]);
    env.tick(10);
    assert_eq!(*seen.borrow(), ["ran"]);
}

#[test]
fn a_task_never_runs_in_the_tick_that_scheduled_it() {
    let mut env = env();
    let seen = log();
    let l = seen.clone();
    env.instance_mut().after(Delay::ZERO).once(move |ctx| {
        note(&l, "outer");
        let l = l.clone();
        ctx.after(Delay::ZERO).once(move |_| note(&l, "inner"));
    });

    env.tick(1);
    assert_eq!(*seen.borrow(), ["outer"]);
    env.tick(1);
    assert_eq!(*seen.borrow(), ["outer", "inner"]);
}

#[test]
fn a_countdown_counts_a_second_at_a_time_and_stops() {
    let mut env = env();
    let seen = log();
    let l = seen.clone();
    let mut left = 3;
    env.instance_mut().after(Delay::secs(1)).run(move |_| {
        note(&l, format!("{left}"));
        left -= 1;
        if left == 0 {
            Next::Stop
        } else {
            Next::After(Delay::secs(1))
        }
    });

    env.tick(19);
    assert!(seen.borrow().is_empty());
    env.tick(1);
    assert_eq!(*seen.borrow(), ["3"]);
    env.tick(40);
    assert_eq!(*seen.borrow(), ["3", "2", "1"]);
    env.tick(100);
    assert_eq!(seen.borrow().len(), 3);
}

#[test]
fn a_repeating_task_runs_every_period_until_it_is_cancelled() {
    let mut env = env();
    let seen = log();
    let l = seen.clone();
    let id = env
        .instance_mut()
        .after(Delay::ZERO)
        .every(std::time::Duration::from_millis(100), move |_| {
            note(&l, "tick")
        });

    env.tick(5);
    // ticks 1, 3 and 5
    assert_eq!(seen.borrow().len(), 3);
    assert!(env.instance_mut().cancel(id));
    env.tick(10);
    assert_eq!(seen.borrow().len(), 3);
    assert!(!env.instance_mut().cancel(id));
}

#[test]
fn a_task_can_cancel_itself_whatever_it_returns() {
    let mut env = env();
    let seen = log();
    let l = seen.clone();
    let me = std::rc::Rc::new(std::cell::Cell::new(None));
    let m = me.clone();
    let id = env.instance_mut().after(Delay::ZERO).run(move |ctx| {
        note(&l, "ran");
        assert!(ctx.cancel(m.get().unwrap()));
        Next::After(Delay::ZERO)
    });
    me.set(Some(id));

    env.tick(5);

    assert_eq!(*seen.borrow(), ["ran"]);
}

#[test]
fn a_task_cancels_one_that_is_due_in_the_same_tick() {
    let mut env = env();
    let seen = log();
    let l = seen.clone();
    let victim = std::rc::Rc::new(std::cell::Cell::new(None));
    let v = victim.clone();
    env.instance_mut()
        .after(Delay::ticks(1))
        .once(move |ctx| assert!(ctx.cancel(v.get().unwrap())));
    let id = env
        .instance_mut()
        .after(Delay::ticks(1))
        .once(move |_| note(&l, "ran"));
    victim.set(Some(id));

    env.tick(2);

    assert!(seen.borrow().is_empty());
}

#[test]
fn a_players_task_stops_when_they_leave() {
    let mut env = env();
    let seen = log();
    let steve = env.connect("Steve");
    let alex = env.connect("Alex");
    for player in [&steve, &alex] {
        let (id, l) = (id_of(&env, player), seen.clone());
        let name = env.instance().name(id).unwrap().to_string();
        env.instance_mut()
            .after(Delay::secs(1))
            .for_player(id)
            .every(Delay::secs(1), move |_| note(&l, name.clone()));
    }

    env.tick(20);
    assert_eq!(seen.borrow().len(), 2);
    env.disconnect(steve);
    env.tick(20);

    let seen = seen.borrow();
    assert_eq!(seen.iter().filter(|s| s.as_str() == "Alex").count(), 2);
    assert_eq!(seen.iter().filter(|s| s.as_str() == "Steve").count(), 1);
}

#[test]
fn a_task_does_not_go_to_a_player_who_came_back_under_the_same_name() {
    let mut env = env();
    let seen = log();
    let steve = env.connect("Steve");
    let (id, l) = (id_of(&env, &steve), seen.clone());
    env.instance_mut()
        .after(Delay::secs(1))
        .for_player(id)
        .every(Delay::secs(1), move |_| note(&l, "ran"));
    env.disconnect(steve);
    let _back = env.connect("Steve");

    env.tick(60);

    assert!(seen.borrow().is_empty());
}

#[test]
fn nothing_is_scheduled_for_a_player_who_is_gone() {
    let mut env = env();
    let seen = log();
    let steve = env.connect("Steve");
    let (id, l) = (id_of(&env, &steve), seen.clone());
    env.disconnect(steve);

    let task = env
        .instance_mut()
        .after(Delay::ZERO)
        .for_player(id)
        .once(move |_| note(&l, "ran"));
    env.tick(5);

    assert!(seen.borrow().is_empty());
    assert!(!env.instance_mut().cancel(task));
}

#[test]
fn tasks_run_in_the_order_of_their_due_tick_and_then_of_when_they_were_made() {
    let mut env = env();
    let seen = log();
    for (delay, name) in [(2, "c"), (1, "a"), (1, "b"), (2, "d")] {
        let l = seen.clone();
        env.instance_mut()
            .after(Delay::ticks(delay))
            .once(move |_| note(&l, name));
    }

    env.tick(2);

    assert_eq!(*seen.borrow(), ["a", "b", "c", "d"]);
}

#[test]
fn a_task_at_the_end_runs_after_the_tasks_at_the_start_of_the_same_tick() {
    let mut env = env();
    let seen = log();
    let (end, start) = (seen.clone(), seen.clone());
    // made first, but at the end of the tick
    env.instance_mut()
        .after(Delay::ZERO)
        .at_end()
        .once(move |_| note(&end, "end"));
    env.instance_mut()
        .after(Delay::ZERO)
        .once(move |_| note(&start, "start"));

    env.tick(1);

    assert_eq!(*seen.borrow(), ["start", "end"]);
}

#[test]
fn what_a_task_emits_is_handled_in_the_same_tick() {
    let mut env = env();
    let seen = log();
    let l = seen.clone();
    env.instance_mut()
        .events_mut()
        .on(move |e: &mut PlayerJoinEvent, _: &mut Ctx| note(&l, format!("join {}", e.name)));
    let steve = env.connect("Steve");
    seen.borrow_mut().clear();
    let id = id_of(&env, &steve);
    env.instance_mut().after(Delay::ZERO).once(move |ctx| {
        let name = ctx.name(id).unwrap().to_string();
        ctx.emit(PlayerJoinEvent { player: id, name });
    });

    env.tick(1);

    assert_eq!(*seen.borrow(), ["join Steve"]);
}

#[test]
fn dropping_the_world_drops_its_tasks() {
    struct Guard(std::rc::Rc<std::cell::Cell<bool>>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }
    let dropped = std::rc::Rc::new(std::cell::Cell::new(false));
    let env = {
        let mut env = env();
        let guard = Guard(dropped.clone());
        env.instance_mut().after(Delay::secs(60)).once(move |_| {
            let _keep = &guard;
        });
        env
    };
    assert!(!dropped.get());

    drop(env);

    assert!(dropped.get());
}

const SCORE: Key<u32> = Key::new("test:score");
const SCORE_TEXT: Key<String> = Key::new("test:score");

#[test]
fn what_is_attached_to_a_player_is_read_back_in_a_later_handler() {
    let mut env = env();
    let seen = log();
    let l = seen.clone();
    env.instance_mut()
        .events_mut()
        .on(|e: &mut PlayerJoinEvent, ctx: &mut Ctx| {
            ctx.player_data_mut(e.player).unwrap().set(&SCORE, 10);
        })
        .on(move |e: &mut ChatEvent, ctx: &mut Ctx| {
            let data = ctx.player_data_mut(e.player).unwrap();
            *data.get_mut(&SCORE).unwrap() += 1;
            // a key of another type under the same name finds nothing
            assert_eq!(data.get(&SCORE_TEXT), None);
            note(&l, format!("{}", data.get(&SCORE).unwrap()));
        });
    let steve = env.connect("Steve");
    let alex = env.connect("Alex");

    say(&mut env, &steve, "a");
    say(&mut env, &steve, "b");
    say(&mut env, &alex, "c");

    assert_eq!(*seen.borrow(), ["11", "12", "11"]);
}

#[test]
fn a_player_who_left_has_no_data_and_one_who_comes_back_starts_empty() {
    let mut env = env();
    let steve = env.connect("Steve");
    let old = id_of(&env, &steve);
    env.instance_mut()
        .player_data_mut(old)
        .unwrap()
        .set(&SCORE, 5);
    env.disconnect(steve);

    assert!(env.instance().player_data(old).is_none());
    assert!(env.instance_mut().player_data_mut(old).is_none());

    let back = env.connect("Steve");
    let new = id_of(&env, &back);
    assert!(
        env.instance()
            .player_data(new)
            .unwrap()
            .get(&SCORE)
            .is_none()
    );
    // the old id does not reach the new player's data
    assert!(env.instance().player_data(old).is_none());
}

#[test]
fn the_data_of_a_player_who_leaves_can_be_read_while_their_leave_is_handled() {
    struct Guard(Rc<std::cell::Cell<bool>>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }
    use std::rc::Rc;
    let mut env = env();
    let seen = log();
    let l = seen.clone();
    let dropped = Rc::new(std::cell::Cell::new(false));
    let guard = Rc::new(std::cell::RefCell::new(Some(Guard(dropped.clone()))));
    env.instance_mut()
        .events_mut()
        .on(move |e: &mut PlayerLeaveEvent, ctx: &mut Ctx| {
            let data = ctx.leaving_data(e.player).unwrap();
            note(&l, format!("{} had {:?}", e.name, data.get(&SCORE)));
            // the player is gone from the rest of the API
            assert!(ctx.player_data(e.player).is_none());
        });
    let steve = env.connect("Steve");
    let id = id_of(&env, &steve);
    let data = env.instance_mut().player_data_mut(id).unwrap();
    data.set(&SCORE, 42);
    data.set_by_type(guard.borrow_mut().take().unwrap());
    assert!(!dropped.get());

    env.disconnect(steve);

    assert_eq!(*seen.borrow(), ["Steve had Some(42)"]);
    // dropped once the handlers were done, not kept by the world
    assert!(dropped.get());
    assert!(env.instance_mut().leaving_data(id).is_none());
}

#[test]
fn leaving_data_is_only_for_the_player_who_is_leaving() {
    let mut env = env();
    let steve = env.connect("Steve");
    let alex = env.connect("Alex");
    let (steve_id, alex_id) = (id_of(&env, &steve), id_of(&env, &alex));
    env.instance_mut()
        .events_mut()
        .on(move |_: &mut PlayerLeaveEvent, ctx: &mut Ctx| {
            assert!(ctx.leaving_data(alex_id).is_none());
        });
    // nobody is leaving: nothing to find, whoever is asked about
    assert!(env.instance_mut().leaving_data(steve_id).is_none());

    env.disconnect(steve);
}

#[test]
fn what_is_attached_to_the_world_lasts_across_ticks_and_goes_with_it() {
    struct Guard(std::rc::Rc<std::cell::Cell<bool>>);
    impl Drop for Guard {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }
    let dropped = std::rc::Rc::new(std::cell::Cell::new(false));
    let mut env = env();
    env.instance_mut().data_mut().set(&SCORE, 3);
    env.instance_mut()
        .data_mut()
        .set_by_type(Guard(dropped.clone()));

    env.tick(5);

    assert_eq!(env.instance().data().get(&SCORE), Some(&3));
    assert!(!dropped.get());
    drop(env);
    assert!(dropped.get());
}

const FIRE: Key<Cooldown> = Key::new("test:fire");

fn fire_on_chat(env: &mut TestEnv<World>, seen: &Log) {
    let l = seen.clone();
    env.instance_mut()
        .events_mut()
        .on(move |e: &mut ChatEvent, ctx: &mut Ctx| {
            let now = ctx.now();
            let cd = ctx
                .player_data_mut(e.player)
                .unwrap()
                .get_or_insert_with(&FIRE, Cooldown::default);
            let used = cd.try_use(now, Delay::secs(3));
            note(&l, format!("{} {used}", e.name));
        });
}

#[test]
fn a_cooldown_waits_for_its_delay_and_is_kept_per_player() {
    let mut env = env();
    let seen = log();
    fire_on_chat(&mut env, &seen);
    let steve = env.connect("Steve");
    let alex = env.connect("Alex");

    say(&mut env, &steve, "a");
    say(&mut env, &steve, "b");
    say(&mut env, &alex, "c");
    env.tick(59);
    say(&mut env, &steve, "d");
    env.tick(1);
    say(&mut env, &steve, "e");

    assert_eq!(
        *seen.borrow(),
        [
            "Steve true",
            "Steve false",
            "Alex true",
            "Steve false",
            "Steve true"
        ]
    );
}

#[test]
fn a_player_who_comes_back_starts_without_a_cooldown() {
    let mut env = env();
    let seen = log();
    fire_on_chat(&mut env, &seen);
    let steve = env.connect("Steve");
    say(&mut env, &steve, "a");
    env.disconnect(steve);

    let back = env.connect("Steve");
    say(&mut env, &back, "b");

    assert_eq!(*seen.borrow(), ["Steve true", "Steve true"]);
}

#[test]
fn the_tick_number_goes_up_by_one_with_each_tick() {
    let mut env = env();
    let start = env.instance().now();

    env.tick(7);

    assert_eq!(env.instance().now().as_ticks() - start.as_ticks(), 7);
}

#[test]
fn a_players_ping_is_what_the_last_keep_alive_took_and_goes_with_them() {
    let mut env = env();
    let steve = env.connect("Steve");
    let alex = env.connect("Alex");
    let steve_id = id_of(&env, &steve);
    let alex_id = id_of(&env, &alex);
    assert_eq!(env.instance().ping(steve_id), None);

    env.ping(&steve, std::time::Duration::from_millis(40));
    env.ping(&steve, std::time::Duration::from_millis(25));

    let ms = std::time::Duration::from_millis;
    assert_eq!(env.instance().ping(steve_id), Some(ms(25)));
    // another player's ping is their own
    assert_eq!(env.instance().ping(alex_id), None);
    env.disconnect(steve);
    assert_eq!(env.instance().ping(steve_id), None);
}

#[test]
fn a_handler_can_read_the_ping() {
    let mut env = env();
    let seen = log();
    let l = seen.clone();
    env.instance_mut()
        .events_mut()
        .on(move |e: &mut ChatEvent, ctx: &mut Ctx| {
            note(&l, format!("{:?}", ctx.ping(e.player)));
        });
    let steve = env.connect("Steve");

    say(&mut env, &steve, "a");
    env.ping(&steve, std::time::Duration::from_millis(33));
    say(&mut env, &steve, "b");

    assert_eq!(*seen.borrow(), ["None", "Some(33ms)"]);
}

/// The payload of a play `Disconnect` that says `reason`.
fn disconnect_payload(reason: Component) -> Vec<u8> {
    let mut buf = Vec::new();
    lodeframe::protocol::packets::play::Disconnect { reason }
        .encode(&mut buf)
        .unwrap();
    buf
}

#[test]
fn a_shutdown_tells_everyone_why_and_disconnects_them() {
    let (mut env, mut steve, mut alex) = two_players();

    env.shutdown();

    for player in [&mut steve, &mut alex] {
        let told = player.drain();
        let disconnect = told
            .iter()
            .find(|r| r.id == out::DISCONNECT)
            .expect("a disconnect");
        assert_eq!(
            disconnect.payload(),
            disconnect_payload(Component::text("Server closed"))
        );
        assert!(player.is_disconnected());
    }
}

#[test]
fn the_shutdown_handlers_see_everyone_and_choose_the_reason() {
    let (mut env, mut steve, alex) = two_players();
    let seen = log();
    let l = seen.clone();
    let uuids = [steve.uuid(), alex.uuid()];
    let leaves = Recorder::<PlayerLeaveEvent>::attach(env.instance_mut().events_mut());
    env.instance_mut()
        .events_mut()
        .on(move |e: &mut ShutdownEvent, ctx: &mut Ctx| {
            let online = uuids.iter().filter_map(|u| ctx.player_id(*u)).count();
            note(&l, format!("online {online}"));
            e.reason = Component::text("Back soon");
        });

    env.shutdown();

    // the handler ran while both were still here, and the leaves came after it
    assert_eq!(*seen.borrow(), ["online 2"]);
    assert_eq!(leaves.take().len(), 2);
    let told = steve.drain();
    let disconnect = told.iter().find(|r| r.id == out::DISCONNECT).unwrap();
    assert_eq!(
        disconnect.payload(),
        disconnect_payload(Component::text("Back soon"))
    );
}

#[test]
fn a_message_sent_while_shutting_down_arrives_before_the_disconnect() {
    let mut env = env();
    let mut steve = env.connect("Steve");
    steve.drain();
    env.instance_mut()
        .events_mut()
        .on(|_: &mut ShutdownEvent, ctx: &mut Ctx| {
            ctx.broadcast(&Component::text("saving"));
        });

    env.shutdown();

    let told = steve.drain();
    assert!(position(&told, out::SYSTEM_CHAT) < position(&told, out::DISCONNECT));
}

fn channel(name: &str) -> lodeframe::protocol::Identifier {
    lodeframe::protocol::Identifier::new(name).unwrap()
}

#[test]
fn a_plugin_message_from_a_player_reaches_the_handlers_of_its_channel() {
    let mut env = env();
    let seen = log();
    let l = seen.clone();
    let mut node = EventNode::new();
    node.only_if::<PluginMessageEvent>(|e, _| e.channel.as_str() == "test:wanted");
    node.on(move |e: &mut PluginMessageEvent, ctx: &mut Ctx| {
        note(
            &l,
            format!(
                "{} {:?} {:?}",
                ctx.name(e.player).unwrap(),
                e.channel.as_str(),
                e.data
            ),
        );
    });
    env.instance_mut().events_mut().add_child(node);
    let steve = env.connect("Steve");

    env.plugin_message(&steve, "test:other", b"no");
    env.plugin_message(&steve, "test:wanted", b"hi");

    assert_eq!(*seen.borrow(), [r#"Steve "test:wanted" [104, 105]"#]);
}

#[test]
fn a_handler_can_send_a_plugin_message_to_a_player() {
    let mut env = env();
    let mut steve = env.connect("Steve");
    env.instance_mut()
        .events_mut()
        .on(|e: &mut PluginMessageEvent, ctx: &mut Ctx| {
            let mut echo = e.data.clone();
            echo.reverse();
            ctx.send_plugin_message(e.player, &e.channel, &echo);
        });
    steve.drain();

    env.plugin_message(&steve, "test:echo", &[1, 2, 3]);

    let sent = steve.drain_as::<lodeframe::protocol::packets::play::ClientboundCustomPayload>();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].channel, channel("test:echo"));
    assert_eq!(sent[0].data, [3, 2, 1]);
}

#[test]
fn a_plugin_message_to_a_player_who_left_goes_nowhere() {
    let mut env = env();
    let mut alex = env.connect("Alex");
    let steve = env.connect("Steve");
    let gone = env.instance().player_id(steve.uuid()).unwrap();
    env.disconnect(steve);
    alex.drain();

    env.instance_mut()
        .send_plugin_message(gone, &channel("test:x"), b"x");

    assert!(alex.drain().is_empty());
}

#[test]
fn too_long_a_plugin_message_is_not_sent() {
    let mut env = env();
    let mut steve = env.connect("Steve");
    let id = env.instance().player_id(steve.uuid()).unwrap();
    steve.drain();

    let long = vec![0; lodeframe::protocol::packets::MAX_CLIENTBOUND_PAYLOAD + 1];
    env.instance_mut()
        .send_plugin_message(id, &channel("test:x"), &long);

    assert!(steve.drain().is_empty());
    assert!(env.instance().is_online(id));
}

fn brand_message(brand: &str) -> PluginMessage {
    let mut data = Vec::new();
    brand.to_owned().encode(&mut data).unwrap();
    PluginMessage {
        channel: channel("minecraft:brand"),
        data,
    }
}

#[test]
fn what_a_client_sent_while_joining_arrives_after_the_join_with_its_brand_known() {
    let mut env = env();
    let seen = log();
    let (l1, l2) = (seen.clone(), seen.clone());
    env.instance_mut()
        .events_mut()
        .on(move |e: &mut PlayerJoinEvent, ctx: &mut Ctx| {
            note(&l1, format!("join {:?}", ctx.client_brand(e.player)));
        })
        .on(move |e: &mut PluginMessageEvent, _: &mut Ctx| {
            note(&l2, format!("message {}", e.channel.as_str()));
        });

    let steve = env.connect_with(
        "Steve",
        vec![
            brand_message("fabric"),
            PluginMessage {
                channel: channel("mod:hello"),
                data: Vec::new(),
            },
        ],
    );

    assert_eq!(
        *seen.borrow(),
        [
            r#"join Some("fabric")"#,
            "message minecraft:brand",
            "message mod:hello"
        ]
    );
    let id = env.instance().player_id(steve.uuid()).unwrap();
    assert_eq!(env.instance().client_brand(id), Some("fabric"));
}

#[test]
fn a_player_without_a_brand_has_none() {
    let mut env = env();
    let steve = env.connect("Steve");
    let id = env.instance().player_id(steve.uuid()).unwrap();
    assert_eq!(env.instance().client_brand(id), None);

    let alex = env.connect_with(
        "Alex",
        vec![PluginMessage {
            channel: channel("minecraft:brand"),
            data: vec![0xff],
        }],
    );
    // bytes that are no string are no brand
    let id = env.instance().player_id(alex.uuid()).unwrap();
    assert_eq!(env.instance().client_brand(id), None);
}

fn skin() -> ProfileProperty {
    ProfileProperty {
        name: "textures".into(),
        value: "dGV4dHVyZXM=".into(),
        signature: Some("c2lnbmVk".into()),
    }
}

fn forwarded(name: &str, uuid: u128) -> Profile {
    Profile {
        uuid: Uuid(uuid),
        name: name.into(),
        properties: vec![skin()],
        remote_addr: Some("203.0.113.7".parse().unwrap()),
    }
}

#[test]
fn what_a_proxy_forwarded_is_known_and_in_the_tab_list() {
    let mut env = env();
    let mut steve = env.connect_as(forwarded("Steve", 7), Vec::new());

    let id = env.instance().player_id(steve.uuid()).unwrap();
    assert_eq!(env.instance().profile_properties(id), [skin()]);
    assert_eq!(
        env.instance().remote_addr(id),
        Some("203.0.113.7".parse().unwrap())
    );
    // the skin is in the player's own list entry, and the player is the one in the list
    let list = steve.drain_as::<PlayerInfoAdd>();
    assert_eq!(list[0].players[0].uuid, Uuid(7));
    assert_eq!(list[0].players[0].properties, [skin()]);
}

#[test]
fn the_skin_reaches_the_players_who_were_there_and_the_ones_who_come() {
    let mut env = env();
    let mut steve = env.connect("Steve");
    steve.drain();
    let mut alex = env.connect_as(forwarded("Alex", 8), Vec::new());

    // Steve hears of Alex with the skin; Alex hears of Steve without one
    let heard = steve.drain_as::<PlayerInfoAdd>();
    assert_eq!(heard[0].players[0].properties, [skin()]);
    let list = alex.drain_as::<PlayerInfoAdd>();
    assert_eq!(names(&list), ["Steve", "Alex"]);
    assert!(list[0].players[0].properties.is_empty());
    assert_eq!(list[0].players[1].properties, [skin()]);

    // a third player gets Alex's skin in the list that is sent to them
    let mut zed = env.connect("Zed");
    let list = zed.drain_as::<PlayerInfoAdd>();
    let alex_entry = list[0].players.iter().find(|p| p.name == "Alex").unwrap();
    assert_eq!(alex_entry.properties, [skin()]);
}

#[test]
fn a_player_without_a_proxy_has_no_forwarded_address_or_skin() {
    let mut env = env();
    let steve = env.connect("Steve");
    let id = env.instance().player_id(steve.uuid()).unwrap();
    assert_eq!(env.instance().remote_addr(id), None);
    assert!(env.instance().profile_properties(id).is_empty());
}

#[test]
fn a_player_who_left_has_no_forwarded_address_or_skin() {
    let mut env = env();
    let steve = env.connect_as(forwarded("Steve", 7), Vec::new());
    let id = env.instance().player_id(steve.uuid()).unwrap();
    env.disconnect(steve);
    assert_eq!(env.instance().remote_addr(id), None);
    assert!(env.instance().profile_properties(id).is_empty());
}
