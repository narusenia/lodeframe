// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Who a message, a title, a sound or a boss bar goes to, and the things to send.
//!
//! An [`Audience`] is a set of players that [`Ctx`] resolves when it sends: one [`PlayerId`],
//! [`Everyone`], a [`Group`], everyone but someone ([`AudienceExt::except`]), or anything of
//! your own. The same call takes any of them:
//!
//! ```
//! # use lodeframe::{Audience, AudienceExt, Everyone, world::{Ctx, PlayerId}, text::Component};
//! fn announce(ctx: &mut Ctx, who: PlayerId, others: &[PlayerId]) {
//!     let line = Component::text("round 1");
//!     ctx.send_message(who, &line); // one player
//!     ctx.send_message(Everyone, &line); // everyone
//!     ctx.send_message(Everyone.except(who), &line); // everyone but one
//!     ctx.send_message(others, &line); // any list
//! }
//! ```
//!
//! What is sent goes through [`Ctx`]: `send_message`, `send_action_bar`, `show_title`,
//! `set_tab_list`, `play_sound`, `stop_sound` and the boss bar methods. Each encodes the packet
//! once for everyone.

use std::{collections::HashSet, time::Duration};

use crate::{
    protocol::{Identifier, SoundEvent, Vec3, packets::play::SoundHolder},
    text::Component,
    world::{Ctx, PlayerId},
};

pub use crate::protocol::packets::play::{BossBarColor, BossBarFlags, BossBarOverlay, SoundSource};

/// A set of players that something can be sent to.
///
/// [`Ctx`] asks for the members when it sends, so an audience can be made before the players
/// are, and keeps meaning what it says: a [`Group`] is who is in it now.
pub trait Audience {
    /// Adds the players to `out`. The same player may come more than once and players who are
    /// gone may be among them: the sender drops those.
    fn members(&self, ctx: &Ctx, out: &mut Vec<PlayerId>);
}

impl Audience for PlayerId {
    fn members(&self, _: &Ctx, out: &mut Vec<PlayerId>) {
        out.push(*self);
    }
}

impl<A: Audience + ?Sized> Audience for &A {
    fn members(&self, ctx: &Ctx, out: &mut Vec<PlayerId>) {
        (**self).members(ctx, out);
    }
}

impl<A: Audience + ?Sized> Audience for Box<A> {
    fn members(&self, ctx: &Ctx, out: &mut Vec<PlayerId>) {
        (**self).members(ctx, out);
    }
}

impl<A: Audience> Audience for [A] {
    fn members(&self, ctx: &Ctx, out: &mut Vec<PlayerId>) {
        for a in self {
            a.members(ctx, out);
        }
    }
}

impl<A: Audience> Audience for Vec<A> {
    fn members(&self, ctx: &Ctx, out: &mut Vec<PlayerId>) {
        self.as_slice().members(ctx, out);
    }
}

impl<A: Audience, const N: usize> Audience for [A; N] {
    fn members(&self, ctx: &Ctx, out: &mut Vec<PlayerId>) {
        self.as_slice().members(ctx, out);
    }
}

/// Everyone in the world. In v0.2 a world is one instance, so this is everyone on the instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Everyone;

impl Audience for Everyone {
    fn members(&self, ctx: &Ctx, out: &mut Vec<PlayerId>) {
        out.extend(ctx.players());
    }
}

/// An audience without some of its members. See [`AudienceExt::except`].
#[derive(Debug, Clone, Copy)]
pub struct Except<A, B> {
    audience: A,
    except: B,
}

impl<A: Audience, B: Audience> Audience for Except<A, B> {
    fn members(&self, ctx: &Ctx, out: &mut Vec<PlayerId>) {
        let mut left_out = Vec::new();
        self.except.members(ctx, &mut left_out);
        let left_out: HashSet<PlayerId> = left_out.into_iter().collect();
        let mut all = Vec::new();
        self.audience.members(ctx, &mut all);
        out.extend(all.into_iter().filter(|p| !left_out.contains(p)));
    }
}

/// Two audiences as one. See [`AudienceExt::and`].
#[derive(Debug, Clone, Copy)]
pub struct And<A, B>(A, B);

impl<A: Audience, B: Audience> Audience for And<A, B> {
    fn members(&self, ctx: &Ctx, out: &mut Vec<PlayerId>) {
        self.0.members(ctx, out);
        self.1.members(ctx, out);
    }
}

