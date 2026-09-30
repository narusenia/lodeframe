# lodeframe

English | [日本語](README.ja.md)

A minimal, lightweight Minecraft: Java Edition server **library** written in Rust,
in the spirit of [Minestom](https://github.com/Minestom/Minestom).

> **Status: pre-alpha.** Nothing runs yet. The name is a working title.

lodeframe ships no vanilla gameplay. There is no mob AI, redstone, world
generation or crafting. You build your own server (lobbies, minigames) in Rust
on top of its protocol, world, entity and event APIs.

## Goals

- **Library first**: you depend on the `lodeframe` crate and write the behavior yourself
- **Latest protocol only**: tracks the latest Minecraft release. Put ViaProxy in front for older clients
- **Lock-free game state**: each `Instance` (world) is owned by a single thread and ticked at 20 TPS. Networking runs on tokio
- **Straightforward events**: typed handlers on a hierarchical event tree, e.g. `node.on::<PlayerChat>(|ev, ctx| ..)`
- **Adventure-style text**: rich components, a MiniMessage parser, and audiences for messages, titles, action bars, sounds and boss bars
- **Macros for developer experience**: `derive(Encode, Decode)`, `#[command]`, `derive(Event)`, declarative text/item/GUI macros
- **Measurably lightweight**: memory, startup and bot-load benchmarks compared against Minestom

## Non-goals

Vanilla parity, multi-version support in the core, and saving worlds to Anvil.
AI, pathfinding and combat may come later as separate util crates.

## Roadmap

| Milestone | Scope |
|---|---|
| v0.1 | Login, flat world, movement, player visibility, chat, block place/break |
| v0.2 | Velocity modern forwarding, commands, entities and simple physics, inventories, async tasks, text components / MiniMessage / audiences |
| v0.3 | Online mode, Anvil loading, lighting, multiple instances, scoreboards, item/GUI macros |
| v0.4 | Performance targets met, crates.io release |

Details live in [`docs/`](docs/README.md). Those documents are written in Japanese.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

lodeframe is not affiliated with Mojang Studios or Microsoft.
"Minecraft" is a trademark of Mojang Synergies AB.
