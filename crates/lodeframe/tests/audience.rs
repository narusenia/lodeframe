// SPDX-License-Identifier: Apache-2.0 OR MIT
//! What goes to whom: the same calls reach one player, everyone, a group or everyone but one,
//! and the boss bar, title, sound and tab list packets are what the client reads.

use std::time::Duration;

use lodeframe::{
    Audience, AudienceExt, BossBar, BossBarColor, BossBarFlags, BossBarOverlay, Everyone, Group,
    Sound, SoundSource, StopSound, Title,
    chunk::FlatGenerator,
    from_fn,
    protocol::{
        Encode, Identifier, Uuid, Vec3,
        ids::play::clientbound as out,
        packets::play::{
            BossEvent, BossEventOperation, ClearTitles, MovePlayerPos, SetActionBarText,
            SetSubtitleText, SetTitleText, SetTitlesAnimation, SoundHolder, StopSound as WireStop,
            SystemChat, TabList,
        },
    },
    registry::Registries,
    sound,
    test_util::{FakePlayer, Received, TestEnv},
    text::Component,
    world::{Ctx, PlayerId, World},
};

fn env() -> TestEnv<World> {
    let mut world = World::new(&Registries::vanilla(), FlatGenerator::default());
    world.view_distance = 1;
    world.chunks_per_tick = usize::MAX;
    TestEnv::new(world)
}

struct Three {
    env: TestEnv<World>,
    steve: FakePlayer,
    alex: FakePlayer,
    kit: FakePlayer,
    steve_id: PlayerId,
    alex_id: PlayerId,
    kit_id: PlayerId,
}

fn three() -> Three {
    let mut env = env();
    let mut steve = env.connect("Steve");
    let mut alex = env.connect("Alex");
    let mut kit = env.connect("Kit");
    for p in [&mut steve, &mut alex, &mut kit] {
        p.drain();
    }
    let id = |env: &TestEnv<World>, p: &FakePlayer| env.instance().player_id(p.uuid()).unwrap();
    let (steve_id, alex_id, kit_id) = (id(&env, &steve), id(&env, &alex), id(&env, &kit));
    Three {
        env,
        steve,
        alex,
        kit,
        steve_id,
        alex_id,
        kit_id,
    }
}

impl Three {
    /// How many chat lines each of the three got since the last call, as (Steve, Alex, Kit).
    fn lines(&mut self) -> (usize, usize, usize) {
        let n = |p: &mut FakePlayer| count(&p.drain(), out::SYSTEM_CHAT);
        (n(&mut self.steve), n(&mut self.alex), n(&mut self.kit))
    }

    fn ctx(&mut self) -> &mut Ctx {
        self.env.instance_mut()
    }
}

fn count(received: &[Received], id: i32) -> usize {
    received.iter().filter(|r| r.id == id).count()
}

fn bytes<P: Encode>(packet: P) -> Vec<u8> {
    let mut b = Vec::new();
    packet.encode(&mut b).unwrap();
    b
}

fn line() -> Component {
    Component::text("hello")
}

#[test]
fn the_same_call_reaches_one_player_everyone_a_group_or_everyone_but_one() {
    let mut t = three();
    let (steve, alex, kit) = (t.steve_id, t.alex_id, t.kit_id);

    t.ctx().send_message(steve, &line());
    assert_eq!(t.lines(), (1, 0, 0));

    t.ctx().send_message(Everyone, &line());
    assert_eq!(t.lines(), (1, 1, 1));

    t.ctx().send_message(Everyone.except(steve), &line());
    assert_eq!(t.lines(), (0, 1, 1));

    t.ctx().join_group("team", alex);
    t.ctx().join_group("team", kit);
    let team = t.ctx().audience("team");
    t.ctx().send_message(&team, &line());
    assert_eq!(t.lines(), (0, 1, 1));
    t.ctx().send_message(team.clone().except(kit), &line());
    assert_eq!(t.lines(), (0, 1, 0));

    // lists, slices and arrays of anything that is an audience
    t.ctx().send_message([steve, kit], &line());
    assert_eq!(t.lines(), (1, 0, 1));
    t.ctx().send_message(vec![alex], &line());
    assert_eq!(t.lines(), (0, 1, 0));
    t.ctx().send_message(&[steve, alex][..], &line());
    assert_eq!(t.lines(), (1, 1, 0));

    // the union and a difference of two sets
    t.ctx().send_message(steve.and(kit), &line());
    assert_eq!(t.lines(), (1, 0, 1));
    t.ctx().send_message(Everyone.except(team), &line());
    assert_eq!(t.lines(), (1, 0, 0));

    // the old call for everyone
    t.ctx().broadcast(&line());
    assert_eq!(t.lines(), (1, 1, 1));
}

