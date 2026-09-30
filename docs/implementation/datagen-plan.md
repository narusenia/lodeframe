# datagen 実装計画（M1-05）

> **Status**: 実装済み — 2026-09-30（26.3 で生成。`blocks.rs` は約 276KB、生成物の合計は約 300KB）

要件: REQ-PROTO-002。決定: D4・D5（[decisions.md](../decisions.md)）、Q23〜Q25（D24〜D26）。
複数 crate（xtask・protocol・macros）にまたがるためここに設計を置く。

## 問題

ブロック状態 35,723 個・パケット ID 約 250 個・レジストリ 95 種を手で持つのは非現実的で、
版ごとに変わる。vanilla の data generator が出す JSON から Rust コードを生成する。

## 調べたこと（26.3 の server.jar、2026-09-30）

| 事実 | 影響 |
|---|---|
| `java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --reports --output <dir>` が 3 秒で終わる。Java 25 が要る | xtask が実行する。Java は `mise run datagen` のタスク単位で揃える（D26） |
| `blocks.json`（1,286 ブロック・35,723 状態）、`packets.json`、`registries.json`（95 種）が出る | この 3 つを使う |
| block レジストリの `protocol_id` は 0..N-1 で欠けが無く、各ブロックの先頭状態 ID は protocol 順に単調増加 | `Block` の添字 = レジストリ ID、状態 → ブロックは二分探索で引ける |
| 状態 ID は「ブロックごとに連続、プロパティを**名前のアルファベット順**に並べ、先頭を最上位の桁とする混合基数」。JSON 上のプロパティ順とは **12 ブロック（チェスト・ピストン類）で違う** | 生成時にプロパティを名前順に並べ、全 35,723 状態で検証する（D24） |
| 各ブロックにちょうど 1 つ `default: true` がある | 既定状態を表に持つ |
| `version.json`（jar 直下）に `protocol_version`・`world_version`・`name` がある | `unzip -p` で読む |

## 生成物（`crates/lodeframe-protocol/src/generated/`、commit する）

| ファイル | 中身 |
|---|---|
| `version.rs` | `VERSION_NAME`・`PROTOCOL_VERSION`・`WORLD_VERSION` |
| `packet_ids.rs` | `play::clientbound::ADD_ENTITY: i32 = 1` のような定数（状態・向きごと）。`ids` として公開 |
| `blocks.rs` | プロパティ定義の表（重複は共有）・`BLOCKS`（名前・先頭状態・既定状態・プロパティ）・`STATE_COUNT`・ブロック定数 |
| `entity_types.rs` | 名前の表とエンティティ型の定数（v0.1 に要るのは PLAYER。他は同じ表から出る） |

`generated/mod.rs` だけ手書き。先頭に `@generated` の注記と SPDX を付ける。

## 手書き側

- `block.rs`: `Block`（レジストリ ID）、`BlockState`（状態 ID）。`default_state`・`states`・`from_name`、状態 → ブロック、プロパティの取得・変更（混合基数）、VarInt としての `Encode` / `Decode`（範囲外は拒否）
- `entity_type.rs`: `EntityType`
- derive の `#[packet(id = ..)]` を整数リテラルから**定数式**に広げる（D25）

## xtask

`mise run datagen [<version>]`（= `cargo xtask datagen`。省略時は最新リリース）

1. バージョン一覧 → 版のメタ → `server.jar` を取得し sha1 を検証。`target/xtask/datagen/<version>/` にキャッシュ（curl を呼ぶ。TLS の依存を持たない）
2. data generator を実行
3. JSON を検証しながら Rust を生成し、`cargo fmt` で整形

## 完了条件

- [ ] 1 コマンドで再生成でき、再実行で差分が出ない
- [ ] 生成物が commit され、通常ビルドはネットワーク・Java 不要
- [ ] 全 35,723 状態で「状態 ID ⇔ (ブロック, プロパティ値)」が往復する（テスト）
- [ ] 検証が壊れた入力（桁順の違い）を検出する（テスト）
- [ ] `#[packet(id = ids::…)]` が動く

## 非対象

item などその他のレジストリ（必要になった単位で xtask の対象リストに足す）、Mojang の JSON / jar の commit（D17）。
