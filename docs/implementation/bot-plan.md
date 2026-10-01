# 自前ボットと統合テスト 実装計画（M1-16）

> **Status**: 実装済み — 2026-10-01（`mise run check` と、実サーバー（release）へのボット 10 体の接続・5 秒の歩行で確認）

要件: REQ-INFRA-002。決定: D15・D27（[decisions.md](../decisions.md)）。
新しい crate（`lodeframe-bot`）と、本体の統合テスト、`xtask bot` にまたがるため、ここに設計を置く。

## 調べたこと

クライアント側の流れは、サーバー側の実装（`login.rs`・`configuration.rs`・`net.rs`）と既存のパケット型から読み取った。パケット型はサーバーが使うものと同じで、26.3 の実 codec との一致は M1-11〜15 の確認で取れている。

| 事実 | ボットでの扱い |
|---|---|
| 入室は Handshake（`Intention`、次の状態 2）→ Login（`Hello` → `LoginCompression`（しきい値）→ `LoginFinished` → `LoginAcknowledged`）→ Configuration → Play | しきい値を受けたら、読む側と書く側の両方に圧縮を入れる |
| Configuration は `UpdateEnabledFeatures`・`ClientboundKnownPacks`（サーバー）→ `ServerboundKnownPacks`（クライアント。サーバーと同じ `minecraft:core` を返さないと切られる）→ registry・tags → `FinishConfiguration` → `AckFinishConfiguration` | known packs は受けた内容をそのまま返す。registry と tags は読み捨てる |
| Play の最初は `Login`、`PlayerPosition`、チャンクのバッチ。keepalive は 15 秒ごとに来て、答えないと切られる | 受信ループの中で keepalive に答える。チャンクは捨てる |
| 受け取る側に `Decode` が無いパケットがある（`DisguisedChat` は `Component` の decode が無い） | ボット側で `Nbt` を使う受信用の型を持ち、平文に畳んで読む。protocol に `Component` の decode は足さない |
| サーバーは出力用のチャンネルが溢れたプレイヤーを切る（`OUTBOX`） | ボットは受信を止めない。負荷試験でも常に読む |

## 設計

### crate `lodeframe-bot`（D27）

lib + bin。依存は `lodeframe-protocol` と tokio・tracing だけ。**本体（`lodeframe`）には依存しない**（ボットは protocol だけで書く、D15）。

- `Bot::connect(addr, name)`: 入室から Play の最初の `Login` と `PlayerPosition` までを済ませる。entity id と位置を持つ
- `bot.recv()` / `bot.recv_until(timeout, f)`: 次のパケット（`Frame`: id と `decode::<P>()`）を返す。keepalive には中で答える。タイムアウトは必須（待ち続けて CI を止めない）
- `bot.move_to(pos)` / `chat(text)` / `dig(pos)` / `place(pos, face)`: 操作。`dig` と `place` は sequence を自動で振り、返す
- `ChatLine { name, text }`: 受け取った `DisguisedChat` を読んだもの
- `bot.wander(index, duration)`: 負荷用。`index` はボットごとの位相と建てる列を分ける。円を描いて歩き、ときどきチャットし、自分の足元の脇に置いて壊す。受信を止めない

bin は `lodeframe-bot --addr <addr> --count <N> --seconds <S>`。N 体を少しずつずらして接続し、S 秒歩かせ、接続できた数・できなかった数・受け取ったパケット数を出す。引数の解析は標準ライブラリで足りるので `clap` は入れない。

### `xtask bot`

`cargo run -p lodeframe-bot --release -- <引数>` を呼ぶだけの薄いラッパー。計画（v0.1-plan）の `xtask bot` を満たす。

### 統合テスト（`crates/lodeframe/tests/bot.rs`）

`lodeframe-bot` を dev-dependency にする（本体は bot に依存されない向きなので循環しない）。テストがサーバーを同じプロセスの空きポートで立てる。組み立ては `examples/offline_login.rs` と同じ（接続、login、configuration、play）。**この重複は意図的**: M1-17 の lobby を書くときに、起動 API を設計する材料にする（backlog の持ち越しに入れる）。

