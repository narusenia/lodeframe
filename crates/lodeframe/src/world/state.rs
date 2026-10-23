// SPDX-License-Identifier: Apache-2.0 OR MIT
//! What a [`Ctx`] knows and changes about a player: the game mode, abilities, permission level,
//! position, teleports and kicks.

use super::{Ctx, Player, PlayerId};
use crate::{
    player::{Abilities, GameMode, GameModeChangedEvent, Relative, Teleport},
    protocol::{
        Packet, Uuid, VarInt, Vec3, packet_body,
        packets::play::{
            ENTITY_EVENT_PERMISSION_LEVEL, EntityEvent, GAME_EVENT_CHANGE_GAME_MODE, GameEvent,
            GameModeEntry, PlayerInfoGameMode, PlayerPosition,
        },
    },
    text::Component,
};

/// The highest permission level, the owner's.
const MAX_PERMISSION_LEVEL: u8 = 4;

impl Ctx {
    fn resolve_mut(&mut self, id: PlayerId) -> Option<&mut Player> {
        self.players
            .get_mut(&id.uuid)
            .filter(|p| p.serial == id.serial)
    }

    /// Sends `packet` to one player, dropping them if they cannot take it.
    fn tell<P: Packet + crate::protocol::Encode>(&mut self, player: Uuid, packet: &P) {
        if self.send(player, packet).is_err() {
            self.leave(player);
        }
    }

    /// The player's game mode, or `None` if they are gone.
    pub fn game_mode(&self, player: PlayerId) -> Option<GameMode> {
        self.resolve(player).map(|p| p.game_mode)
    }

    /// Puts the player in `mode`. The client changes what it lets them do, the tab list shows it
    /// to everyone, and their abilities go back to what the mode has (their speeds stay).
    /// Returns `false`, sending nothing, if they are gone or are in `mode` already.
    ///
    /// A [`GameModeChangedEvent`] follows once the handler that called this is done (the next
    /// tick, if it was not called from a handler).
    pub fn set_game_mode(&mut self, player: PlayerId, mode: GameMode) -> bool {
        let Some(p) = self.resolve_mut(player) else {
            return false;
        };
        if p.game_mode == mode {
            return false;
        }
        let from = p.game_mode;
        p.game_mode = mode;
        let mut abilities = Abilities::for_mode(mode);
        abilities.fly_speed = p.abilities.fly_speed;
        abilities.walk_speed = p.abilities.walk_speed;
        p.abilities = abilities;
        self.tell(
            player.uuid,
            &GameEvent {
                event: GAME_EVENT_CHANGE_GAME_MODE,
                value: f32::from(mode.id()),
            },
        );
        self.tell(player.uuid, &abilities.packet());
        let tab = PlayerInfoGameMode::new(vec![GameModeEntry {
            uuid: player.uuid,
            game_mode: VarInt(i32::from(mode.id())),
        }]);
        if let Ok(body) = packet_body(&tab) {
            self.send_many(None, body);
        }
        self.emit(GameModeChangedEvent {
            player,
            from,
            to: mode,
        });
        true
    }

    /// What the player may do, or `None` if they are gone.
    pub fn abilities(&self, player: PlayerId) -> Option<Abilities> {
        self.resolve(player).map(|p| p.abilities)
    }

    /// Sets what the player may do, and tells their client. Changing their game mode puts the
    /// abilities back to what the mode has.
    pub fn set_abilities(&mut self, player: PlayerId, abilities: Abilities) {
        let Some(p) = self.resolve_mut(player) else {
            return;
        };
        p.abilities = abilities;
        self.tell(player.uuid, &abilities.packet());
    }

    /// The player's permission level (an op level, 0 to 4), or `None` if they are gone. It is
    /// what the client uses to show F3+F4 and the commands; nothing else in lodeframe acts on it.
    pub fn permission_level(&self, player: PlayerId) -> Option<u8> {
        self.resolve(player).map(|p| p.permission_level)
    }

