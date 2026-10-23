// SPDX-License-Identifier: Apache-2.0 OR MIT
//! What a player is allowed and where they are: the game mode, abilities and teleports. The
//! calls are on [`Ctx`](crate::world::Ctx), which takes a [`PlayerId`].

use crate::{
    event::{Event, Parents},
    protocol::{
        Vec3,
        packets::play::{
            ABILITY_ALLOW_FLYING, ABILITY_FLYING, ABILITY_INSTANT_BREAK, ABILITY_INVULNERABLE,
            ClientInformation, PlayerAbilities,
        },
    },
    text::Component,
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

/// The most health a player has: ten hearts.
pub const MAX_HEALTH: f32 = 20.0;
/// The most food a player has: ten drumsticks.
pub const MAX_FOOD: i32 = 20;
/// The saturation a player has at full food after respawning.
pub const RESPAWN_SATURATION: f32 = 5.0;

/// A player's experience: the level on the bar, how far it is to the next, and the points
/// earned in all. See [`Ctx::set_experience`](crate::world::Ctx::set_experience).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Experience {
    /// The level shown, 0 or more.
    pub level: i32,
    /// How far to the next level, 0.0 to 1.0.
    pub progress: f32,
    /// All the points earned, which the death screen shows as the score.
    pub total: i32,
}

impl Experience {
    /// Points to get from `level` to the next one, by the game's rule.
    fn cost(level: i64) -> i64 {
        match level {
            ..=15 => 2 * level + 7,
            16..=30 => 5 * level - 38,
            _ => 9 * level - 158,
        }
    }

    /// Points it takes to reach `level` from nothing.
    fn points_for(level: i64) -> i64 {
        (0..level).map(Self::cost).sum()
    }

    /// `level` and how far `progress` (0.0 to 1.0) is to the next, with the points that makes.
    pub fn new(level: i32, progress: f32) -> Self {
        let level = level.max(0);
        let progress = if progress.is_finite() {
            progress.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let level_points = Self::points_for(i64::from(level));
        let into = (f64::from(progress) * Self::cost(i64::from(level)) as f64).round() as i64;
        Self {
            level,
            progress,
            total: i32::try_from(level_points + into).unwrap_or(i32::MAX),
        }
    }

    /// The level and progress that `points` come to, by the game's rule: 7 points reach level
    /// 1, 37 more from level 15 reach 16, 112 more from level 30 reach 31.
    pub fn from_points(points: i32) -> Self {
        let total = points.max(0);
        let mut level = 0;
        let mut left = i64::from(total);
        while left >= Self::cost(level) {
            left -= Self::cost(level);
            level += 1;
        }
        Self {
            level: i32::try_from(level).unwrap_or(i32::MAX),
            progress: (left as f64 / Self::cost(level) as f64) as f32,
            total,
        }
    }

    /// The same, brought into what the client takes: a level of 0 or more, a progress from 0.0 to
    /// 1.0 (not a number is 0.0), points of 0 or more.
    pub(crate) fn clamped(self) -> Self {
        Self {
            level: self.level.max(0),
            progress: if self.progress.is_finite() {
                self.progress.clamp(0.0, 1.0)
            } else {
                0.0
            },
            total: self.total.max(0),
        }
    }
}

/// Where a player comes back after dying, and which way they face.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RespawnPoint {
    /// The place.
    pub position: Vec3,
    /// Degrees around the y axis.
    pub yaw: f32,
    /// Degrees up and down.
    pub pitch: f32,
}

impl RespawnPoint {
    /// At `position`, facing yaw 0 and pitch 0.
    pub fn new(position: Vec3) -> Self {
        Self {
            position,
            yaw: 0.0,
            pitch: 0.0,
        }
    }
}

/// A player died, by [`Ctx::kill`](crate::world::Ctx::kill) or by their health falling to 0.
/// The death screen is open; they come back after [`PlayerRespawnEvent`].
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerDeathEvent {
    /// Who died.
    pub player: PlayerId,
    /// The death message on the screen.
    pub message: Component,
}

/// A dead player pressed respawn. What they come back with is in the fields, which a handler
/// may change; the player respawns unless it is cancelled, and then stays dead (their client
/// lets them press it again).
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerRespawnEvent {
    /// Who respawns.
    pub player: PlayerId,
    /// Where. The player's [respawn point](crate::world::Ctx::respawn_point) to begin with.
    pub position: Vec3,
    /// Degrees around the y axis.
    pub yaw: f32,
    /// Degrees up and down.
    pub pitch: f32,
    /// Health points, full to begin with.
    pub health: f32,
    /// Food level, full to begin with.
    pub food: i32,
    /// Saturation, 5.0 to begin with.
    pub saturation: f32,
    pub(crate) cancelled: bool,
}

