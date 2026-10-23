# 体力・空腹・経験値・リスポーンの実装計画（M2-15b）

> **Status**: 実装済み — 2026-10-23

要件: REQ-PLAYER-001（残り）。決定: D47、D48（[decisions.md](../decisions.md)）。M2-15 の後半（前半は [player-state-plan.md](player-state-plan.md)）。
`lodeframe-protocol` と `lodeframe` にまたがり、公開 API が増えるので、コードの前にここで形を決める。

ダメージの仕組み（攻撃・落下・窒息）は無い（REQ-ENT 系）。**体力は利用者が直接設定する**。

## 使い方

```rust
use lodeframe::{player::{Experience, RespawnPoint}, text, world::{Ctx, PlayerJoinEvent}};

ctx.set_health(player, 10.0);                 // 画面のハート
ctx.set_food(player, 6);                      // 6 以下ではダッシュできない（クライアントが判断）
ctx.set_saturation(player, 2.0);
ctx.set_experience(player, Experience::new(5, 0.5));   // レベル 5、次まで半分
ctx.set_experience(player, Experience::from_points(120));

ctx.set_respawn_point(player, Some(RespawnPoint::new(Vec3::new(0.5, -60.0, 0.5))));

// 体力が 0 以下になると死ぬ。死亡画面が開く
ctx.set_health(player, 0.0);
ctx.kill(player, &text!("<red>fell into the void"));      // 死亡メッセージを指定する

// 死亡画面の「リスポーン」を押すと PlayerRespawnEvent が出て、既定ではリスポーン地点へ戻る
world.events_mut().on(|e: &mut PlayerRespawnEvent, ctx: &mut Ctx| { e.position = lobby; });
ctx.respawn(player);                          // サーバー側から戻す（イベントは出さない）
```

## 判断

| 判断 | 選んだ案 | 選ばなかった案 |
|---|---|---|
| 死ぬ契機（利用者に確認。2026-10-23） | **体力が 0 以下になったら死ぬ**（`set_health(0.0)`）+ 明示の `kill(player, &message)`。`PlayerDeathEvent` を出す | 明示の `kill` だけ（体力 0 で死なない状態ができる） |
| リスポーンでの復元（利用者に確認） | **体力・空腹・満腹度を満タンに、経験値は保つ**（Minestom と同じ）。`PlayerRespawnEvent` のフィールドを書き換えて変えられる | vanilla と同じに経験値も 0 / 何も戻さない |
| API の形 | `Ctx` のフラットなメソッドと値型（D46・D47 と同じ） | プレイヤーごとのビュー |
| 死んだプレイヤーを他の人に見せるか | **見せない**。死んだ時点で他の人の画面から消し、リスポーンで出す。死亡アニメーションは持たない（エンティティのイベント。REQ-ENT 側） | `EntityEvent`（死亡）を送って一定時間後に消す（時間を持つ必要があり、エンティティの設計と一緒に） |
| 死亡画面を出すか | `Ctx::show_death_screen`（公開フィールド、既定 `true`）。`false` ならクライアントは画面を出さず、すぐリスポーンを要求する（vanilla の immediate respawn）。入室前に設定する | プレイヤーごと・実行中の変更（要望が出たら） |

## 1. 体力・空腹・満腹度

- `ctx.health(p) -> Option<f32>`・`ctx.set_health(p, f32)`: **0.0〜20.0 に丸める**（最大体力の属性は無い。属性の更新パケットは REQ-ENT 側）。NaN は無視。0 以下は**死ぬ**（下）。死んでいる間は何もしない（リスポーンしてから）
- `ctx.food(p) -> Option<i32>`・`ctx.set_food(p, i32)`: 0〜20 に丸める
- `ctx.saturation(p) -> Option<f32>`・`ctx.set_saturation(p, f32)`: 0.0〜**今の空腹値**に丸める（vanilla と同じ）。空腹値を下げたら、満腹度もその値まで下がる
- どれも変えると `SetHealth`（体力・空腹・満腹度の 3 つをまとめて）を本人へ。同じ値なら送らない
- 自動の回復・飢え・消耗は**しない**（vanilla の規則は持たない）

## 2. 経験値

- `Experience { level: i32, progress: f32, total: i32 }`（`level` は 0 以上、`progress` は 0.0〜1.0 で次のレベルまでの割合、`total` は通算の点数）
- `Experience::new(level, progress)`: `total` はそのレベルまでの点数に `progress` ぶんを足して求める。`Experience::from_points(points)`: 点数から `level` と `progress` を求める。どちらも vanilla の式（レベル 0〜15 は 2L+7、16〜30 は 5L−38、31 以上は 9L−158 点で次へ）
- `ctx.experience(p) -> Option<Experience>`・`ctx.set_experience(p, Experience)`: 範囲に丸めて（負・NaN は 0）`SetExperience`（進み・レベル・通算の順）を本人へ

