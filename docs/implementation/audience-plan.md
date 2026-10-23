# Audience の実装計画（M2-14）

> **Status**: 実装済み — 2026-10-23

要件: REQ-TEXT-003。決定: D19・D20・D29 の (5)・D46（[decisions.md](../decisions.md)）。
`lodeframe-protocol`（パケットと生成データ）・`xtask`（datagen）・`lodeframe`（本体）にまたがり、公開 API が増えるので、コードの前にここで形を決める。

## 使い方

```rust
use lodeframe::{Everyone, Title, BossBar, Sound, text};

// 宛先は PlayerId・全員・グループ・1 人を除く、のどれでも同じメソッドに渡せる
ctx.send_message(player, &text!("<green>hello"));
ctx.send_message(Everyone.except(player), &text!("<yellow>{name} joined"));
ctx.send_action_bar(&lobby, &text!("<gray>waiting for players"));

ctx.show_title(Everyone, &Title::new(text!("<gold>Start!")).subtitle(text!("<gray>3 rounds"))
    .times(Duration::from_millis(500), Duration::from_secs(2), Duration::from_millis(500)));

let bar = ctx.create_bossbar(BossBar::new(text!("<red>Boss")).color(BossBarColor::Red));
ctx.show_bossbar(&arena, bar);
ctx.set_bossbar_progress(bar, 0.5);          // 見ている人の画面に反映される

ctx.play_sound(Everyone.except(player), &Sound::new(sound::ENTITY_PLAYER_LEVELUP).at(pos));
ctx.set_tab_list(Everyone, &text!("<gold>My server"), &text!("<gray>mc.example.com"));

// グループ。退室した人は全グループから自動で外れる
ctx.join_group("arena", player);
ctx.send_message(ctx.audience("arena"), &text!("<red>round 1"));
```

## 判断（利用者に確認した。2026-10-23）

| 判断 | 選んだ案 | 選ばなかった案 |
|---|---|---|
| API の形 | `trait Audience`（宛先の PlayerId を列挙する）を `PlayerId`・`Everyone`・`Group`・`Except` などが実装し、`Ctx` のメソッドが `impl Audience` を取る。既存の `ctx.send_message(player, ..)` がそのまま通る。1 回の encode を Ctx 側で保証できる | 束縛したビュー `ctx.to(aud).title(..)`（`&mut Ctx` を借り続け、既存の `send_message` と入口が二重になる） / Adventure 風の `aud.send_message(ctx, ..)`（宛先の型ごとに全メソッドを実装するか既定実装に頼り、合成のたびに書く） |
| グループと独自の audience | trait を外に開く。`Ctx` に key で `Group`（メンバー集合）を作れ、任意の audience も key で登録できる。退室した人は全 `Group` から自動で外れる | `Group` だけ（独自の audience が書けず要件が残る） / trait だけで登録は利用者（退室の掃除を毎回書き、key 登録の要件を満たさない） |
| BossBar の状態 | `Ctx` が持ち、`BossBarId` で操作する。閲覧者を `Ctx` が追跡し、更新は閲覧者だけに 1 回の encode で送る。退室で閲覧者から外す | 値型を利用者が持つ（更新を届けるには閲覧者の表が結局 `Ctx` に要る） |

## 1. 宛先（`audience.rs`）

```rust
pub trait Audience {
    /// 宛先の人を `out` に足す。同じ人が複数回あっても、いない人が混ざっていてもよい
    /// （送る側が重複と退室済みを除く）。
    fn members(&self, ctx: &Ctx, out: &mut Vec<PlayerId>);
}
```

- 実装: `PlayerId`・`&A`・`Box<dyn Audience>`・`[A]`・`Vec<A>`・`[A; N]`（`A: Audience`）・`Everyone`・`Except`・`Named`（key で引く。下）・`from_fn(|ctx, out| ..)`
- `Everyone`: この世界（`Ctx`）にいる全員。v0.2 は 1 つの `World` が 1 つの Instance（複数 Instance は REQ-WORLD-005、v0.3）なので「Instance 全員」と同じ
- `AudienceExt`（blanket impl）: `.except(audience)`（`PlayerId` も `Group` も渡せる。「1 人を除く」は `Everyone.except(player)`。D29 の (5)）・`.and(audience)`（和集合）
- `Ctx::players()` を足す: 全員の `PlayerId`（順序は不定）。独自の `Audience` を書くときの入口で、`Everyone` もこれを使う
- 送るとき: `members` を集めて重複を除き（`PlayerId` で整列して `dedup`）、退室済みを除いてから、**1 回だけ encode して全員へ同じ本体を送る**（`send_many` が今も `clone` で送っている形）。送れない相手（切断）は今の `send_many` と同じく退室扱い

