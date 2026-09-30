# チャンクと ChunkLoader 実装計画（M1-10）

> **Status**: 実装済み・実機確認済み（vanilla 26.3 が表示・移動できる）— 2026-09-30

要件: REQ-WORLD-002。決定: D6・D16（[decisions.md](../decisions.md)）。
protocol（チャンクのワイヤ形式）と本体（`ChunkLoader`・送受信管理）にまたがり、利用者が実装する trait を決めるためここに設計を置く。

## 調べたこと（26.3 の server.jar を `javap -c -p` で読んだ、2026-09-30）

| 事実 | 影響 |
|---|---|
| `LevelChunkWithLight` = `x: i32`, `z: i32`, チャンクデータ、ライトデータ | 順序どおりに手書きする |
| チャンクデータ = `heightmaps`（Map: 種別 → `long[]`）、`buffer`（`byte[]`、全セクションを連結）、`blockEntities`（リスト） | v0.1 は block entity 空 |
| 送る高さマップの種別は `sendToClient()` が真のもの。種別は WORLD_SURFACE_WG / WORLD_SURFACE / OCEAN_FLOOR_WG / OCEAN_FLOOR / MOTION_BLOCKING / MOTION_BLOCKING_NO_LEAVES | どれを送るかは**実機で確かめる**（WORLD_SURFACE と MOTION_BLOCKING が要るはず） |
| セクション = `nonEmptyBlockCount: i16`、**`fluidCount: i16`**（26.3 で増えた）、ブロックのパレットコンテナ、バイオームのパレットコンテナ | `fluidCount` を忘れるとズレる |
| パレットコンテナ = `bits: u8`、パレット、生の `long[]`（**長さ接頭辞なし**。サイズは bits から決まる） | 1.21.4 以前の形式と違う |
| ライトデータ = 4 つの BitSet（sky / block / emptySky / emptyBlock）、sky と block の `byte[]` リスト | v0.1 は全 15 で送る（REQ-WORLD-002） |

## 確認したこと（javap で追加）

- パレットは 3 種。単一値は bits=0 + 値の VarInt、間接は bits + 長さ + 値の VarInt 列、直接は bits のみ。いずれも後ろに生の `long[]`（長さ接頭辞なし、1 long に値は跨がない）
- 高さマップの種別は enum の ordinal（WORLD_SURFACE=1、MOTION_BLOCKING=4、MOTION_BLOCKING_NO_LEAVES=5 がクライアント向け）。配列は VarInt の長さ付き
- ライトは BitSet 4 つ + `byte[2048]` のリスト 2 つ。マスクはセクション数 + 2 ビット
- **BitSet は Java の `BitSet.toByteArray()`**（VarInt バイト長 + リトルエンディアン、末尾ゼロ省略）。u64 配列ではない。実機で decode エラーになり、server.jar の codec で直接 decode して特定した

## 実機で確かめた（M1-11、平坦ワールドで表示・移動を確認）

以下は見た目で問題が出ていないだけで、厳密には未検証。壊れて見えたらここから疑う。

- 間接パレットの幅のしきい値（ブロック 4〜8 bits、バイオーム 1〜3 bits）。`Strategy` の定数から読んだが、どの bits でどの palette を使うかの対応表は bytecode から直接は追えていない
- MOTION_BLOCKING を「空気以外の全ブロック」で近似している（草や葉は不正確）
- `fluidCount` は水・溶岩・waterlogged のみ数える

## 分担

| 場所 | 置くもの |
|---|---|
| `lodeframe-protocol` | パレットコンテナ、`ChunkSection`、高さマップ、`LevelChunkWithLight` パケット。ソケットにも Instance にも依存しない |
| `lodeframe` | `Chunk`、`ChunkLoader` trait、平坦生成器、Instance ごとのチャンク管理（view distance に応じた送信・破棄） |

## 要確認

| 論点 | 案 | 影響 |
|---|---|---|
| `ChunkLoader` を同期にするか非同期にするか | **同期 `fn load(&self, pos) -> Option<Chunk>`** を Instance スレッドで呼ぶ。ディスクなど遅い実装は、利用者が自分で先読み・キャッシュする。非同期 API は `ctx.spawn`（D16、v0.2）で後から足す | 同期は単純で、平坦生成器と Anvil（v0.3）の初期実装に足りる。ただし遅い loader は tick を止める |
| 高さ・`min_y` | v0.1 は overworld 固定（`min_y = -64`、`height = 384`、24 セクション）。dimension_type の設定可能化は後 | `Registries` で dimension_type を差し替えた利用者は自分で合わせる必要がある |
| M1-11（スポーンと移動）との境界 | M1-10 はチャンクの**データと管理**まで。プレイヤーへの送信は M1-11 の Login / Respawn パケットと一緒にしか実機で確認できないので、実機確認は M1-11 で行う | M1-10 単体の完了条件はテスト（往復・形式）で満たす |

## 完了条件（案）

- [x] パレットコンテナが 3 種のパレット（単一値・間接・直接）で、期待するバイト列に書き込まれる（テスト。デコーダは作っていないので読み戻しはしない）
- [x] 平坦生成器が出したチャンクが、javap で読んだ形式どおりのパケットになる（高さマップ・ライト・セクション数をテスト。vanilla 実バイトとの照合はしていない）
- [x] view distance に応じて、近づくと送信・離れると破棄の判断が出る（テスト、パケットは記録するだけ）
- [x] 利用者が独自の `ChunkLoader` を差し込める（テスト）
- [ ] 実機での描画は M1-11 で確認する（ここで形式の正しさが初めて確かめられる）

## 非対象

Anvil 読み込み（v0.3）、ライティング計算（v0.3）、block entity、ワールドの保存、非同期 loader。