#[test]
fn an_audience_of_your_own_is_found_by_key() {
    let mut t = three();
    let (steve, alex, kit) = (t.steve_id, t.alex_id, t.kit_id);

    // nobody is behind a key that is not there
    let alive = t.ctx().audience("alive");
    t.ctx().send_message(&alive, &line());
    assert_eq!(t.lines(), (0, 0, 0));

    // a function, a struct, anything: the key is looked up when it is sent to
    t.ctx().register_audience(
        "alive",
        from_fn(move |_: &Ctx, out: &mut Vec<PlayerId>| out.extend([steve, kit])),
    );
    t.ctx().send_message(&alive, &line());
    assert_eq!(t.lines(), (1, 0, 1));

    struct Alex(PlayerId);
    impl Audience for Alex {
        fn members(&self, _: &Ctx, out: &mut Vec<PlayerId>) {
            out.push(self.0);
        }
    }
    t.ctx().register_audience("alive", Alex(alex));
    t.ctx().send_message(&alive, &line());
    assert_eq!(t.lines(), (0, 1, 0));

    // a group of the same key is found first
    t.ctx().join_group("alive", kit);
    t.ctx().send_message(&alive, &line());
    assert_eq!(t.lines(), (0, 0, 1));

    assert!(t.ctx().remove_group("alive"));
    assert!(t.ctx().unregister_audience("alive"));
    assert!(!t.ctx().unregister_audience("alive"));
    t.ctx().send_message(&alive, &line());
    assert_eq!(t.lines(), (0, 0, 0));
}

#[test]
fn a_player_in_two_ways_gets_it_once() {
    let mut t = three();
    let (steve, alex) = (t.steve_id, t.alex_id);
    t.ctx().join_group("a", steve);
    t.ctx().join_group("b", steve);
    t.ctx().join_group("b", alex);
    let (a, b) = (t.ctx().audience("a"), t.ctx().audience("b"));

    t.ctx().send_message(vec![a.clone(), b, a], &line());
    assert_eq!(t.lines(), (1, 1, 0));
    t.ctx().send_message([steve, steve, steve], &line());
    assert_eq!(t.lines(), (1, 0, 0));
}

#[test]
fn a_player_who_left_is_skipped_and_the_others_still_get_it() {
    let t = three();
    let (steve, alex, kit) = (t.steve_id, t.alex_id, t.kit_id);
    let Three {
        mut env,
        steve: steve_client,
        alex: mut alex_client,
        kit: mut kit_client,
        ..
    } = t;
    env.disconnect(steve_client);

    env.instance_mut().send_message([steve, alex, kit], &line());
    env.instance_mut().send_message(steve, &line());
    assert_eq!(count(&alex_client.drain(), out::SYSTEM_CHAT), 1);
    assert_eq!(count(&kit_client.drain(), out::SYSTEM_CHAT), 1);
}

#[test]
fn a_group_loses_a_player_who_leaves_and_does_not_get_them_back() {
    let mut t = three();
    let (steve, alex) = (t.steve_id, t.alex_id);
    t.ctx().join_group("team", steve);
    t.ctx().join_group("team", alex);
    assert!(!t.ctx().join_group("team", alex), "already in it");
    assert_eq!(t.ctx().group("team").map(Group::len), Some(2));

    let Three {
        mut env,
        steve: steve_client,
        ..
    } = t;
    env.disconnect(steve_client);
    assert_eq!(env.instance().group("team").map(Group::len), Some(1));
    assert!(
        !env.instance_mut().join_group("team", steve),
        "they are gone"
    );

    // the same name coming back is a new stay, who is in no group
    let mut again = env.connect("Steve");
    again.drain();
    let new_steve = env.instance().player_id(again.uuid()).unwrap();
    assert_ne!(new_steve, steve);
    let team = env.instance().audience("team");
    env.instance_mut().send_message(&team, &line());
    assert_eq!(count(&again.drain(), out::SYSTEM_CHAT), 0);
    assert!(env.instance_mut().leave_group("team", alex));
    assert!(!env.instance_mut().leave_group("team", alex));
}

