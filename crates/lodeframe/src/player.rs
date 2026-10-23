// SPDX-License-Identifier: Apache-2.0 OR MIT
//! What a player is allowed and where they are: the game mode, abilities and teleports. The
//! calls are on [`Ctx`](crate::world::Ctx), which takes a [`PlayerId`].

use crate::{
    event::{Event, Parents},
    protocol::{
        Vec3,
        packets::play::{
            ABILITY_ALLOW_FLYING, ABILITY_FLYING, ABILITY_INSTANT_BREAK, ABILITY_INVULNERABLE,
            PlayerAbilities,
        },
    },
    world::{PlayerEvent, PlayerId},
};

/// How the game treats a player: whether they can break and place blocks, fly, or be hurt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum GameMode {
    /// Breaks blocks by digging, takes damage and has a health bar.
    Survival,
    /// Breaks blocks at once, can fly and takes no damage. What players start in.
    #[default]
    Creative,
    /// Cannot break or place blocks.
    Adventure,
    /// Flies through everything and cannot break or place blocks.
    Spectator,
}

impl GameMode {
    /// The number the protocol gives it: 0 survival, 1 creative, 2 adventure, 3 spectator.
    pub fn id(self) -> u8 {
        match self {
            Self::Survival => 0,
            Self::Creative => 1,
            Self::Adventure => 2,
            Self::Spectator => 3,
        }
    }

    /// The mode with this number, if there is one.
    pub fn from_id(id: i32) -> Option<Self> {
        match id {
            0 => Some(Self::Survival),
            1 => Some(Self::Creative),
            2 => Some(Self::Adventure),
            3 => Some(Self::Spectator),
            _ => None,
        }
    }

    /// Whether a player in this mode can break and place blocks.
    pub fn can_edit_blocks(self) -> bool {
        matches!(self, Self::Survival | Self::Creative)
    }
}

/// What a player may do, and how fast. See [`Ctx::set_abilities`](crate::world::Ctx::set_abilities).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Abilities {
    /// Takes no damage.
    pub invulnerable: bool,
    /// Is flying now.
    pub flying: bool,
    /// May start flying (by jumping twice).
    pub allow_flying: bool,
    /// Breaks blocks at once.
    pub instant_break: bool,
    /// How fast they fly. 0.05 is the usual.
    pub fly_speed: f32,
    /// How fast they walk, which the client shows as how wide the field of view is. 0.1 is the
    /// usual.
    pub walk_speed: f32,
}

impl Abilities {
    /// The usual speed of flying.
    pub const FLY_SPEED: f32 = 0.05;
    /// The usual speed of walking.
    pub const WALK_SPEED: f32 = 0.1;

    /// What a player has in `mode`, at the usual speeds: survival and adventure have nothing,
    /// creative can fly, break at once and take no damage, spectator also flies from the start.
    pub fn for_mode(mode: GameMode) -> Self {
        Self {
            invulnerable: matches!(mode, GameMode::Creative | GameMode::Spectator),
            flying: mode == GameMode::Spectator,
            allow_flying: matches!(mode, GameMode::Creative | GameMode::Spectator),
            instant_break: mode == GameMode::Creative,
            fly_speed: Self::FLY_SPEED,
            walk_speed: Self::WALK_SPEED,
        }
    }

    pub(crate) fn packet(&self) -> PlayerAbilities {
        let bit = |on: bool, flag: u8| if on { flag } else { 0 };
        PlayerAbilities {
            flags: bit(self.invulnerable, ABILITY_INVULNERABLE)
                | bit(self.flying, ABILITY_FLYING)
                | bit(self.allow_flying, ABILITY_ALLOW_FLYING)
                | bit(self.instant_break, ABILITY_INSTANT_BREAK),
            fly_speed: self.fly_speed,
            walk_speed: self.walk_speed,
        }
    }
}

