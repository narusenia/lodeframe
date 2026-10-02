# ハンドラの文脈とプレイヤーのハンドル 実装計画（M2-01）

> **Status**: 実装済み — 2026-10-02

要件: REQ-API-003 の前提（`ctx.spawn`・スケジューラ・Audience が受ける型）。決定: D6・D16・D29 の (1)(2)(5)（[decisions.md](../decisions.md)）。
本体（`world.rs`・`event.rs`・`test_util.rs`）と examples/lobby にまたがる。v0.2 の他の単位の形を決めるので、最初にやる。

## 困っていたこと

v0.1 の lobby を書いて分かったこと（D29）:

1. ハンドラの文脈が `&mut World<L>` で、**ローダーの型が利用者のコードに漏れる**。lobby は `type Lobby = World<FlatGenerator>` を書いて回避していた
2. 発火中は `World::events` が `mem::take` で空になる。ハンドラの中で起きたイベント（送信に失敗した人の退出など）は **届かずに捨てられる**
3. プレイヤーは `Uuid` だけで、退出して同じ名前で入り直すと、古い参照（これから作るスケジューラのタスクなど）が**新しいセッションを指す**

## 決めたこと

### 1. `World` を「状態の `Ctx`」と「ハンドラの木」に分ける

```rust
pub struct Ctx { /* チャンク・プレイヤー・送信先。いまの World の中身 */ }
pub struct World { ctx: Ctx, events: EventNode<Ctx> }
```

- ハンドラは `FnMut(&mut E, &mut Ctx)`（architecture.md の `fn(&mut E, &mut Ctx)` と同じ）。`EventNode<C>` は文脈の型引数を持ったまま。利用者が自作した Instance でも、ハーネスの `Recorder` でも使えるので
- **ローダーは `Box<dyn ChunkLoader>` で持つ**。型引数 `L` は `Ctx` にも `World` にも無い。読み込みは 1 チャンクにつき 1 回なので、動的な呼び出しのコストは無視できる
- 木は `Ctx` の外にある。`event.rs` の doc が元から勧めていた形で、発火に `mem::take` が要らない（`self.events.emit(&mut event, &mut self.ctx)` は別のフィールドの借用）。ハンドラは `Ctx` しか持たないので、**発火中の木には触れない**（型で保証される）
- `World` は `Deref<Target = Ctx>` / `DerefMut` を持つ。`world.view_distance = 2`・`world.set_block(..)` のように、「`World` は `Ctx` にハンドラを足したもの」として今までどおり書ける。標準の指針では `Deref` はスマートポインタ向けだが、ここは設定の書き方を変えないことを優先する
- `Ctx` が公開するのは、設定のフィールド（`spawn`・`view_distance`・`chunks_per_tick`・`entity_view_distance`）と、`block`・`set_block`・`send_message`・`broadcast`・`name`・`is_online`・`player_id`。内部の表は公開しない。Audience（M2-14）・`spawn`（M2-03）・スケジューラ（M2-05）が足すものはここに足す

### 2. 退出は `Ctx` が記録し、`World` が 1 回の処理の後にまとめて発火する

送信に失敗した人を落とす `leave` は、状態の側のあちこちから呼ばれる。`Ctx` は木を持たないので、退出を `departed` に積み、`World` が `handle`・`tick` の終わりに `PlayerLeaveEvent` を発火する。ハンドラの中で起きた退出も、そのハンドラが終わってから届く（上の 2 が解消する）。

入室は、`PlayerJoinEvent` を発火する位置（他の人へ知らせた後・チャンクを送る前）を保つため、`Ctx` の前半 → 発火 → `Ctx` の後半に分ける。チャット・ブロックの破壊と設置も、同じく「検証して event を作る → 発火 → 結果を反映」に分ける。

### 3. プレイヤーは `PlayerId { uuid, serial }`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayerId { uuid: Uuid, serial: u64 }
```

- `serial` は入室のたびに増える。`Ctx` が持つ現在の `serial` と合わなければ、そのハンドルは**もういない人**を指す。`ctx.name(id)`・`ctx.is_online(id)`・`ctx.send_message(id, ..)` は、いない人には `None` / `false` / 何もしない（panic しない）
- 内部の表は `Uuid` 引きのまま（遅くならない）。`PlayerId` は公開の入り口だけ
- イベントの `player` が `PlayerId` になる。`name` はそのまま持つ（退出したあとの `PlayerLeaveEvent` でも名前が読める）
- `Ctx::player_id(Uuid)` で、ハーネスの `FakePlayer`（`uuid()` だけを持つ）から `PlayerId` を引ける
- エンティティの ID は M2-17 で決める。プレイヤーを同じ表に載せるなら `From<PlayerId>` で足せる形にしてある

### 4. 利用者データの置き場（実装は M2-19）

`Player` の記録に型付きの key で引く表を持つ（`PlayerId` から引く）。`serial` が合わなければ引けないので、退出した人のデータを新しいセッションが読むことは無い。key の形は [user-data-plan.md](user-data-plan.md)（D35）で決めた。

## やらないこと（M2-01）

| 項目 | なぜ外したか | 受け皿 |
|---|---|---|
| ハンドラの中での `ctx.emit` と、木へのハンドラ追加 | `Ctx` が木を持たないので、遅延キューが要る。優先度・条件と同じ設計で足す | M2-02 |
| `ctx.spawn` | tokio との接続が要る | M2-03 |
| 全員へ送る・1 人を除いて送る | Audience の設計と一緒に | M2-14（D29 の (5)） |
| `PlayerId` からプレイヤーの位置などを読む API | 使う単位（M2-15・M2-16）で足す | M2-15 |
| 公開型 `World<L>` との互換 | v0.0.0 で未公開。移行の手間を取らない | — |

## テスト

- `World` から型引数が消えた: `FlatGenerator` 以外のローダー（`Fn(ChunkPos) -> Option<Chunk>`）を渡しても、ハンドラの型に現れない
- 退出した人の `PlayerId` を使っても panic しない: `name` が `None`、`is_online` が `false`、`send_message` は何も送らない。**同じ名前で入り直したあとも、古い `PlayerId` は新しい人を指さない**
- ハンドラの中で送信に失敗した人が落ちたとき、`PlayerLeaveEvent` が（そのハンドラが終わった後に）届く
- `PlayerJoinEvent` の位置が変わらない（他の人へ知らせた後・チャンクの前）。既存の順序のテストが通る
- 既存のテストと lobby のテストは、`PlayerId` と `World` の型に合わせて直すだけで通る
- `mise run check`（通った）
