// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A player joins a world, walks over a chunk border, and gets the chunks they need.

use lodeframe::{
    chunk::{ChunkLoader, ChunkPos, FlatGenerator},
    protocol::{
        BlockPos, Direction, Encode, VarInt, Vec3,
        block::{AIR, COBBLESTONE, STONE},
        ids::play::{clientbound as out, serverbound},
        packets::play::{
            AddEntity, BlockChangedAck, BlockUpdate, Chat, DisguisedChat, EntityPositionSync,
            ForgetLevelChunk, INPUT_SNEAK, Login, MovePlayerPos, MovePlayerPosRot, MovePlayerRot,
            PlayerAction, PlayerInfoAdd, PlayerInfoRemove, PlayerInput, PlayerPosition,
            RemoveEntities, RotateHead, SetChunkCacheCenter, SetEntityFlagsAndPose, SystemChat,
            UseItemOn,
        },
    },
    registry::Registries,
    test_util::{FakePlayer, Received, Recorder, TestEnv},
    text::{Color, Component},
    world::{BlockBreakEvent, BlockPlaceEvent, ChatEvent, World},
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

fn env() -> TestEnv<World<FlatGenerator>> {
    let mut world = World::new(&Registries::vanilla(), FlatGenerator::default());
    world.view_distance = 2;
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
fn movement_look_and_sneaking_reach_the_other_player_only() {
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
    let sync: Vec<EntityPositionSync> = steve.drain_as();
    assert_eq!(sync.len(), 1);
    assert_eq!(sync[0].position, Vec3::new(3.5, -60.0, 0.5));
    assert_eq!((sync[0].yaw, sync[0].pitch), (90.0, 10.0));
    assert!(
        alex.drain().is_empty(),
        "the mover is not told about themselves"
    );

    // the head follows the yaw
    env.send(
        &alex,
        &MovePlayerRot {
            yaw: 180.0,
            pitch: 10.0,
            flags: 1,
        },
    );
    let packets = steve.drain();
    let head: RotateHead = packets
        .iter()
        .find(|r| r.is::<RotateHead>())
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!(head.head_yaw, 128);

    // a move that changes nothing is not repeated
    env.send(
        &alex,
        &MovePlayerRot {
            yaw: 180.0,
            pitch: 10.0,
            flags: 1,
        },
    );
    assert!(steve.drain().is_empty());

    env.send(&alex, &PlayerInput { flags: INPUT_SNEAK });
    let crouch: Vec<SetEntityFlagsAndPose> = steve.drain_as();
    assert_eq!((crouch[0].flags, crouch[0].pose.0), (2, 5));
    env.send(&alex, &PlayerInput { flags: INPUT_SNEAK });
    assert!(steve.drain().is_empty(), "still sneaking: nothing new");
    env.send(&alex, &PlayerInput { flags: 0 });
    let stand: Vec<SetEntityFlagsAndPose> = steve.drain_as();
    assert_eq!((stand[0].flags, stand[0].pose.0), (0, 0));
}

fn say(env: &mut TestEnv<World<FlatGenerator>>, who: &FakePlayer, text: &str) {
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

fn two_players() -> (TestEnv<World<FlatGenerator>>, FakePlayer, FakePlayer) {
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
    assert_eq!(events[0].player, steve.uuid());
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
        .on(|e: &mut ChatEvent, world: &mut World<FlatGenerator>| {
            e.cancel();
            world.send_message(e.player, &Component::text("chat is closed"));
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

    env.instance_mut().send_message(steve.uuid(), &note);
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
    assert_eq!((events[0].player, events[0].pos), (steve.uuid(), GROUND));
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
