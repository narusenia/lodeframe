# `ctx.spawn`（非同期処理の投げ出しと戻し）実装計画（M2-03）

> **Status**: 実装済み — 2026-10-02

要件: REQ-API-003 の Instance 側の 2 項目（ログイン前の async イベントは M2-04）。決定: D6・D16・D21・D31（[decisions.md](../decisions.md)）。
本体（`task.rs` 新規・`world.rs`・`instance.rs`・`server.rs`・`test_util.rs`）にまたがる。M2-05（スケジューラ）が同じ文脈の型で結果を受けるので、先にこの形を決める。

## 困っていたこと

ハンドラは同期 `fn` で、`.await` もブロッキングもできない（D16）。DB・HTTP のような待つ処理を書く道が無い。
architecture.md には「`ctx.spawn(fut).then(cb)`: 結果は次 tick 冒頭で Instance のスレッドに戻る」とあるが、実装が無い。

## 決めたこと

### 1. API

```rust
ctx.spawn(async move { db.load(id).await })        // Future<Output = T> + Send + 'static, T: Send + 'static
    .then(|loaded, ctx| { ... });                   // FnOnce(T, &mut Ctx) + 'static（Send 不要）

ctx.spawn(fut).then_for(player, |loaded, ctx| { ... });   // player が退出済みなら呼ばない
```

- `spawn` は**呼んだ時点で** tokio に投げる。`.then` を付けなければ結果は捨てる（fire and forget）
- コールバックは Instance のスレッドに残り `Send` を要らない。tokio に渡るのは future と結果だけ。コールバックは `Ctx` の表に残し、結果は id で引く
- 結果は**次の `tick` の冒頭**（`ctx.tick()` の前）に、完了した順で渡る。コールバックの中の `ctx.emit` などは、M2-02 の遅延キューで同じ `tick` のうちに反映される
- `then_for(player, cb)`: 届く時点で `ctx.is_online(player)` が偽なら `cb` を呼ばない（`PlayerId` の `serial` を見るので、同名で入り直した別のセッションにも呼ばない）。`then` は常に呼ぶ
- future が panic した、またはランタイムが止まって捨てられたときは、コールバックを呼ばず `tracing::error!` を出す。完了を待つ側（ハーネス）が永久に待たないよう、完了の通知は drop ガードで必ず送る

### 2. Handle の受け取り: `Instance::attach`

```rust
trait Instance {
    /// 非同期処理を投げる先。Instance が動き出す前に 1 回呼ばれる。既定は何もしない
    fn attach(&mut self, runtime: tokio::runtime::Handle) {}
    ...
}
```

- 本番: `instance::spawn(name, clock, runtime, factory)` が Instance を作った直後に `attach` を呼ぶ。`Server::start` は `Handle::current()` を渡す
- ハーネス: `TestEnv::new` が自前の current-thread ランタイムを持ち、その Handle で `attach` する
- `World` は `attach` で `Ctx` に Handle を持たせる。`attach` が呼ばれる前に `ctx.spawn` すると panic（メッセージで原因を示す）。入室前の設定ミスであり、最初のテストで気づくため、黙って捨てるより良い
- 理由: 本番とテストが同じ経路で渡せ、グローバル（thread local 等）が要らない（D6）。自作 Instance も使える

### 3. 結果の戻し

- 完了した future が、`Ctx` の `UnboundedSender<Done>` に `(id, 結果)` を送る。`World::tick` の冒頭で `try_recv` を回して、表からコールバックを引いて呼ぶ
- 戻りの待ちは**最大 1 tick（50 ms）**。Instance は idle でも 20 TPS で回るので、専用の起こし方は要らない
- channel を unbounded にする理由: 送る側は tokio のタスクで、詰まったら待たせる相手がいない。同時に走る数は利用者が決める。上限と観測は REQ-OPS-001（v0.8）
- 型消去: `then` が結果の型 `T` を知っている間に `Box<dyn FnOnce(Box<dyn Any + Send>, &mut Ctx)>` へ包んで表に入れる。`Done` は `Box<dyn Any + Send>`

### 4. ハーネス: `TestEnv::run_until_idle`

