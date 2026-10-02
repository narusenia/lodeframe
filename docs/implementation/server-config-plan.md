# サーバーの設定と停止 実装計画（M2-06・M2-28）

> **Status**: M2-06・M2-28 実装済み — 2026-10-02

要件: REQ-NET-006、REQ-NET-002（在線人数）。決定: D6・D16・D21・D34（[decisions.md](../decisions.md)）。
本体の単一 crate 内の変更（`server.rs`・`play.rs`・`instance.rs`・`world.rs`・`status.rs`）。設計ゲートの対象外だが、M2-07〜10（proxy 系）が設定を足す入り口になるので、形を先に決める。

## 分け方

REQ-NET-006 は触る場所が 2 つに分かれるので、2 単位にする。

| 単位 | 中身 | 触る場所 |
|---|---|---|
| **M2-06** | 設定項目、在線人数と最大人数、keep alive の検証と遅延の計測 | net 側（`server.rs`・`play.rs`・`configuration.rs`・`status.rs`）と `World` の遅延の記録 |
| **M2-28** | Ctrl-C での停止、`ShutdownEvent`、全員への切断理由 | Instance 側（`instance.rs`・`world.rs`） |

M2-07・09・10 が待つのは設定の入り口だけなので、依存は M2-06 のままにする。M2-28 は独立している。

## M2-06

### 1. 設定項目（`Server` のビルダー）

既存の `motd`・`brand` と同じく、`Server` のメソッドを平らに足す。設定をまとめる struct は作らない（`Server` 自身が設定の入れ物）。

| メソッド | 意味 | 既定（今の挙動） |
|---|---|---|
| `compression_threshold(usize)` | この長さ以上の本体を圧縮する | 256 |
| `keep_alive(interval, timeout)` | keep alive を送る間隔と、応答が無くて切るまでの時間 | 15 秒・30 秒 |
| `known_packs_timeout(Duration)` | configuration で Known Packs の応答を待つ時間 | 30 秒（今の `read_timeout`） |
| `max_players(u32)` | 最大人数。一覧に出し、ログインで数える | 20 |
| `tick_rate(u32)` | 1 秒あたりの tick | 20 |
| `max_catch_up(Duration)` | 遅れた tick を追いつく上限。超えたら今から数え直す | 2 秒（`MAX_BEHIND`） |
| `nodelay(bool)` | ソケットの `TCP_NODELAY` | `false`（今は触っていない） |

- `tick_rate` を変えても `Delay::secs` は **1 秒 = 20 tick** のまま（D34 の換算は変えない）。遅い設定ではゲーム内の時間が遅くなる。vanilla のクライアントは 20 を前提にしているので、変えるのは負荷試験や特殊用途と割り切る
- 範囲外（`tick_rate(0)`・`keep_alive` の timeout が interval 以下）は `start` が `io::ErrorKind::InvalidInput` で返す。黙って直さない
- 既定値が今と変わるのは**最大人数**だけ。今は一覧に 20 と出すだけで数えていない。既定でも 20 を超えるログインを断る。20 人を超えて繋ぐ利用（負荷試験）は `max_players` を渡す。tests と bench の該当箇所は同じ変更で直す

### 2. 在線人数と最大人数

- `Server` が `Arc<AtomicU32>` の**数だけ**を持ち、conn task が数える。Instance の状態は共有しない（D6 の対象は Instance の状態。読み取り専用の数 1 つは別）
- ログイン成功の直後、configuration の前に「`online < max` なら +1」を 1 回の compare-exchange で行う。取れなければ `Disconnect`（configuration 段階。`LoginFinished` を送った後なので login の Disconnect は使えない）で理由を出して切る。接続が終わるときの drop で -1
- status は `StatusInfo` を問い合わせごとにカウンタから作る（`StatusInfo` の既存の説明どおり）。Instance の tick を待たない
- 数えるのは**接続**。同じ UUID の再ログインで古い接続が残っている短い間は 2 と数える。満員のときはこの再ログインも断られる（古い接続が切れるのを待つ設計は取らない。D 番号なしの割り切りとして、ここに残す）

### 3. keep alive の検証と遅延

- 今は 15 秒ごとに送るだけで、応答の id を見ず、切断は `read_timeout`（30 秒）任せ。これを `play.rs` の中で持つ
- 送った id と時刻を覚え、応答が来たら id を照らす。**一致**なら遅延 = 今 - 送った時刻。**不一致**・**予期しない応答**は切る（vanilla も切る）。**timeout** の間応答が無ければ理由つきで切る
- play の読みは `read_timeout` ではなく keep alive の timeout で切れる（今の `read_timeout` の役割を引き継ぐ。ログインまでは `read_timeout` のまま）
- 計測した遅延は `Message::Latency { player, rtt }` で Instance に送る。`World` が `Player` に最後の値を持ち、`ctx.ping(PlayerId) -> Option<Duration>` で読む。`Message` は列挙型なので、自作 Instance は新しい変種を `_ =>` で受けるか処理する（v0.x の間の変更として docs に書く）
- ハーネスは `env.ping(player, Duration)` で値を入れられる形にする（D21）

## M2-28

### 4. 停止

