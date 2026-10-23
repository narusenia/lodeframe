// SPDX-License-Identifier: Apache-2.0 OR MIT
//! What a [`Ctx`] sends to an [`Audience`]: text on the screen, sounds, boss bars and the tab
//! list. Each is encoded once and the same bytes go to everyone.

use std::collections::HashSet;

use super::{Ctx, PlayerId};
use crate::{
    audience::{
        Audience, BossBar, BossBarColor, BossBarFlags, BossBarId, BossBarOverlay, Group, Named,
        Shown, Sound, StopSound, Title, clamp_progress, ticks,
    },
    instance::Packets,
    protocol::{
        Packet, Uuid, Vec3, packet_body,
        packets::play::{
            BossEvent, BossEventOperation, ClearTitles, SetActionBarText, SetSubtitleText,
            SetTitleText, SetTitlesAnimation, SystemChat, TabList,
        },
    },
    text::Component,
};

/// The high half of the UUIDs of boss bars, which the client needs only to tell them apart. It
/// is a version 4 UUID with a variant that the number of the bar is added to.
const BOSS_BAR_UUID: u128 = 0x6c6f_6465_6672_4d65_8000_0000_0000_0000;

fn boss_bar_uuid(id: BossBarId) -> Uuid {
    Uuid(BOSS_BAR_UUID | u128::from(id.0))
}