impl Default for Abilities {
    /// What a player has in [`GameMode::default`].
    fn default() -> Self {
        Self::for_mode(GameMode::default())
    }
}

/// The parts of a position that a [`Teleport`] is relative in: the ones that are set are added
/// to what the player has, the others replace it. Combine them with `|`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Relative(u8);

impl Relative {
    /// Nothing is relative.
    pub const NONE: Self = Self(0);
    /// The x coordinate.
    pub const X: Self = Self(1);
    /// The y coordinate.
    pub const Y: Self = Self(2);
    /// The z coordinate.
    pub const Z: Self = Self(4);
    /// The yaw (which way they face around).
    pub const YAW: Self = Self(8);
    /// The pitch (up and down).
    pub const PITCH: Self = Self(16);

    /// Whether every part of `other` is in this.
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// The bits the protocol gives these parts: x, y, z, then yaw and pitch.
    pub(crate) fn bits(self) -> i32 {
        i32::from(self.0)
    }
}

impl std::ops::BitOr for Relative {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// Where to put a player, for [`Ctx::teleport`](crate::world::Ctx::teleport).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Teleport {
    pub(crate) position: Vec3,
    pub(crate) rotation: Option<(f32, f32)>,
    pub(crate) relative: Relative,
}

impl Teleport {
    /// To `position`, facing the way they do.
    pub fn new(position: Vec3) -> Self {
        Self {
            position,
            rotation: None,
            relative: Relative::NONE,
        }
    }

    /// Turns them to face `yaw` and `pitch` degrees.
    pub fn yaw_pitch(mut self, yaw: f32, pitch: f32) -> Self {
        self.rotation = Some((yaw, pitch));
        self
    }

    /// Makes the parts in `relative` offsets from where the player is. A position of
    /// `Vec3::new(0.0, 5.0, 0.0)` that is relative in x, y and z lifts them 5 blocks.
    pub fn relative(mut self, relative: Relative) -> Self {
        self.relative = relative;
        self
    }
}

/// A player asked for another game mode with F3+F4. Nothing happens unless a handler changes it
/// with [`Ctx::set_game_mode`](crate::world::Ctx::set_game_mode): who may is up to you.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameModeRequestEvent {
    /// Who asked.
    pub player: PlayerId,
    /// The mode they asked for.
    pub mode: GameMode,
}

/// A player's game mode changed. The abilities and the tab list already show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameModeChangedEvent {
    /// Whose mode.
    pub player: PlayerId,
    /// What it was.
    pub from: GameMode,
    /// What it is.
    pub to: GameMode,
}

/// A player started or stopped flying (by jumping twice). If it is not cancelled, they fly if
/// their [`Abilities::allow_flying`] is set; otherwise, or if it is cancelled, the client is
/// told to stay as it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlyingToggleEvent {
    /// Who.
    pub player: PlayerId,
    /// Whether they want to fly.
    pub flying: bool,
    pub(crate) cancelled: bool,
}

impl FlyingToggleEvent {
    /// Keeps the player as they were.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
}

macro_rules! player_event {
    ($($event:ty),*) => {$(
        impl PlayerEvent for $event {
            fn player(&self) -> PlayerId {
                self.player
            }
        }
    )*};
}
player_event!(
    GameModeRequestEvent,
    GameModeChangedEvent,
    FlyingToggleEvent
);

impl Event for GameModeRequestEvent {
    fn parents<C: 'static>(&mut self, parents: &mut Parents<'_, C>) {
        parents.visit::<dyn PlayerEvent>(self);
    }
}

impl Event for GameModeChangedEvent {
    fn parents<C: 'static>(&mut self, parents: &mut Parents<'_, C>) {
        parents.visit::<dyn PlayerEvent>(self);
    }
}

impl Event for FlyingToggleEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    fn parents<C: 'static>(&mut self, parents: &mut Parents<'_, C>) {
        parents.visit::<dyn PlayerEvent>(self);
    }
}