/// Ways to combine audiences. Import it to use them: `use lodeframe::AudienceExt;`.
pub trait AudienceExt: Audience + Sized {
    /// This audience without the players of `except` (one [`PlayerId`], a [`Group`], ...).
    /// `Everyone.except(player)` is everyone but `player`.
    fn except<B: Audience>(self, except: B) -> Except<Self, B> {
        Except {
            audience: self,
            except,
        }
    }

    /// The players of both.
    fn and<B: Audience>(self, other: B) -> And<Self, B> {
        And(self, other)
    }
}

impl<A: Audience> AudienceExt for A {}

/// An audience made by a function. The function adds the players to the list it is given, as
/// [`Audience::members`] does.
pub fn from_fn<F: Fn(&Ctx, &mut Vec<PlayerId>)>(f: F) -> FromFn<F> {
    FromFn(f)
}

/// An audience made by [`from_fn`].
pub struct FromFn<F>(F);

impl<F: Fn(&Ctx, &mut Vec<PlayerId>)> Audience for FromFn<F> {
    fn members(&self, ctx: &Ctx, out: &mut Vec<PlayerId>) {
        (self.0)(ctx, out);
    }
}

/// A set of players you add to and remove from.
///
/// One of your own works as an audience. The ones in a [`Ctx`] (see [`Ctx::join_group`]) also
/// lose a player who leaves.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Group {
    members: HashSet<PlayerId>,
}

impl Group {
    /// An empty group.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a player. Returns `false` if they were in it already.
    pub fn add(&mut self, player: PlayerId) -> bool {
        self.members.insert(player)
    }

    /// Takes a player out. Returns `false` if they were not in it.
    pub fn remove(&mut self, player: PlayerId) -> bool {
        self.members.remove(&player)
    }

    /// Whether the player is in it.
    pub fn contains(&self, player: PlayerId) -> bool {
        self.members.contains(&player)
    }

    /// How many are in it.
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// Whether nobody is.
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// Takes everyone out.
    pub fn clear(&mut self) {
        self.members.clear();
    }

    /// Who is in it, in no particular order.
    pub fn iter(&self) -> impl Iterator<Item = PlayerId> + '_ {
        self.members.iter().copied()
    }
}

impl Audience for Group {
    fn members(&self, _: &Ctx, out: &mut Vec<PlayerId>) {
        out.extend(self.members.iter().copied());
    }
}

/// An audience the [`Ctx`] has under a key: a group made with [`Ctx::join_group`], or one given
/// to [`Ctx::register_audience`]. See [`Ctx::audience`].
///
/// The key is looked up when something is sent, so this can be made before the group exists. A
/// key that is both a group and a registered audience means the group; one that is neither is
/// nobody.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named(pub(crate) String);

impl Named {
    /// The key.
    pub fn key(&self) -> &str {
        &self.0
    }
}

impl Audience for Named {
    fn members(&self, ctx: &Ctx, out: &mut Vec<PlayerId>) {
        if let Some(group) = ctx.group(&self.0) {
            group.members(ctx, out);
        } else if let Some(audience) = ctx.registered_audience(&self.0) {
            audience.members(ctx, out);
        }
    }
}

/// A title in the middle of the screen with a subtitle under it, for [`Ctx::show_title`].
#[derive(Debug, Clone, PartialEq)]
pub struct Title {
    /// The big text.
    pub title: Component,
    /// The smaller text under it. Empty by default, which also clears an earlier subtitle.
    pub subtitle: Component,
    /// How long it fades in, stays and fades out. `None` keeps what the client has, which is
    /// half a second, three and a half seconds and one second until it is told otherwise.
    pub times: Option<TitleTimes>,
}

impl Title {
    /// A title without a subtitle and with the client's usual times.
    pub fn new(title: impl Into<Component>) -> Self {
        Self {
            title: title.into(),
            subtitle: Component::default(),
            times: None,
        }
    }

    /// Sets the subtitle.
    pub fn subtitle(mut self, subtitle: impl Into<Component>) -> Self {
        self.subtitle = subtitle.into();
        self
    }

    /// Sets how long it fades in, stays and fades out.
    pub fn times(mut self, fade_in: Duration, stay: Duration, fade_out: Duration) -> Self {
        self.times = Some(TitleTimes {
            fade_in,
            stay,
            fade_out,
        });
        self
    }
}

/// How long a [`Title`] takes. The client counts ticks of 50 ms: a time that is not a whole
/// number of them is rounded up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TitleTimes {
    /// Fading in.
    pub fade_in: Duration,
    /// Staying.
    pub stay: Duration,
    /// Fading out.
    pub fade_out: Duration,
}