    /// Sets the permission level, 0 to 4 (more is 4), and tells their client.
    pub fn set_permission_level(&mut self, player: PlayerId, level: u8) {
        let level = level.min(MAX_PERMISSION_LEVEL);
        let Some(p) = self.resolve_mut(player) else {
            return;
        };
        if p.permission_level == level {
            return;
        }
        p.permission_level = level;
        let entity_id = p.entity_id;
        self.tell(
            player.uuid,
            &EntityEvent {
                entity_id,
                event: ENTITY_EVENT_PERMISSION_LEVEL + level,
            },
        );
    }

    /// Where the player is, as far as they have said (a position is believed as it comes), or
    /// `None` if they are gone.
    pub fn position(&self, player: PlayerId) -> Option<Vec3> {
        self.resolve(player).map(|p| p.pos)
    }

    /// Which way the player faces, as (yaw, pitch) in degrees, or `None` if they are gone.
    pub fn rotation(&self, player: PlayerId) -> Option<(f32, f32)> {
        self.resolve(player).map(|p| (p.yaw, p.pitch))
    }

    /// Whether the player's client says they stand on something. `false` if they are gone.
    pub fn is_on_ground(&self, player: PlayerId) -> bool {
        self.resolve(player).is_some_and(|p| p.on_ground)
    }

    /// Whether the player is sneaking. `false` if they are gone.
    pub fn is_sneaking(&self, player: PlayerId) -> bool {
        self.resolve(player).is_some_and(|p| p.sneaking)
    }

    /// Moves the player. Their position here is the new one at once; the others see it at the
    /// next tick, and the chunks around the new place are sent.
    ///
    /// Until the player's client confirms the teleport, the positions it sends are from before
    /// it and are dropped. Teleporting again before that makes the client's answer to the last
    /// one the one that counts. Does nothing if they are gone or the position is not a number.
    pub fn teleport(&mut self, player: PlayerId, to: Teleport) {
        let at = to.position;
        if ![at.x, at.y, at.z].iter().all(|v| v.is_finite()) {
            return;
        }
        if let Some((yaw, pitch)) = to.rotation
            && !(yaw.is_finite() && pitch.is_finite())
        {
            return;
        }
        let Some(p) = self.resolve_mut(player) else {
            return;
        };
        let relative = to.relative;
        let axis = |part: Relative, now: f64, value: f64| {
            if relative.contains(part) {
                now + value
            } else {
                value
            }
        };
        p.pos = Vec3::new(
            axis(Relative::X, p.pos.x, at.x),
            axis(Relative::Y, p.pos.y, at.y),
            axis(Relative::Z, p.pos.z, at.z),
        );
        // without a rotation the client keeps the way it faces: relative, by nothing
        let (yaw, pitch, bits) = match to.rotation {
            Some((yaw, pitch)) => {
                if relative.contains(Relative::YAW) {
                    p.yaw += yaw;
                } else {
                    p.yaw = yaw;
                }
                if relative.contains(Relative::PITCH) {
                    p.pitch += pitch;
                } else {
                    p.pitch = pitch;
                }
                (yaw, pitch, relative.bits())
            }
            None => (
                0.0,
                0.0,
                relative.bits() | (Relative::YAW | Relative::PITCH).bits(),
            ),
        };
        let teleport_id = p.next_teleport;
        // ids stay positive and well clear of the join's 1
        p.next_teleport = if teleport_id == i32::MAX {
            2
        } else {
            teleport_id + 1
        };
        p.awaiting_teleport = Some(teleport_id);
        let queued = !std::mem::replace(&mut p.moved, true);
        let packet = PlayerPosition {
            teleport_id: VarInt(teleport_id),
            position: at,
            velocity: Vec3::ZERO,
            yaw,
            pitch,
            relative: bits,
        };
        if queued {
            self.movers.push(player.uuid);
        }
        self.tell(player.uuid, &packet);
        if self.follow_chunks(player.uuid).is_err() {
            self.leave(player.uuid);
        }
    }

    /// Tells the player why they are being disconnected and removes them as if they had left.
    /// Their connection ends once it has written the message. Does nothing if they are gone.
    pub fn kick(&mut self, player: PlayerId, reason: &Component) {
        if self.resolve(player).is_some() {
            self.disconnect_with(player.uuid, reason.clone());
        }
    }
}
