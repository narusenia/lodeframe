# イベントノードの拡張 実装計画（M2-02）

> **Status**: 計画確定（実装前） — 2026-10-02

要件: REQ-API-009。決定: D6・D16・D29 の (2)・D31（[decisions.md](../decisions.md)）。Minestom との対照は [minestom-parity.md](minestom-parity.md) の 6.1〜6.12。
`event.rs` と `world.rs` にまたがる。M2-16（操作イベント）・M2-17（エンティティ）・M2-21（GUI）・M2-25（`derive(Event)`）が、この形の上に載る。

## 困っていたこと

M2-01 で木は `Ctx` の外に出たが、まだ次が足りない。

1. 順序が追加順だけ。「ロビーの保護より先に走らせたい」ができない
2. 条件で絞れない。「ゲーム中のプレイヤーのイベントだけ受ける」を、ハンドラごとに `if` で書く
3. ハンドラが一度きり・N 回・条件が成り立つ間、を書けない。外す手段（ハンドル）も無い
4. ハンドラの中で起こしたイベントと、木へのハンドラ追加が**できない**（`Ctx` が木を持たないため。M2-01 の持ち越し）
5. 親のイベントを受けるハンドラ（「プレイヤーに関するイベントなら全部」）を書けない

## 決めたこと

### 1. 優先度

- `Priority(i32)`、大きいほど先。既定は 0。同じ優先度は追加順
- **付ける場所は 2 つ**: ハンドラ（同じノード内の順）とノード（兄弟の子ノードの順）。木をまたいだ全ハンドラの総順位は作らない
- 理由: 総順位にすると発火のたびに部分木の全ハンドラを集めて並べ直すことになり、ホットパス（`PlayerMove` 等）で確保が要る。ノード単位なら、追加時に 1 回整列するだけで済む。Minestom も優先度はノード単位

### 2. 登録の形: `Listener` のオプション

```rust
node.on::<Chat>(h);                                   // 今までどおり
node.listen::<Chat>().priority(10).times(1).until(|e, ctx| ..).ignore_cancelled().add(h);  // -> ListenerId
node.off(id);
```

- `listen` は builder を返し、最後の `.add(h)` で登録して `ListenerId` を返す。途中で捨てても何も登録されない。`on` は `listen::<E>().add(h)` の薄い別名で、今までどおり `&mut Self` を返す
- `times(n)`: n 回呼んだら外れる。`until(pred)`: pred が `true` になった発火の**前**に外れる（呼ばれない）。両方付けたら先に成り立った方
- `ignore_cancelled()`: その時点で `is_cancelled()` のイベントを受けない。既定は受ける（今の動き。「全部呼ぶ」は `emit` の doc が保証している）
- 外れたハンドラは、その発火の最後に取り除く（走査中に `Vec` を変えない）

### 3. 条件で絞るノード

```rust
let mut game = EventNode::<Ctx>::new();
game.only_if::<PlayerEvent>(|e, ctx| ctx.is_online(e.player()));   // 他の型は素通し
let mut chat = EventNode::<Ctx>::new();
chat.only_for::<PlayerChatEvent>(|e, ctx| ..);                      // この型専用。他の型は止める
```

- どちらもノードの**ゲート**。条件を見るのは E（または E を親に持つイベント）が来たときだけで、偽ならその部分木を通さない
- `only_if::<E>(pred)`: E 以外の型のイベントは**素通し**。`game` の下に chat・block・move のハンドラを並べたまま「このプレイヤーがゲーム中のときだけ」と書ける
- `only_for::<E>(pred)`: E 以外の型は**止める**（Minestom の `EventNode.type` と同じ）。型専用のノードを明示したいとき用。条件が要らなければ `only_for::<E>(|_, _| true)`
- 1 つのノードにゲートは 1 つ。2 つ目を付けたら置き換える（積みたいときは入れ子にする）。内部は「E の TypeId・他の型を通すか・pred」だけなので、2 つは同じ仕組みの別の既定値
- 理由: 素通しだけだと型専用の木を作れず、型限定だけだと 1 ゲーム 1 ノードに束ねられない。差は 1 ビットなので両方持つ
- 利用者データで絞る（REQ-API-009 の「利用者データ」）は、`pred` が `ctx` を読めば足りる。key の形は M2-19 で決まるので、ここではテストに使わない

