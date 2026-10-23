// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Health, food, experience, dying and coming back: what the client is told, and what a dead
//! player cannot do.

use lodeframe::{
    chunk::FlatGenerator,
    player::{Experience, GameMode, PlayerDeathEvent, PlayerRespawnEvent, RespawnPoint},
    protocol::{
        BlockPos, Direction, VarInt, Vec3,
        ids::play::clientbound as out,
        packets::play::{
            AcceptTeleportation, CLIENT_COMMAND_PERFORM_RESPAWN, ClientCommand,
            GAME_EVENT_IMMEDIATE_RESPAWN, GameEvent, LEVEL_CHUNKS_LOAD_START, Login, MovePlayerPos,
            PlayerAction, PlayerCombatKill, PlayerPosition, Respawn, SetExperience, SetHealth,
            UseItemOn,
        },
    },
    registry::Registries,
    test_util::{FakePlayer, Received, Recorder, TestEnv},
    text::Component,
    world::{BlockBreakEvent, BlockPlaceEvent, Ctx, PlayerId, World},
};

fn env() -> TestEnv<World> {
    let mut world = World::new(&Registries::vanilla(), FlatGenerator::default());
    world.view_distance = 2;
    world.chunks_per_tick = usize::MAX;
    TestEnv::new(world)
}

struct Joined {
    client: FakePlayer,
    id: PlayerId,
    entity_id: i32,
}

fn join(env: &mut TestEnv<World>, name: &str) -> Joined {
    let mut client = env.connect(name);
    let packets = client.drain();
    let login: Login = packets[0].decode().unwrap();
    let id = env.instance().player_id(client.uuid()).unwrap();
    Joined {
        client,
        id,
        entity_id: login.entity_id,
    }
}

fn count(received: &[Received], id: i32) -> usize {
    received.iter().filter(|r| r.id == id).count()
}

fn vitals(received: &[Received]) -> Vec<SetHealth> {
    received
        .iter()
        .filter(|r| r.is::<SetHealth>())
        .map(|r| r.decode().unwrap())
        .collect()
}

fn experiences(received: &[Received]) -> Vec<SetExperience> {
    received
        .iter()
        .filter(|r| r.is::<SetExperience>())
        .map(|r| r.decode().unwrap())
        .collect()
}

fn respawn_button() -> ClientCommand {
    ClientCommand {
        action: VarInt(CLIENT_COMMAND_PERFORM_RESPAWN),
    }
}

fn walk(x: f64) -> MovePlayerPos {
    MovePlayerPos {
        position: Vec3::new(x, -60.0, 0.5),
        flags: 1,
    }
}

#[test]
fn a_player_joins_with_full_health_food_and_no_experience() {
    let mut env = env();
    let mut steve = env.connect("Steve");
    let joined = steve.drain();
    let login: Login = joined[0].decode().unwrap();
    assert!(login.show_death_screen);
    assert_eq!(
        vitals(&joined),
        [SetHealth {
            health: 20.0,
            food: VarInt(20),
            saturation: 5.0
        }]
    );
    assert_eq!(
        experiences(&joined),
        [SetExperience {
            progress: 0.0,
            level: VarInt(0),
            total: VarInt(0)
        }]
    );
    assert_eq!(count(&joined, out::GAME_EVENT), 1, "only the terrain one");

    // a world without the death screen tells the client to respawn at once
    env.instance_mut().show_death_screen = false;
    let mut alex = env.connect("Alex");
    let joined = alex.drain();
    let login: Login = joined[0].decode().unwrap();
    assert!(!login.show_death_screen);
    let events: Vec<GameEvent> = joined
        .iter()
        .filter(|r| r.is::<GameEvent>())
        .map(|r| r.decode().unwrap())
        .collect();
    assert!(
        events
            .iter()
            .any(|e| e.event == GAME_EVENT_IMMEDIATE_RESPAWN && e.value == 1.0)
    );
}