/// A time in the ticks the client counts, rounded up and capped at what the packet holds.
pub(crate) fn ticks(time: Duration) -> i32 {
    i32::try_from(time.as_millis().div_ceil(50)).unwrap_or(i32::MAX)
}

/// Where a [`Sound`] is heard from.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Origin {
    Listener,
    At(Vec3),
}

/// A sound to play, for [`Ctx::play_sound`].
#[derive(Debug, Clone, PartialEq)]
pub struct Sound {
    sound: SoundHolder,
    source: SoundSource,
    volume: f32,
    pitch: f32,
    seed: Option<i64>,
    origin: Origin,
}

impl Sound {
    /// A sound of the game, like `sound::ENTITY_PLAYER_LEVELUP`. It plays at the volume and
    /// pitch of 1.0, under the master slider, where each listener stands.
    pub fn new(sound: SoundEvent) -> Self {
        Self::of(SoundHolder::Registered(sound))
    }

    /// A sound that is not in the game, such as one of a resource pack: the client looks it up
    /// by `name`.
    pub fn custom(name: Identifier) -> Self {
        Self::of(SoundHolder::Direct {
            name,
            fixed_range: None,
        })
    }

    fn of(sound: SoundHolder) -> Self {
        Self {
            sound,
            source: SoundSource::Master,
            volume: 1.0,
            pitch: 1.0,
            seed: None,
            origin: Origin::Listener,
        }
    }

    /// Under which volume slider of the client it plays.
    pub fn source(mut self, source: SoundSource) -> Self {
        self.source = source;
        self
    }

    /// The volume. 1.0 is the usual; more makes it carry further instead of louder.
    pub fn volume(mut self, volume: f32) -> Self {
        self.volume = volume;
        self
    }

    /// The pitch. 1.0 is the usual; the client takes 0.5 to 2.0.
    pub fn pitch(mut self, pitch: f32) -> Self {
        self.pitch = pitch;
        self
    }

    /// Picks which of the sound's variants plays: the same seed gives the same one. Without it
    /// every play picks by its own.
    pub fn seed(mut self, seed: i64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// How far, in blocks, a sound made with [`custom`](Self::custom) is heard, whatever its
    /// volume. Does nothing to a sound of the game.
    pub fn fixed_range(mut self, blocks: f32) -> Self {
        if let SoundHolder::Direct { fixed_range, .. } = &mut self.sound {
            *fixed_range = Some(blocks);
        }
        self
    }

    /// Plays it at a place. It is encoded once for everyone, and each listener hears it from
    /// where it is.
    pub fn at(mut self, pos: Vec3) -> Self {
        self.origin = Origin::At(pos);
        self
    }

    /// Plays it where each listener stands, so that they all hear it right at them. This is
    /// what a sound does until [`at`](Self::at) is called, and it is encoded for each listener
    /// since the place differs.
    pub fn at_listener(mut self) -> Self {
        self.origin = Origin::Listener;
        self
    }

    /// The packet, for a listener at `listener` and a sound that is `seed` unless it has its own.
    pub(crate) fn packet(
        &self,
        listener: Vec3,
        seed: i64,
    ) -> crate::protocol::packets::play::Sound {
        let pos = match self.origin {
            Origin::Listener => listener,
            Origin::At(pos) => pos,
        };
        let eighths = |v: f64| (v * 8.0) as i32;
        crate::protocol::packets::play::Sound {
            sound: self.sound.clone(),
            source: self.source,
            x: eighths(pos.x),
            y: eighths(pos.y),
            z: eighths(pos.z),
            volume: self.volume,
            pitch: self.pitch,
            seed: self.seed.unwrap_or(seed),
        }
    }

    /// Whether the packet is the same for every listener.
    pub(crate) fn is_placed(&self) -> bool {
        matches!(self.origin, Origin::At(_))
    }
}

/// Which sounds to stop, for [`Ctx::stop_sound`]. Without anything set it stops them all.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StopSound {
    source: Option<SoundSource>,
    name: Option<Identifier>,
}

impl StopSound {
    /// Stops every sound.
    pub fn all() -> Self {
        Self::default()
    }

    /// Narrows it to the sounds under one slider.
    pub fn source(mut self, source: SoundSource) -> Self {
        self.source = Some(source);
        self
    }

    /// Narrows it to the sound with this name.
    pub fn sound(mut self, name: Identifier) -> Self {
        self.name = Some(name);
        self
    }

