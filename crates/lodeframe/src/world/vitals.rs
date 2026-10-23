// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A player's health, food and experience, and dying and coming back.

use super::{Ctx, PlayerId, chunk_of};
use crate::{
    chunk::ChunkTracker,
    player::{
        Experience, MAX_FOOD, MAX_HEALTH, PlayerDeathEvent, PlayerRespawnEvent, RESPAWN_SATURATION,
        RespawnPoint,
    },
    protocol::{
        VarInt, Vec3,
        packets::play::{
            ENTITY_EVENT_PERMISSION_LEVEL, EntityEvent, GAME_EVENT_IMMEDIATE_RESPAWN, GameEvent,
            LEVEL_CHUNKS_LOAD_START, PlayerCombatKill, PlayerPosition, Respawn,
            SetChunkCacheCenter, angle,
        },
    },
    text::Component,
};

/// The message on the death screen of a player who died without a reason being given.
fn death_message(name: &str) -> Component {
    Component::translatable("death.attack.generic").arg(Component::text(name))
}

/// A food level the client takes.
fn clamp_food(food: i32) -> i32 {
    food.clamp(0, MAX_FOOD)
}

impl Ctx {
    /// The player's health, 0.0 to 20.0, or `None` if they are gone. A player who is dead has 0.
    pub fn health(&self, player: PlayerId) -> Option<f32> {
        self.resolve(player).map(|p| p.health)
    }

    /// Sets the health, which is shown as hearts: 20.0 is ten full ones. It is brought into 0.0
    /// to 20.0, and not a number is ignored. A health of 0 or less kills the player, with a
    /// death message of the game's own ([`kill`](Self::kill) takes yours). Does nothing to a
    /// player who is dead or gone.
    pub fn set_health(&mut self, player: PlayerId, health: f32) {
        if health.is_nan() {
            return;
        }
        let health = health.min(MAX_HEALTH);
        let Some(p) = self.resolve_mut(player) else {
            return;
        };
        if p.dead {
            return;
        }
        if health <= 0.0 {
            let message = death_message(&p.name);
            self.kill(player, &message);
            return;
        }
        if p.health == health {
            return;
        }
        p.health = health;
        let packet = p.health_packet();
        self.tell(player.uuid, &packet);
    }

    /// The player's food level, 0 to 20, or `None` if they are gone.
    pub fn food(&self, player: PlayerId) -> Option<i32> {
        self.resolve(player).map(|p| p.food)
    }

    /// Sets the food level, 0 to 20 (anything else is brought into that). The client does not
    /// let a player sprint at 6 or less. A saturation above the new level comes down to it.
    /// Nothing eats or starves by itself. Does nothing to a player who is dead or gone.
    pub fn set_food(&mut self, player: PlayerId, food: i32) {
        let food = clamp_food(food);
        let Some(p) = self.resolve_mut(player) else {
            return;
        };
        if p.dead {
            return;
        }
        let saturation = p.saturation.min(food as f32);
        if p.food == food && p.saturation == saturation {
            return;
        }
        p.food = food;
        p.saturation = saturation;
        let packet = p.health_packet();
        self.tell(player.uuid, &packet);
    }

    /// The player's saturation, which food spends before the level goes down, or `None` if they
    /// are gone.
    pub fn saturation(&self, player: PlayerId) -> Option<f32> {
        self.resolve(player).map(|p| p.saturation)
    }

    /// Sets the saturation, from 0.0 to the food level (anything else is brought into that, and
    /// not a number is ignored). Does nothing to a player who is dead or gone.
    pub fn set_saturation(&mut self, player: PlayerId, saturation: f32) {
        if saturation.is_nan() {
            return;
        }
        let Some(p) = self.resolve_mut(player) else {
            return;
        };
        if p.dead {
            return;
        }
        let saturation = saturation.clamp(0.0, p.food as f32);
        if p.saturation == saturation {
            return;
        }
        p.saturation = saturation;
        let packet = p.health_packet();
        self.tell(player.uuid, &packet);
    }

    /// The player's experience, or `None` if they are gone.
    pub fn experience(&self, player: PlayerId) -> Option<Experience> {
        self.resolve(player).map(|p| p.experience)
    }

    /// Sets the experience bar, level and points. A level or points below 0 are 0, and a
    /// progress is brought into 0.0 to 1.0 (not a number is 0.0). Does nothing to a player who is
    /// dead or gone.
    pub fn set_experience(&mut self, player: PlayerId, experience: Experience) {
        let experience = experience.clamped();
        let Some(p) = self.resolve_mut(player) else {
            return;
        };
        if p.dead || p.experience == experience {
            return;
        }
        p.experience = experience;
        let packet = p.experience_packet();
        self.tell(player.uuid, &packet);
    }

    /// Whether the player is dead and on the death screen. `false` if they are gone.
    pub fn is_dead(&self, player: PlayerId) -> bool {
        self.resolve(player).is_some_and(|p| p.dead)
    }

    /// Where the player last died, or `None` if they have not or are gone. The client points a
    /// recovery compass at it.
    pub fn last_death(&self, player: PlayerId) -> Option<Vec3> {
        self.resolve(player).and_then(|p| p.last_death)
    }

    /// Where the player comes back after dying: the one set with
    /// [`set_respawn_point`](Self::set_respawn_point), or else [`spawn`](Self::spawn). `None` if
    /// they are gone.
    pub fn respawn_point(&self, player: PlayerId) -> Option<RespawnPoint> {
        self.resolve(player)
            .map(|p| p.respawn_point.unwrap_or(RespawnPoint::new(self.spawn)))
    }