`mise run test`（と既存の CI ジョブ）で走る。view distance は 2 にして debug ビルドでも速くする。

シナリオ（ボット 2 体、各待ちに 10 秒のタイムアウト）:

1. 2 体が入室し、互いの `PlayerInfoAdd` と `AddEntity` を受ける
2. 片方が動くと、もう片方が `EntityPositionSync`（その entity id と位置）を受ける
3. 片方がチャットすると、両方が `DisguisedChat`（名前と本文が合う）を受ける
4. 片方が地面に置くと、両方が `BlockUpdate`（石）を、本人が `BlockChangedAck` を受ける
5. 片方が壊すと、両方が `BlockUpdate`（空気）を受ける
6. 片方が切断すると、もう片方が `RemoveEntities` と `PlayerInfoRemove` を受ける

### v0.1 でやらなかったこと（後で対応する）

意図して外したものを、理由と受け皿つきで置く。`backlog.md` の「持ち越し」にも同じ項目がある。

| 項目 | 今の動き | なぜ外したか | 受け皿 |
|---|---|---|---|
| 負荷の**計測**（TPS・RSS・遅延） | 接続数とパケット数を出すだけ | 計測条件（Minestom と同条件）の設計が M1-18 の仕事 | M1-18 |
| vanilla や他実装のサーバーへの接続 | lodeframe のサーバーだけを相手にする | ログインと configuration を lodeframe に合わせている（`ClientInformation` を送らない等）。版追従の検出は、vanilla が使えるようになってから | v0.2 以降。roadmap の「版追従」のリスクに対応 |
| online mode・暗号化 | offline login のみ | サーバー側が未対応（REQ-AUTH-001） | REQ-AUTH-001 |
| ボットの世界の保持（チャンク・他プレイヤー・衝突） | 受け取ったチャンクは捨てる。位置は自分で決める | 検証に要るのはパケットだけ | 要る時が来たら（バグの再現など） |
| 再接続・レート制御・失敗の再試行 | 失敗は数えるだけ | 負荷試験の目的に要らない | M1-18 で必要なら |
| N 体の配信が O(N²)（全員に全員の動きを送る） | そのまま | v0.1 は視界で絞らない | REQ-ENT-002 の視界。M1-18 で計測 |
| `ChunkBatchReceived`・`AcceptTeleportation`・`ClientInformation` の送信 | 送らない | サーバーが読み捨てるため。サーバーが使うようになったら要る | 該当の機能の単位で |

## テスト

- bot の単体テスト: 圧縮の有無でフレームを往復する、keepalive に答える、`ChatLine` が平文と styled の両方を読める（TCP を使わず `tokio::io::duplex` で）
- 統合テスト: 上のシナリオ 1〜6
- 負荷用の `wander`: 統合テストから 3 体を 2 秒動かし、3 体とも切られずに終わり、受け取ったパケットがあることを確かめる（bin は `CARGO_BIN_EXE_*` が bot crate のテストでしか使えず、本体のテストから起動できないので、ライブラリ側を動かす。bin は動作確認で試す）

## 動作確認

- `mise run check`
- `cargo run --release -p lodeframe --example offline_login 127.0.0.1:25599` を立てて `cargo xtask bot --addr 127.0.0.1:25599 --count 10 --seconds 5`

## 動かして分かったこと

release のサーバー（view distance 8）にボット 10 体を 20ms 間隔で入れると、全員が接続して 5 秒歩き、`connected 10, failed 0, packets read 17119`、全体で約 12 秒かかった。サーバーのログには `can't keep up, skipping ticks behind_ms=6310` が出た。入室のたびに全チャンク（289 個）の encode が instance スレッドを塞ぐためで、M1-13 の確認で分かっていた問題（チャンクを tick ごとに少しずつ送る）が N 体の入室で目に見える形になった。直す単位は backlog の持ち越しに入れた。
