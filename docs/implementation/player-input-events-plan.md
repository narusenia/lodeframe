# プレイヤーの移動・入力イベントの実装計画（M2-16a）

> **Status**: 実装済み — 2026-10-23

要件: REQ-API-005（前半）。決定: D16・D31・D47（[decisions.md](../decisions.md)）。
`lodeframe-protocol`・`lodeframe`（本体）にまたがり、公開 API が増え、`Message::Join` が変わるので、コードの前にここで形を決める。

M2-16 は 3 つに分けた（利用者の判断。2026-10-23）。

| 単位 | 中身 |
|---|---|
| **M2-16a**（この計画書） | 移動（キャンセルで位置を戻す・行き先の書き換え）・入力・ダッシュ・スニーク・tick・クライアント設定と言語・`PlayerLoaded`・腕振り、lobby の落下復帰 |
| M2-16b | 採掘の開始・中断・完了、ピック、アイテムの使用、持ち替え（スロット選択）、ドロップ、手の入れ替え（インベントリが無いのでスロット番号まで） |
| M2-16c | チャットの書式と chat type（D29 の (4)。設計判断がある） |

## 使い方

```rust
use lodeframe::player::{PlayerMoveEvent, PlayerSprintEvent, ClientSettingsEvent, Teleport};

// 落下したらスポーンへ戻す（lobby）
world.events_mut().on(|e: &mut PlayerMoveEvent, ctx: &mut Ctx| {
    if e.position.y < -70.0 {
        e.position = ctx.spawn;        // 行き先の書き換え。クライアントはテレポートで戻される
    }
});

// 立入禁止の線: キャンセルすると、クライアントは直前の位置へ戻される
world.events_mut().on(|e: &mut PlayerMoveEvent, _: &mut Ctx| {
    if e.position.x > 100.0 { e.cancel(); }
});

world.events_mut().on(|e: &mut PlayerSprintEvent, ctx: &mut Ctx| { /* ダッシュの開始・終了 */ });
world.events_mut().on(|e: &mut ClientSettingsEvent, ctx: &mut Ctx| { /* 言語・描画距離の変更 */ });
let language = ctx.locale(player);          // "ja_jp"
```

## 判断

| 判断 | 選んだ案 | 選ばなかった案 |
|---|---|---|
| 分割 | 3 つ（利用者に確認） | 2 つ / 1 つ |
| 移動のキャンセル | **テレポートで戻す**（M2-15a の確認 ID。確認が来るまで古い申告は捨てるので、キャンセルは 1 回の往復で済む） | 次の tick でまとめて戻す（キャンセルした結果が 1 tick 遅れる） |
| 行き先の書き換え | `PlayerMoveEvent` の `position`・`yaw`・`pitch` を書き換えると、**クライアントが送った値と違えばその値へテレポート**する（Minestom の `newPosition` と同じ） | 読み取り専用 + キャンセルだけ（立入禁止の線の内側へ押し戻すなどが `teleport` を別に呼ぶ形になる） |
| ダッシュの出どころ | `PlayerInput` のダッシュのビットだけ（スニークと同じ。`PlayerCommand` の `START_SPRINTING` も来るが、二重に数えない） | `PlayerCommand` の開始・終了（入力のビットと食い違いうる） |
| クライアント設定の持ち方 | 入室時から持てるよう `Message::Join` に載せる（configuration で来た `ClientInformation` を捨てずに渡す）。変更は `ClientSettingsEvent` | play で来た分だけ（入室直後の言語が分からない） |
| 腕振り | **イベントだけ**。他の人へ見せる（`SwingAnimation`）は持たない（中身にアイテムの部品が入る。確認が要る） | 受け取ったら全員へ中継する |

## 1. 移動

- `PlayerMoveEvent { player, from, position, yaw, pitch, on_ground, cancelled }`: クライアントの `MovePlayerPos`・`MovePlayerPosRot`・`MovePlayerRot` が**位置か向きを変えたときだけ**出す（変えない申告・`MovePlayerStatusOnly` は出さない）。`from` は今の位置。`position`・`yaw`・`pitch` は書き換えられる。`cancel()` できる
- 既定の動作: 変えなければクライアントの申告を受け入れる。**キャンセルしたら `from`（と今の向き）へテレポート**で戻す。書き換えたら**その値へテレポート**する。どちらも M2-15a のテレポート（確認 ID を付け、確認までの申告は捨てる）なので、戻している間の古い申告ではイベントも出ない
- 位置が NaN・無限の申告は捨てる（クライアントの不正。今までは信じていた）
- 死んでいる間とテレポートの確認待ちの間は、今までどおり申告を捨てる（イベントも出ない）
- **tick ごとにまとめない**: パケットごとに出す。リスナーが無ければ `EventNode::emit` は軽い（1 回の表引き）

## 2. 入力・ダッシュ・スニーク

