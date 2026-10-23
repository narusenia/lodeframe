# プレイヤーの状態の実装計画（M2-15a）

> **Status**: 実装済み — 2026-10-23

要件: REQ-PLAYER-001。決定: D6・D16・D21・D31・D35・D46（[decisions.md](../decisions.md)）。
`lodeframe-protocol`（パケット）と `lodeframe`（本体）にまたがり、公開 API が増えるので、コードの前にここで形を決める。

M2-15 は大きいので **2 つに分けた**（利用者の判断、下の表）。

| 単位 | 中身 |
|---|---|
| **M2-15a**（この計画書） | ゲームモード（要求・変更のイベント）・能力・権限レベル・テレポート（確認 ID）・kick・位置の読み取り |
| M2-15b（別の計画書） | 体力・空腹・経験値・リスポーン（地点・死亡画面・死亡地点・イベント） |

コマンド（M2-23）が要る権限レベルと、lobby の落下復帰（テレポート）を先に使えるようにするため。

## 使い方

```rust
use lodeframe::{player::{GameMode, Abilities, Teleport}, world::{Ctx, PlayerJoinEvent}};

// 入室したときのゲームモードは World の設定（既定は creative。今までと同じ）
world.default_game_mode = GameMode::Adventure;

// 実行中の変更。クライアントの操作（飛行・体力表示）が変わり、全員のタブリストにも反映される
ctx.set_game_mode(player, GameMode::Survival);

// 能力。ゲームモードを変えると、能力はそのモードの既定に戻る（速度は保つ）
ctx.set_abilities(player, Abilities { allow_flying: true, ..ctx.abilities(player).unwrap() });

// 権限レベル（op 0〜4）。クライアントの F3+F4 やコマンド画面が見える範囲が変わる
ctx.set_permission_level(player, 2);

// テレポート。クライアントの確認（AcceptTeleportation）まで、その人の古い位置の申告は無視する
ctx.teleport(player, Teleport::new(Vec3::new(0.5, -60.0, 0.5)));
ctx.teleport(player, Teleport::new(Vec3::new(0.0, 5.0, 0.0)).relative(Relative::X | Relative::Y | Relative::Z));

ctx.kick(player, &text!("<red>You were removed"));
let at = ctx.position(player);
```

## 判断

| 判断 | 選んだ案 | 選ばなかった案 |
|---|---|---|
| API の形 | `Ctx` のフラットなメソッドと、小さな値型（`GameMode`・`Abilities`・`Teleport`）。D46 と同じ | プレイヤーごとのビュー `ctx.player(id).teleport(..)`（Audience で利用者が選ばなかった案と同じ理由: `&mut Ctx` を借り続ける） |
| F3+F4（利用者に確認。2026-10-23） | **要求のイベントだけ**。既定は何もしない。変えるなら、ハンドラが `ctx.set_game_mode` を呼ぶ | 権限レベル 2 以上なら既定で適用 / 常に適用（どちらも改造クライアントで誰でも変えられる事故がありうる。vanilla の規則を既定で持たない方針と合わない） |
| ブロック操作（利用者に確認） | **creative と survival は掘り始めで即破壊、adventure と spectator は破壊・設置とも拒否**して元の状態を送り返す（イベントも出さない）。survival の掘り時間・ツール・ドロップは v0.6（REQ-WORLD-015） | creative だけ即破壊 / ゲームモードで分けない |
| 分割（利用者に確認） | M2-15a と M2-15b | 1 PR のまま |
| 入室時のゲームモード | `Ctx::default_game_mode`（公開フィールド。既定 creative）。`Login` と最初のタブリストに載る | 入室の前にプレイヤーごとに決めるフック（`Server::on_login` の戻りに足す案。要望が出たら。入室後に `set_game_mode` で変えられる） |

## 1. ゲームモード