    /// Narrows it to a sound of the game.
    pub fn event(self, sound: SoundEvent) -> Self {
        // the names of the registry are all valid identifiers
        let name = Identifier::new(sound.name()).expect("sound event names are identifiers");
        self.sound(name)
    }

    pub(crate) fn packet(&self) -> crate::protocol::packets::play::StopSound {
        crate::protocol::packets::play::StopSound {
            source: self.source,
            name: self.name.clone(),
        }
    }
}

/// A boss bar: what it says, how full it is and how it looks. Made with [`BossBar::new`], then
/// given to [`Ctx::create_bossbar`] and shown with [`Ctx::show_bossbar`].
#[derive(Debug, Clone, PartialEq)]
pub struct BossBar {
    pub(crate) title: Component,
    pub(crate) progress: f32,
    pub(crate) color: BossBarColor,
    pub(crate) overlay: BossBarOverlay,
    pub(crate) flags: BossBarFlags,
}

impl BossBar {
    /// A full pink bar without notches.
    pub fn new(title: impl Into<Component>) -> Self {
        Self {
            title: title.into(),
            progress: 1.0,
            color: BossBarColor::default(),
            overlay: BossBarOverlay::default(),
            flags: BossBarFlags::default(),
        }
    }

    /// How full it is, 0.0 to 1.0. Anything else is brought into that, and NaN is empty.
    pub fn progress(mut self, progress: f32) -> Self {
        self.progress = clamp_progress(progress);
        self
    }

    /// The colour.
    pub fn color(mut self, color: BossBarColor) -> Self {
        self.color = color;
        self
    }

    /// How it is cut into notches.
    pub fn overlay(mut self, overlay: BossBarOverlay) -> Self {
        self.overlay = overlay;
        self
    }

    /// Darkens the sky while it is shown.
    pub fn darken_sky(mut self) -> Self {
        self.flags.darken_sky = true;
        self
    }

    /// Plays the boss music while it is shown.
    pub fn play_boss_music(mut self) -> Self {
        self.flags.play_music = true;
        self
    }

    /// Makes the fog thicker while it is shown.
    pub fn create_fog(mut self) -> Self {
        self.flags.create_fog = true;
        self
    }

    /// The title.
    pub fn get_title(&self) -> &Component {
        &self.title
    }

    /// How full it is.
    pub fn get_progress(&self) -> f32 {
        self.progress
    }

    /// The colour.
    pub fn get_color(&self) -> BossBarColor {
        self.color
    }

    /// How it is cut into notches.
    pub fn get_overlay(&self) -> BossBarOverlay {
        self.overlay
    }

    /// What it does to the world.
    pub fn get_flags(&self) -> BossBarFlags {
        self.flags
    }
}

pub(crate) fn clamp_progress(progress: f32) -> f32 {
    if progress.is_nan() {
        0.0
    } else {
        progress.clamp(0.0, 1.0)
    }
}

/// Names a boss bar of a [`Ctx`]. It is `Copy`, so a handler can keep it. After
/// [`Ctx::remove_bossbar`] it names nothing and the calls that take it do nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BossBarId(pub(crate) u64);

/// What a [`Ctx`] keeps for audiences: the groups, the audiences registered by key and the boss
/// bars with who sees them.
#[derive(Default)]
pub(crate) struct Audiences {
    pub(crate) groups: std::collections::HashMap<String, Group>,
    pub(crate) registered: std::collections::HashMap<String, Box<dyn Audience>>,
    pub(crate) bossbars: std::collections::HashMap<BossBarId, Shown>,
    pub(crate) next_bossbar: u64,
    pub(crate) sounds: u64,
}

/// A boss bar and the players it is shown to.
pub(crate) struct Shown {
    pub(crate) bar: BossBar,
    pub(crate) viewers: HashSet<crate::protocol::Uuid>,
}

impl Audiences {
    /// Takes a player who left out of every group and boss bar.
    pub(crate) fn forget(&mut self, player: crate::protocol::Uuid) {
        for group in self.groups.values_mut() {
            group.members.retain(|p| p.uuid() != player);
        }
        for shown in self.bossbars.values_mut() {
            shown.viewers.remove(&player);
        }
    }

    /// A seed for a sound that has none: a different one each time.
    pub(crate) fn next_seed(&mut self) -> i64 {
        // splitmix64 over a counter: cheap, and the neighbours look nothing alike
        self.sounds = self.sounds.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.sounds;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        (z ^ (z >> 31)) as i64
    }
}
