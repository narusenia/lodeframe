// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A player joins a world, walks over a chunk border, and gets the chunks they need.

use lodeframe::{
    chunk::FlatGenerator,
    protocol::{
        Encode, Vec3,
        ids::play::{clientbound as out, serverbound},
        packets::play::{
            AddEntity, Chat, DisguisedChat, EntityPositionSync, ForgetLevelChunk, INPUT_SNEAK,
            Login, MovePlayerPos, MovePlayerPosRot, MovePlayerRot, PlayerInfoAdd, PlayerInfoRemove,
            PlayerInput, PlayerPosition, RemoveEntities, RotateHead, SetChunkCacheCenter,
            SetEntityFlagsAndPose, SystemChat,
        },
    },
    registry::Registries,
    test_util::{FakePlayer, Received, Recorder, TestEnv},
    text::{Color, Component},
    world::{ChatEvent, World},
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
