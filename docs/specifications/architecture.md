# アーキテクチャ

要件は [requirements.md](../requirements.md)、根拠は [decisions.md](../decisions.md)。
API の具体形は実装で確定させ、確定したらここを直す。

## crate 構成（D14）

```
lodeframe/
├── crates/
│   ├── lodeframe-text/       # Component・MiniMessage パーサ。protocol と macros が依存
│   ├── lodeframe-protocol/   # 基本型・Encode/Decode・NBT・パケット・生成データ。本体非依存
│   │   └── src/generated/    # xtask datagen の出力（commit する）
│   ├── lodeframe-macros/     # proc-macro（Encode/Decode, Packet, command, event, text!, UI）
│   ├── lodeframe-bot/        # protocol だけで書いた軽量ボット（lib + bin）。統合テストと負荷試験
│   └── lodeframe/            # 本体。Audience を持ち、text / protocol / macros を re-export する facade
├── xtask/                    # datagen / bot（lodeframe-bot を呼ぶ）/ bench
└── examples/lobby/
```

利用者は `lodeframe` だけに依存する。util crate（AI 等）は必要になった時点で足す。

## スレッドと所有（D6, D16）

```
          tokio runtime (I/O)
  ┌─────────────────────────────────┐
  │ conn task ×N                    │   decode / encode / 圧縮 / 暗号化
  │   └ inbound  mpsc ──┐           │
  │   ┌ outbound mpsc ◀─┼───┐       │
  └───┼─────────────────┼───┼───────┘
      │                 ▼   │
  ┌───┴─────────────────────┴───┐
  │ Instance thread (20 TPS)    │  Instance を &mut で単独所有
  │  1. inbound を drain        │  イベントノードを発火
  │  2. spawn 結果を drain      │  ctx.spawn の then を実行
  │  3. tick（スケジューラ・物理）│
  │  4. 差分を outbound へ       │
  └─────────────────────────────┘
        ▲  Instance 間は message のみ
        ▼
  ┌─────────────────────────────┐
  │ Instance thread …           │
  └─────────────────────────────┘
```

- conn task は状態機械（Handshake → Status / Login → Configuration → Play）を持ち、Play 以降のパケットだけを Instance に渡す。
- ログイン前の async イベント（forwarding 検証、利用者の BAN 照会等）は conn task 側で await する。Instance には確定したプレイヤーだけが入る。利用者のフックは `Server::on_login`（複数・登録順）で、Profile の確定後・`LoginFinished` の前・在線の枠を取る前に走り、拒否（`Component` の理由）か Profile の差し替えを返す（[計画](../implementation/login-event-plan.md)、D42）。Velocity の転送は `login::velocity` がここで検証し、確定した UUID・名前・スキン・接続元を `Profile` として Instance に渡す（[計画](../implementation/velocity-forwarding-plan.md)、D39）。BungeeCord / BungeeGuard は handshake のアドレス欄を `login::bungeecord` / `login::bungeeguard` が読み、許可リストの照合には `net::serve` が渡す接続のソケットアドレスだけを使う（[計画](../implementation/bungeecord-forwarding-plan.md)、D40）。HAProxy の PROXY protocol は handshake より前の接続層で `net::serve` が読み（`net::ProxyProtocol`）、ヘッダが名乗るクライアントを peer・`Connection::proxied_addr`・`Profile.remote_addr`（転送が無いとき）に載せる（[計画](../implementation/haproxy-proxy-protocol-plan.md)、D41）。
- `ctx.spawn(fut).then(cb)`: fut は tokio で実行（`Instance::attach` で受け取った Handle）、結果は `Ctx` の channel に戻り、次 tick 冒頭で cb が `&mut Ctx` 付きで走る。`then_for(player, cb)` は退出済みなら呼ばない。cb は `Send` 不要で Instance のスレッドに残る（D33）。
- `ctx.after(delay).run(task)`: タスクは tick 数で数え、`World::tick` の順に「spawn 結果 → 開始時のタスク → tick 本体 → 終了時（`at_end`）のタスク → 遅延キュー」で走る。戻り値の `Next` で次回を決める。持ち主は全体とプレイヤー（`for_player`）で、プレイヤーの退出で止まる。同じ時点では走る tick・登録順（D34）。
- `Cooldown` は終わる tick だけを持つ値で、`Data` の key に入れる。現在の tick は `ctx.now()`、長さは `try_use(now, delay)` で渡す（D36）。
- 利用者データ: `Data`（型付きの値の表）を `Player` と `Ctx` が持つ。`Key<T>`（名前つき）か型そのものを key に引き、型が合わなければ `None`。プレイヤーのデータは退出で消え、その人の `PlayerLeaveEvent` を処理している間だけ `ctx.leaving_data(id)` で読める（D35）。
- 在線人数と最大人数は、`Server` が持つ `AtomicU32` の数だけを conn task が数える（ログイン直後に compare-exchange、接続の終わりで戻す）。サーバー一覧は Instance を待たずにこれで答える。Instance の状態は共有しない（D37）。
- keep alive は conn task が id と時刻を持ち、応答の id を照らす。間違い・頼まれていない応答・応答の無いまま `timeout` は理由つきで切る。計測した往復は `Message::Latency` で Instance に渡り、`ctx.ping` で読める（D37）。
- plugin message: login は `login::Queries::ask`（id を照らし、タイムアウトは `TimedOut`。`Server` にはつないでいない）。configuration でクライアントが送った分は `Message::Join` に載り、入室後に `PluginMessageEvent` になる（brand は `ctx.client_brand`）。play の受信も同じイベント、送信は `ctx.send_plugin_message`（D38）。
- 停止: `RunningServer::shutdown` が受付を止め、Instance に `Instance::shutdown` を頼む。`World` は `ShutdownEvent` を発火してから全員に理由つきの Disconnect を送り、退出させる。接続が書き終えるまで `shutdown_timeout` まで待つ。`Server::run` は Ctrl-C でこれを呼ぶ（D37）。
- グローバル可変シングルトンは置かない。サーバー全体の共有物（レジストリ等）は起動時に確定し不変で共有する。

