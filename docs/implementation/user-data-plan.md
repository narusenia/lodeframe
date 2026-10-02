# 利用者データ 実装計画（M2-19）

> **Status**: 計画 — 2026-10-02

要件: REQ-API-007（v0.2 の Player・Instance）。決定: D6・D30・D31（[decisions.md](../decisions.md)）。
本体の単一 crate 内の変更（`data.rs` 新規・`world.rs`）。設計ゲートの対象外だが、`Data` の形が M2-17（エンティティ）と M2-20（ItemStack）に効くので先に決める。

## 困っていたこと

ゲームを書く利用者は、プレイヤーごとの得点・チーム・直前の操作時刻のような値を持ちたい。今は `HashMap<PlayerId, _>` を自分で持ち、退出のたびに掃除するしかない
（`PlayerId` の `serial` を見落とすと、同名で入り直した人が前の人の値を引き継ぐ）。Minestom の Tag API の目的（D30）は、型付きの値を持ち主に付けること。

## 決めたこと

### 1. `Data`: 型付きの値の表

```rust
const SCORE: Key<u32> = Key::new("myplugin:score");
const TEAM: Key<Team> = Key::new("myplugin:team");

let data = ctx.player_data_mut(player)?;                 // Option<&mut Data>
data.set(&SCORE, 10);                                    // Option<T>（前の値）
*data.get_or_insert_with(&SCORE, || 0) += 1;
let n: Option<&u32> = data.get(&SCORE);
data.remove(&SCORE);                                     // Option<T>

// 型そのものを key にする（名前を付けない）
data.set_by_type(Lives(3));
data.get_by_type::<Lives>();
```

- **名前つきの `Key<T>` と、型そのものの key の両方**を持つ。内部は `(名前 or なし, TypeId)` で引く 1 つの表
- 名前が同じでも `T` が違えば**別の枠**。型の合わない読み出しは `None`（panic しない。受入条件）。同じ `u32` でも名前ごとに別の値
- `Key::new` は `const fn`。`const` で宣言して使い回す。名前は `&'static str`。衝突を避けるため `"crate名:key"` の形を勧める（実行時に検査はしない）
- 値は `T: 'static` だけを要求し、`Send` は要らない（Instance のスレッドから出ない。D6）
- 型だけの key は名前が無いので、M2-20 でアイテムの `custom_data` に入れる対象にはならない。保存・往復する値は名前つきの `Key` を使う
- API: 名前つき `get`・`get_mut`・`set`・`remove`・`contains`・`get_or_insert_with`、型だけの `get_by_type`・`get_mut_by_type`・`set_by_type`・`remove_by_type`。`Data` は `Default`・`Debug`（個数だけ）

### 2. 持ち主ごとの入り口

| 持ち主 | 入り口 | 消えるとき |
|---|---|---|
| プレイヤー | `ctx.player_data(id)` / `player_data_mut(id)`（`Option`） | 退出したとき。`PlayerId` の `serial` を見るので、同名で入り直した人は空の `Data` から始まり、古い ID では引けない |
| Instance | `ctx.data()` / `data_mut()` | `World` が落ちたとき |

- 今は `Ctx` と `World` と Instance が 1 対 1 なので、Instance のデータは `Ctx` が持つ（`World` は `Deref` で同じ名前で使える）。複数 Instance（M2-17）でも、`Ctx` ごとに持つので形は変わらない
- いない人（退出済みの `PlayerId`）には `None`。`name`・`is_online` と同じ「何も指さない」の扱い（D31）
- エンティティ・ItemStack は同じ `Data` を持たせる（M2-17・M2-20）。入り口だけ足す

### 3. 退出するプレイヤーのデータ

`PlayerLeaveEvent` が発火する時点で、そのプレイヤーは `Ctx` から消えている（`player_data(id)` は `None`）。DB への保存のような退出時の処理のために:

```rust
events.on(|e: &mut PlayerLeaveEvent, ctx: &mut Ctx| {
    if let Some(data) = ctx.leaving_data(e.player) {
        let score = data.get(&SCORE).copied();      // 持ち出しは data を mem::take してもよい
        ctx.spawn(save(e.name.clone(), score)).then(|_, _| {});
    }
});
```