- `Input { forward, backward, left, right, jump, sneak, sprint }`（`Copy`、`Default`）。`Input::from_bits`
- `PlayerInputEvent { player, input, previous }`: 入力が変わったとき（キーを押す・離す）。キャンセルは無い（クライアントの状態の通知）
- `PlayerSneakEvent { player, sneaking }`・`PlayerSprintEvent { player, sprinting }`: それぞれのビットが変わったとき。スニークは今までどおり、他の人への姿勢の配信をしてから出す
- `ctx.input(player) -> Option<Input>`・`ctx.is_sprinting(player)`（`is_sneaking` は既にある）

## 3. tick

- `PlayerTickEvent { player }`: **world の tick ごとに、いる人全員へ**（死んでいる人にも）。tick の最後（タスクの後）。リスナーが無ければ、人数ぶんの表引きだけ（500 人で数百 ns 台のはず。計測は M1-18 の枠で、問題になったら止める手段を足す）
- プレイヤーごとの繰り返しの処理は `ctx.after(..).for_player(..)` でも書ける。イベントは「全員に同じ処理を、ノードの木の中で」書きたい場合のため

## 4. クライアント設定と言語

- `ClientSettings { language, view_distance, chat_mode, chat_colors, skin_parts, main_hand, text_filtering, allow_listing, particles }`（言語は `ja_jp` のような文字列、描画距離はクライアントの設定値）。型: `ChatMode`（`Full`・`CommandsOnly`・`Hidden`）、`SkinParts`（ビット: マント 1・上着 2・左袖 4・右袖 8・左ズボン 16・右ズボン 32・帽子 64）、`MainHand`（`Left`・`Right`）、`ParticleStatus`（`All`・`Decreased`・`Minimal`）
- `ctx.client_settings(player) -> Option<&ClientSettings>`・`ctx.locale(player) -> Option<&str>`。クライアントが 1 度も送っていなければ、vanilla の既定（`en_us`・描画距離 2 など）
- `ClientSettingsEvent { player, settings, previous }`: play の間に設定が変わったとき（同じ値なら出さない）。入室時の設定では出さない（入室で読める）
- 入室時: configuration で来た最後の `ClientInformation` を `ClientSettings` に直して `Message::Join { client_settings: Option<ClientSettings> }` で渡す（configuration と play のパケットは別の型なので、運ぶ値は facade の型に統一した）。既存の `configuration::run`・`run_with` と `play::run_with` はそのままで、新しい `configuration::configure`（戻り値は `Configured { plugin_messages, client_settings }`）と `play::run_configured` が運ぶ
- 範囲外の値（描画距離が負など）はクライアントの申告として丸める（描画距離 2〜32）。言語は 16 文字まで（プロトコルの上限）

## 5. `PlayerLoaded`・腕振り

- `PlayerLoadedEvent { player }`: クライアントが世界の読み込みを終えた（`PlayerLoaded`。入室とリスポーンのあと）。通知のみ
- `ArmSwingEvent { player, cancelled }`: クライアントが腕を振った（`Punch`。空振り・ブロックを殴る）。キャンセルしても今は何も止まらない（既定の動作が無い）。**他の人へは見せない**

## 6. プロトコル

26.3 の `server.jar` を `javap -c -p` で読んで確定した（2026-10-23）。

| パケット | 中身 |
|---|---|
| `ClientInformation`（serverbound。configuration と play で同じ中身、id が違う） | 言語（文字列、16 まで）・描画距離（`i8`）・チャット（`VarInt`: 0 全部・1 コマンドのみ・2 隠す）・チャットの色（`bool`）・スキンの部品（`u8`）・利き手（`VarInt`: 0 左・1 右）・テキストの検閲（`bool`）・一覧に載せる（`bool`）・パーティクル（`VarInt`: 0 全部・1 減らす・2 最小） |
| `PlayerInput`（既存） | キーのビット（前 1・後 2・左 4・右 8・ジャンプ 0x10・スニーク 0x20・ダッシュ 0x40） |
| `PlayerLoaded`（既存） | 中身なし |
| `Punch`（serverbound） | 中身なし。腕振り（`PlayerAction`・`UseItem` に手があるのとは別） |

## 7. ファイル

- `crates/lodeframe-protocol/src/packets.rs`: `ClientInformation`（configuration・play）・`Punch`
- `crates/lodeframe/src/player.rs`: `Input`・`ClientSettings` と関連の型・イベント 7 種
- `crates/lodeframe/src/world.rs`: `World::on_packet` の移動・入力・`PlayerLoaded`・`Punch`・`ClientInformation`、`tick` の `PlayerTickEvent`、`Ctx::on_packet` の移動の本体を `apply_move` に切り出し、`Player` のフィールド（入力・設定）
- `crates/lodeframe/src/world/state.rs`: `input`・`is_sprinting`・`client_settings`・`locale`
- `crates/lodeframe/src/{configuration.rs, play.rs, server.rs, instance.rs, test_util.rs}`: 設定を Join まで運ぶ（`TestEnv::connect_with_client_settings`）
- `examples/lobby`: 落下復帰