#[test]
fn everyone_gets_the_same_bytes() {
    let mut t = three();
    t.ctx().send_message(Everyone, &Component::text("same"));
    let first = t.steve.drain()[0].payload().to_vec();
    assert_eq!(t.alex.drain()[0].payload(), first);
    assert_eq!(t.kit.drain()[0].payload(), first);
    assert_eq!(
        first,
        bytes(SystemChat {
            content: Component::text("same"),
            overlay: false
        })
    );
}

#[test]
fn the_action_bar_and_the_tab_list_are_their_own_packets() {
    let mut t = three();
    let steve = t.steve_id;
    t.ctx()
        .send_action_bar(Everyone.except(steve), &Component::text("hi"));
    assert!(t.steve.drain().is_empty());
    let got = t.alex.drain();
    assert_eq!(got[0].id, out::SET_ACTION_BAR_TEXT);
    assert_eq!(
        got[0].payload(),
        bytes(SetActionBarText {
            text: Component::text("hi")
        })
    );
    t.kit.drain();

    t.ctx()
        .set_tab_list(steve, &Component::text("head"), &Component::text("foot"));
    let got = t.steve.drain();
    assert_eq!(got[0].id, out::TAB_LIST);
    assert_eq!(
        got[0].payload(),
        bytes(TabList {
            header: Component::text("head"),
            footer: Component::text("foot")
        })
    );
    assert!(t.alex.drain().is_empty());
}

#[test]
fn a_title_comes_as_one_message_with_the_times_the_subtitle_and_then_the_title() {
    let mut t = three();
    let steve = t.steve_id;
    let title = Title::new(Component::text("Go"))
        .subtitle(Component::text("fast"))
        .times(
            Duration::from_millis(500),
            Duration::from_millis(2010),
            Duration::ZERO,
        );
    t.ctx().show_title(steve, &title);

    let messages = t.steve.drain_messages();
    assert_eq!(messages.len(), 1);
    let ids: Vec<i32> = messages[0].iter().map(|r| r.id).collect();
    assert_eq!(
        ids,
        [
            out::SET_TITLES_ANIMATION,
            out::SET_SUBTITLE_TEXT,
            out::SET_TITLE_TEXT
        ]
    );
    // ticks of 50 ms, rounded up
    assert_eq!(
        messages[0][0].payload(),
        bytes(SetTitlesAnimation {
            fade_in: 10,
            stay: 41,
            fade_out: 0
        })
    );
    assert_eq!(
        messages[0][1].payload(),
        bytes(SetSubtitleText {
            text: Component::text("fast")
        })
    );
    assert_eq!(
        messages[0][2].payload(),
        bytes(SetTitleText {
            text: Component::text("Go")
        })
    );
    assert!(t.alex.drain().is_empty());

    // without times the client keeps its own, and the subtitle of before is blanked
    t.ctx()
        .show_title(Everyone, &Title::new(Component::text("again")));
    let got = t.alex.drain();
    let ids: Vec<i32> = got.iter().map(|r| r.id).collect();
    assert_eq!(ids, [out::SET_SUBTITLE_TEXT, out::SET_TITLE_TEXT]);
    assert_eq!(
        got[0].payload(),
        bytes(SetSubtitleText {
            text: Component::default()
        })
    );

    t.ctx().clear_title(steve);
    t.ctx().reset_title(steve);
    let got = t.steve.drain();
    let last_two: Vec<&Received> = got.iter().rev().take(2).collect();
    assert_eq!(last_two[1].payload(), bytes(ClearTitles { reset: false }));
    assert_eq!(last_two[0].payload(), bytes(ClearTitles { reset: true }));
}