### 4. 発火中の追加と内側のイベントは遅延キュー（D29 の (2)）

`Ctx` が木を持たないので、ハンドラの中からは積むだけにして、`World` が**その発火が終わってから**反映する。

```rust
ctx.emit(event);                    // 内側のイベント。FIFO
ctx.add_listener::<E>(..) -> ListenerId   // 木のルートに足す
ctx.add_node(node) -> ChildId             // ルートの子に足す
ctx.remove_listener(id); ctx.remove_node(id);
```

- `World` は `handle`・`tick` の中の各発火の後に、`flush_departures` と同じ形でキューが空になるまで回す。**同じ tick のうちに届く**（受入条件の 2 つ目）
- 内側のイベントは**発火した瞬間には走らない**ので、ハンドラは結果（キャンセルされたか）を受け取れない。受け取りたいときは、既定動作を持つ側（`World`）が発火する今の形のままにする。この制約を `ctx.emit` の doc に書く
- 追加したハンドラは**次の発火から**効く（今の発火には混ざらない）。Minestom と同じ
- ID は先に返す必要がある（`ctx.add_listener` の戻り値）。`ListenerId` / `ChildId` は **プロセス全体の `AtomicU64` から取る**。木ごとの連番だと、遅延分と直接分が衝突する。ID の発番はゲームの状態ではないので D6（グローバル可変シングルトンを置かない）の対象外と読む → **確認したい 1**
- キューに積めるのは `Box<dyn FnOnce(&mut EventNode<Ctx>)>` と `Box<dyn FnOnce(&mut World)>`（emit 用）。暴走（ハンドラが自分を再発火し続ける）は、1 回の処理あたりの上限（既定 1000 件）で止め、`tracing::error!` で出す。上限の値は計測せず置くので、定数にして doc に理由を書く

### 5. 継承イベントは trait で

Rust に継承は無いので、**親は trait、ハンドラは `dyn Trait` で受ける**。

```rust
pub trait PlayerEvent { fn player(&self) -> PlayerId; }

impl Event for PlayerChatEvent {
    fn parents(&mut self, v: &mut Parents<'_>) { v.visit::<dyn PlayerEvent>(self); }
}
node.on::<dyn PlayerEvent>(|e, ctx| ..);   // チャットもブロック設置も受ける
```

- `E: ?Sized` を許し、`TypeId::of::<dyn PlayerEvent>()` で引く。`Parents::visit` が `&mut Self` を `&mut dyn PlayerEvent` に変えて、親の型のハンドラを呼ぶ
- 順序は「具体的な型のハンドラ → 親の型のハンドラ」を、ノードごとに行う（優先度は型ごとの中で効く）。型をまたぐ総順位は作らない（1. と同じ理由）
- 理由: enum にまとめる案は、利用者の独自イベントが親を足せない。`TypeId` の継承表を実行時に持つ案は、`&mut dyn Any` を親の型へ落とす手段が無い
- `derive(Event)`（M2-25）が `parents` を生成する。それまでは手書き
- この単位で用意する親は `PlayerEvent`（`player()`）だけ。`CancellableEvent` のような分類は、使う単位（M2-16）で足す

### 6. 束ね

- 複数のイベントを受けるひとまとまりを、`trait Bundle<C> { fn register(self, node: &mut EventNode<C>); }` と `node.install(bundle)` で付けられる。lobby の「入退室メッセージ」「ブロック保護」のような単位を、関数ではなく型として配れる
- 状態の共有（チャットの履歴を join と leave で読む等）は、`Bundle` が `Rc<RefCell<S>>` を各ハンドラに複製して渡す書き方になる。Instance は 1 スレッドなので `Rc` で足りる。`Bundle` 自体は何も隠さない