## 3. 死亡

- `ctx.kill(p, &Component) -> bool`: 生きていれば死なせて `true`。体力 0 を `SetHealth` で、死亡メッセージを `PlayerCombatKill` で本人へ送る（**死亡画面を開くのは `PlayerCombatKill`** で、`SetHealth` だけでは開かない）。死んだ場所を覚える（`last_death`。次のリスポーンで `SpawnInfo.last_death` に載せる）。他の人の画面からは消す
- `set_health(p, <= 0)` は既定のメッセージ（`death.attack.generic` に名前）で `kill` する
- `PlayerDeathEvent { player, message }`: 死んだあとに `ctx.emit` で出す（呼んだハンドラが終わってから。**キャンセルは無い**）
- 死んでいる間: 位置・向きの申告・ブロックの破壊と設置・体力などの変更は捨てる。チャットは通す（vanilla と同じ）
- `ctx.is_dead(p) -> bool`・`ctx.last_death(p) -> Option<Vec3>`

## 4. リスポーン

- `RespawnPoint { position: Vec3, yaw: f32, pitch: f32 }`（`RespawnPoint::new(position)` は向き 0）。`ctx.respawn_point(p) -> Option<RespawnPoint>`（設定した点、無ければ `Ctx::spawn`）・`ctx.set_respawn_point(p, Option<RespawnPoint>)`（`None` で既定に戻す）
- クライアントが死亡画面から `ClientCommand`（`PERFORM_RESPAWN`）を送ると、死んでいれば `PlayerRespawnEvent { player, position, yaw, pitch, health, food, saturation }`（既定はリスポーン地点と満タン）を出す。**キャンセルすると死んだまま**（クライアントは再度押せる）。キャンセルされなければ、フィールドの値でリスポーンする。生きている人の要求は無視する
- `ctx.respawn(p) -> bool`: サーバー側から、リスポーン地点と満タンで戻す（イベントは出さない）。死んでいなければ `false`
- リスポーンの中身: 状態を戻し、`Respawn`（`SpawnInfo` にゲームモードと `last_death`）でクライアントの世界を作り直させ、位置（`PlayerPosition`。確認 ID を付け、確認までの申告は捨てる）・「地形を読み込み中」の解除（`GameEvent`）・チャンクを**最初から送り直す**（`ChunkTracker` を作り直す）・体力・経験値・能力・権限レベルを送る。他の人には視界の更新で出す

## 5. プロトコル

26.3 の `server.jar` を `javap -c -p` で読んで確定した（2026-10-23）。

| パケット | 中身 |
|---|---|
| `SetHealth`（clientbound） | `health: f32`・`food: VarInt`・`saturation: f32` |
| `SetExperience` | `progress: f32`・`level: VarInt`・`total: VarInt`（この順。クラスのコンストラクタの引数順とは違う） |
| `PlayerCombatKill` | `player_id: VarInt`（エンティティ id）・`message: Component` |
| `Respawn` | `SpawnInfo`（`Login` と同じ形）・`data_kept: u8`（1 = 属性、2 = エンティティのデータ。0 を送る） |
| `ClientCommand`（serverbound） | `action: VarInt`（0 = `PERFORM_RESPAWN`、1 = 統計の要求、2 = ゲームルールの要求）。リスポーンだけ扱う |
| `GameEvent` のイベント 11 | `IMMEDIATE_RESPAWN`、値 1.0 = すぐ戻る |

## 6. ファイル

- `crates/lodeframe-protocol/src/packets.rs`: 上のパケット
- `crates/lodeframe/src/player.rs`: `Experience`・`RespawnPoint`・`PlayerDeathEvent`・`PlayerRespawnEvent`
- `crates/lodeframe/src/world/vitals.rs`: `Ctx` のメソッド（`world.rs` の非公開のフィールドに触れるので `world` の子）
- `crates/lodeframe/src/world.rs`: `Player` にフィールド、`on_packet` の `ClientCommand`、入室時の `show_death_screen`、`SpawnInfo` の共通化、視界の更新で死者を除く、死んでいる間の捨て方

## 利用者に見える変更（v0.x）

- 追加: 上の型・メソッド・イベント・`Ctx::show_death_screen`
- 変更: 入室時の `Login` の `show_death_screen` が `Ctx::show_death_screen` になる（既定は今までと同じ `true`）。入室時に `SetHealth`・`SetExperience` を送る（満タンと 0。`Login`・`PlayerPosition` の順は変わらない）
- 新しい依存は無い

## やらないこと