- `GameMode`（`Survival`・`Creative`・`Adventure`・`Spectator`。`id()`・`from_id()`）
- `ctx.game_mode(player) -> Option<GameMode>`、`ctx.set_game_mode(player, mode) -> bool`（変わったら `true`。同じなら何も送らない）
- 変えると: 本人に `GameEvent`（イベント 3、値 = モード）、**全員**に `PlayerInfoUpdate`（`UPDATE_GAME_MODE`）、本人に `PlayerAbilities`（能力はそのモードの既定へ。下）、そして `GameModeChangedEvent { player, from, to }` を発火（`ctx.emit`。呼んだハンドラが終わってから。ハンドラの外から呼んだときは次の tick）
- `GameModeRequestEvent { player, mode }`: クライアントの F3+F4（`ChangeGameMode`）。**キャンセルも既定の動作も無い**。ハンドラが許すなら `ctx.set_game_mode`
- 入室時の `Login` の `game_mode` と、最初に全員のタブリストに載る `PlayerInfo.game_mode` は、プレイヤーのモード（今は 1 固定）

## 2. 能力

- `Abilities { invulnerable, flying, allow_flying, instant_break, fly_speed, walk_speed }`（`fly_speed` の既定 0.05、`walk_speed` の既定 0.1。`walk_speed` は視野の広がりにも効く）
- `Abilities::for_mode(mode)`: survival・adventure は全部 `false`、creative は `allow_flying`・`instant_break`・`invulnerable`、spectator は `allow_flying`・`flying`・`invulnerable`（vanilla の `GameType.updatePlayerAbilities`）
- `ctx.abilities(player)`・`ctx.set_abilities(player, Abilities)`（`PlayerAbilities` を本人へ）。モードを変えると、速度は保ち、旗はそのモードの既定に戻る
- クライアントが二段ジャンプで飛行を切り替える（`ServerboundPlayerAbilities`）と `FlyingToggleEvent { player, flying, cancelled }`。**既定の動作**: `allow_flying` なら受け入れ、そうでなければ（またはキャンセルしたら）クライアントの状態を戻すために `PlayerAbilities` を送り返す。**飛行をやめる要求は、キャンセルしない限り能力がなくても受け入れる**（着地は止める理由がない）

## 3. 権限レベル

- `ctx.permission_level(player) -> Option<u8>`、`ctx.set_permission_level(player, level)`（0〜4。4 を超えたら 4）。入室時は 0
- クライアントへは `EntityEvent`（本人の entity id、イベント 24 + レベル）。入室時に必ず送る（vanilla と同じ）
- サーバー側では持つだけ。コマンドの可視性（M2-23）が `permission_level` を読む

## 4. テレポート

- `Teleport::new(Vec3)`（絶対）・`.yaw_pitch(yaw, pitch)`・`.relative(Relative)`（`Relative::X | Y | Z | YAW | PITCH`。立てた軸は今の値に足す）。向きを指定しなければクライアントの向きを保つ（相対の 0 を送る）。速度は 0
- `ctx.teleport(player, Teleport)`: 確認 ID（その人ごとの連番。入室時の 1 の次から）を付けて `PlayerPosition` を送り、**サーバー側の位置はすぐ新しい値にする**。他の人には次の tick の移動として届く（遠ければ既存の `EntityPositionSync`）。チャンクの出し入れ（`SetChunkCacheCenter`・`ForgetLevelChunk`・新しいチャンク）と視界（見える・見えない）も、移動と同じ経路で更新する
- **確認前の古い位置は無視する**: 確認 ID が未確認の間、その人の `MovePlayer*`（位置・向き）は捨てる。`AcceptTeleportation` が最後に送った ID なら再開。古い ID・知らない ID は無視。複数回送ったら最後の ID だけを待つ。入室時の 1 は**待たない**（vanilla は待つが、ハーネスとボットの既存のテストが確認を送らないため。範囲外）
- 確認が来ないまま止まる場合の再送は持たない（vanilla は 20 tick で再送。要望が出たら）

## 5. kick と位置の読み取り