#[test]
fn health_food_and_saturation_are_set_and_held_in_range() {
    let mut env = env();
    let mut steve = join(&mut env, "Steve");
    let id = steve.id;

    env.instance_mut().set_health(id, 10.0);
    assert_eq!(env.instance().health(id), Some(10.0));
    assert_eq!(
        vitals(&steve.client.drain()),
        [SetHealth {
            health: 10.0,
            food: VarInt(20),
            saturation: 5.0
        }]
    );

    // the same again says nothing, and not a number is ignored
    env.instance_mut().set_health(id, 10.0);
    env.instance_mut().set_health(id, f32::NAN);
    env.instance_mut().set_saturation(id, f32::NAN);
    assert!(steve.client.drain().is_empty());

    // too much is the most
    env.instance_mut().set_health(id, 500.0);
    assert_eq!(env.instance().health(id), Some(20.0));
    assert_eq!(vitals(&steve.client.drain())[0].health, 20.0);

    // saturation goes down with the food level
    env.instance_mut().set_food(id, 3);
    assert_eq!(env.instance().food(id), Some(3));
    assert_eq!(env.instance().saturation(id), Some(3.0));
    assert_eq!(
        vitals(&steve.client.drain()),
        [SetHealth {
            health: 20.0,
            food: VarInt(3),
            saturation: 3.0
        }]
    );
    env.instance_mut().set_saturation(id, 99.0);
    assert!(
        steve.client.drain().is_empty(),
        "already as high as it goes"
    );
    env.instance_mut().set_saturation(id, 1.5);
    assert_eq!(vitals(&steve.client.drain())[0].saturation, 1.5);
    env.instance_mut().set_food(id, -4);
    assert_eq!(env.instance().food(id), Some(0));
    assert_eq!(env.instance().saturation(id), Some(0.0));
}

#[test]
fn experience_follows_the_games_rule() {
    // 7 points reach level 1; 37 take level 15 to 16; 112 take level 30 to 31
    let cost =
        |level: i32| Experience::new(level + 1, 0.0).total - Experience::new(level, 0.0).total;
    assert_eq!(cost(0), 7);
    assert_eq!(cost(15), 37);
    assert_eq!(cost(16), 42);
    assert_eq!(cost(30), 112);

    assert_eq!(Experience::from_points(0), Experience::new(0, 0.0));
    let one = Experience::from_points(7);
    assert_eq!((one.level, one.progress, one.total), (1, 0.0, 7));
    let part = Experience::from_points(6);
    assert_eq!((part.level, part.total), (0, 6));
    assert!((part.progress - 6.0 / 7.0).abs() < 1e-6);
    for level in [0, 1, 14, 15, 16, 30, 31, 100] {
        let e = Experience::new(level, 0.0);
        assert_eq!(Experience::from_points(e.total).level, level, "{level}");
    }
    let half = Experience::new(5, 0.5);
    assert_eq!(half.level, 5);
    assert_eq!(half.total, 55 + 9);
}

#[test]
fn experience_is_set_in_the_order_the_client_reads_it() {
    let mut env = env();
    let mut steve = join(&mut env, "Steve");
    let id = steve.id;

    env.instance_mut()
        .set_experience(id, Experience::new(5, 0.5));
    assert_eq!(env.instance().experience(id), Some(Experience::new(5, 0.5)));
    assert_eq!(
        experiences(&steve.client.drain()),
        [SetExperience {
            progress: 0.5,
            level: VarInt(5),
            total: VarInt(64)
        }]
    );
    env.instance_mut()
        .set_experience(id, Experience::new(5, 0.5));
    assert!(steve.client.drain().is_empty());

    // what the client cannot take is brought into range
    env.instance_mut().set_experience(
        id,
        Experience {
            level: -3,
            progress: 7.0,
            total: -1,
        },
    );
    assert_eq!(
        env.instance().experience(id),
        Some(Experience {
            level: 0,
            progress: 1.0,
            total: 0
        })
    );
    env.instance_mut().set_experience(
        id,
        Experience {
            level: 2,
            progress: f32::NAN,
            total: 9,
        },
    );
    assert_eq!(env.instance().experience(id).unwrap().progress, 0.0);
}

#[test]
fn health_at_zero_kills_with_the_games_message() {
    let mut env = env();
    let mut steve = join(&mut env, "Steve");
    let mut alex = join(&mut env, "Alex");
    steve.client.drain();
    env.tick(1);
    alex.client.drain();
    let deaths = Recorder::<PlayerDeathEvent>::attach(env.instance_mut().events_mut());

    env.instance_mut().set_health(steve.id, 0.0);
    assert!(env.instance().is_dead(steve.id));
    assert_eq!(env.instance().health(steve.id), Some(0.0));

    let got = steve.client.drain();
    // the hearts go first, then the screen opens
    assert_eq!(vitals(&got)[0].health, 0.0);
    let kill: PlayerCombatKill = got
        .iter()
        .find(|r| r.is::<PlayerCombatKill>())
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!(kill.player_id.0, steve.entity_id);
    assert_eq!(
        kill.message,
        Component::translatable("death.attack.generic").arg(Component::text("Steve"))
    );
    // nobody sees a dead player
    assert_eq!(count(&alex.client.drain(), out::REMOVE_ENTITIES), 1);

    // the handlers hear of it once the call is over
    assert!(deaths.take().is_empty());
    env.tick(1);
    let heard = deaths.take();
    assert_eq!(heard.len(), 1);
    assert_eq!(heard[0].player, steve.id);
}