## 利用者に見える変更（v0.x）

- 追加: 上の型・メソッド・イベント
- 変更: **`Message::Join` に `client_settings` が増える**（自前の Instance で `Message::Join` を組み立てるコードは足す。取り出すだけなら `..` で足りる）。configuration の中の不正な `ClientInformation` は、無視せず接続を切る（今までは中身を読まず捨てていた）。位置が NaN・無限の申告は捨てる
- 新しい依存は無い

## やらないこと

| 項目 | なぜ外したか | 受け入れ先 |
|---|---|---|
| 腕振りを他の人へ見せる | `SwingAnimation` の中身（アイテムの部品）の確認が要る | M2-17（エンティティの表示と一緒） |
| 採掘・ピック・アイテムの使用・持ち替え・ドロップ・手の入れ替え | 分割 | M2-16b |
| チャットの書式と chat type | 分割。設計判断がある | M2-16c |
| 飛行（エリトラ）・騎乗のジャンプ・睡眠・インベントリを開く（`PlayerCommand`） | 対応する仕組みが無い | 要望が出たとき |
| 移動の検証（速度・めり込み） | イベントで利用者が書ける | REQ-ENT-006（v0.5）とは別に要望が出たとき |
| `PlayerTickEvent` を止める手段（リスナーが無いときの省略） | まず計測 | 計測で問題になったら |

## テスト

- protocol: `ClientInformation`（configuration・play の id と同じ中身、往復と手で組んだバイト列）・`Punch`
- ハーネス（`crates/lodeframe/tests/player_input.rs`）:
  - 移動: 位置を変える申告でだけイベントが出る（`MovePlayerStatusOnly`・同じ位置は出ない）／変えなければ受け入れる／キャンセルで `from` へテレポート（`PlayerPosition` の確認 ID）して、確認までの古い申告ではイベントも出ない／書き換えるとその値へテレポート／向きだけ変える申告も扱う／NaN は捨てる／死んでいる間・確認待ちは出ない／他の人への配信は受け入れたときだけ
  - 入力: 変わったときだけ `PlayerInputEvent`（`previous` つき）／スニーク・ダッシュのイベントとビットの対応／スニークは姿勢が他の人へ届く／`input`・`is_sprinting`
  - tick: 全員に毎 tick 1 回／退室した人には出ない／死んでいる人にも出る
  - クライアント設定: 入室時から読める（Join で渡した設定。無ければ既定）／play で変わるとイベント（`previous` つき）・同じなら出ない／範囲外の値の丸め／`locale`
  - `PlayerLoadedEvent`・`ArmSwingEvent`
  - lobby: 落下したら移動のイベントでスポーンへ戻る
  - サーバー（`tests/server.rs`）: 実接続で configuration の `ClientInformation` が入室時の設定になる
- `mise run check`・`mise run msrv`・`mise run lint:license`

## 実装の順

1. protocol（`ClientInformation`・`Punch`）とバイト列のテスト
2. `Message::Join` と configuration・play・server・harness を通して設定を運ぶ
3. `player.rs` の型とイベント、`World::on_packet` の移動・入力・その他、`tick`
4. ハーネスのテストと lobby の落下復帰
5. 文書（requirements・minestom-parity・architecture・backlog・decisions D49）。PR

## 実装の結果

- 計画どおり。`lodeframe::player::{PlayerMoveEvent, PlayerInputEvent, PlayerSneakEvent, PlayerSprintEvent, PlayerTickEvent, ClientSettingsEvent, PlayerLoadedEvent, ArmSwingEvent, Input, ClientSettings, ChatMode, SkinParts, MainHand, ParticleStatus}`、`Ctx` の `input`・`is_sprinting`・`client_settings`・`locale`、`configuration::configure`・`Configured`、`play::run_configured`、`TestEnv::connect_with_client_settings`、lobby の落下復帰（`PlayerMoveEvent` でスポーンへ）
- 計画から変えたこと
  - 運ぶ値は `ClientInformation` ではなく `ClientSettings`（上の「クライアント設定と言語」）
  - 変更しなかった申告（位置も向きも同じ）は、今までどおり `on_ground` だけ受け取る
  - 移動の本体を `Ctx::apply_move`・`World::player_move` に分けた（`Ctx::on_packet` はテレポートの確認だけ）
  - `ArmSwingEvent` はキャンセルできるが、今は何も止まらない（既定の動作が無い）
- 検証: protocol のバイト列テスト 1 件、ハーネスのテスト 12 件（移動・キャンセル・書き換え・不正な値・死亡と確認待ち・配信・入力・tick・設定・読み込みと腕振り）、configuration のテスト 1 件、lobby のテスト 1 件、既存の全テスト、`mise run check`・`msrv`・`lint:license`
- 制約: 実クライアントでの動作（落下復帰の見た目、ダッシュ・スニークのイベント）は未確認。`PlayerTickEvent` は人数ぶんの表引きを毎 tick 行う（計測していない）