- `env.run_until_idle()`: 投げた future が**全部終わる**まで current-thread ランタイムを回す。終わらなければ 5 秒（実時間）で panic し、どの future が残っているかは分からないので「N 個が終わっていない」を出す
- コールバックは動かさない。**次の `env.tick(1)` で動く**（本番と同じ順序をテストで再現するため。tick が遅れないことのテストにもなる）。典型は `env.run_until_idle(); env.tick(1);`
- 残りの数は `Handle::metrics().num_alive_tasks()` で見る（`Instance` に口を増やさない）
- ハーネスのランタイムは `start_paused(true)`（`tokio/test-util`、`test-util` feature で有効）。`sleep` を含む future は、実時間を待たず、決定的に完了する（REQ-API-006 の `env.tick(n)` と同じ考え方、D21）。新しい依存 crate は無く、tokio の feature を足すだけ

## 確認して決めたこと（2026-10-02）

| 判断 | 選んだ案 | 選ばなかった案 |
|---|---|---|
| Handle の渡し方 | `Instance::attach`（既定は何もしない） | `World::set_runtime`（利用者が毎回呼ぶ・ハーネスが渡せない）、World が専用 runtime を持つ（runtime が二重・テストが決定的でない） |
| 退出の扱い | `then` と `then_for`（退出済みなら呼ばない） | `then` のみ（確認の書き忘れ）、`Option<T>` を常に渡す（全部で Option を扱う） |
| `run_until_idle` | 終わるまで待つ。コールバックは次の tick | その場でコールバックまで実行（1 tick の遅れが隠れる・trait に口が増える）、`tick(n)` が非同期も進める（決定的でない） |

## やらないこと（M2-03）

| 項目 | なぜ外したか | 受け皿 |
|---|---|---|
| ログイン前の async イベント | conn task 側で await する別の経路 | M2-04 |
| 投げた処理の中止（`abort`） | `then_for` が cb を呼ばないので見える挙動は足りる。future 自体の中止は `JoinHandle` の持ち方が別の設計 | 要望が出てから（[scheduler-plan.md](scheduler-plan.md) のやらないこと） |
| 同時に走る数の上限・観測 | 計測してから | REQ-OPS-001（v0.8） |
| `spawn_blocking` | future の中から `tokio::task::spawn_blocking` を使える | — |
| `World` を落としたときに走っている future を止める | 結果の送り先が閉じるので、完了時に捨てるだけで害は無い | 同上 |

## テスト

`tests/world.rs`（ハーネス）と `instance.rs` の単体テスト。

- `spawn(async { 21 * 2 }).then(..)`: コールバックは `run_until_idle()` だけでは呼ばれず、`tick(1)` で 42 を受けて呼ばれる
- 終わらない future（`pending`）を投げても `tick(n)` は遅れず返る（受入: tick が遅延しない）。コールバックは呼ばれない
- 時間のかかる future（`sleep(10s)`）が `run_until_idle()` で実時間を待たずに終わる
- `then_for`: 退出したプレイヤーには呼ばれない。同名で入り直したプレイヤーにも呼ばれない。いるプレイヤーには呼ばれる。`then` は退出済みでも呼ばれ、`PlayerId` は何も指さない（panic しない）
- 結果は完了した順に、同じ tick で渡る。コールバックの中の `ctx.emit` と `ctx.spawn` が動く（後者は次の tick に届く）
- panic する future でコールバックは呼ばれず、`run_until_idle` が終わる
- `attach` の前に `spawn` すると分かりやすいメッセージで panic する
- `Server` 経由（ボットの統合テスト）で、実ランタイムの future の結果が tick に戻る
- `mise run check`

## 実装の順

1. `instance.rs`: `Instance::attach`、`spawn` が Handle を受けて呼ぶ。`server.rs` が渡す
2. `task.rs`: `Tasks`・`Spawned`・drop ガード。`world.rs`: `Ctx` に `spawn`、`World::attach`・`tick` 冒頭の取り込み
3. `test_util.rs`: ランタイムを持つ `TestEnv`、`run_until_idle`、`tokio/test-util`
4. 文書: REQ-API-003 の受入条件（Instance 側 2 項目）、architecture、minestom-parity、backlog、decisions