impl PlayerRespawnEvent {
    /// Keeps the player dead.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
}

/// Which keys a player holds: what their client says with every change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Input {
    /// Forward (W).
    pub forward: bool,
    /// Backward (S).
    pub backward: bool,
    /// Left (A).
    pub left: bool,
    /// Right (D).
    pub right: bool,
    /// Jump.
    pub jump: bool,
    /// Sneak.
    pub sneak: bool,
    /// Sprint.
    pub sprint: bool,
}

impl Input {
    /// The keys of the bits of the packet: forward 1, backward 2, left 4, right 8, jump 0x10,
    /// sneak 0x20, sprint 0x40.
    pub fn from_bits(bits: u8) -> Self {
        let held = |bit: u8| bits & bit != 0;
        Self {
            forward: held(0x01),
            backward: held(0x02),
            left: held(0x04),
            right: held(0x08),
            jump: held(0x10),
            sneak: held(0x20),
            sprint: held(0x40),
        }
    }
}

/// Which chat a client shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChatMode {
    /// All of it.
    #[default]
    Full,
    /// Only what comes of commands.
    CommandsOnly,
    /// None.
    Hidden,
}

/// The parts of the skin a client shows, as the bits it sends: cape 1, jacket 2, left sleeve 4,
/// right sleeve 8, left pants 16, right pants 32, hat 64.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkinParts(pub u8);

impl SkinParts {
    /// Every part.
    pub const ALL: Self = Self(0x7f);
    /// The cape.
    pub const CAPE: u8 = 0x01;
    /// The jacket.
    pub const JACKET: u8 = 0x02;
    /// The hat.
    pub const HAT: u8 = 0x40;

    /// Whether every bit of `part` is shown.
    pub fn shows(self, part: u8) -> bool {
        self.0 & part == part
    }
}

/// Which hand a player uses most.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MainHand {
    /// The left.
    Left,
    /// The right.
    #[default]
    Right,
}

/// How many particles a client shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParticleStatus {
    /// All of them.
    #[default]
    All,
    /// Fewer.
    Decreased,
    /// The least.
    Minimal,
}

/// The options of a player's client: what their language is, how far they want to see, and so
/// on. See [`Ctx::client_settings`](crate::world::Ctx::client_settings).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientSettings {
    /// The language, like `ja_jp`; at most 16 characters.
    pub language: String,
    /// The view distance in chunks the client asks for, 2 to 32.
    pub view_distance: u8,
    /// Which chat it shows.
    pub chat_mode: ChatMode,
    /// Whether it shows colours in the chat.
    pub chat_colors: bool,
    /// The parts of the skin it shows.
    pub skin_parts: SkinParts,
    /// The main hand.
    pub main_hand: MainHand,
    /// Whether the client filters text.
    pub text_filtering: bool,
    /// Whether the player may show in server lists.
    pub allow_listing: bool,
    /// How many particles it shows.
    pub particles: ParticleStatus,
}

impl Default for ClientSettings {
    /// What the game has before the player changes anything.
    fn default() -> Self {
        Self {
            language: "en_us".into(),
            view_distance: 2,
            chat_mode: ChatMode::default(),
            chat_colors: true,
            skin_parts: SkinParts::ALL,
            main_hand: MainHand::default(),
            text_filtering: false,
            allow_listing: true,
            particles: ParticleStatus::default(),
        }
    }
}

/// What a client said, brought into what the game takes: a language of 16 characters at most, a
/// view distance of 2 to 32, and the options that name a choice it does not know are the default.
/// The packets of configuration and of play say it the same way.
macro_rules! settings_from_packet {
    ($($packet:ty),*) => {$(
        impl From<&$packet> for ClientSettings {
            fn from(info: &$packet) -> Self {
                Self {
                    language: info.language.chars().take(16).collect(),
                    view_distance: info.view_distance.clamp(2, 32) as u8,
                    chat_mode: match info.chat_mode.0 {
                        1 => ChatMode::CommandsOnly,
                        2 => ChatMode::Hidden,
                        _ => ChatMode::Full,
                    },
                    chat_colors: info.chat_colors,
                    skin_parts: SkinParts(info.skin_parts & 0x7f),
                    main_hand: if info.main_hand.0 == 0 {
                        MainHand::Left
                    } else {
                        MainHand::Right
                    },
                    text_filtering: info.text_filtering,
                    allow_listing: info.allow_listing,
                    particles: match info.particles.0 {
                        1 => ParticleStatus::Decreased,
                        2 => ParticleStatus::Minimal,
                        _ => ParticleStatus::All,
                    },
                }
            }
        }
    )*};
}
settings_from_packet!(
    ClientInformation,
    crate::protocol::packets::configuration::ClientInformation
);

