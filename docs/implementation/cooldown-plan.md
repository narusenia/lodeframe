# Cooldown 実装計画（M2-27）

> **Status**: 計画 — 2026-10-02

要件: REQ-API-006 の「Cooldown 等の補助」（サーバー側の判定。vanilla のアイテムのクールダウン表示とは別）。決定: D34・D35（[decisions.md](../decisions.md)）。
本体の単一 crate 内の変更（`cooldown.rs` 新規・`schedule.rs`）。設計ゲートの対象外。

## 困っていたこと

「火の玉は 3 秒に 1 回」のような使用間隔を、プレイヤーごとに管理したい。今は `Data` に最後に使った時刻を入れて自分で比べるしかないが、
現在の tick を読む道が無い（`Scheduler` が数えているが外から見えない）。

## 決めたこと

### 1. `Cooldown`: 「いつまで使えないか」を持つ値

```rust
const FIRE: Key<Cooldown> = Key::new("mygame:fire");

events.on(|e: &mut ChatEvent, ctx: &mut Ctx| {
    let now = ctx.now();                                   // Data を借りる前に読む
    let cd = ctx.player_data_mut(e.player).unwrap().get_or_insert_with(&FIRE, Cooldown::default);
    if cd.try_use(now, Delay::secs(3)) {
        // 使えた。次に使えるのは 3 秒後
    } else {
        // cd.remaining(now) で残りを出せる
    }
});
```

- `Cooldown` は**終わる tick だけ**を持つ小さな `Copy` の値。`Default` は「いつでも使える」
- `ready(now) -> bool`、`start(now, delay)`（使えるかに関わらずやり直す）、`try_use(now, delay) -> bool`（使えれば開始して `true`）、`remaining(now) -> Delay`（使えるなら `Delay::ZERO`）
- 長さは**使うときに渡す**。レベルや装備で変えられ、key は `const` で宣言して `Default` で作れる
- `start(now, delay)` で `delay` tick 後の tick から使える。`Delay::ZERO` なら常に使える（スケジューラの「次の tick 以降」とは違い、最小値を設けない。クールダウンに 0 を渡すのは「無し」の意味）
- 置き場は `Data` の key。持ち主ごとの入り口（`player_data`・`data`、後でエンティティ・アイテム）がそのまま使え、退出で消える。専用の表や登録は持たない

### 2. 現在の tick: `Tick` と `ctx.now()`

- `Tick` は tick 数の newtype（`Copy`・`Ord`）。`Tick + Delay` で後の tick が出る。`Delay` と取り違えないための型で、`as_ticks()` で数も取れる
- `ctx.now() -> Tick`: 今動いている（ハンドラの間は最後に走った）tick の番号。`World` が作られた時が 0 で、`World::tick` のたびに 1 増える。`Scheduler` が数えているものを公開する
- 実時間は読まないので、`env.tick(n)` でそのまま進む（D21）

## 確認して決めたこと（2026-10-02）

| 判断 | 選んだ案 | 選ばなかった案 |
|---|---|---|
| 形 | 値の型 `Cooldown` を `Data` の key に入れる | `ctx` の持ち主ごとのメソッド（持ち主ごとに同じメソッドを作る）、スケジューラに登録する型（tick 数を見るだけで足りるのに、タスクと後始末を持つ） |
| 長さ | 使うときに渡す | 作るときに持たせる（途中で長さを変えるには作り直す） |

## やらないこと（M2-27）

| 項目 | なぜ外したか | 受け皿 |
|---|---|---|
| `Cooldown` を借用なしで 1 行にする `ctx` のメソッド | `Data` を借りたまま `ctx` を読めない不便は `let now = ctx.now()` の 1 行で済む。持ち主ごとのメソッドは増やさない | 要望が出てから |
| アイテム・エンティティの Cooldown | アイテム・エンティティが無い。同じ `Data` を持てば同じ key で使える | M2-17・M2-20 |
| vanilla のアイテムのクールダウン（アイコンの灰色の表示とクライアント側の使用停止） | この `Cooldown` はサーバー側の判定だけで、クライアントには何も送らない。表示にはアイテムとパケットが要る | M2-20 以降（同じ期限の値でパケットを送れる形にしておく） |
| 使用間隔の通知（終わったら呼ぶ） | スケジューラの `once` で書ける | — |
| 実時間のクールダウン | tick 数で数えるのが D21 の前提 | — |

## テスト

`cooldown.rs` の単体テストと `tests/world.rs`（ハーネス）。

- 新しい `Cooldown` は使える。`try_use` は使えれば `true` で、以後 `delay` tick の間は `false`。`delay` tick 後に `true`
- `try_use` は失敗しても期限を伸ばさない。`start` は期限をやり直す。`remaining` は経過で減り、使えるなら `Delay::ZERO`
- `Delay::ZERO` のクールダウンは常に使える。期限の計算は桁あふれしない
- ハーネス: プレイヤーごとに独立する。`env.tick(n)` で期限が過ぎる。退出して入り直した人は新しい `Cooldown` から始まる。`ctx.now()` が tick ごとに 1 増える
- `mise run check`・`mise run msrv`

## 実装の順

1. `schedule.rs`: `Tick`・`Ctx::now`。`cooldown.rs`: `Cooldown`・単体テスト。`lib.rs` に `pub mod cooldown`
2. `tests/world.rs` にハーネスのテスト
3. 文書: REQ-API-006 の受入条件、minestom-parity（8.13）、architecture、backlog（M2-27 を ✅）、decisions（D36）