#[test]
fn a_sound_plays_at_a_place_for_everyone_or_where_each_listener_stands() {
    let mut t = three();
    let (steve, alex) = (t.steve_id, t.alex_id);
    let levelup = sound::ENTITY_PLAYER_LEVELUP;

    let placed = Sound::new(levelup)
        .at(Vec3::new(1.5, 64.0, -2.0))
        .source(SoundSource::Players)
        .volume(0.5)
        .pitch(2.0)
        .seed(42);
    t.ctx().play_sound(Everyone.except(alex), &placed);
    assert!(t.alex.drain().is_empty());
    let expected = bytes(lodeframe::protocol::packets::play::Sound {
        sound: SoundHolder::Registered(levelup),
        source: SoundSource::Players,
        x: 12,
        y: 512,
        z: -16,
        volume: 0.5,
        pitch: 2.0,
        seed: 42,
    });
    let a = t.steve.drain();
    assert_eq!(a[0].id, out::SOUND);
    assert_eq!(a[0].payload(), expected);
    assert_eq!(t.kit.drain()[0].payload(), expected);

    // Alex walks away; the sound that plays where the listener is differs between them
    t.env.send(
        &t.alex,
        &MovePlayerPos {
            position: Vec3::new(10.0, -60.0, 0.5),
            flags: 1,
        },
    );
    t.alex.drain();
    t.steve.drain();
    t.kit.drain();
    let near = Sound::new(levelup).seed(1);
    t.ctx().play_sound([steve, alex], &near);
    let at_steve = t.steve.drain();
    let at_alex = t.alex.drain();
    assert_ne!(at_steve[0].payload(), at_alex[0].payload());
    let packet = |x: f64| {
        bytes(lodeframe::protocol::packets::play::Sound {
            sound: SoundHolder::Registered(levelup),
            source: SoundSource::Master,
            x: (x * 8.0) as i32,
            y: -480,
            z: 4,
            volume: 1.0,
            pitch: 1.0,
            seed: 1,
        })
    };
    assert_eq!(at_steve[0].payload(), packet(0.5));
    assert_eq!(at_alex[0].payload(), packet(10.0));

    // a sound without a seed of its own gets a different one each time
    let plain = Sound::new(levelup).at(Vec3::ZERO);
    t.ctx().play_sound(steve, &plain);
    t.ctx().play_sound(steve, &plain);
    let got = t.steve.drain();
    assert_eq!(got.len(), 2);
    assert_ne!(got[0].payload(), got[1].payload());
}

#[test]
fn a_sound_the_game_does_not_have_is_sent_by_name() {
    let mut t = three();
    let steve = t.steve_id;
    let name = Identifier::new("pack:boom").unwrap();
    let boom = Sound::custom(name.clone())
        .fixed_range(16.0)
        .seed(9)
        .at(Vec3::ZERO);
    t.ctx().play_sound(steve, &boom);
    assert_eq!(
        t.steve.drain()[0].payload(),
        bytes(lodeframe::protocol::packets::play::Sound {
            sound: SoundHolder::Direct {
                name: name.clone(),
                fixed_range: Some(16.0)
            },
            source: SoundSource::Master,
            x: 0,
            y: 0,
            z: 0,
            volume: 1.0,
            pitch: 1.0,
            seed: 9,
        })
    );
}

#[test]
fn stopping_sounds_names_what_to_stop() {
    let mut t = three();
    let steve = t.steve_id;
    let name = Identifier::new("minecraft:entity.player.levelup").unwrap();

    t.ctx().stop_sound(steve, &StopSound::all());
    t.ctx().stop_sound(
        steve,
        &StopSound::all()
            .source(SoundSource::Music)
            .event(sound::ENTITY_PLAYER_LEVELUP),
    );
    let got = t.steve.drain();
    assert_eq!(got[0].id, out::STOP_SOUND);
    assert_eq!(
        got[0].payload(),
        bytes(WireStop {
            source: None,
            name: None
        })
    );
    assert_eq!(
        got[1].payload(),
        bytes(WireStop {
            source: Some(SoundSource::Music),
            name: Some(name)
        })
    );
}

