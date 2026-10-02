# plugin message 実装計画（M2-07）

> **Status**: 計画 — 2026-10-03

要件: REQ-NET-005。決定: D6・D16・D21・D37・D38（[decisions.md](../decisions.md)）。
`lodeframe-protocol`・`lodeframe`（本体）・`lodeframe-bot` にまたがるので、コードの前にここで形を決める。M2-08（Velocity）が login の要求を使う。

## 分け方

plugin message は 3 つの段階で形が違う。

| 段階 | 向き | 扱い |
|---|---|---|
| login | サーバー → クライアントの要求（`CustomQuery`）と、クライアントの応答（`CustomQueryAnswer`） | **関数 `login::Queries::ask`**。利用者向けのフックは作らない（M2-04 が決める） |
| configuration | クライアント → サーバー（brand など） | いま捨てている分を集めて Join に載せ、入室後にイベントにする |
| play | 両方向の任意チャンネル | 受信は `PluginMessageEvent`、送信は `ctx.send_plugin_message` |

## 1. パケット（`lodeframe-protocol`）

- `login::CustomQuery { transaction_id: VarInt, channel: Identifier, data: Vec<u8> }`（clientbound）。`data` はパケットの残り全部
- `login::CustomQueryAnswer { transaction_id: VarInt, data: Option<Vec<u8>> }`（serverbound）。`data` は bool の有無 + 残り全部。`None` は「クライアントがそのチャンネルを知らない」
- `configuration::ServerboundCustomPayload`、`play::{ClientboundCustomPayload, ServerboundCustomPayload}`。`configuration::ClientboundCustomPayload`（既存）と同じ形で、`channel` + 残り全部
- 長さの上限: serverbound の `data` は 32767 バイト、clientbound は 1048576 バイト（vanilla と同じ）。超えたデコードは `Error` で、接続が切れる。`Vec` を確保する前に残りバイト数と照らす（rust.md の `check_remaining`）
- 同じ形の構造体が 4 つになるので、Encode/Decode の実装は小さなマクロか共通の関数にまとめる

## 2. login（`login.rs`）

```rust
pub struct Queries { next: i32 }
impl Queries {
    /// 要求を送り、応答を待つ。`Ok(None)` はクライアントがチャンネルを知らない。
    pub async fn ask<S>(&mut self, conn: &mut Connection<S>, channel: Identifier, data: &[u8], timeout: Duration) -> Result<Option<Vec<u8>>>;
}
```

- `transaction_id` は `Queries` が接続ごとに数える。**応答の id が違えば `Error`**（取り違えた応答を黙って受けない）
- **タイムアウト**は `io::ErrorKind::TimedOut` の `Error`。呼ぶ側（Velocity など）が理由つきの Disconnect で切る
- login 中に他のパケットが来たら `Error`（Hello の後に許されるのは応答だけ）
- `login::offline` と `Server` には**つなげない**。つなぐのは M2-08（内部）と M2-04（利用者向けのフック）

## 3. configuration（`configuration.rs`）

- `read_until` が捨てている `CUSTOM_PAYLOAD` を `Vec<PluginMessage>` に集める。**件数は 64、合計 64KB まで**。超えたら `Error`（configuration は認証の後だが、制限の本命は REQ-NET-007。ここは無限に溜めないための歯止めだけ）
- `configuration::run_with` の戻りが `Result<Vec<PluginMessage>>` になる（`run` も）。v0.x の変更として docs に書く
- `PluginMessage { channel: Identifier, data: Vec<u8> }` を `instance.rs` に置く（`Message::Join` が持つため）

## 4. play（`play.rs`・`world.rs`・`instance.rs`）

