# チャット 実装計画（M1-14）

> **Status**: 実装済み・実機確認済み（2 クライアントで発言が表示され、色付きに差し替えた発言も色付きで表示）— 2026-10-01

要件: REQ-API-002（チャット部分）・REQ-TEXT-001（v0.1: 色・装飾付きテキストをチャットに表示）。決定: D9・D16・D21（[decisions.md](../decisions.md)）。
protocol（パケット型）と本体（`World` のイベント配線と配信）にまたがるため、ここに設計を置く。

## 調べたこと

26.3 の server.jar の codec にパケットを実際に encode させて確かめた。Holder の符号化は bytecode で確認した（`chat_type` の registry を組んだ `RegistryAccess` が要り、実 encode は見送り）。

| 事実 | 影響 |
|---|---|
| `Chat`（serverbound 9）= 本文（String）、timestamp（i64 ミリ秒）、salt（i64）、署名（**Optional** = bool + 256 バイト）、offset（VarInt）、acknowledged（**固定 20 bit = 3 バイト**）、checksum（1 バイト）。署名なしで 2 文字の本文は 25 バイト、署名付きは 281 バイト | 本文だけ使い、残りは読み捨てる。署名の有無は問わない |
| `ChatCommand`（serverbound 7）= 本文（String）のみ。`ChatCommandSigned`（8）・`ChatSessionUpdate`（10）・`ChatAck`（6）は本文を持たない | v0.1 はコマンドを扱わない。7・8 は無視する |
| `DisguisedChat`（clientbound 33）= message（Component）、`ChatType.Bound` = chat type（Holder）、name（Component）、target name（Optional Component） | プレイヤー発言の配信に使う。署名を付けられない offline に合う |
| chat type の Holder は `ByteBufCodecs.holder(CHAT_TYPE, ..)`。入室時のディメンション型と同じで、**registry id + 1**（0 はインライン） | `Registries::network_id("minecraft:chat_type", "minecraft:chat")` + 1 |
| `SystemChat`（clientbound 124）= content（Component）、overlay（bool） | サーバーからのメッセージ API に使う |

## 設計

### protocol

`packets::play` に足す。

- `Chat`（serverbound）: 本文だけを取り出す型。残りのバイトは decode で読み捨てる（コマンド・署名の形式を v0.1 で模倣しない）
- `DisguisedChat`・`SystemChat`（clientbound）

Component は `lodeframe-protocol/src/component.rs` の NBT 化をそのまま使う。

### 本体（`World`）

イベントノードを `World` に配線する。M1-15 のブロック操作も同じ仕組みに乗る。

- `World` が `EventNode<Ctx>` を持つ（`events_mut()` で利用者が付ける）。ハンドラは `&mut Ctx` を受ける（REQ-API-001）。木は `Ctx` の外にあるので、**発火中のハンドラは木を変更できない**（型で保証。M2-01 で、取り出して空にする方式から変えた。D31）
- `ChatEvent { player, message, cancelled }`。`message` は `Component`。受信した本文が平文で入り、ハンドラが差し替えれば色・装飾付きで配られる。`cancel()` で配信を止める
- 受信した本文は、空・256 文字超・制御文字を含むものを捨てる（vanilla は切断する。v0.1 は無視してログに残す）
- 既定動作: 同じ World の全員（本人を含む）へ `DisguisedChat`。name は送信者名の Component
- 公開 API: `World::send_message(player, &Component)`（`SystemChat`）と `World::broadcast(&Component)`。M1-17 の lobby が使う
- `Chat` 以外（`ChatCommand` 等）は既存どおり読み捨てる

### v0.1 でやらないこと

- 署名付きチャット（`PlayerChat`）と検証。offline では署名を付けられない
- コマンド（`/` 始まりの入力）。`ChatCommand` は無視する
- 送信者ごとの chat type 切り替え、narration、ミュート・レート制限
- 発火中のイベントノード変更

## テスト

`test-util` のハーネスで書く。注入は既存の `TestEnv::send` に `Chat` を渡す（専用のヘルパーは足さない）。DisguisedChat は Component の decode がないため、`Received::payload()` を足してバイト列で比べる。

- 全員に届く: 送信者を含む全員が `DisguisedChat` を受け、name と本文が合う
- キャンセル: 誰にも届かない
- 改変: 差し替えた Component（色・装飾付き）が届く
- 不正な本文（空・256 文字超・制御文字）は届かず、切断もされない
- `send_message` / `broadcast` が `SystemChat` で届く
- コマンドは配られない

## 実機で確かめる

- 2 クライアントで発言が互いに表示される（`<名前> 本文` の形）
- ハンドラで色付きに差し替えた発言が色付きで表示される
