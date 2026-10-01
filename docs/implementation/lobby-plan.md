# lobby と起動 API 実装計画（M1-17）

> **Status**: 実装済み・実機確認済み（2 クライアントで挨拶・建築の制限・チャットの断りが効く）— 2026-10-01。実機確認は `cargo run -p lobby`

要件: REQ-API-001・REQ-API-002 の使い心地の評価（v0.1-plan の M1-17）。決定: D9・D16・D28・D29（[decisions.md](../decisions.md)）。
本体に起動 API（`Server`）と入退室のイベントを足し、`examples/lobby` をそれだけで書く。

## 進め方

lobby の `main` を**先に**理想の形で書き、それが通る最小の API を足す。いまの起動（TCP の待ち受け、login、configuration、play の組み立て）は `examples/offline_login.rs`・`chat.rs`・`blocks.rs` と `tests/bot.rs` で 40 行前後が 4 か所に重複している（M1-16 で意図的に残した）。

```rust
#[tokio::main]
async fn main() -> std::io::Result<()> {
    Server::new("127.0.0.1:25565")
        .motd("lodeframe lobby")
        .run(lobby)
        .await
}

fn lobby(registries: &Registries) -> World<FlatGenerator> {
    let mut world = World::new(registries, FlatGenerator::default());
    let events = world.events_mut();
    events.on(|e: &mut PlayerJoinEvent, world: &mut World<FlatGenerator>| { .. });
    // ...
    world
}
```

## 設計

### 起動 API（`lodeframe::server`、D28）

- `Server::new(addr)`、`.motd(..)`。ほかの設定は持たない
- `server.start(world).await -> io::Result<RunningServer>`: 待ち受けを始めて返す。テストが `addr()`（空きポートの実際の番号）を取り、`stop()` で止める
- `server.run(world).await`: `start` して、待ち受けが終わるまで待つ
- `world` は `FnOnce(&Registries) -> World<L> + Send + 'static`。instance のスレッドの上で呼ばれる（`World` はイベントノードを持ち `Send` ではないため。D6）
- 接続ごとの組み立て（status の応答、offline login（しきい値 256）、configuration、play）は中に閉じる。接続の失敗は今までどおり、その接続だけを落とす

### 入退室イベント

- `PlayerJoinEvent { player, name }`: 入室の通知が他に出たあと、**チャンクを送る前**に発火する（入室が遅い間も挨拶が出せる。チャンク送信の失敗で `leave` になっても、入室と退室の対が崩れない）
- `PlayerLeaveEvent { player, name }`: 退出の通知が他に出たあとに発火する
- どちらもキャンセルできない。入室の拒否はログイン前の async イベント（REQ-API-003、D16）の仕事

### lobby（`examples/lobby`）

- 入室: 本人へ「ようこそ」、全員へ「+ 名前」。退出: 全員へ「- 名前」
- 建築の制限: 地面（y ≦ -61。草・土・岩盤）は壊せず、置けない。置けるのはスポーンから水平に 16 ブロックの内側で、y が -60〜-50 の間だけ
- チャット: 先頭が `.` の行は断り、ヒントを本人へ返す。ほかは本文を白にして通す

### 評価（D29）

書いて分かったことを、`decisions.md` の D29 に 1 行ずつ記録する。**直さないものは直さない理由と受け皿を付ける**。書く前に分かっているもの:

| 問題 | 今 | v0.2 の方向 |
|---|---|---|
| ハンドラの文脈の型が `&mut World<L>`（`L` は具体的なローダ）で、クロージャごとに型注釈が要る | 注釈を書く | `ctx` を型消去したハンドルにする（`ctx.spawn` の導入と一緒に設計。REQ-API-003） |
| 発火中は `events` が空。ハンドラの中で足したハンドラは失われ、その中で起きたイベント（退出など）は届かない | ドキュメントに明記 | 追加を発火の後に反映する。ハンドラ内のイベントは発火後に回す |
| `Component` は 1 つの色・1 つのスタイルのテキストだけ。「名前は金、本文は白」のように混ぜられない | 1 行 1 スタイルにする | 子要素（REQ-TEXT-001 の v0.2）と `text!` |
| チャットの送信者名は chat type が書式を決め、`ChatEvent` からは変えられない | 名前の見た目は変えられない | 自前で整形して `broadcast` する経路か、送信者名の差し替えを `ChatEvent` に足す |
| プレイヤーを指すのは `Uuid` と名前だけ。メッセージ送信は `world.send_message(uuid, ..)` | そのまま | Audience（REQ-TEXT-003） |
| **書いて分かった**: 特定の 1 人を除いて送れない。入室の「+ 名前」は本人にも届く | 本人にも届くまま | Audience（除外つきの宛先） |
| **書いて分かった**: ハンドラの文脈の型注釈が `World<FlatGenerator>` の別名なしでは読めない（lobby は `type Lobby = ..` を置いた） | 別名で凌ぐ | 上の「文脈の型」と同じ。型消去したハンドルで消える |
| **解消した**: 起動の組み立て（`main` は 5 行、4 か所の重複は消えた） | `Server` | — |

### v0.1 でやらなかったこと（後で対応する）

意図して外したものを、理由と受け皿つきで置く。`backlog.md` の「持ち越し」にも同じ項目がある。

| 項目 | 今の動き | なぜ外したか | 受け皿 |
|---|---|---|---|
| 落下したらスポーンへ戻す（位置のイベントとテレポート） | しない。位置は同期するだけ | 位置のイベントとテレポートのパケットで API が増え、v0.1 を超える | v0.2 |
| `Server` の設定（圧縮しきい値、ポート以外の待ち受け、online mode、proxy 転送、複数 Instance、Ctrl-C での停止） | motd とアドレスだけ。しきい値は 256 固定 | 起動 API の形を決めるのが目的で、設定の面は要るものから足す | REQ-AUTH-001、REQ-WORLD-005 |
| サーバー一覧の在線人数 | 常に 0 | World から接続側へ人数を渡す経路が無い | v0.2 |
| 入室の拒否（ログイン前の async イベント） | できない | D16 の別枠。`ctx.spawn` と一緒 | REQ-API-003 |
| 評価の表のうち「直す」もの | 上の表のとおり | 設計が `ctx.spawn`・Audience・Component の完全版に依存する | v0.2 |

## テスト

- `Server` の単体テスト: `start` が空きポートで待ち受け、ボットが入れて、`stop` で接続が切れる。status の応答が motd を返す
- 入退室イベント（`tests/world.rs`、ハーネス）: 入室で本人以外にも通知が出たあとに発火する、退出で発火する、チャンクの送信前に出た挨拶が本人に先に届く
- `tests/bot.rs` を `Server::start` に置き換える（重複を消す）
- lobby: ボットで入室して挨拶を受ける、地面を壊せない、範囲外に置けない、範囲内に置ける、`.` 始まりのチャットが断られる（`examples/lobby` を lib + bin に分け、lib の `lobby()` を `main` と `examples/lobby/tests/` の両方から使う）

## 動作確認

- `mise run check`
- `cargo run -p lobby` を立て、ボット 5 体で `cargo xtask bot --count 5 --seconds 5`
- 実機: 2 クライアントで挨拶・建築の制限・チャットの断りが効く