| 項目 | なぜ外したか | 受け入れ先 |
|---|---|---|
| ダメージ・回復・飢え・窒息・落下 | ダメージの仕組みが無い。vanilla の規則は持たない | REQ-ENT 系 |
| 最大体力・吸収・属性（`UpdateAttributes`） | エンティティの属性の設計が要る | REQ-ENT-009 など |
| 死亡アニメーション・倒れたプレイヤーの見た目 | エンティティのイベントと一緒に | REQ-ENT 側 |
| 死亡画面の「タイトル画面へ」・統計の要求・ゲームルールの要求 | `ClientCommand` のリスポーン以外 | 要望が出たとき |
| プレイヤーごとの死亡画面の設定・実行中の変更 | 世界の設定で足りる | 要望が出たとき |
| ベッド・リスポーンアンカー・羅針盤（`SetDefaultSpawnPosition`） | リスポーン地点は利用者が決める | 要望が出たとき |
| 別の世界・次元へのリスポーン | 複数 Instance が v0.3 | REQ-WORLD-005 |

## テスト

- protocol: 各パケットのバイト列（`SetHealth`・`SetExperience` の順序・`PlayerCombatKill`・`Respawn`・`ClientCommand` の復号）
- ハーネス（`crates/lodeframe/tests/player_vitals.rs`）:
  - 入室: `SetHealth` が 20・20・5.0、`SetExperience` が 0。`show_death_screen = false` で `GameEvent` 11
  - 体力・空腹・満腹度: 設定と読み出し・丸め（0〜20、NaN 無視、満腹度は空腹値まで）・同じ値は送らない・3 つがまとまって届く
  - 経験値: `new`・`from_points` の境界（レベル 0→1 は 7、15→16 は 37、30→31 は 112）・丸め・パケットの順
  - 死亡: `set_health(0.0)` が `SetHealth` と `PlayerCombatKill`（既定のメッセージ）を送り、`kill` はメッセージを使う。2 回目は `false`。他の人の画面から消える。死んでいる間の申告・ブロック操作・体力の変更は捨てる。`PlayerDeathEvent`
  - リスポーン: クライアントの要求でイベントが出て既定の地点・満タンで戻る／ハンドラが位置と体力を変えられる／キャンセルすると死んだまま／生きている人の要求は無視／`respawn` はイベントなし／リスポーン地点の設定と既定／`Respawn` の `last_death` と `game_mode`／チャンクが送り直される／他の人に再び見える／確認前の古い位置は捨てる／経験値は保つ
  - 退室した人・古い `PlayerId` には何もしない
- `mise run check`・`mise run msrv`・`mise run lint:license`

## 実装の順

1. protocol のパケットとバイト列のテスト
2. `player.rs` の型、`Player` のフィールド、入室時の送信、`SpawnInfo` の共通化
3. `world/vitals.rs`（体力・経験値・死亡・リスポーン）、視界から死者を除く、`ClientCommand`、死んでいる間の捨て方
4. ハーネスのテスト（と example）
5. 文書（requirements・minestom-parity・architecture・backlog・decisions D48）。PR

## 実装の結果

- 計画どおり。`lodeframe::player::{Experience, RespawnPoint, PlayerDeathEvent, PlayerRespawnEvent, MAX_HEALTH, MAX_FOOD, RESPAWN_SATURATION}`、`Ctx` の `health`・`set_health`・`food`・`set_food`・`saturation`・`set_saturation`・`experience`・`set_experience`・`is_dead`・`last_death`・`respawn_point`・`set_respawn_point`・`kill`・`respawn`、`Ctx::show_death_screen`
- 計画から足したこと
  - 死んでいる間は、経験値・空腹・満腹度の変更も捨てる（体力と同じ。リスポーンしてから）
  - `PlayerRespawnEvent` の値が壊れていても（NaN・体力 0 以下）リスポーンで直す（体力は満タン、位置は `Ctx::spawn`、満腹度は 5.0）
  - リスポーンの `Respawn` の後に、`show_death_screen = false` なら `GameEvent`（イベント 11）を再び送る（クライアントはこの状態をプレイヤーごとに持つため）
  - `SpawnInfo` の組み立てを `spawn_info` に切り出し、入室とリスポーンで共用
- 検証: protocol のバイト列テスト 1 件、ハーネスのテスト 14 件（入室・体力と空腹と満腹度の丸め・経験値の式と順序・死亡と死亡メッセージ・死んでいる間の捨て方・リスポーンの要求とイベント・視界・確認前の申告・ハンドラの変更とキャンセル・生きている人の要求・`respawn`・リスポーン地点・退室済み・死亡画面なし）、既存の全テスト、`mise run check`・`msrv`・`lint:license`
- 制約: パケットの形は 26.3 の `server.jar` を `javap` で読んだものだが、**実クライアントでの表示（死亡画面・リスポーン後の世界・ハート・経験値バー）は未確認**。最大体力は 20 に固定（属性の更新が無い）。死亡アニメーションは無い
