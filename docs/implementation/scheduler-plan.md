# スケジューラ 実装計画（M2-05）

> **Status**: 実装済み — 2026-10-02

要件: REQ-API-006。決定: D6・D21・D31・D33（[decisions.md](../decisions.md)）。
本体の単一 crate 内の変更（`schedule.rs` 新規・`world.rs`）で、設計ゲートの対象外だが、API の形が M2-17（エンティティごとのスケジューラ）と M2-26（カウントダウン）に効くので先に決める。

## 困っていたこと

ハンドラは同期 `fn` で、「1 秒ごとにカウントダウン」「5 秒後に消す」を書く道が無い（`ctx.spawn` の中で `sleep` すると、結果が tick に戻るまで 1 tick 遅れ、`Ctx` にも触れない）。
REQ-API-006 の受入条件は、カウントダウンが書けることと、プレイヤーの退出でそのプレイヤーのタスクが止まることと、`env.tick(n)` で決定的に進むこと。

## 決めたこと

### 1. API: 1 つの基本形と糖衣

```rust
// 基本形: 戻り値で次回を自分で決める
ctx.after(Delay::secs(1)).run(move |ctx| {
    left -= 1;
    if left == 0 { Next::Stop } else { Next::After(Delay::secs(1)) }
});

// 糖衣（どちらも run の短い書き方）
ctx.after(Delay::ticks(40)).once(|ctx| { .. });                 // FnOnce(&mut Ctx)
ctx.after(Delay::ZERO).every(Delay::secs(1), |ctx| { .. });     // FnMut(&mut Ctx)。止めるのは cancel か持ち主の消滅

// 修飾
ctx.after(d).for_player(player).run(..)    // 持ち主をプレイヤーにする
ctx.after(d).at_end().run(..)              // tick の終わりに走らせる（既定は開始時）
let id: TaskId = ..run(..);  ctx.cancel(id) -> bool
```

- `after(delay)` は `Schedule`（`#[must_use]`）を返し、`run`・`once`・`every` のどれかで登録する。`run` 以外は内部で `run` に落とす（仕組みは 1 つ）
- `Next` は `Stop` と `After(Delay)` の 2 つ。同じ周期で続けるなら `every` を使う。「次回を自分で決める」は `After` に別の `Delay` を入れる
- コールバックは `FnMut(&mut Ctx) -> Next` で `Send` 不要（Instance のスレッドに残る。`then` と同じ）
- `cancel` は未実行のタスクを消す。すでに終わった・消えたタスクには `false`。タスク自身の中から自分を `cancel` してもよい（`Next::After` を返しても再登録しない）

### 2. 時間の単位: `Delay`

- `Delay::ticks(n)`・`Delay::secs(n)`（20 tick）・`Delay::ZERO`・`From<Duration>`（50 ms 単位に**切り上げ**）
- 数える基準は **tick 数**で、実時間（時計）を読まない。`World::tick` が 1 回呼ばれるたびに進むので、`env.tick(n)` で決定的に動く（D21）
- 登録した tick の `n` 個後の tick に走る。`n = 0` は 1 と同じ（次の tick）。同じ tick のうちには走らないので、`every(Delay::ZERO, ..)` が tick の中で回り続けることはない
- 1 tick の周期は `TICK` 定数（50 ms）を `schedule.rs` に 1 つ置き、`Duration` の換算だけがこれを使う（TPS を変えられる設計にはしない。D6 の範囲外）

### 3. いつ走るか

`World::tick` の順序（architecture.md の図の 2→3）:

1. `ctx.spawn` の結果を戻す（M2-03）
2. **開始時のタスク**（`at_end` でないもの）
3. `ctx.tick()`（チャンク送信・移動の配信）
4. **終了時のタスク**（`at_end`）
5. `settle`（遅延キュー。タスクの中の `emit`・`add_listener` などはここで反映される）

- 同じ時点に走るタスクは、**走る tick が早い順 → 同じ tick なら登録した順**（`TaskId` の昇順）。決定的な順にするため
- 走る tick が来たタスクを先に全部取り出してから走らせる。タスクが `after` で足したタスクは、`n = 0` でも次の tick 以降になる

### 4. 持ち主と止まり方

| 持ち主 | 作り方 | 止まるとき |
|---|---|---|
| 全体 | `ctx.after(..)` | `cancel`、`Next::Stop`、`World` が落ちたとき |
| プレイヤー | `.for_player(p)` | 上に加え、**そのプレイヤーの退出**。同名で入り直した別のセッションは対象外（`PlayerId` の `serial` で指す） |