    /// Sets where the player comes back after dying; `None` goes back to the spawn of the world.
    pub fn set_respawn_point(&mut self, player: PlayerId, point: Option<RespawnPoint>) {
        if let Some(p) = self.resolve_mut(player) {
            p.respawn_point = point;
        }
    }

    /// Kills the player: their health is 0, their client opens the death screen with `message`
    /// on it, and nobody else sees them. Returns `false`, doing nothing, if they are gone or are
    /// dead already.
    ///
    /// A dead player cannot move, edit blocks or have their health changed. They come back when
    /// they press respawn (a [`PlayerRespawnEvent`]) or with [`respawn`](Self::respawn). A
    /// [`PlayerDeathEvent`] follows once the handler that called this is done.
    pub fn kill(&mut self, player: PlayerId, message: &Component) -> bool {
        let Some(p) = self.resolve_mut(player) else {
            return false;
        };
        if p.dead {
            return false;
        }
        p.dead = true;
        p.health = 0.0;
        p.last_death = Some(p.pos);
        let (entity_id, vitals) = (p.entity_id, p.health_packet());
        self.tell(player.uuid, &vitals);
        self.tell(
            player.uuid,
            &PlayerCombatKill {
                player_id: VarInt(entity_id),
                message: message.clone(),
            },
        );
        // nobody sees them any more, and they see nobody
        self.update_visibility(player.uuid);
        self.emit(PlayerDeathEvent {
            player,
            message: message.clone(),
        });
        true
    }

    /// Brings a dead player back at their [respawn point](Self::respawn_point), with full health
    /// and food and the experience they had. Returns `false`, doing nothing, if they are gone or
    /// not dead. No [`PlayerRespawnEvent`] is emitted: that is for the player's own request.
    pub fn respawn(&mut self, player: PlayerId) -> bool {
        let Some(point) = self.respawn_point(player) else {
            return false;
        };
        self.respawn_to(
            player,
            &PlayerRespawnEvent {
                player,
                position: point.position,
                yaw: point.yaw,
                pitch: point.pitch,
                health: MAX_HEALTH,
                food: MAX_FOOD,
                saturation: RESPAWN_SATURATION,
                cancelled: false,
            },
        )
    }

    /// Brings a dead player back as `event` says. The client builds its world again, so it is sent
    /// the chunks from the start, and the players around come into view.
    pub(crate) fn respawn_to(&mut self, player: PlayerId, event: &PlayerRespawnEvent) -> bool {
        let at = event.position;
        let position = if [at.x, at.y, at.z].iter().all(|v| v.is_finite()) {
            at
        } else {
            self.spawn
        };
        let (yaw, pitch) = if event.yaw.is_finite() && event.pitch.is_finite() {
            (event.yaw, event.pitch)
        } else {
            (0.0, 0.0)
        };
        let health = if event.health.is_nan() || event.health <= 0.0 {
            MAX_HEALTH
        } else {
            event.health.min(MAX_HEALTH)
        };
        let food = clamp_food(event.food);
        let saturation = if event.saturation.is_nan() {
            RESPAWN_SATURATION
        } else {
            event.saturation.clamp(0.0, food as f32)
        };
        let view_distance = self.view_distance;
        let Some(p) = self.resolve_mut(player) else {
            return false;
        };
        if !p.dead {
            return false;
        }
        p.dead = false;
        p.health = health;
        p.food = food;
        p.saturation = saturation;
        p.pos = position;
        p.yaw = yaw;
        p.pitch = pitch;
        p.on_ground = false;
        p.sneaking = false;
        p.known_pos = position;
        p.known_yaw = angle(yaw);
        p.known_pitch = angle(pitch);
        // the client forgets its chunks with its world, so it gets them again from the start
        p.chunk = chunk_of(position);
        p.chunks = ChunkTracker::new();
        p.pending = p.chunks.update(p.chunk, view_distance).send.into();
        let teleport_id = p.next_teleport;
        p.next_teleport = if teleport_id == i32::MAX {
            2
        } else {
            teleport_id + 1
        };
        p.awaiting_teleport = Some(teleport_id);
        let uuid = player.uuid;
        let Some(p) = self.players.get(&uuid) else {
            return false;
        };
        let (entity_id, chunk, level) = (p.entity_id, p.chunk, p.permission_level);
        let (vitals, experience, abilities) = (
            p.health_packet(),
            p.experience_packet(),
            p.abilities.packet(),
        );
        let Ok(spawn) = self.spawn_info(p) else {
            return false;
        };
        self.tell(
            uuid,
            &Respawn {
                spawn,
                data_kept: 0,
            },
        );
        self.tell(
            uuid,
            &PlayerPosition {
                teleport_id: VarInt(teleport_id),
                position,
                velocity: Vec3::ZERO,
                yaw,
                pitch,
                relative: 0,
            },
        );
        self.tell(
            uuid,
            &GameEvent {
                event: LEVEL_CHUNKS_LOAD_START,
                value: 0.0,
            },
        );
        if !self.show_death_screen {
            self.tell(
                uuid,
                &GameEvent {
                    event: GAME_EVENT_IMMEDIATE_RESPAWN,
                    value: 1.0,
                },
            );
        }
        self.tell(
            uuid,
            &SetChunkCacheCenter {
                x: VarInt(chunk.x),
                z: VarInt(chunk.z),
            },
        );
        self.tell(uuid, &vitals);
        self.tell(uuid, &experience);
        self.tell(uuid, &abilities);
        self.tell(
            uuid,
            &EntityEvent {
                entity_id,
                event: ENTITY_EVENT_PERMISSION_LEVEL + level,
            },
        );
        self.update_visibility(uuid);
        if self.flush_chunks(uuid).is_err() {
            self.leave(uuid);
        }
        true
    }
}
