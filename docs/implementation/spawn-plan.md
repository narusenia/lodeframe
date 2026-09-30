# スポーンと移動 実装計画（M1-11）

> **Status**: 実装済み・実機確認済み（vanilla 26.3 で入室と移動）— 2026-09-30。実装は `lodeframe::world::World`、実機確認は `cargo run -p lodeframe --example offline_login`

要件: REQ-WORLD-001・REQ-WORLD-002・REQ-NET-002。決定: D6・D16（[decisions.md](../decisions.md)）。
protocol（パケット型）と本体（スポーン・移動・チャンク追従）にまたがるため、ここに設計を置く。

## 調べたこと（26.3 の server.jar を `javap -c -p` で読んだ、2026-09-30）

| 事実 | 影響 |
|---|---|
| `Login` = `player_id: i32`（固定 4 バイト）、hardcore、ディメンション名の列（VarInt 長 + Identifier）、max_players・view_distance・simulation_distance（VarInt）、reduced_debug、show_death_screen、limited_crafting、SpawnInfo、online_mode、enforces_secure_chat | 順序どおりに手書き。`player_id` だけ VarInt でない |
| `SpawnInfo` = dimension_type（**VarInt。レジストリ内 ID + 1**、0 は直接値）、dimension 名、seed（i64）、game_type（VarInt）、previous_game_type（VarInt、**0 = なし、それ以外 = ID + 1**）、is_debug、is_flat、last_death_location（bool + 名前 + 位置）、portal_cooldown・sea_level（VarInt） | dimension_type の ID は `Registries::network_id` で引く |
| `PlayerPosition` = teleport_id（VarInt）、位置（f64 ×3）、速度（f64 ×3）、yaw・pitch（f32）、relatives（**i32 のビット集合**、bit = `Relative` の ordinal） | 全部絶対なら relatives = 0 |
| `SetChunkCacheCenter` = x, z（VarInt）。`ForgetLevelChunk` = `ChunkPos` を **i64 1 個**（x が下位 32 ビット、z が上位） | |
| `GameEvent` = event（u8）、param（f32）。`LEVEL_CHUNKS_LOAD_START` = 13 | チャンクを待つ画面を閉じるのに要る |
| `ChunkBatchStart`（空）/ `ChunkBatchFinished`（VarInt 個数）/ `ChunkBatchReceived`（f32） | 複数チャンクを送るとき囲む |
| `MovePlayer` は 4 種（Pos 30 / PosRot 31 / Rot 32 / StatusOnly 33）。フィールドは Pos = f64 ×3、Rot = **yaw, pitch**（f32 ×2）、最後に flags（u8: bit0 = on_ground、bit1 = horizontal_collision） | |
| `AcceptTeleportation` = teleport_id（VarInt）、`PlayerLoaded` は空 | |

## 設計

### protocol

`packets::play` に上のパケットを足す。ソケットにも Instance にも依存しない。往復テストを付ける。

### 本体

- `Player`（Instance 側の状態）: 位置・向き・on_ground・`ChunkTracker`・保留中の teleport_id
- 入室（`Message::Join`）: `Login` → `PlayerPosition` → `GameEvent(LEVEL_CHUNKS_LOAD_START)` → `SetChunkCacheCenter` → チャンク群（`ChunkBatchStart` … `ChunkBatchFinished`）
- 移動（`MovePlayer*`）: 位置・向きを更新。チャンク境界を越えたら `ChunkTracker::update` の差分を送る（`SetChunkCacheCenter`・新チャンク・`ForgetLevelChunk`）
- `AcceptTeleportation` / `PlayerLoaded` / `ChunkBatchReceived`: 受けて捨てる（v0.1 は検証しない）

### v0.1 でやらないこと

- `Respawn`（ディメンション移送は v0.3 の転送で必要になる時に足す）
- 移動の検証（速度・衝突）。クライアントの申告をそのまま信じる
- `ChunkBatchReceived` の速度でのチャンク送信の絞り込み

## 実機で確かめた

- チャンクの形式（[chunk-plan.md](chunk-plan.md) の「実機で確かめる」）
- `Login` の並びと `ForgetLevelChunk` の long のパック順