- `Message::Join` に `plugin_messages: Vec<PluginMessage>` を足す。`play::run_with` は configuration の戻りを受けて載せる。**`Message` は列挙型なので、自作 Instance は Join の新しい欄を受ける**（v0.x の変更として docs に書く）
- 受信: `World::on_packet` が play の `CUSTOM_PAYLOAD` を `ServerboundCustomPayload` にデコードし、`PluginMessageEvent { player: PlayerId, channel: Identifier, data: Vec<u8> }` を発火する。`PlayerEvent` を親に持つ。チャンネルでの絞り込みは既存の `only_if` で足りるので、チャンネルの登録表は作らない。取り消しはできない（副作用の無い通知）
- 入室時の configuration の分: `Ctx::join` が先に **`client_brand`** を取り出して `Player` に持たせ（`minecraft:brand` の文字列。読めなければ `None`）、`PlayerJoinEvent` の**後**に残りを含む全部を `PluginMessageEvent` として発火する（brand も含める。ハンドラは join の時点で `ctx.client_brand(p)` を読める）
- 送信: `ctx.send_plugin_message(player: PlayerId, channel: &Identifier, data: &[u8])`。退出済みの `PlayerId` には何もしない（D31）。上限を超える `data` は送らずに `warn` を出す
- `ctx.client_brand(player) -> Option<&str>`

## 5. ハーネスとボット（D21）

- `TestEnv::connect_with(name, Vec<PluginMessage>)`（`connect` はこれの空版）。`TestEnv::plugin_message(&player, channel, data)` で serverbound を送れる。受信側は `FakePlayer::drain_as::<play::ClientboundCustomPayload>()`
- `Bot` は configuration で `minecraft:brand`（`lodeframe-bot`）を送るようになり、`Bot::plugin_message(channel, data)` で play の任意チャンネルを送れる。受け取りは `Frame::decode::<play::ClientboundCustomPayload>()`（既存の `recv_until`）

## 確認して決めたこと（2026-10-03）

| 判断 | 選んだ案 | 選ばなかった案 |
|---|---|---|
| login の範囲 | 関数 `login::Queries::ask` だけ。利用者向けのフックは M2-04 | 簡易フックも足す（M2-04 でイベントとして作り直す公算が高く、API が二度変わる）／M2-04 と一緒にやる（単位が大きく、M2-04 が M2-06 待ちの依存順を組み替える） |
| configuration の分 | 全部イベント化。brand は `ctx.client_brand` でも引ける | brand だけ保持（mod 系のチャンネルが見えず、要件の半分）／捨てたまま（要件と食い違う） |
| play の受信 | 1 種類の `PluginMessageEvent` + 既存の `only_if` | チャンネルごとの登録表（ノードの仕組みと二重になる） |

## やらないこと

| 項目 | なぜ外したか | 受け皿 |
|---|---|---|
| login の利用者向けフック | 上の判断。M2-04 で async イベントとして決める | M2-04 |
| `minecraft:register` / `unregister` の管理 | チャンネルを知らせ合う仕組みで、mod 連携が出てから | — |
| 全員への plugin message の送信（broadcast） | 要望が出てから。`send_plugin_message` を回せば足りる | — |
| configuration 段階でサーバーから任意の payload を送る | 利用者が configuration に触る入り口がまだ無い | — |
| パケットの数・大きさの制限（認証前を含む） | REQ-NET-007 の対象 | v0.3 |

## テスト

- 単体（protocol）: 4 種類のパケットの往復、上限を超える `data` の拒否、`CustomQueryAnswer` の有無
- 単体（login）: 応答を受ける、`None` を受ける、id の違う応答は `Error`、応答が無ければ `TimedOut`、Hello の後に他のパケットが来たら `Error`
- ハーネス（`tests/world.rs`）: serverbound が `PluginMessageEvent` になる／`only_if` でチャンネルを絞れる／`send_plugin_message` が届く／退出済みへは何も送らない／`connect_with` の分が Join の後に発火し、join のハンドラが `client_brand` を読める
- 統合（`tests/server.rs`）: ボットが送った brand が `client_brand` になり、ボットの送ったメッセージをハンドラが同じチャンネルで返し、ボットが受け取る
- `mise run check`・`mise run msrv`

## 実装の順

1. パケットと往復テスト（protocol）
2. `login::Queries` とテスト
3. configuration の収集、`Message::Join`・`PluginMessage`、`World` のイベントと送信、ハーネス
4. ボット、統合テスト
5. 文書（requirements の受入条件、minestom-parity 1.9・1.16、architecture、backlog、v0.2-plan）。PR