## 2. グループと key で登録する audience

- `Group`: メンバーの集合（`add`・`remove`・`contains`・`iter`・`len`・`is_empty`・`clear`）。`Audience` を実装する。利用者が自分で持つこともできる（その場合、退室の掃除は利用者の責任で、送る側が退室済みを除く）
- `ctx.join_group(key, player) -> bool`（無ければ空の `Group` を作って足す。退室済みの人は足さず `false`）・`ctx.leave_group(key, player)`・`ctx.group(key) -> Option<&Group>`・`ctx.remove_group(key)`。**`&mut Group` は渡さない**: 外から退室済みの `PlayerId` を足されて残り続けるのを避けるため（計画では `group_mut` だった）
- `ctx.register_audience(key, impl Audience + 'static)` / `ctx.unregister_audience(key)`: 計算で決まる宛先（チーム・「生きている人」など）を key で持つ
- `ctx.audience(key) -> Named`: key で引く宛先。**送るときに**引くので、先に作って持っておける。`Group` を先に探し、無ければ登録した audience を引く（key が両方にあったら `Group` が勝つ。文書に書く）。どちらも無ければ空
- 退室した人は全 `Group` から自動で外れる（`leave` に足す）。登録した audience は利用者の責任だが、送る側が退室済みを除くので、古い `PlayerId` が残っても誰にも届かない
- key は `&str`（`String` で持つ）。型付きの key（`TypeId`）は要望が出たら

## 3. 送るもの

メッセージ・ActionBar・Title・タブリスト・Sound・BossBar を、すべて `Ctx` のメソッドにする。`impl Audience` を取る。

| メソッド | 送るパケット |
|---|---|
| `send_message(aud, &Component)`（既存を `impl Audience` に広げる）・`broadcast(&Component)`（`Everyone` への `send_message`。既存） | `SystemChat`（overlay = false） |
| `send_action_bar(aud, &Component)` | `SetActionBarText`（`SystemChat` の overlay ではなく専用パケット。クライアントの扱いが vanilla と同じ） |
| `show_title(aud, &Title)` | （`times` があれば）`SetTitlesAnimation`、`SetSubtitleText`、`SetTitleText` の順 |
| `clear_title(aud)`・`reset_title(aud)` | `ClearTitles`（reset = false / true） |
| `set_tab_list(aud, &header, &footer)` | `TabList` |
| `play_sound(aud, &Sound)`・`stop_sound(aud, &StopSound)` | `Sound`・`StopSound` |
| BossBar（下） | `BossEvent` |

### Title

`Title::new(title)`・`.subtitle(Component)`・`.times(fade_in, stay, fade_out: Duration)`。`times` が無ければアニメーションのパケットを送らない（クライアントの直前の設定、既定は 10・70・20 tick）。`Duration` は tick（50 ms）に切り上げ、`i32` に収まらなければ上限に丸める。

### Sound

- `Sound::new(SoundEvent)`（生成した registry の定数。`sound::ENTITY_PLAYER_LEVELUP`）・`Sound::custom(Identifier)`（リソースパックの音。直接指定の形で送る）
- `.source(SoundSource)`（既定 `Master`。11 種）・`.volume(f32)`（既定 1.0）・`.pitch(f32)`（既定 1.0）・`.seed(i64)`（同じ seed なら同じ音の選択になる。**既定は `Ctx` の連番から作り、再生のたびに違う**。常に 0 だと複数の音源を持つ音がいつも同じになる）・`.fixed_range(f32)`（`custom` の音の聞こえる距離）
- **どこで鳴るか**は `.at(Vec3)`（その座標。1 回の encode で全員）か `.at_listener()`（既定。**受け取る人それぞれの位置**。人ごとに encode する。パケットが小さいので許容し、文書に書く）。エンティティから鳴らす `SoundEntity` は汎用エンティティ（M2-17）の後
- `StopSound`: 全部止める（`StopSound::all()`）・音の種類（`SoundSource`）だけ・音の名前だけ・両方（`StopSound::new().source(..).sound(..)`）

### BossBar