/// A player moves or turns. If it is not cancelled they do, as their client says; the fields
/// may be changed, and then they are put where the fields say instead. Cancelled, they are put
/// back where they were. Either way the client is teleported, and what it sent in the meantime
/// is dropped until it confirms (see [`Ctx::teleport`](crate::world::Ctx::teleport)).
///
/// It is not sent for a client that says it stands where it stood, or for one that has not
/// confirmed a teleport, or for a dead one.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerMoveEvent {
    /// Who moves.
    pub player: PlayerId,
    /// Where they are now.
    pub from: Vec3,
    /// Where they say they are.
    pub position: Vec3,
    /// Degrees around the y axis, as they say.
    pub yaw: f32,
    /// Degrees up and down, as they say.
    pub pitch: f32,
    /// Whether they say they stand on something.
    pub on_ground: bool,
    pub(crate) cancelled: bool,
}

impl PlayerMoveEvent {
    /// Puts the player back where they were.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
}

/// A player pressed or released a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerInputEvent {
    /// Whose keys.
    pub player: PlayerId,
    /// The keys they hold now.
    pub input: Input,
    /// The keys they held.
    pub previous: Input,
}

/// A player started or stopped sneaking. Their pose has changed for the players who see them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerSneakEvent {
    /// Who.
    pub player: PlayerId,
    /// Whether they sneak now.
    pub sneaking: bool,
}

/// A player started or stopped sprinting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerSprintEvent {
    /// Who.
    pub player: PlayerId,
    /// Whether they sprint now.
    pub sprinting: bool,
}

/// Once for each player in every tick of the world, at the end of it. A dead player gets it too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerTickEvent {
    /// Who.
    pub player: PlayerId,
}

/// A player changed the options of their client while they were playing. What they had when
/// they joined is not an event: it is in [`Ctx::client_settings`](crate::world::Ctx::client_settings).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientSettingsEvent {
    /// Whose options.
    pub player: PlayerId,
    /// What they are now.
    pub settings: ClientSettings,
    /// What they were.
    pub previous: ClientSettings,
}

/// A player's client finished loading the world, after they joined or respawned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerLoadedEvent {
    /// Who.
    pub player: PlayerId,
}

/// A player's client swung its arm: they clicked at nothing, or at a block. The other players
/// are not shown it, so cancelling stops nothing yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArmSwingEvent {
    /// Who.
    pub player: PlayerId,
    pub(crate) cancelled: bool,
}

impl ArmSwingEvent {
    /// Marks the swing as cancelled, for the handlers that run after.
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
    FlyingToggleEvent,
    PlayerDeathEvent,
    PlayerRespawnEvent,
    PlayerMoveEvent,
    PlayerInputEvent,
    PlayerSneakEvent,
    PlayerSprintEvent,
    PlayerTickEvent,
    ClientSettingsEvent,
    PlayerLoadedEvent,
    ArmSwingEvent
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

impl Event for PlayerDeathEvent {
    fn parents<C: 'static>(&mut self, parents: &mut Parents<'_, C>) {
        parents.visit::<dyn PlayerEvent>(self);
    }
}

impl Event for PlayerRespawnEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    fn parents<C: 'static>(&mut self, parents: &mut Parents<'_, C>) {
        parents.visit::<dyn PlayerEvent>(self);
    }
}

macro_rules! plain_event {
    ($($event:ty),*) => {$(
        impl Event for $event {
            fn parents<C: 'static>(&mut self, parents: &mut Parents<'_, C>) {
                parents.visit::<dyn PlayerEvent>(self);
            }
        }
    )*};
}
plain_event!(
    PlayerInputEvent,
    PlayerSneakEvent,
    PlayerSprintEvent,
    PlayerTickEvent,
    ClientSettingsEvent,
    PlayerLoadedEvent
);

impl Event for PlayerMoveEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    fn parents<C: 'static>(&mut self, parents: &mut Parents<'_, C>) {
        parents.visit::<dyn PlayerEvent>(self);
    }
}

impl Event for ArmSwingEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    fn parents<C: 'static>(&mut self, parents: &mut Parents<'_, C>) {
        parents.visit::<dyn PlayerEvent>(self);
    }
}