#[test]
fn kill_gives_its_own_message_and_works_once() {
    let mut env = env();
    let mut steve = join(&mut env, "Steve");
    env.instance_mut().teleport(
        steve.id,
        lodeframe::player::Teleport::new(Vec3::new(4.5, -60.0, 7.5)),
    );
    steve.client.drain();

    let message = Component::text("fell out of the world");
    assert!(env.instance_mut().kill(steve.id, &message));
    let got = steve.client.drain();
    let kill: PlayerCombatKill = got
        .iter()
        .find(|r| r.is::<PlayerCombatKill>())
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!(kill.message, message);
    assert_eq!(
        env.instance().last_death(steve.id),
        Some(Vec3::new(4.5, -60.0, 7.5))
    );

    // dead already
    assert!(!env.instance_mut().kill(steve.id, &message));
    assert!(steve.client.drain().is_empty());
}

#[test]
fn a_dead_player_cannot_move_edit_blocks_or_be_healed() {
    let mut env = env();
    let mut steve = join(&mut env, "Steve");
    let breaks = Recorder::<BlockBreakEvent>::attach(env.instance_mut().events_mut());
    let places = Recorder::<BlockPlaceEvent>::attach(env.instance_mut().events_mut());
    env.instance_mut().kill(steve.id, &Component::text("died"));
    steve.client.drain();

    env.send(&steve.client, &walk(9.5));
    assert_eq!(env.instance().position(steve.id).unwrap().x, 0.5);

    let ground = BlockPos::new(0, -61, 0);
    env.send(
        &steve.client,
        &PlayerAction {
            action: VarInt(0),
            pos: ground,
            face: Direction::Up.id(),
            sequence: VarInt(1),
        },
    );
    env.send(
        &steve.client,
        &UseItemOn {
            hand: VarInt(0),
            pos: ground,
            face: VarInt(Direction::Up.id().into()),
            cursor_x: 0.5,
            cursor_y: 1.0,
            cursor_z: 0.5,
            inside: false,
            world_border_hit: false,
            sequence: VarInt(2),
        },
    );
    assert!(breaks.take().is_empty() && places.take().is_empty());

    steve.client.drain();
    env.instance_mut().set_health(steve.id, 20.0);
    env.instance_mut().set_food(steve.id, 1);
    env.instance_mut().set_saturation(steve.id, 0.0);
    env.instance_mut()
        .set_experience(steve.id, Experience::new(3, 0.0));
    assert!(steve.client.drain().is_empty());
    assert_eq!(env.instance().health(steve.id), Some(0.0));
    assert_eq!(
        env.instance().experience(steve.id),
        Some(Experience::default())
    );
}