- `BossBar::new(title)`・`.progress(f32)`（0.0〜1.0 に丸める。NaN は 0）・`.color(BossBarColor)`（7 色）・`.overlay(BossBarOverlay)`（`Progress`・`Notched6/10/12/20`）・`.darken_sky()`・`.play_boss_music()`・`.create_fog()`
- `ctx.create_bossbar(BossBar) -> BossBarId`（`Copy`。UUID は `Ctx` の連番から作る）
- `ctx.show_bossbar(aud, id)`（`Add` を送り、閲覧者の表に足す。すでに見ている人には送らない）・`ctx.hide_bossbar(aud, id)`（`Remove` を送り、表から外す）・`ctx.remove_bossbar(id)`（全閲覧者から消して `id` を無効にする）
- 更新: `set_bossbar_title`・`set_bossbar_progress`・`set_bossbar_color`・`set_bossbar_overlay`・`set_bossbar_flags`（`UpdateName`・`UpdateProgress`・`UpdateStyle`（色と overlay）・`UpdateProperties`）。**表にいる人だけ**に 1 回の encode で送る。値が変わらなければ送らない
- `ctx.bossbar(id) -> Option<&BossBar>`・`ctx.bossbar_viewers(id)`
- 退室した人は全 BossBar の閲覧者から外す（`leave`）。無効な `id` への呼び出しは何もしない（`PlayerId` と同じ扱い）

## 4. プロトコル（`lodeframe-protocol`）

26.3 の `server.jar` を `javap -c -p` で読んで確定した（2026-10-23）。

| パケット | 中身 |
|---|---|
| `SetTitleText`・`SetSubtitleText`・`SetActionBarText` | `Component` |
| `SetTitlesAnimation` | `fade_in: i32`・`stay: i32`・`fade_out: i32`（tick。`VarInt` ではなく `int`） |
| `ClearTitles` | `reset: bool` |
| `TabList` | `header: Component`・`footer: Component` |
| `Sound` | `sound`（下）・`source: VarInt`（`SoundSource` の序数）・`x`・`y`・`z: i32`（座標 × 8）・`volume: f32`・`pitch: f32`・`seed: i64` |
| `StopSound` | フラグ 1 byte（1 = `source` あり、2 = 音の名前あり）、`source: VarInt`、`name: Identifier` |
| `BossEvent` | `id: Uuid`、操作の種類 `VarInt`（0 Add・1 Remove・2 UpdateProgress・3 UpdateName・4 UpdateStyle・5 UpdateProperties）、操作ごとの本体 |

- `sound` の形: `VarInt`（登録済みなら registry id + 1、直接指定なら 0）。直接指定のあとに `Identifier` と `Option<f32>`（固定範囲）
- `SoundSource` の順: master・music・records・weather・blocks・hostile・neutral・players・ambient・voice・ui
- `BossEvent` の本体: Add は `name: Component`・`progress: f32`・`color: VarInt`（pink・blue・red・green・yellow・purple・white）・`overlay: VarInt`（progress・notched_6・10・12・20）・フラグ 1 byte（1 = 空を暗くする、2 = ボス音楽、4 = 霧）。Remove は無し。UpdateProgress は `f32`、UpdateName は `Component`、UpdateStyle は色と overlay、UpdateProperties はフラグ 1 byte
- `SoundEvent`（`EntityType` と同じ作りの registry id の newtype。`name()`・`from_name()`・`from_id()`）を `entity_type.rs` と同じ形で足す。定数は **datagen が `registries.json` の `minecraft:sound_event`（1991 件）から生成**（`generated/sound_events.rs`）。名前の `.` と `:` は `_` にして大文字（`entity.player.levelup` → `ENTITY_PLAYER_LEVELUP`）。重複したら datagen が失敗する（`entity_types` と同じ）
- 本体の `lodeframe::sound`（生成した定数）として公開する（`lodeframe::protocol::sound_event` からも）

## 5. ファイル

- `xtask/src/codegen.rs`: `sound_events.rs` を生成
- `crates/lodeframe-protocol/src/{sound_event.rs, generated/sound_events.rs, packets.rs}`: `SoundEvent`・パケット・`SoundSource`・`SoundHolder`（登録済み／直接指定）
- `crates/lodeframe/src/audience.rs`: `Audience`・`AudienceExt`・`Everyone`・`Except`・`Group`・`Named`・`from_fn`・`Title`・`Sound`・`StopSound`・`BossBar` と、その型
- `crates/lodeframe/src/world/send.rs`: `Ctx` の送信メソッド（`world.rs` の非公開のフィールドに触れるので `world` の子にする）
- `crates/lodeframe/examples/audience.rs`: 入室した人に Title・BossBar・Sound・タブリストを出す example

## 利用者に見える変更（v0.x）

