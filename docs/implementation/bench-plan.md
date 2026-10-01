# 性能ベースライン 実装計画（M1-18）

> **Status**: 計画 — 2026-10-01

要件: REQ-PERF-001（暫定値の見直し）。決定: D12・D15（[decisions.md](../decisions.md)）。
本体に tick の統計を足し、`xtask bench` で起動時間・アイドル RSS・ボット N 体での tick を表にする。結果をこの文書に残す。

## 測るもの

| 指標 | どう測るか | REQ-PERF-001 の暫定値 |
|---|---|---|
| 起動からリッスン開始まで | `xtask bench` が lobby のプロセスを起動し、TCP 接続が通るまでを 1ms 刻みで待つ。プロセスの起動を含む | < 100ms |
| アイドル RSS | プレイヤー 0 のまま 2 秒待ち、`ps -o rss=` で読む（macOS・Linux 共通） | < 30MB |
| ボット N 体での TPS | サーバーが数える tick の数を、1 秒ごとの出力から、窓（ボットが全員入ったあと）の始めと終わりの差で割る | 500 体で 20 TPS |
| 1 tick の処理時間（平均・最大）、遅れた tick の数 | サーバーの tick 統計（下） | — |
| N 体のときの RSS | 窓の終わりで `ps` | — |

N は 10・50・100・250・500。500 で崩れたら崩れたまま表にする（崩れ方が基準値になる。視界による絞り込みは REQ-ENT-002）。

## 設計

### tick の統計（本体）

`Runner::run` が 1 tick の処理（`step`）にかかった時間を測り、`InstanceHandle::tick_stats()` で読めるようにする。値は起動からの累計。

- `TickStats { ticks, busy, busy_max, late, skipped }`: tick の数、処理時間の合計と最大、予定より 1 tick 以上遅れて始まった tick の数、`MAX_BEHIND` を超えて追いつくのを諦めた回数
- 時計は `Clock::now()` を使う（D21: 実時間を直接読まない）。テストのハーネス（`step` を手で回す）は測らない
- 更新は atomic。`RunningServer::tick_stats()` で取れる

### lobby の統計出力

環境変数 `LOBBY_STATS` があるとき、lobby が 1 秒ごとに 1 行を標準出力へ出す。`xtask bench` が読む。

```
tick-stats ticks=100 busy_us=1234 busy_max_us=900 late=0 skipped=0
```

計測のためだけの出力で、普段は出さない。ログ（tracing）ではなく `println!` にして、色や書式に依存せず読める形にする。

### `xtask bench`

```
cargo xtask bench [--counts 10,50,100,250,500] [--window 10] [--port 25590]
```

1. `cargo build --release -p lobby -p lodeframe-bot`
2. 起動時間とアイドル RSS: lobby を起動し、接続できるまでの時間と、2 秒後の RSS を取る。3 回やって最小値を表に出す（最初の 1 回はディスクキャッシュの影響が出るため）
3. N ごとに: lobby を**新しく**起動する（キャッシュと前の回の状態を持ち越さない）。ボットの bin に `--count N --seconds (入室の間隔 + 窓)` を渡し、全員が入るまでの待ち（`N × 20ms + 2s`）のあと、`--window` 秒を窓にする。窓の始めと終わりの `tick-stats` から TPS・平均・最大・遅れ・諦めを出し、窓の終わりに RSS を取る
4. 表（Markdown）と、マシン（OS・CPU 数・メモリ）を標準出力へ出す。プロセスは最後に確実に止める

xtask は lobby とボットを**別プロセス**で起動する（RSS をサーバーだけで測るため）。xtask 自体は本体に依存しない（D26 のとおり軽いまま）。

## v0.1 でやらなかったこと（後で対応する）

| 項目 | 今の動き | なぜ外したか | 受け皿 |
|---|---|---|---|
| **同条件の Minestom との比較** | 計測しない。手順に「未実施（Minestom が 26.3 をまだ話せない）」と書く | Minestom の最新版が 26.2 までで、同じプロトコル版に揃えられない。版が揃わないとボットが同じ条件で入れない | Minestom が 26.3 に対応したとき。REQ-PERF-001 の最後の項目 |
| CI での比較結果の出力 | しない。手元で `xtask bench` を実行する | CI のマシンは遅く揺れる。比較対象（Minestom）も無い | v0.4（REQ-PERF-001・REQ-INFRA-003） |
| 同じ条件での複数回の計測と、ばらつきの評価 | N ごとに 1 回（起動時間と RSS だけ 3 回の最小値） | まず基準値を取る。ばらつきが大きいと分かった指標から直す | 結果を見て |
| Linux など他の環境での計測 | 手元の macOS（Apple M4 Pro）だけ | CI と同じ環境の値は、CI での出力と一緒に決める | v0.4 |
| ボットとサーバーが同じマシンでCPU を取り合う影響の切り分け | 切り分けない。マシンの CPU 数を表に書く | 別マシンの用意が要る | v0.4 |
| 長時間の計測（メモリの漏れ、tick の劣化） | 窓は短い | ベースラインの目的を超える | v0.4 |
| 実クライアントの描画・回線の遅延 | ループバックだけ | ボットは protocol だけを話す | — |

## テスト

- `TickStats`: `Runner::run` を偽の時計で回して、tick の数・処理時間・遅れ・諦めが数えられる（`SystemClock` に頼らず、`Clock` を実装した偽物で）
- `RunningServer::tick_stats()` が tick の進みを返す（統合テスト）
- `xtask bench`: 出力の解析（`tick-stats` の行）と窓の計算の単体テスト。実際の計測はテストにしない（遅く、環境に依存する）

## 動作確認

- `mise run check`
- `cargo xtask bench --counts 10 --window 5`（短い実行で、表が出る）
- `cargo xtask bench`（本番。結果は下の「結果」に貼る）

## 結果

（実装のあとに記録する）