## テストハーネス（D21）

- tick ループは実時間を直接読まず、時計を差し替え可能にする。本番は 50ms 周期で回し、`TestEnv` は `tick(n)` で同期的に進める。
- `TestEnv` では conn task の代わりに `FakePlayer` が Instance の inbound channel に直接パケットを入れ、outbound を記録する。エンコード層を通すかどうかは選択可能にする（既定は通さない）。
- スケジューラは時計でなく tick 数で進むので、`tick(n)` でそのまま決定的に動く。
- `ctx.spawn` は `TestEnv` 内では時間を止めた current-thread runtime で実行し、`run_until_idle()` で完了させる（cb は次の `tick` で動く）。

## イベント（D9）

- `EventNode<C>` は木。ルート（サーバー全体）と Instance ごとのノードがあり、利用者は子ノードを付け外しする。
- ハンドラは `fn(&mut E, &mut Ctx)`。`World` は状態の `Ctx` と `EventNode<Ctx>` を並べて持つので、ハンドラは発火中の木に触れない（D31）。キャンセル可能イベントは `ev.cancel()` で既定動作を止める。
- プレイヤーは `PlayerId`（UUID と入室ごとの通し番号）で指す。退出した人のハンドルは何も指さない（D31）。
- 退出は `Ctx` が記録し、`World` が 1 回の処理の後にまとめて `PlayerLeaveEvent` を発火する。ハンドラの中で落ちた人の分も、そのハンドラが終わってから届く。
- 順序は、ノードの中ではハンドラの優先度（高いほど先、同じなら追加順）、兄弟の子ノードでは子の優先度。木全体の総順位は無い（D32）。
- ハンドラは回数・条件で失効でき、`ListenerId` で外せる。ノードは `only_if` / `only_for` のゲートで部分木ごと絞れ、`Bundle` でまとまりを配れる。
- 親のイベントは trait。`Event::parents` で `dyn Trait` として見せ、`on::<dyn Trait>` が受ける。
- ハンドラの中の `ctx.emit` と木への追加・削除は遅延キューに積まれ、`World` が `handle`・`tick` の終わりに順に反映する。同じ tick のうちに届くが、ハンドラは結果（キャンセルされたか）を受け取れない。追加したハンドラは次のイベントから効く。
- 発火は Instance スレッド上で同期実行。ハンドラ内のブロッキングは禁止（ドキュメントで明示）。

## プロトコルデータ（D3〜D5）

- 対象は最新リリース 1 本。版番号は着手時点の最新 stable で確定し、`lodeframe-protocol` の定数に置く。
- `mise run datagen [version]`（= `cargo xtask datagen`）: server.jar を取得 → data generator（`--reports` 等）を実行 → JSON からブロック状態・レジストリ・パケット ID の Rust コードを生成。server.jar と JSON 自体は commit しない。
- configuration フェーズは Known Packs で vanilla データの送信を省き、利用者が追加したレジストリだけ本体を送る。

## 版追従の手順

1. `mise run datagen <新版>` で生成物を差し替え
2. 差分のあるパケットを手書き分で修正（protocol の変更点は wiki.vg 後継資料と生成物の diff で確認）
3. ボット統合テストを通す
