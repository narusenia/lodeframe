// SPDX-License-Identifier: Apache-2.0 OR MIT
//! The registries sent to a client during configuration.

use std::collections::HashMap;

use crate::protocol::{
    DATAPACK_REGISTRIES, DYNAMIC_TAGS, Error, Identifier, Nbt, Result, STATIC_TAGS, VarInt,
    packets::configuration::{RegistryData, RegistryEntry, RegistryTags, Tag, UpdateTags},
};

/// The data-driven registries (dimension types, biomes, ...) and their entries.
///
/// Starts with every vanilla entry by name only: a client that has the vanilla data pack
/// reads the contents itself. [`set`](Self::set) adds an entry or replaces a vanilla one,
/// and only those entries are sent with data.
///
/// An entry's position is its network id, so entries keep the order they were added in.
#[derive(Debug, Clone)]
pub struct Registries {
    registries: Vec<(Identifier, Vec<RegistryEntry>)>,
}

impl Registries {
    /// Every vanilla entry, without data.
    pub fn vanilla() -> Self {
        let registries = DATAPACK_REGISTRIES
            .iter()
            .map(|(registry, names)| {
                let entries = names
                    .iter()
                    .map(|n| RegistryEntry {
                        id: Identifier::new(*n).expect("generated names are valid"),
                        data: None,
                    })
                    .collect();
                (
                    Identifier::new(*registry).expect("generated names are valid"),
                    entries,
                )
            })
            .collect();
        Self { registries }
    }

    /// Adds `name` to `registry` with `data`, or replaces the data of an existing entry.
    ///
    /// Fails if the registry is not one the client syncs.
    pub fn set(&mut self, registry: &str, name: &str, data: Nbt) -> Result<()> {
        let registry = Identifier::new(registry)?;
        let id = Identifier::new(name)?;
        let (_, entries) = self
            .registries
            .iter_mut()
            .find(|(r, _)| *r == registry)
            .ok_or(Error::InvalidValue("unknown registry"))?;
        match entries.iter_mut().find(|e| e.id == id) {
            Some(entry) => entry.data = Some(data),
            None => entries.push(RegistryEntry {
                id,
                data: Some(data),
            }),
        }
        Ok(())
    }

    /// The position of `name` in `registry`: the id the client uses for it.
    pub fn network_id(&self, registry: &str, name: &str) -> Option<usize> {
        let registry = Identifier::new(registry).ok()?;
        let id = Identifier::new(name).ok()?;
        let (_, entries) = self.registries.iter().find(|(r, _)| *r == registry)?;
        entries.iter().position(|e| e.id == id)
    }

    /// Every vanilla tag, with entry ids as this `Registries` numbers them.
    ///
    /// Tags of registries with fixed ids (blocks, items, ...) come straight from the
    /// generated table; tags of the registries in here are resolved by entry name, so entries
    /// added with [`set`](Self::set) get ids but are in no tag.
    // ponytail: rebuilt per connection; cache in Registries if connection rate ever matters
    pub fn tags_packet(&self) -> UpdateTags {
        let id = |s: &str| Identifier::new(s).expect("generated names are valid");
        let mut registries: Vec<RegistryTags> = STATIC_TAGS
            .iter()
            .map(|(registry, tags)| RegistryTags {
                registry: id(registry),
                tags: tags
                    .iter()
                    .map(|(name, ids)| Tag {
                        name: id(name),
                        // generated ids are below the registry size, far under i32::MAX
                        entries: ids.iter().map(|&i| VarInt(i as i32)).collect(),
                    })
                    .collect(),
            })
            .collect();
        for (registry, tags) in DYNAMIC_TAGS.iter() {
            let registry = id(registry);
            let Some((_, entries)) = self.registries.iter().find(|(r, _)| *r == registry) else {
                continue;
            };
            let ids: HashMap<&Identifier, usize> = entries
                .iter()
                .enumerate()
                .map(|(i, e)| (&e.id, i))
                .collect();
            let tags = tags
                .iter()
                .map(|(name, members)| Tag {
                    name: id(name),
                    entries: members
                        .iter()
                        .filter_map(|m| ids.get(&id(m)).map(|&i| VarInt(i as i32)))
                        .collect(),
                })
                .collect();
            registries.push(RegistryTags { registry, tags });
        }
        UpdateTags { registries }
    }

    /// The `RegistryData` packets to send, one per registry.
    pub fn packets(&self) -> impl Iterator<Item = RegistryData> + '_ {
        self.registries
            .iter()
            .map(|(registry, entries)| RegistryData {
                registry: registry.clone(),
                entries: entries.clone(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_entries_set_by_the_user_carry_data() {
        let mut r = Registries::vanilla();
        let n = r
            .network_id("minecraft:dimension_type", "minecraft:overworld")
            .unwrap();
        r.set("minecraft:dimension_type", "demo:sky", Nbt::from("x"))
            .unwrap();
        r.set(
            "minecraft:dimension_type",
            "minecraft:overworld",
            Nbt::from("y"),
        )
        .unwrap();
        // replacing keeps the id; adding appends
        assert_eq!(
            r.network_id("minecraft:dimension_type", "minecraft:overworld"),
            Some(n)
        );
        let dim = r
            .packets()
            .find(|p| p.registry == Identifier::new("minecraft:dimension_type").unwrap())
            .unwrap();
        let with_data: Vec<_> = dim.entries.iter().filter(|e| e.data.is_some()).collect();
        assert_eq!(with_data.len(), 2);
        assert!(dim.entries.last().unwrap().data.is_some());
        assert!(r.set("minecraft:nope", "a:b", Nbt::from("x")).is_err());
    }

    #[test]
    fn dynamic_tags_use_this_registrys_ids() {
        let r = Registries::vanilla();
        let tags = r.tags_packet();
        let damage = tags
            .registries
            .iter()
            .find(|t| t.registry == Identifier::new("minecraft:damage_type").unwrap())
            .unwrap();
        let fire = damage
            .tags
            .iter()
            .find(|t| t.name == Identifier::new("minecraft:is_fire").unwrap())
            .unwrap();
        let on_fire = r
            .network_id("minecraft:damage_type", "minecraft:on_fire")
            .unwrap();
        assert!(fire.entries.contains(&VarInt(on_fire as i32)));
        // the client needs block tags too, sent by fixed id
        assert!(
            tags.registries
                .iter()
                .any(|t| t.registry == Identifier::new("minecraft:block").unwrap())
        );
    }
}