### 7. Instance ごとのノード

- 今の `Server` は Instance を 1 つしか走らせない（`RunningServer::instance()`）。**サーバー全体のルートと Instance ごとのノードの二層**は、複数 Instance が来る単位がやる（受け皿 → 下の表）
- この単位では、`World` が持つ木が「その Instance のノード」であることを、doc と `events_mut` のテスト（2 つの `World` が互いのハンドラを呼ばない）で固定する

## 確認して決めたこと（2026-10-02）

1. ID は**プロセス全体の `AtomicU64`** で発番する（4.）。発番はゲームの状態ではないので、D6 の対象外と読む。実装時に decisions.md へ D として残す
2. ゲートは**素通し（`only_if`）を基本**にし、型専用（`only_for`）も持つ（3.）
3. 優先度は**ハンドラとノード単位**。総順位にしない（1.）

## やらないこと（M2-02）

| 項目 | なぜ外したか | 受け皿 |
|---|---|---|
| サーバー全体のルートノード | 複数 Instance が無い | 複数 Instance を扱う単位（v0.3 以降。backlog に起こす） |
| エンティティごとのノード | エンティティが無い | M2-17 |
| 内側のイベントの結果を返す | 木を借用中は同期に走らせられない | — （設計上の制約。`ctx.emit` の doc に書く） |
| 総順位の優先度 | ホットパスで確保が要る | 計測して必要なら REQ-PERF-001（v0.4） |
| ListenerHandle（検索を省いた直接呼び出し） | 計測してから | REQ-PERF-001（v0.4） |
| `PlayerEvent` 以外の親 trait | 使う単位が無い | M2-16 |
| `derive(Event)` | マクロの単位 | M2-25 |

## テスト

`event.rs` の単体テスト（`World` を使わない）と、`tests/world.rs` のハーネスのテスト。

- 優先度の高いハンドラが先に呼ばれる。同じ優先度は追加順。ノードの優先度は兄弟の子の順を決める
- `times(1)` は 1 回で外れる。`until` が成り立つと呼ばれない。`off(id)` で外れる。外れた後に同じ型の別のハンドラは影響を受けない
- `ignore_cancelled` は、先のハンドラがキャンセルした後に呼ばれない。付けなければ呼ばれる
- `only_if` が偽の間、部分木のハンドラが呼ばれない。別の型のイベントは通る。真に戻れば呼ばれる
- `only_for` は、別の型のイベントを通さない。条件が偽の間は指定の型も通さない。ゲートを付け直すと置き換わる
- 親 trait のハンドラが、2 種類のイベントで呼ばれる。具体的な型のハンドラが先
- ハンドラの中の `ctx.emit` は、**その発火が終わってから**、同じ `handle` の呼び出しのうちに届く。入れ子（A が B を、B が C を起こす）が順に届く。自分を再発火し続けるハンドラが上限で止まる
- ハンドラの中の `ctx.add_listener` は、その発火には混ざらず、次の発火から効く。`ctx.remove_listener` も同様
- 2 つの `World` は互いのハンドラを呼ばない
- 既存のテストと lobby が、変更なしで通る
- `mise run check`

## 実装の順

1. `event.rs`: `ListenerId`、優先度、`listen` のオプション、`off`、外れる処理（単体テスト）
2. `event.rs`: `only_if`、`Bundle`、`Parents`（単体テスト）
3. `world.rs`: 遅延キュー、`Ctx` の `emit` / `add_listener` / `add_node` / `remove_*`、`PlayerEvent` と既存 5 イベントの `parents`（ハーネスのテスト）
4. 文書: minestom-parity の 6.x、REQ-API-009 の受入条件、backlog、architecture の EventNode の節、decisions（ID の発番を D として）