- `ctx.leaving_data(id) -> Option<&mut Data>`: その人の `PlayerLeaveEvent` を処理している間だけ引ける。ハンドラが全部終わると捨てる
- 退出済みの ID に `player_data` が `None` を返す決まり（D31）は変えない。入り口を分けるのは、イベントの外から古い ID でデータを読めないようにするため
- `PlayerLeaveEvent` に `Data` を載せる案は、`Clone`・`Eq` が外れて `Recorder`（`Clone` が要る）と既存テストを直すことになるので採らない

## 確認して決めたこと（2026-10-02）

| 判断 | 選んだ案 | 選ばなかった案 |
|---|---|---|
| key の形 | 名前つき `Key<T>` と型そのものの key の**両方**（利用者の希望。同じ表に `(名前 or なし, TypeId)` で入るので実装の差は小さい） | 片方だけ（名前つきだけだと同じ型の 1 値に newtype が要らないのに型 key が無く、型だけだと NBT に入れる名前が無い） |
| NBT への備え | メモリ上の値だけ。`T: 'static`。NBT に入れる key は M2-20 で同じ `Key` に足す | 今から変換関数を持たせる（ItemStack の形が決まる前に NBT の表現を固める） |
| 取り出し口 | 表の型 `Data` + 持ち主ごとの入り口 | `ctx` に直接生やす（持ち主ごとに同じ API を作ることになる） |
| Cooldown | 含めない（別の単位） | 含める（`Data` が未検証のまま形を決める） |
| 退出時のデータ | 退出ハンドラの間だけ読める `leaving_data` | イベントに載せる（`Clone`・`Eq` が外れる）、対応しない（保存のたびに自前の表へ写す） |

## やらないこと（M2-19）

| 項目 | なぜ外したか | 受け皿 |
|---|---|---|
| Cooldown 等の時間の補助 | `Data` の形が固まってから、置き場（key か専用型）を選ぶ | 別の単位（backlog に起こす。REQ-API-006 の残り） |
| エンティティ・ItemStack への付与 | エンティティ・アイテムがまだ無い | M2-17・M2-20 |
| 値の保存・NBT の符号化 | 実行中の値だけ（REQ-API-007 の v0.2）。NBT に入れる key は ItemStack の要件を見てから | M2-20（アイテム）、v0.3（ブロック） |
| key の名前の衝突検査 | 実行時の検査は `const` の宣言を使う利用法と合わない。型が違えば別の枠なので壊れない | — |
| 値の変更の通知（watch） | 要望が出てから | — |

## テスト

`tests/world.rs`（ハーネス）と `data.rs` の単体テスト。

- 名前つき: `set` した値が `get` で読める。`set` は前の値を返す。`get_mut`・`get_or_insert_with`・`remove`・`contains`
- 同じ名前で型が違う key は別の枠: 読み出しは `None` で、panic しない（受入）。同じ型で名前が違えば別の値
- 型だけの key: `set_by_type`・`get_by_type`・`remove_by_type`。名前つきの同じ型の値とは別
- プレイヤー: 入室時に付けた値を、チャットのハンドラで読める。退出した人の ID では `None`。同名で入り直した人は空から始まり、前の人の値は見えない
- 退出: `leaving_data` は退出ハンドラの中で値を読め（持ち出せ）、ハンドラの外・別のプレイヤーの退出では `None`。退出した人のデータが、ハンドラが終わった後に残らない（drop ガードで確かめる）
- Instance: `data_mut` で付けた値が tick をまたいで読める。`World` を落とすと値が drop される
- `mise run check`・`mise run msrv`

## 実装の順

1. `data.rs`: `Key`・`Data`・単体テスト。`lib.rs` に `pub mod data`
2. `world.rs`: `Player` と `Ctx` が `Data` を持つ。`player_data`・`player_data_mut`・`data`・`data_mut`、`leaving_data`（`leave` が預け、`flush_departures` が捨てる）
3. 文書: REQ-API-007 の受入条件、architecture、minestom-parity（10.2・3.36・5.1）、backlog（M2-19 を ✅、Cooldown を新しい行に）、handler-context-plan の「利用者データの置き場」、decisions（D35）
