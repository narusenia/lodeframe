# Instance と tick ループ 実装計画（M1-09）

> **Status**: 実装済み（実機確認済み）— 2026-09-30。ロギングは tracing + tracing-subscriber、超過は追いつき 2 秒で合わせ直す

要件: REQ-WORLD-001。決定: D6・D16・D21（[decisions.md](../decisions.md)）。
所有モデルの土台で、以後の全単位（イベント・チャンク・テストハーネス）が載るため、ここに設計を置く。

## 問題

Instance を 1 スレッドが `&mut` で所有し、20 TPS で tick する。接続（tokio の conn task）とは channel だけでやり取りする。
同じ単位で、いま握りつぶしている接続層の失敗（handshake 失敗・ハンドラのエラー・タイムアウト）を出すロギングも決める。

## 設計

### 所有と型による保証

- `Instance` は**スレッドの中で生成する**。`spawn(factory: impl FnOnce() -> I + Send)` がスレッドを起こし、中で `I` を作る。`I` は `Send` でなくてよい。
- 外に出るのは `InstanceHandle`（`Clone + Send`）だけ。`Instance` への参照は外に出ない。
  「Instance 間の相互作用はメッセージ経由でしか行えない」は、`Instance` がスレッドの外で手に入らないことで型が保証する（受入条件）。
- Instance 間の通信は `handle.send(msg)`（channel）だけ。

### tick

- `Instance::tick(&mut self)` は**1 tick 分を進めるだけ**の同期関数。時計もスレッドも知らない。テストハーネスは `tick(n)` をこれで直接回す（D21）。
- tick の順（architecture.md どおり）:
  1. inbound を drain（接続の入退室・パケット）
  2. `spawn` の結果を drain（M1-12 以降で中身が入る。ここは枠だけ）
  3. tick 本体（物理・スケジューラ。いまは空）
  4. outbound へ差分を送る
- 周期は 50ms（20 TPS）。

### 時計

```rust
pub trait Clock {
    fn now(&self) -> Instant;
    fn sleep_until(&self, t: Instant);
}
```

- 本番 `SystemClock`（`std::thread::sleep`）、テスト用は後で M1-19 が足す。ループ `run(instance, clock)` だけが時計を使う。

### 超過（受入条件）

- tick が 50ms を超えたら、次の deadline を**遅れた分だけ詰めて**回す（追いつき）。
- 遅れが 2 秒を超えたら追いつきを諦めて deadline を現在に合わせ直し、警告を 1 回出す（vanilla の "Can't keep up" と同じ考え）。無限に詰めて他を飢えさせないため。
- 超過のたびに「何 ms 遅れたか」をログに出す。

### conn task との channel

- conn task → Instance（inbound）: 1 つの bounded mpsc。`send().await` なので、詰まるとソケットの読み取りが止まり、自然に背圧がかかる。
- Instance → conn task（outbound）: 接続ごとの bounded mpsc。**満杯になった接続は切断する**（遅いクライアントが Instance を止めないため）。
- Play 中の conn task（`play::run`）は、inbound への転送に加えて keepalive（15 秒ごと送信、応答は Instance に渡さない）を受け持つ。Play の読み取りタイムアウト 30 秒が、そのままクライアントのタイムアウトになる。
- メッセージは最小: 参加（`Join { profile, outbound }`）、パケット（`Packet { player, body }`。body は id + payload）、退出（`Leave`）。中身の解釈は M1-11 以降。
- `Sessions`（Instance が使う補助）が outbound を持つ。`try_send` が失敗した接続は外して channel を閉じ、conn task が終わる。

### ロギング

- ライブラリは**イベントだけ**を出す。形式・色・レベルは利用者と example が決める。
- 接続ごとに名前・UUID を文脈（span）として付ける。
- 出す箇所: handshake 失敗、ハンドラのエラー、タイムアウト、tick 超過、Instance の起動と停止。
- crate は下の「要確認」。

## 要確認

| 論点 | 案 | 影響 |
|---|---|---|
| ロギング crate | `tracing`（本体）+ `tracing-subscriber`（example のみ） | 新規依存 2 つ。`log` より span で接続ごとの文脈を付けやすい |
| tokio の追加 feature | `sync`（mpsc）、`macros`（`select!`） | tokio は導入済み。feature の追加のみ |

## 完了条件

- [x] `tick` が 50ms 周期で回る（実時間に近い許容で確認）
- [x] 遅れた tick の後、追いつきで平均周期が戻る。2 秒超で警告して合わせ直す（偽の時計でテスト）
- [x] `Instance` に `!Send` の型（`Rc`）を持たせられる。`Instance` をスレッドの外に出す書き方はコンパイルエラー（trybuild）
- [x] inbound が詰まると conn task の読み取りが止まる。outbound が満杯の接続は切断され、他は続く（テスト）
- [x] 接続層の失敗がログに出る（`tracing` のテスト用購読で確認）
- [x] 実クライアントで Play に入り、ログが整形されて出る（実機。30 秒を超えても keepalive で接続が維持された）

## 実装メモ

- `Instance` trait は `handle(Message)` と `tick()`。`Runner::step()` が「inbox を drain → tick」の 1 歩で、`run(clock, stop)` が周期と追いつきを受け持つ。テストハーネス（M1-19）は `step()` を直接回す。
- `Uuid` に `Display`（ハイフン形式）を足した（ログ用）。
- Play の keepalive は conn task が持つ（15 秒ごと）。`KeepAlive` / `KeepAliveResponse` は protocol に置いた。

## 非対象

イベントノード（M1-12）、チャンク（M1-10）、プレイヤーの表示と移動（M1-11・M1-13）、偽の時計を含むテストハーネス本体（M1-19。ここでは `Clock` の口だけ）。
