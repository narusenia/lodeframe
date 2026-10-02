// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A player joins a world, walks over a chunk border, and gets the chunks they need.

use lodeframe::{
    chunk::{ChunkLoader, ChunkPos, FlatGenerator},
    protocol::{
        BlockPos, Direction, Encode, VarInt, Vec3,
        block::{AIR, COBBLESTONE, STONE},
        chunk::LevelChunkWithLight,
        ids::play::{clientbound as out, serverbound},
        packets::play::{
            AddEntity, BlockChangedAck, BlockUpdate, Chat, ChunkBatchFinished, DisguisedChat,
            EntityPositionSync, FLAG_SNEAKING, ForgetLevelChunk, INPUT_SNEAK, Login, MoveEntityPos,
            MoveEntityPosRot, MovePlayerPos, MovePlayerPosRot, MovePlayerRot, PlayerAction,
            PlayerInfoAdd, PlayerInfoRemove, PlayerInput, PlayerPosition, RemoveEntities,
            RotateHead, SetChunkCacheCenter, SetEntityFlagsAndPose, SystemChat, UseItemOn,
        },
    },
    registry::Registries,
    test_util::{FakePlayer, Received, Recorder, TestEnv},
    text::{Color, Component},
    world::{
        BlockBreakEvent, BlockPlaceEvent, ChatEvent, Ctx, PlayerJoinEvent, PlayerLeaveEvent, World,
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