#[test]
fn the_respawn_button_brings_a_dead_player_back_at_the_respawn_point() {
    let mut env = env();
    let mut steve = join(&mut env, "Steve");
    let mut alex = join(&mut env, "Alex");
    steve.client.drain();
    env.tick(1);
    alex.client.drain();
    let requests = Recorder::<PlayerRespawnEvent>::attach(env.instance_mut().events_mut());
    env.instance_mut()
        .set_game_mode(steve.id, GameMode::Adventure);
    env.instance_mut()
        .set_experience(steve.id, Experience::new(4, 0.25));
    env.instance_mut().set_food(steve.id, 3);
    env.instance_mut().set_respawn_point(
        steve.id,
        Some(RespawnPoint {
            position: Vec3::new(100.5, -60.0, 0.5),
            yaw: 90.0,
            pitch: 5.0,
        }),
    );
    env.instance_mut().kill(steve.id, &Component::text("died"));
    env.tick(1);
    steve.client.drain();
    alex.client.drain();

    env.send(&steve.client, &respawn_button());

    let event = requests.take().remove(0);
    assert_eq!(event.player, steve.id);
    assert_eq!(event.position, Vec3::new(100.5, -60.0, 0.5));
    assert_eq!((event.yaw, event.pitch), (90.0, 5.0));
    assert_eq!(
        (event.health, event.food, event.saturation),
        (20.0, 20, 5.0)
    );

    assert!(!env.instance().is_dead(steve.id));
    assert_eq!(
        env.instance().position(steve.id),
        Some(Vec3::new(100.5, -60.0, 0.5))
    );
    assert_eq!(env.instance().rotation(steve.id), Some((90.0, 5.0)));
    assert_eq!(env.instance().health(steve.id), Some(20.0));
    assert_eq!(env.instance().food(steve.id), Some(20));
    // the experience is what it was
    assert_eq!(
        env.instance().experience(steve.id),
        Some(Experience::new(4, 0.25))
    );

    let got = steve.client.drain();
    // the world is built again, then the player is put in it, then the rest
    assert_eq!(got[0].id, out::RESPAWN);
    let respawn: Respawn = got[0].decode().unwrap();
    assert_eq!(respawn.spawn.game_mode.0, 2);
    assert_eq!(
        respawn.spawn.last_death.unwrap().pos,
        BlockPos::new(0, -60, 0)
    );
    assert_eq!(respawn.data_kept, 0);
    assert_eq!(got[1].id, out::PLAYER_POSITION);
    let position: PlayerPosition = got[1].decode().unwrap();
    assert_eq!(position.position, Vec3::new(100.5, -60.0, 0.5));
    assert_eq!((position.yaw, position.pitch), (90.0, 5.0));
    let load: Vec<GameEvent> = got
        .iter()
        .filter(|r| r.is::<GameEvent>())
        .map(|r| r.decode().unwrap())
        .collect();
    assert!(load.iter().any(|e| e.event == LEVEL_CHUNKS_LOAD_START));
    assert_eq!(vitals(&got)[0].health, 20.0);
    assert_eq!(experiences(&got)[0].level.0, 4);
    assert_eq!(count(&got, out::PLAYER_ABILITIES), 1);
    assert_eq!(count(&got, out::ENTITY_EVENT), 1);
    // the chunks come again from the start: a 5 by 5 square around the new place
    assert_eq!(count(&got, out::LEVEL_CHUNK_WITH_LIGHT), 25);
    assert_eq!(count(&got, out::SET_CHUNK_CACHE_CENTER), 1);

    // Alex does not see Steve, who is 100 blocks away now
    env.tick(1);
    assert!(alex.client.drain().iter().all(|r| r.id != out::ADD_ENTITY));
}

#[test]
fn a_player_who_respawns_in_view_is_seen_again_and_waits_for_the_teleport_to_be_confirmed() {
    let mut env = env();
    let mut steve = join(&mut env, "Steve");
    let mut alex = join(&mut env, "Alex");
    steve.client.drain();
    env.tick(1);
    alex.client.drain();
    env.instance_mut().kill(steve.id, &Component::text("died"));
    alex.client.drain();
    steve.client.drain();

    env.send(&steve.client, &respawn_button());
    assert_eq!(count(&alex.client.drain(), out::ADD_ENTITY), 1);
    let got = steve.client.drain();
    // Alex is shown to the one who came back
    assert_eq!(count(&got, out::ADD_ENTITY), 1);
    let position: PlayerPosition = got
        .iter()
        .find(|r| r.is::<PlayerPosition>())
        .unwrap()
        .decode()
        .unwrap();

    // positions from before the respawn are dropped until it is confirmed
    env.send(&steve.client, &walk(3.5));
    assert_eq!(env.instance().position(steve.id).unwrap().x, 0.5);
    env.send(
        &steve.client,
        &AcceptTeleportation {
            teleport_id: position.teleport_id,
        },
    );
    env.send(&steve.client, &walk(3.5));
    assert_eq!(env.instance().position(steve.id).unwrap().x, 3.5);
}

#[test]
fn a_handler_can_change_where_and_with_what_a_player_comes_back_or_say_no() {
    let mut env = env();
    let mut steve = join(&mut env, "Steve");
    env.instance_mut().kill(steve.id, &Component::text("died"));
    steve.client.drain();

    env.instance_mut()
        .events_mut()
        .on(|e: &mut PlayerRespawnEvent, _: &mut Ctx| {
            e.position = Vec3::new(8.5, -60.0, 8.5);
            e.health = 7.0;
            e.food = 9;
            e.saturation = 100.0;
        });
    env.send(&steve.client, &respawn_button());
    assert_eq!(
        env.instance().position(steve.id),
        Some(Vec3::new(8.5, -60.0, 8.5))
    );
    assert_eq!(env.instance().health(steve.id), Some(7.0));
    assert_eq!(env.instance().food(steve.id), Some(9));
    // up to the food level
    assert_eq!(env.instance().saturation(steve.id), Some(9.0));

    // a handler that says no keeps the player dead, and they can press it again
    let mut alex = join(&mut env, "Alex");
    env.instance_mut().kill(alex.id, &Component::text("died"));
    alex.client.drain();
    env.instance_mut()
        .events_mut()
        .on(|e: &mut PlayerRespawnEvent, _: &mut Ctx| {
            if e.player.uuid() == lodeframe::protocol::Uuid::offline("Alex") {
                e.cancel();
            }
        });
    env.send(&alex.client, &respawn_button());
    assert!(env.instance().is_dead(alex.id));
    assert!(alex.client.drain().is_empty());
}