- `Server::run` は既定で **Ctrl-C を待つ**。受け取ったら `RunningServer::stop` と同じ道で止まる。`.handle_ctrl_c(false)` で切れる。`start` は何もせず、組み込みの利用者は `stop` を自分で呼ぶ
- 止め方: ① 新しい接続を受けない ② Instance に停止を送る ③ Instance が `ShutdownEvent` を発行し、ハンドラが終わるのを待つ ④ 全員に理由つきで切断（play 段階の Disconnect）⑤ スレッドを止める。Ctrl-C を 2 回目に受けたら待たずに終える
- **`ShutdownEvent { reason: Component }`**: 既存のイベントノードに載る。ハンドラは `Ctx` でデータ・プレイヤーに触れる（保存など）。`reason` を書き換えると切断理由が変わる。既定は「Server closed」。`env.emit` でハーネスから検証できる
- 停止の上限時間（ハンドラが終わらないとき）は `shutdown_timeout(Duration)`（既定 10 秒）。ハンドラは同期 `fn` なので、超えるのは `ctx.spawn` の待ちを含む場合に限る

### 実装での形

- `Instance::shutdown(&mut self)`（既定は何もしない）を足した。`Message` の変種にしなかったのは、自作 Instance の `match` を壊さず、停止が tick の途中に割り込まないため。`InstanceHandle::shutdown()` が旗を立て、`Runner::run` が次の tick の前に `Instance::shutdown` を呼んで抜ける。`InstanceHandle::stopped()` で終わりを待てる。`InstanceHandle::stop()` は従来どおり挨拶なしで止める
- `World::shutdown`: `ShutdownEvent` を発火 → ハンドラの要求を処理 → 全員を `reason` で切断（各自の `PlayerLeaveEvent` が続く）
- `RunningServer::stop` は待たずに「受付を止めて shutdown を頼む」まで。`RunningServer::shutdown().await` は加えて、Instance の終了と全接続の書き出し（`online() == 0`）を `shutdown_timeout` まで待ち、超えたら `InstanceHandle::stop` で切る
- `Server::run` は Ctrl-C で `shutdown` に入り、2 回目の Ctrl-C は待たずに返る。シグナル待ちのために tokio の `signal` feature を足した（新しい crate は `signal-hook-registry` と `errno`）
- 切断理由は play 段階の `Disconnect`。ハーネスは `env.shutdown()`

## 確認して決めたこと（2026-10-02）

| 判断 | 選んだ案 | 選ばなかった案 |
|---|---|---|
| 分け方 | 2 単位（設定と keep alive／停止） | 1 単位で全部（PR が大きくレビューと CI の切り分けが重い） |
| 在線人数 | 共有カウンタ（`AtomicU32`） | Instance への問い合わせ（status のたびに往復し、tick が詰まると一覧が応答しない。複数 Instance では全部に聞く） |
| Ctrl-C | `run` だけ既定で有効、`handle_ctrl_c(false)` で無効 | 明示的に有効化（要件の「Ctrl-C で切断」が書かないと満たされない） |
| 停止時のタスク | `ShutdownEvent`（ノードに載る） | `Server::on_stop`（優先度・ゲートが使えず、ハーネスの入り口が別に要る） |

## やらないこと

| 項目 | なぜ外したか | 受け皿 |
|---|---|---|
| タブリストの遅延表示（`UpdateLatency`） | 遅延を読む道が先。表示はプレイヤー状態と一緒に決めたい | M2-15 |
| 遅延の平滑化・履歴 | 最後の値で足りる。要望が出てから | — |
| 認証前の受信制限（パケットサイズ・数） | REQ-NET-007 の対象 | v0.3 |
| 複数 Instance の人数・停止順 | Instance が 1 つの間は不要 | REQ-WORLD-005（v0.3） |
| `ShutdownEvent` の中止（止めない） | 止めるかどうかは利用者が `stop` を呼ぶ側で決める | — |

## テスト

- 単体: 設定の範囲検査（0 の tick_rate、timeout <= interval は `InvalidInput`）、カウンタの +1/-1（満員で断る・drop で戻る）、`StatusInfo` が数を出す
- 統合（`tests/server.rs`・`tests/bot.rs`）: 満員のボットが理由つきで断られ、1 人出ると入れる。一覧に実際の人数が出る。keep alive の id が違うボットは切られる。応答しないボットは timeout で切られる。`max_players` を超える負荷試験の箇所は引数を足す
- ハーネス（`tests/world.rs`）: `env.ping` の値を `ctx.ping` が返す。退出で消える。`ShutdownEvent` のハンドラが全員を読める・`reason` を変えられる
- M2-28 の統合: `stop` で全員に切断理由が届き、停止時のハンドラが先に走る（Ctrl-C のシグナル自体は手で確かめる）
- `mise run check`・`mise run msrv`

## 実装の順

1. M2-06: `Server` の設定と範囲検査。`status.rs` の人数。ログインの満員。`play.rs` の keep alive と `Message::Latency`。`ctx.ping`・`env.ping`
2. 文書（REQ-NET-006 の受入条件のうち遅延、REQ-NET-002、backlog、minestom-parity、architecture）。PR
3. M2-28: `ShutdownEvent`・停止の手順・Ctrl-C。文書。PR