- 追加: `Audience`・`Everyone`・`Group`・`Title`・`Sound`・`BossBar` と、`Ctx` の `send_action_bar`・`show_title`・`set_tab_list`・`play_sound`・`create_bossbar` など
- 変更: `Ctx::send_message` の第 1 引数が `PlayerId` から `impl Audience` になる（`PlayerId` は `Audience` なので既存の呼び出しは変わらない）
- 新しい依存は無い

## やらないこと

| 項目 | なぜ外したか | 受け入れ先 |
|---|---|---|
| エンティティから鳴らす `SoundEntity` | 汎用エンティティが無い | M2-17 の後 |
| Scoreboard・Team を `Audience` にする | Scoreboard・Team がまだ無い | REQ-ENT-004（v0.3） |
| 本・リソースパックの送信 | 別の要件 | REQ-UI-005・REQ-NET-008 |
| プレイヤー一覧（タブリストの行）の操作 | ヘッダーとフッターだけ | REQ-PLAYER-002 |
| 受信者の言語による翻訳 | 翻訳は別枠 | D19 |
| 型付きの key（`TypeId`）・複数の Instance をまたぐ宛先 | 要望が出たら。複数 Instance は v0.3 | REQ-WORLD-005 |

## テスト

- `lodeframe-protocol`: 各パケットのバイト列（手で組んだ期待値と比べる。`SetTitlesAnimation` が 3 つの `int`、`Sound` の直接指定・登録済み、`StopSound` の 4 通り、`BossEvent` の 6 操作）／`SoundEvent` の名前と id が合う・未知の id を拒否
- `lodeframe`（ハーネスで実クライアント相当のボットに届くパケットを読む）:
  - 1 人・全員・グループ・「1 人を除く」・和集合・独自の audience に同じコードで届く（受入条件 1・3）
  - 退室した人・古い `PlayerId` には届かず、他の人に届く
  - 重複したメンバーに 1 回だけ届く
  - グループは退室で自動で外れる／同じ名前で入り直した人は入っていない
  - BossBar: 作って見せる→進捗・タイトル・色の更新が**見ている人だけ**に届く／隠した人・退室した人には届かない／値が変わらなければ送らない／`remove_bossbar` で消える（受入条件 2）
  - Title の順序と tick への丸め／`clear`・`reset`
  - Sound: `at`（座標 × 8）と `at_listener`（人ごとの位置）
  - 1 回の encode: 全員に同じバイト列が届く
- `mise run check`・`mise run msrv`・`mise run lint:license`

## 実装の順

1. datagen に `sound_events.rs` を足し、`mise run datagen` で再生成する
2. プロトコルのパケット・`SoundEvent`・`SoundSource` とバイト列のテスト
3. `audience.rs`（宛先・`Group`・`Title`・`Sound`・`BossBar`）と `world/send.rs`、`Ctx::players()`、`leave` の掃除
4. ハーネスのテストと example
5. 文書（requirements・minestom-parity・architecture・backlog・decisions D46）。PR

## 実装の結果

- 計画どおり。`lodeframe::{Audience, AudienceExt, Everyone, Group, Named, Sound, StopSound, Title, BossBar, BossBarId, ..}`・`lodeframe::sound`（定数）・`Ctx` の送信メソッド。`Ctx::send_message` は `impl Audience` を取り、`broadcast` は `Everyone` への `send_message`
- 計画から変えたこと
  - グループは `join_group`・`leave_group`・`group`・`remove_group`（`group_mut` は無し。上の「グループと key」）
  - sound の seed の既定は連番から作る（上の「Sound」）
  - `Everyone` は `Copy`、`Except`・`And` は中身が `Copy` なら `Copy`、`Named`・`Group` は `Clone`。同じ宛先を何度も渡せる（`Everyone.except(player)` は `Copy`）
  - プロトコルの `BossBarColor`・`BossBarOverlay`・`BossBarFlags`・`SoundSource` は protocol が持ち、`lodeframe::audience` と `lodeframe` から re-export
- 検証: protocol のバイト列テスト 3 件（title 系・sound 系・BossEvent の 6 操作）、`SoundEvent` の名前と id、ハーネスのテスト 14 件（宛先の組み合わせ・独自 audience・重複・退室・グループの掃除・1 回の encode・Title の 1 メッセージ 3 パケット・Sound の位置・BossBar の表示/更新/隠す/消す/退室・新しい stay が引き継がないこと）、`mise run check`・`msrv`・`lint:license`
- 制約: パケットの形は 26.3 の `server.jar` を `javap` で読んだものだが、**実クライアントでの表示は未確認**（`cargo run -p lodeframe --example audience` で利用者に確認を頼む）。`at_listener` の sound は人ごとに encode する。`Everyone` は 1 つの `World`（Instance が複数になるのは v0.3）