#[test]
fn only_a_dead_player_can_respawn() {
    let mut env = env();
    let mut steve = join(&mut env, "Steve");
    let requests = Recorder::<PlayerRespawnEvent>::attach(env.instance_mut().events_mut());

    env.send(&steve.client, &respawn_button());
    assert!(requests.take().is_empty());
    assert!(!env.instance_mut().respawn(steve.id));
    assert!(steve.client.drain().is_empty());

    // the server can bring them back without the button, and without the event
    env.instance_mut().kill(steve.id, &Component::text("died"));
    steve.client.drain();
    assert!(env.instance_mut().respawn(steve.id));
    assert!(requests.take().is_empty());
    assert_eq!(steve.client.drain()[0].id, out::RESPAWN);
    assert!(!env.instance().is_dead(steve.id));
    assert!(!env.instance_mut().respawn(steve.id));

    // the other buttons of the screen (statistics, game rules) are not respawning
    env.instance_mut().kill(steve.id, &Component::text("died"));
    env.send(&steve.client, &ClientCommand { action: VarInt(1) });
    assert!(env.instance().is_dead(steve.id));
}

#[test]
fn the_respawn_point_is_the_spawn_until_one_is_set() {
    let mut env = env();
    let steve = join(&mut env, "Steve");
    let spawn = env.instance().spawn;
    assert_eq!(
        env.instance().respawn_point(steve.id),
        Some(RespawnPoint::new(spawn))
    );
    let point = RespawnPoint {
        position: Vec3::new(1.0, 2.0, 3.0),
        yaw: 10.0,
        pitch: 20.0,
    };
    env.instance_mut().set_respawn_point(steve.id, Some(point));
    assert_eq!(env.instance().respawn_point(steve.id), Some(point));
    env.instance_mut().set_respawn_point(steve.id, None);
    assert_eq!(
        env.instance().respawn_point(steve.id),
        Some(RespawnPoint::new(spawn))
    );
}

#[test]
fn a_player_who_is_gone_is_not_there_to_hurt_or_bring_back() {
    let mut env = env();
    let steve = join(&mut env, "Steve");
    let (id, client) = (steve.id, steve.client);
    env.disconnect(client);
    assert_eq!(env.instance().health(id), None);
    assert_eq!(env.instance().food(id), None);
    assert_eq!(env.instance().saturation(id), None);
    assert_eq!(env.instance().experience(id), None);
    assert_eq!(env.instance().respawn_point(id), None);
    assert_eq!(env.instance().last_death(id), None);
    assert!(!env.instance().is_dead(id));
    assert!(!env.instance_mut().kill(id, &Component::text("died")));
    assert!(!env.instance_mut().respawn(id));
    env.instance_mut().set_health(id, 5.0);
    env.instance_mut().set_food(id, 5);
    env.instance_mut().set_saturation(id, 1.0);
    env.instance_mut()
        .set_experience(id, Experience::new(1, 0.0));
    env.instance_mut().set_respawn_point(id, None);
}

#[test]
fn a_world_without_the_death_screen_says_so_again_after_a_respawn() {
    let mut env = env();
    env.instance_mut().show_death_screen = false;
    let mut steve = join(&mut env, "Steve");
    env.instance_mut().kill(steve.id, &Component::text("died"));
    steve.client.drain();
    env.instance_mut().respawn(steve.id);
    let events: Vec<GameEvent> = steve
        .client
        .drain()
        .iter()
        .filter(|r| r.is::<GameEvent>())
        .map(|r| r.decode().unwrap())
        .collect();
    assert!(
        events
            .iter()
            .any(|e| e.event == GAME_EVENT_IMMEDIATE_RESPAWN && e.value == 1.0)
    );
}