- `ctx.kick(player, &Component)`: `Disconnect` を送り、その人を退室させる（`PlayerLeaveEvent` は通常どおり）。今の内部の `kick_with` を公開する
- `ctx.position(player) -> Option<Vec3>`・`ctx.rotation(player) -> Option<(f32, f32)>`（yaw, pitch）・`ctx.is_on_ground(player)`・`ctx.is_sneaking(player)`（`PlayerId` が古ければ `None` / `false`）。lobby の落下復帰は、スケジューラで `position` を見て `teleport` する

## 6. ブロック操作とゲームモード

- `player_action`（破壊）と `place`（設置）は、プレイヤーが adventure か spectator なら、イベントを出さず、その位置の今の状態を `BlockUpdate` で送り返し、`BlockChangedAck` を返す（既存の「拒否」の経路）
- creative と survival は今までどおり

## 7. プロトコル

26.3 の `server.jar` を `javap` で読んで確定した（2026-10-23）。

| パケット | 中身 |
|---|---|
| `PlayerAbilities`（clientbound） | フラグ 1 byte（1 無敵・2 飛行中・4 飛行を許す・8 即時破壊）、`fly_speed: f32`、`walk_speed: f32` |
| `ServerboundPlayerAbilities` | フラグ 1 byte（2 = 飛行中） |
| `ChangeGameMode`（serverbound） | モード `VarInt` |
| `EntityEvent`（clientbound） | `entity_id: i32`、`event: u8` |
| `PlayerInfoUpdate` の `UPDATE_GAME_MODE`（0x04） | action のビット 1 byte、人数、（UUID・モード `VarInt`）の並び。`PlayerInfoGameMode` として足す |
| `GameEvent` のイベント 3 | `CHANGE_GAME_MODE`、値はモードの id |
| `PlayerPosition` の `relative` | ビット: X 1・Y 2・Z 4・Y_ROT 8・X_ROT 16・DELTA_X 32・DELTA_Y 64・DELTA_Z 128・ROTATE_DELTA 256（既存の `relative: i32`） |

モードの id は survival 0・creative 1・adventure 2・spectator 3。

## 8. ファイル

- `crates/lodeframe-protocol/src/packets.rs`: 上のパケット
- `crates/lodeframe/src/player.rs`: `GameMode`・`Abilities`・`Teleport`・`Relative`・イベント 3 種
- `crates/lodeframe/src/world/state.rs`: `Ctx` のメソッド（`world.rs` の非公開のフィールドに触れるので `world` の子）
- `crates/lodeframe/src/world.rs`: `Player` にフィールド、`on_packet` の分岐、入室時の送信、ブロック操作のゲート、チャンク更新の共通化

## 利用者に見える変更（v0.x）

- 追加: 上の型・メソッド・イベント。`World::default_game_mode`
- 変更: 入室時に `PlayerAbilities` と権限レベルの `EntityEvent` が最後に増える（`Login`・`PlayerPosition` の順は変わらない）。adventure・spectator では破壊・設置ができない
- 新しい依存は無い

## やらないこと

| 項目 | なぜ外したか | 受け入れ先 |
|---|---|---|
| 体力・空腹・経験値・リスポーン | 分割 | M2-15b |
| 入室時のテレポートの確認待ち・確認が来ない場合の再送 | 既存のテストとボットが確認を送らない | 要望が出たとき |
| 移動の検証（速度・めり込み・チートの検出） | 位置はクライアントの申告を信じる（今までどおり）。利用者が `position` を見て `teleport` で戻せる | REQ-ENT-006（v0.5）の物理とは別に要望が出たとき |
| survival の掘り時間・ツール・ドロップ | 破壊時間の計算が要る | REQ-WORLD-015（v0.6） |
| プレイヤーごとの view distance | | REQ-WORLD-006（v0.3） |
| 観戦・カメラ | | REQ-UI-007（v0.7） |
| 入室前にプレイヤーごとのゲームモードを決めるフック | 入室後に `set_game_mode` で足りる | 要望が出たとき |

## テスト