fn boss(received: &[Received]) -> Vec<Vec<u8>> {
    received
        .iter()
        .filter(|r| r.id == out::BOSS_EVENT)
        .map(|r| r.payload().to_vec())
        .collect()
}

/// The packet for operation `operation` of the bar that has this id (the first the world made).
fn boss_packet(id: Uuid, operation: BossEventOperation) -> Vec<u8> {
    bytes(BossEvent { id, operation })
}

#[test]
fn a_boss_bar_shows_and_what_changes_reaches_those_who_see_it() {
    let mut t = three();
    let (steve, alex, kit) = (t.steve_id, t.alex_id, t.kit_id);
    let bar = BossBar::new(Component::text("Boss"))
        .progress(0.5)
        .color(BossBarColor::Red)
        .overlay(BossBarOverlay::Notched10)
        .darken_sky();
    let id = t.ctx().create_bossbar(bar);
    // nobody sees it until it is shown
    t.ctx().set_bossbar_progress(id, 0.4);
    assert!(boss(&t.steve.drain()).is_empty());

    t.ctx().show_bossbar([steve, alex], id);
    let seen = boss(&t.steve.drain());
    assert_eq!(seen.len(), 1);
    assert_eq!(boss(&t.alex.drain()), seen);
    assert!(boss(&t.kit.drain()).is_empty());
    let uuid = Uuid(u128::from_be_bytes(seen[0][..16].try_into().unwrap()));
    assert_eq!(
        seen[0],
        boss_packet(
            uuid,
            BossEventOperation::Add {
                title: Component::text("Boss"),
                progress: 0.4,
                color: BossBarColor::Red,
                overlay: BossBarOverlay::Notched10,
                flags: BossBarFlags {
                    darken_sky: true,
                    ..Default::default()
                },
            }
        )
    );
    let mut viewers = t.ctx().bossbar_viewers(id);
    viewers.sort_by_key(|p| p.uuid());
    let mut expected = vec![steve, alex];
    expected.sort_by_key(|p| p.uuid());
    assert_eq!(viewers, expected);

    // showing again to those who see it, and to one more, sends to the one more only
    t.ctx().show_bossbar(Everyone, id);
    assert!(boss(&t.steve.drain()).is_empty());
    assert!(boss(&t.alex.drain()).is_empty());
    assert_eq!(boss(&t.kit.drain()).len(), 1);
    t.ctx().hide_bossbar(kit, id);
    assert_eq!(
        boss(&t.kit.drain()),
        [boss_packet(uuid, BossEventOperation::Remove)]
    );

    // every change reaches the two who see it, and not the one who does not
    t.ctx().set_bossbar_progress(id, 0.25);
    t.ctx().set_bossbar_title(id, &Component::text("Boss 2"));
    t.ctx().set_bossbar_color(id, BossBarColor::Green);
    t.ctx().set_bossbar_overlay(id, BossBarOverlay::Progress);
    let flags = BossBarFlags {
        darken_sky: false,
        play_music: true,
        create_fog: true,
    };
    t.ctx().set_bossbar_flags(id, flags);
    let expected = [
        boss_packet(uuid, BossEventOperation::UpdateProgress(0.25)),
        boss_packet(
            uuid,
            BossEventOperation::UpdateTitle(Component::text("Boss 2")),
        ),
        boss_packet(
            uuid,
            BossEventOperation::UpdateStyle {
                color: BossBarColor::Green,
                overlay: BossBarOverlay::Notched10,
            },
        ),
        boss_packet(
            uuid,
            BossEventOperation::UpdateStyle {
                color: BossBarColor::Green,
                overlay: BossBarOverlay::Progress,
            },
        ),
        boss_packet(uuid, BossEventOperation::UpdateFlags(flags)),
    ];
    assert_eq!(boss(&t.steve.drain()), expected);
    assert_eq!(boss(&t.alex.drain()), expected);
    assert!(boss(&t.kit.drain()).is_empty());
    let now = t.ctx().bossbar(id).unwrap().clone();
    assert_eq!(now.get_progress(), 0.25);
    assert_eq!(now.get_color(), BossBarColor::Green);
    assert_eq!(now.get_overlay(), BossBarOverlay::Progress);
    assert_eq!(now.get_flags(), flags);
    assert_eq!(now.get_title(), &Component::text("Boss 2"));

    // the same value again sends nothing; progress is held between 0 and 1
    t.ctx().set_bossbar_progress(id, 0.25);
    t.ctx().set_bossbar_color(id, BossBarColor::Green);
    t.ctx().set_bossbar_title(id, &Component::text("Boss 2"));
    assert!(boss(&t.steve.drain()).is_empty());
    t.alex.drain();
    t.ctx().set_bossbar_progress(id, 7.0);
    t.ctx().set_bossbar_progress(id, f32::NAN);
    assert_eq!(
        boss(&t.steve.drain()),
        [
            boss_packet(uuid, BossEventOperation::UpdateProgress(1.0)),
            boss_packet(uuid, BossEventOperation::UpdateProgress(0.0)),
        ]
    );

    // hiding it from one leaves the other
    t.ctx().hide_bossbar(steve, id);
    assert_eq!(boss(&t.steve.drain()).len(), 1);
    t.ctx().set_bossbar_progress(id, 0.5);
    assert!(boss(&t.steve.drain()).is_empty());
    // 1.0 and 0.0 from before, which only Steve's side drained, and 0.5 now
    assert_eq!(boss(&t.alex.drain()).len(), 3);
}