impl Ctx {
    /// Everyone who is here, in no particular order. This is what [`Everyone`](crate::Everyone)
    /// is, and where an audience of your own starts.
    pub fn players(&self) -> impl Iterator<Item = PlayerId> + '_ {
        self.players.iter().map(|(uuid, player)| PlayerId {
            uuid: *uuid,
            serial: player.serial,
        })
    }

    /// The players of `audience` who are here, each once.
    fn recipients(&self, audience: &(impl Audience + ?Sized)) -> Vec<Uuid> {
        let mut members = Vec::new();
        audience.members(self, &mut members);
        let mut to: Vec<Uuid> = members
            .into_iter()
            .filter(|p| self.resolve(*p).is_some())
            .map(|p| p.uuid)
            .collect();
        to.sort_unstable();
        to.dedup();
        to
    }

    /// Sends `packets` to each of `to` as one message. Those who cannot take it are dropped.
    fn deliver(&mut self, to: &[Uuid], packets: &Packets) {
        for player in to {
            if !self.sessions.send_all(*player, packets.clone()) {
                self.leave(*player);
            }
        }
    }

    /// Encodes `packet` once and sends it to `audience`.
    fn send_packet<P: Packet + crate::protocol::Encode>(
        &mut self,
        audience: &(impl Audience + ?Sized),
        packet: &P,
    ) {
        let to = self.recipients(audience);
        if to.is_empty() {
            return;
        }
        let Ok(body) = packet_body(packet) else {
            return;
        };
        self.deliver(&to, &vec![body]);
    }

    /// Shows `message` in the chat of everyone in `audience`. Those who are gone are skipped.
    pub fn send_message(&mut self, audience: impl Audience, message: &Component) {
        self.send_packet(
            &audience,
            &SystemChat {
                content: message.clone(),
                overlay: false,
            },
        );
    }

    /// Shows `message` to everyone in the chat.
    pub fn broadcast(&mut self, message: &Component) {
        self.send_message(crate::Everyone, message);
    }

    /// Shows `message` above the hotbar of everyone in `audience`.
    pub fn send_action_bar(&mut self, audience: impl Audience, message: &Component) {
        self.send_packet(
            &audience,
            &SetActionBarText {
                text: message.clone(),
            },
        );
    }

    /// Shows `title` to everyone in `audience`.
    pub fn show_title(&mut self, audience: impl Audience, title: &Title) {
        let to = self.recipients(&audience);
        if to.is_empty() {
            return;
        }
        let mut bodies = Vec::new();
        if let Some(times) = title.times
            && let Ok(body) = packet_body(&SetTitlesAnimation {
                fade_in: ticks(times.fade_in),
                stay: ticks(times.stay),
                fade_out: ticks(times.fade_out),
            })
        {
            bodies.push(body);
        }
        // the subtitle always goes, so that the one from before does not show under this title,
        // and the title last, since it is what makes them show
        let subtitle = packet_body(&SetSubtitleText {
            text: title.subtitle.clone(),
        });
        let text = packet_body(&SetTitleText {
            text: title.title.clone(),
        });
        let (Ok(subtitle), Ok(text)) = (subtitle, text) else {
            return;
        };
        bodies.push(subtitle);
        bodies.push(text);
        self.deliver(&to, &bodies);
    }

    /// Takes the title away from everyone in `audience`. The subtitle and the times stay.
    pub fn clear_title(&mut self, audience: impl Audience) {
        self.send_packet(&audience, &ClearTitles { reset: false });
    }

    /// Takes the title away from everyone in `audience`, and forgets the subtitle and the times.
    pub fn reset_title(&mut self, audience: impl Audience) {
        self.send_packet(&audience, &ClearTitles { reset: true });
    }

    /// Sets the text above and below the player list of everyone in `audience`. An empty
    /// component shows nothing there.
    pub fn set_tab_list(
        &mut self,
        audience: impl Audience,
        header: &Component,
        footer: &Component,
    ) {
        self.send_packet(
            &audience,
            &TabList {
                header: header.clone(),
                footer: footer.clone(),
            },
        );
    }

    /// Plays `sound` to everyone in `audience`. One that was given a place with
    /// [`Sound::at`] is encoded once; one that plays where each listener stands is encoded for
    /// each of them.
    pub fn play_sound(&mut self, audience: impl Audience, sound: &Sound) {
        let to = self.recipients(&audience);
        if to.is_empty() {
            return;
        }
        let seed = self.audiences.next_seed();
        if sound.is_placed() {
            if let Ok(body) = packet_body(&sound.packet(Vec3::ZERO, seed)) {
                self.deliver(&to, &vec![body]);
            }
            return;
        }
        for player in to {
            let Some(at) = self.players.get(&player).map(|p| p.pos) else {
                continue;
            };
            let Ok(body) = packet_body(&sound.packet(at, seed)) else {
                continue;
            };
            self.deliver(&[player], &vec![body]);
        }
    }

    /// Stops the sounds that `stop` names for everyone in `audience`.
    pub fn stop_sound(&mut self, audience: impl Audience, stop: &StopSound) {
        self.send_packet(&audience, &stop.packet());
    }

    /// Puts `player` in the group `key`, making the group if there is none. Returns `false`,
    /// doing nothing, if `player` is gone or was in it already.
    ///
    /// A player who leaves is taken out of every group. [`audience`](Self::audience) gives the
    /// group as something to send to.
    pub fn join_group(&mut self, key: &str, player: PlayerId) -> bool {
        if self.resolve(player).is_none() {
            return false;
        }
        self.audiences
            .groups
            .entry(key.to_owned())
            .or_default()
            .add(player)
    }

    /// Takes `player` out of the group `key`. Returns `false` if they were not in it.
    pub fn leave_group(&mut self, key: &str, player: PlayerId) -> bool {
        self.audiences
            .groups
            .get_mut(key)
            .is_some_and(|group| group.remove(player))
    }

    /// The group `key`, if it was made.
    pub fn group(&self, key: &str) -> Option<&Group> {
        self.audiences.groups.get(key)
    }

    /// Drops the group `key`. Returns `false` if there was none.
    pub fn remove_group(&mut self, key: &str) -> bool {
        self.audiences.groups.remove(key).is_some()
    }

    /// The audience registered under `key`.
    pub(crate) fn registered_audience(&self, key: &str) -> Option<&dyn Audience> {
        self.audiences.registered.get(key).map(|a| &**a)
    }

    /// Keeps `audience` under `key`, for [`audience`](Self::audience) to find: a team, the
    /// players who are alive, anything that is worked out when it is sent to. Replaces what
    /// was under the key. A group of the same key is found before it.
    pub fn register_audience(&mut self, key: &str, audience: impl Audience + 'static) {
        self.audiences
            .registered
            .insert(key.to_owned(), Box::new(audience));
    }

    /// Forgets the audience under `key`. Returns `false` if there was none.
    pub fn unregister_audience(&mut self, key: &str) -> bool {
        self.audiences.registered.remove(key).is_some()
    }

    /// What is under `key`: the group of that name, or else the audience registered under it.
    /// It is looked up when something is sent, so this works before either exists, and sends to
    /// nobody for as long as neither does.
    pub fn audience(&self, key: &str) -> Named {
        Named(key.to_owned())
    }

    /// Adds `bar` and returns what names it. Nobody sees it until
    /// [`show_bossbar`](Self::show_bossbar). Remove it with
    /// [`remove_bossbar`](Self::remove_bossbar) when it is not needed any more.
    pub fn create_bossbar(&mut self, bar: BossBar) -> BossBarId {
        self.audiences.next_bossbar += 1;
        let id = BossBarId(self.audiences.next_bossbar);
        self.audiences.bossbars.insert(
            id,
            Shown {
                bar,
                viewers: HashSet::new(),
            },
        );
        id
    }

    /// The boss bar, as it is now.
    pub fn bossbar(&self, id: BossBarId) -> Option<&BossBar> {
        self.audiences.bossbars.get(&id).map(|shown| &shown.bar)
    }

    /// Who sees the boss bar.
    pub fn bossbar_viewers(&self, id: BossBarId) -> Vec<PlayerId> {
        let Some(shown) = self.audiences.bossbars.get(&id) else {
            return Vec::new();
        };
        self.players()
            .filter(|p| shown.viewers.contains(&p.uuid))
            .collect()
    }

    /// Shows the boss bar to everyone in `audience`. Those who see it already are not sent it
    /// again.
    pub fn show_bossbar(&mut self, audience: impl Audience, id: BossBarId) {
        let Some(shown) = self.audiences.bossbars.get(&id) else {
            return;
        };
        let to: Vec<Uuid> = self
            .recipients(&audience)
            .into_iter()
            .filter(|p| !shown.viewers.contains(p))
            .collect();
        let bar = &shown.bar;
        let operation = BossEventOperation::Add {
            title: bar.title.clone(),
            progress: bar.progress,
            color: bar.color,
            overlay: bar.overlay,
            flags: bar.flags,
        };
        self.send_to_viewers(id, &to, operation);
        if let Some(shown) = self.audiences.bossbars.get_mut(&id) {
            shown.viewers.extend(to);
        }
    }

    /// Takes the boss bar away from everyone in `audience` who sees it.
    pub fn hide_bossbar(&mut self, audience: impl Audience, id: BossBarId) {
        let Some(shown) = self.audiences.bossbars.get(&id) else {
            return;
        };
        let to: Vec<Uuid> = self
            .recipients(&audience)
            .into_iter()
            .filter(|p| shown.viewers.contains(p))
            .collect();
        self.send_to_viewers(id, &to, BossEventOperation::Remove);
        if let Some(shown) = self.audiences.bossbars.get_mut(&id) {
            for player in &to {
                shown.viewers.remove(player);
            }
        }
    }

    /// Takes the boss bar away from everyone who sees it and forgets it.
    pub fn remove_bossbar(&mut self, id: BossBarId) {
        let Some(shown) = self.audiences.bossbars.remove(&id) else {
            return;
        };
        let to: Vec<Uuid> = shown
            .viewers
            .into_iter()
            .filter(|p| self.players.contains_key(p))
            .collect();
        self.send_to_viewers(id, &to, BossEventOperation::Remove);
    }

    /// Changes the title of the boss bar, for those who see it too.
    pub fn set_bossbar_title(&mut self, id: BossBarId, title: &Component) {
        self.update_bossbar(id, |bar| {
            (bar.title != *title).then(|| {
                bar.title = title.clone();
                BossEventOperation::UpdateTitle(title.clone())
            })
        });
    }

    /// Changes how full the boss bar is, 0.0 to 1.0 (anything else is brought into that), for
    /// those who see it too. Nothing is sent if it is the same.
    pub fn set_bossbar_progress(&mut self, id: BossBarId, progress: f32) {
        let progress = clamp_progress(progress);
        self.update_bossbar(id, |bar| {
            (bar.progress != progress).then(|| {
                bar.progress = progress;
                BossEventOperation::UpdateProgress(progress)
            })
        });
    }

    /// Changes the colour of the boss bar, for those who see it too.
    pub fn set_bossbar_color(&mut self, id: BossBarId, color: BossBarColor) {
        self.update_bossbar(id, |bar| {
            (bar.color != color).then(|| {
                bar.color = color;
                BossEventOperation::UpdateStyle {
                    color,
                    overlay: bar.overlay,
                }
            })
        });
    }

    /// Changes how the boss bar is cut into notches, for those who see it too.
    pub fn set_bossbar_overlay(&mut self, id: BossBarId, overlay: BossBarOverlay) {
        self.update_bossbar(id, |bar| {
            (bar.overlay != overlay).then(|| {
                bar.overlay = overlay;
                BossEventOperation::UpdateStyle {
                    color: bar.color,
                    overlay,
                }
            })
        });
    }

    /// Changes what the boss bar does to the world, for those who see it too.
    pub fn set_bossbar_flags(&mut self, id: BossBarId, flags: BossBarFlags) {
        self.update_bossbar(id, |bar| {
            (bar.flags != flags).then(|| {
                bar.flags = flags;
                BossEventOperation::UpdateFlags(flags)
            })
        });
    }

    /// Applies `change` to the boss bar and sends what it returns to those who see it.
    fn update_bossbar(
        &mut self,
        id: BossBarId,
        change: impl FnOnce(&mut BossBar) -> Option<BossEventOperation>,
    ) {
        let Some(shown) = self.audiences.bossbars.get_mut(&id) else {
            return;
        };
        let Some(operation) = change(&mut shown.bar) else {
            return;
        };
        let to: Vec<Uuid> = shown
            .viewers
            .iter()
            .copied()
            .filter(|p| self.players.contains_key(p))
            .collect();
        self.send_to_viewers(id, &to, operation);
    }

    /// Sends one boss bar operation, encoded once, to `to`.
    fn send_to_viewers(&mut self, id: BossBarId, to: &[Uuid], operation: BossEventOperation) {
        if to.is_empty() {
            return;
        }
        let Ok(body) = packet_body(&BossEvent {
            id: boss_bar_uuid(id),
            operation,
        }) else {
            return;
        };
        self.deliver(to, &vec![body]);
    }
}