- protocol: 各パケットのバイト列（`PlayerAbilities` のフラグと 2 つの float、`EntityEvent`、`PlayerInfoGameMode`、serverbound の復号）
- ハーネス（`crates/lodeframe/tests/player_state.rs`）:
  - 入室: 既定は creative（`Login` が 1、タブリストが 1）。`default_game_mode` を変えると `Login`・タブリストが変わる。入室の最後に `PlayerAbilities` とレベル 0 の `EntityEvent`
  - `set_game_mode`: 本人に `GameEvent`、全員に `PlayerInfoUpdate`、本人に能力、変更イベント。同じモードなら何も送らない。能力はモードの既定に戻り、速度は保つ
  - F3+F4: `GameModeRequestEvent` が届き、ハンドラが呼ぶまで変わらない
  - 能力: `set_abilities` と読み出し。飛行の切り替えは `allow_flying` のときだけ受け入れ、そうでなければ能力を送り返す。キャンセルも同じ
  - 権限レベル: `EntityEvent` が 24 + レベル。5 以上は 4 に丸める
  - テレポート: 絶対・相対（立てた軸だけ足す）・向きの保持・確認 ID の連番／確認前の `MovePlayer*` を無視する・古い ID の確認では再開しない・確認後は受け入れる／他の人に移動が届く／遠い先でチャンクが入れ替わる
  - kick: `Disconnect` が届き、`PlayerLeaveEvent` が出て、古い `PlayerId` は何もしない
  - 位置の読み取り: 移動のあと `position`・`rotation`・`is_on_ground`・`is_sneaking` が合う
  - ブロック操作: adventure・spectator は破壊・設置とも元に戻りイベントが出ない。survival・creative は壊せる
  - lobby の落下復帰: スケジューラで `position` を見て `teleport` で戻す
- `mise run check`・`mise run msrv`・`mise run lint:license`

## 実装の順

1. protocol のパケットとバイト列のテスト
2. `player.rs` の型、`Player` のフィールド、入室時の送信、チャンク更新の共通化
3. `world/state.rs` のメソッド、`on_packet` の分岐、ブロック操作のゲート
4. ハーネスのテストと example（`examples/` の既存か、lobby の落下復帰）
5. 文書（requirements・minestom-parity・architecture・backlog の分割・decisions D47）。PR

## 実装の結果

- 計画どおり。`lodeframe::player::{GameMode, Abilities, Teleport, Relative, GameModeRequestEvent, GameModeChangedEvent, FlyingToggleEvent}`、`Ctx` の `game_mode`・`set_game_mode`・`abilities`・`set_abilities`・`permission_level`・`set_permission_level`・`position`・`rotation`・`is_on_ground`・`is_sneaking`・`teleport`・`kick`、`Ctx::default_game_mode`
- 計画から足したこと
  - 飛行をやめる要求は能力がなくても受け入れる（上の「能力」）
  - `GameModeChangedEvent` の発火時期（上の「ゲームモード」）
  - `Teleport` の位置か向きが NaN・無限なら何もしない（クライアントに不正な値を送らない）
  - 内部の `kick`・`kick_with` は `disconnect`・`disconnect_with` に改名（`kick` を公開するため）
  - `Ctx::on_packet` の末尾のチャンク更新を `follow_chunks` に切り出し、テレポートと移動で共用
- 検証: protocol のバイト列テスト 1 件、ハーネスのテスト 16 件（入室・ゲームモード・F3+F4・能力・飛行・権限レベル・テレポートの絶対／相対／確認前の無視／連続／遠い先のチャンク／他の人への移動・kick・読み取り・ブロック操作のゲート・落下復帰）、既存の全テスト、`mise run check`・`msrv`・`lint:license`
- 制約: パケットの形は 26.3 の `server.jar` を `javap` で読んだものだが、**実クライアントでの表示（creative の飛行・F3+F4 の画面・権限レベル）は未確認**。入室時のテレポートの確認待ちと、確認が来ないときの再送は持たない