#[test]
fn a_boss_bar_that_is_removed_goes_from_its_viewers_and_names_nothing() {
    let mut t = three();
    let (steve, alex) = (t.steve_id, t.alex_id);
    let id = t
        .ctx()
        .create_bossbar(BossBar::new(Component::text("Boss")));
    t.ctx().show_bossbar(steve, id);
    let shown = boss(&t.steve.drain());
    let uuid = Uuid(u128::from_be_bytes(shown[0][..16].try_into().unwrap()));

    t.ctx().remove_bossbar(id);
    assert_eq!(
        boss(&t.steve.drain()),
        [boss_packet(uuid, BossEventOperation::Remove)]
    );
    assert!(t.ctx().bossbar(id).is_none());
    assert!(t.ctx().bossbar_viewers(id).is_empty());
    // everything that takes the id finds nothing
    t.ctx().show_bossbar(alex, id);
    t.ctx().set_bossbar_progress(id, 0.1);
    t.ctx().hide_bossbar(steve, id);
    t.ctx().remove_bossbar(id);
    assert!(boss(&t.steve.drain()).is_empty());
    assert!(boss(&t.alex.drain()).is_empty());

    // two bars are two ids and two UUIDs for the client
    let one = t.ctx().create_bossbar(BossBar::new(Component::text("1")));
    let two = t.ctx().create_bossbar(BossBar::new(Component::text("2")));
    assert_ne!(one, two);
    t.ctx().show_bossbar(steve, one);
    t.ctx().show_bossbar(steve, two);
    let got = boss(&t.steve.drain());
    assert_ne!(got[0][..16], got[1][..16]);
}

#[test]
fn a_player_who_leaves_stops_seeing_the_boss_bars_and_a_new_stay_does_not_inherit_them() {
    let mut t = three();
    let (steve, alex) = (t.steve_id, t.alex_id);
    let id = t
        .ctx()
        .create_bossbar(BossBar::new(Component::text("Boss")));
    t.ctx().show_bossbar([steve, alex], id);
    t.alex.drain();

    let Three {
        mut env,
        steve: steve_client,
        mut alex,
        ..
    } = t;
    env.disconnect(steve_client);
    assert_eq!(
        env.instance().bossbar_viewers(id),
        [alex_id_of(&env, &alex)]
    );

    let mut again = env.connect("Steve");
    again.drain();
    env.instance_mut().set_bossbar_progress(id, 0.1);
    assert!(boss(&again.drain()).is_empty());
    assert_eq!(boss(&alex.drain()).len(), 1);
}

fn alex_id_of(env: &TestEnv<World>, alex: &FakePlayer) -> PlayerId {
    env.instance().player_id(alex.uuid()).unwrap()
}