- 今は `Ctx` と `World` と Instance が 1 対 1 なので、要件の「全体」と「Instance」は同じ持ち主。複数 Instance（M2-17）で `Ctx` が複数になったとき、それぞれが自分のタスクを持つので、型は変わらない
- プレイヤー持ちのタスクは `Ctx::leave` が消す。`leave` はキックと入れ替わりの両方が通る入口で、ここなら取りこぼさない。念のため、再登録の直前にも持ち主がいるかを確かめる（実行中のタスクが自分の持ち主を退出させた場合）
- すでに退出した `PlayerId` で `for_player` すると、登録せず `TaskId` だけ返す（`cancel` は `false`）。`then_for` と同じ「何もしない」の扱い
- エンティティ持ちは M2-17 で足す（`for_entity`）。`Owner` を enum にしておく

## 確認して決めたこと（2026-10-02）

| 判断 | 選んだ案 | 選ばなかった案 |
|---|---|---|
| API の形 | 基本形 `run`（`Next` を返す）+ 糖衣 `once`・`every` | 種類ごとの別メソッド（内部が 3 経路になり、停止・持ち主の判定が散る）、クロージャが自分を再登録する（定型が長く、止め忘れで漏れる） |
| 時間の単位 | `Delay`（ticks と `Duration`） | ticks のみ（秒で考える利用者が毎回 ×20）、`Duration` のみ（tick 単位の指定ができない） |
| Cooldown | 入れない | 最小の `Cooldown` を入れる（持ち主ごとの置き場が無いまま形を決めることになる） |

## やらないこと（M2-05）

| 項目 | なぜ外したか | 受け皿 |
|---|---|---|
| Cooldown 等の補助 | 利用者データ（M2-19）の key に載せるのが自然で、置き場の設計と重なる | M2-19 の後（REQ-API-006 の記述の残り。backlog に記す） |
| エンティティ持ちのタスク | エンティティがまだ無い | M2-17 |
| `ctx.spawn` で投げた future の中止 | `then_for` が退出済みで cb を呼ばないので、見える挙動は満たしている。中止は走っている future の無駄を省く最適化で、`JoinHandle` の持ち方が別の設計になる | 要望が出てから。M2-03 の表（受け皿を M2-05 とした行）はこの行に合わせて直す |
| 実時間のスケジュール（cron 風・壁時計） | tick 数で数えるのが D21 の前提 | — |
| 同時に走るタスク数の上限・観測 | 計測してから | REQ-OPS-001（v0.8） |
| タスクの panic の捕捉 | ハンドラの panic と同じ扱い | REQ-OPS-003（v0.3） |

## テスト

`tests/world.rs`（ハーネス）と `schedule.rs` の単体テスト（`Delay` の換算）。

- `once`: `Delay::ticks(3)` で登録すると、`tick(2)` までは走らず、3 tick 目に 1 回だけ走る
- `Delay::ZERO` は次の tick に走る。登録した tick のうちには走らない
- カウントダウン: `run` が `After(Delay::secs(1))` を返し続け、20 tick ごとに 3, 2, 1 を数えて `Stop` で止まる（受入）
- `every`: 周期どおり走り続け、`cancel` で止まる。自分の中からの `cancel` で再登録されない
- `for_player`: そのプレイヤーの退出で止まり、同名で入り直したプレイヤーには走らない。退出済みの `PlayerId` では登録されない（受入）
- 順序: 同じ tick に走るタスクは登録順。`at_end` は `ctx.tick()` の後。タスクが足したタスクは同じ tick に走らない
- タスクの中の `ctx.emit`・`add_listener` が、その tick のうちに反映される
- `Duration` の換算: 50 ms → 1 tick、51 ms → 2 tick、0 → 次の tick
- `World` を落とすと、タスクのクロージャが drop される（持ち主が消えたら止まる）
- `mise run check`・`mise run msrv`

## 実装の順（この順で入れた）

1. `schedule.rs`: `Delay`・`Next`・`TaskId`・`Scheduler`（tick 数と `BTreeMap<(due, id), Task>`）・`Schedule`。`Ctx` に `after`・`cancel`
2. `world.rs`: `Ctx` が `Scheduler` を持つ。`World::tick` の開始・終了で走らせ、`Ctx::leave` でプレイヤー持ちを消す
3. 公開: `lodeframe::schedule` に `Delay`・`Next`・`TaskId`・`Schedule`・`TICK`（本体が facade なので re-export は要らない）
4. 文書: REQ-API-006 の受入条件、architecture（tick の順序とスケジューラ）、minestom-parity、backlog（M2-05 を ✅、Cooldown の残りを記す）、M2-03 の計画書の表、decisions（D34）
