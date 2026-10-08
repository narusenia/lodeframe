# Component の完全版 実装計画（M2-11）

> **Status**: 実装済み — 2026-10-08

要件: REQ-TEXT-001（v0.2 の項目）。決定: D14・D20・D29 の (3)・D43（[decisions.md](../decisions.md)）。
`lodeframe-text`・`lodeframe-protocol`・本体にまたがり、公開型（`Component`・`Style`）の形が変わる。M2-12（MiniMessage）・M2-13（`text!`）・M2-14（Audience）・M2-23（コマンド）がこの型の上に載るので、コードの前にここで形を決める。

## いまの形と問題

`Component { text: String, style: Style }` で、`Style` は色と 5 つの装飾だけ（`Copy`）。子要素が無いので、1 行に 2 つのスタイルを置けない（D29 の (3)）。translatable・hover・click なども表せない。

## 1. 26.3 の実際の形（server.jar の codec で確認した。2026-10-08）

`target/xtask/datagen/26.3/` にキャッシュ済みの server.jar と Java 25 で、`ComponentSerialization.CODEC` を `Bootstrap` 後に動かして NBT を出した（リポジトリには入れない使い捨ての Java。テストの正解値は結果をコメント付きで貼る）。

| 内容 | NBT の形（codec が出したもの） |
|---|---|
| 素のテキスト（装飾も子も無い） | 文字列のタグ `"hi"` |
| 装飾つき | `{text:"hi", bold:1b, italic:0b, color:"#FF0000", shadow_color:287454020, insertion:"ins", font:"minecraft:uniform"}`。`shadow_color` は ARGB の Int |
| 子要素 | `{text:"a", extra:["b", {bold:1b, text:"c"}]}`。素のテキストの子は文字列のまま |
| 空 | `""` |
| translatable | `{translate:"chat.type.text", with:["a","b"]}`、`fallback:"fb"` があれば足す |
| keybind | `{keybind:"key.jump"}` |
| score | `{score:{name:"@p", objective:"obj"}}` |
| selector | `{selector:"@a", separator:", "}`（separator は Component） |
| nbt | `{nbt:"Health", entity:"@s"}`・`block:"~ ~ ~"`・`storage:"minecraft:x"` のどれか 1 つ、`interpret:1b` か `plain:1b`（両方は不可）、`separator` |
| object（スプライト） | `{sprite:"minecraft:block/stone"}`（atlas が既定のときは省く）、`fallback`（Component） |
| click | `click_event:{action:"open_url", url}`・`run_command`/`suggest_command` は `command`・`change_page` は `page`（Int）・`copy_to_clipboard` は `value` |
| hover | `hover_event:{action:"show_text", value}`・`show_item` は `id`・`count`・`show_entity` は `id`・`uuid`（IntArray 4 つ）・`name` |

読む側（JSON も NBT も同じ codec）の規則:

- 文字列は素のテキスト。**配列は先頭が本体で、残りがその子**（`["a",{"text":"b"}]` → `{text:"a", extra:["b"]}`）
- 数値や真偽値だけの JSON は 26.3 では**拒否される**（古い版は受けた）。lodeframe も拒否する
- `shadow_color` は Int のほか、JSON では `[r,g,b,a]` の小数 4 つでも書ける
- 色は名前（`red`）か `#RRGGBB`。codec は大文字の 16 進を出すが、クライアントは大文字小文字を区別しない。**書くときは今のまま小文字**にし、読むときは両方を受ける

26.x で増えたもの（`ClickEvent$Custom`・`ShowDialog`、`ObjectContents` のプレイヤーのスプライト）は下の「やらないこと」。

## 1.5 NBT のリストは型が混ざってよい（26.x の形式。実装で見つかった）

`extra: ["b", {bold:1b, text:"c"}]` のように、文字列とコンパウンドが 1 つのリストに混ざる。いまの `Nbt` の書き出しは「型が違う要素」を拒否する。実 codec が出したバイト列で確かめた形:

- 要素の型が揃っていれば、これまでどおり（全部文字列・全部コンパウンド）
- 揃っていなければ、リストの要素の型を **10（コンパウンド）**にし、コンパウンドでない要素は `{"": 値}`（空のキー 1 つ）に**包んで**書く。コンパウンドの要素はそのまま
- 読むときは、要素の型が 10 のリストで、**キー `""` が 1 つだけのコンパウンドを中の値に戻す**

`Nbt` 全体の挙動が変わる（レジストリのデータなど他の NBT にも効く）。vanilla の 26.3 と同じなので、そのまま合わせる。既存の「混在を拒否する」テストは、この規則のテストに置き換える。

## 2. モデル（`lodeframe-text`）

```rust
pub struct Component {
    pub content: Content,
    pub style: Style,
    pub children: Vec<Component>,
}

#[non_exhaustive]
pub enum Content {
    Text(String),
    Translatable { key: String, fallback: Option<String>, args: Vec<Component> },
    Score { name: String, objective: String },
    Selector { selector: String, separator: Option<Box<Component>> },
    Keybind(String),
    Nbt { path: String, source: NbtSource, mode: NbtMode, separator: Option<Box<Component>> },
    Sprite { atlas: Option<String>, sprite: String, fallback: Option<Box<Component>> },
}
pub enum NbtSource { Entity(String), Block(String), Storage(String) }
pub enum NbtMode { Default, Interpret, Plain }   // codec は interpret と plain の両方 true を拒否する（確認済み）ので、型で排他にした

pub struct Style {          // Clone（Copy ではなくなる）
    pub color: Option<Color>,
    pub shadow_color: Option<u32>,   // 0xAARRGGBB
    pub bold / italic / underlined / strikethrough / obfuscated: Option<bool>,
    pub insertion: Option<String>,
    pub font: Option<String>,        // `minecraft:uniform` のような id
    pub click: Option<ClickEvent>,
    pub hover: Option<Box<HoverEvent>>,
}

#[non_exhaustive]
pub enum ClickEvent { OpenUrl(String), RunCommand(String), SuggestCommand(String), ChangePage(i32), CopyToClipboard(String) }
#[non_exhaustive]
pub enum HoverEvent {
    ShowText(Component),
    ShowItem { id: String, count: i32 },
    ShowEntity { entity_type: String, uuid: u128, name: Option<Component> },
}
```

- **`Content` は 1 つ**にして、`text` と `translate` が同時に立つ不正な状態を型で作れなくする（D43）。`Default` は `Text("")`
- 子要素のスタイルは vanilla と同じく親から継承される（クライアント側の規則。サーバーは計算しない）
- `ShowEntity` の `uuid` は `u128`。`lodeframe-text` は protocol に依存しない（D20）ので `Uuid` 型は使えない。protocol 側で IntArray に直す
- `Content`・`ClickEvent`・`HoverEvent` は `non_exhaustive`（26.x のように版で増える）

## 3. builder（`lodeframe-text`）

- 生成: `Component::text`・`empty()`・`newline()`・`space()`・`translatable(key)`（`.arg(c)`・`.args(iter)`・`.fallback(s)`）・`score(name, objective)`・`selector(s)`・`keybind(k)`・`nbt_entity/nbt_block/nbt_storage(path, source)`・`sprite(name)`
- スタイル: 既存の `color`・`bold`・`italic`・`underlined`・`strikethrough`・`obfuscated` に加えて、`decorate(Decoration, bool)`・`undecorate(Decoration)`（継承に戻す）・`shadow_color`・`font`・`insertion`・`click(ClickEvent)`・`hover(HoverEvent)`、近道として `click_open_url`・`click_run_command`・`click_suggest_command`・`hover_text`
- 子要素: **`append(impl Into<Component>)`**・`append_all(iter)`・**`Component::join(separator, iter)`**。いずれも自分を消費して返す
- **`a + b`**（`impl Add<T: Into<Component>>`）と **`FromIterator<Component>`**（`iter.collect::<Component>()`）。意味は「兄弟として並べる」: 左が**入れ物**（内容が `Text("")`・スタイル無し）ならその子に足し、そうでなければ左を最初の子にした入れ物を作る。`a + b + c` は `[a, b, c]` の 1 段になる。左のスタイルが右へ漏れない（右は自分のスタイルを持つ）
- `as_text() -> Option<&str>`（`Content::Text` のときだけ）。`text` フィールドの代わり

## 4. シリアライズ

| 方向 | 置き場所 | 中身 |
|---|---|---|
| Component → JSON | `lodeframe-text` の `to_json`（拡張） | 手書きの書き出し。キーの順を保つ（`text` が先）。login の Disconnect が使う |
| JSON → Component | `lodeframe-text` の `Component::from_json` | `serde_json` で読み、上の規則で解釈する。エラーは型つき（`JsonError`） |
| Component → NBT | `lodeframe-protocol` の `From<&Component> for Nbt`（拡張） | 上の表のとおり。素のテキスト・空は文字列のタグ、子の素のテキストも文字列 |
| NBT → Component | `lodeframe-protocol` の `TryFrom<&Nbt> for Component` と `Decode` | 文字列・コンパウンド・リストを受ける。型の違いはエラー |

- 依存: `lodeframe-text` に **`serde_json`** を足す（読む側だけ。workspace に宣言済みで、本体と xtask がすでに使っており、Cargo.lock にもある。新しく取得するものは無い）。理由: 標準ライブラリに JSON パーサは無く、利用者が渡す JSON（コマンドや設定）を自前のパーサで読むと取りこぼしが出る。MiniMessage（M2-12）も同じ crate に入る。書き出しは手書きのまま
- **深さの上限**を読む側に置く（rust.md の「再帰とスタック」）: 入れ子（`extra`・`with`・`separator`・`value`・`fallback`）は **32 段**まで。`serde_json` 自身の上限（128 段。Component 1 段がオブジェクトと配列の 2 段）より手前で自分の上限に当たるようにした（64 段にすると JSON では届かない）。512KB のスタックのスレッドで確かめる。越えたら `TooDeep` エラー
- 入力に含まれる長さの確保は、残りに照らす（NBT の既存の `check_remaining` に乗る。JSON は `serde_json` がすでに実体を持つので不要）
- 本体の既存の使い方（`Component::text(..)`・`.color(..)` など）は変わらない。**`text` フィールドを直接読んでいる箇所と、`Style` を `Copy` として使っている箇所**だけを直す

## 5. 検証

- **正解値は実 codec が出したもの**（上の表）を、テストの定数にする。Rust の出力を NBT のまま比べる（色の大文字小文字は正規化）
- **逆向きの確認**: 全種の Component を Rust で NBT にして SNBT で書き出し、Java の codec で読み直す（使い捨て。結果をこの計画書の末尾に追記する）。これで「vanilla が読める」ことを確かめる
- 実クライアントでの表示（REQ-TEXT-001 の「全種の Component が vanilla クライアントで正しく表示される」）は、利用者に頼む。サーバーが送る形が codec と一致していることまでが、ここでの検証
- 単体（`lodeframe-text`）: builder（`append`・`join`・`+`・`collect` の平坦化と「左のスタイルが漏れない」）、`from_json`（文字列・配列・オブジェクト・全種・不正・数値/真偽値の拒否・深さ）、`to_json` と `from_json` の往復
- 単体（`lodeframe-protocol`）: NBT の正解値（全種）、NBT の往復（`Component → Nbt → Component` が一致）、不正な NBT の拒否、`RUST_MIN_STACK=524288` で深さ上限
- 統合: ボットが受けるチャットなどの NBT が同じ形で届く（既存の経路で子要素・hover・click つきを送る）
- `mise run check`・`mise run msrv`

## 利用者に見える変更（v0.x）

- **`Component` のフィールドが `text` から `content`・`children` に変わった**（`as_text()` が代わり）。`Style` は `Copy` でなくなり、`click`・`hover`・`insertion`・`font`・`shadow_color` が増えた
- `Content`・`NbtSource`・`ClickEvent`・`HoverEvent`・`Decoration`、`Component::{from_json, join, append, append_all}`、`+`、`FromIterator`、各生成関数が増えた
- `Component::to_json` が全種に対応する
- 新しい依存: `lodeframe-text` に `serde_json`（読む側）。理由は上

## 確認して決めたこと（2026-10-08）

| 判断 | 選んだ案 | 選ばなかった案 |
|---|---|---|
| `Component` の形 | `content: Content`（enum）+ `style` + `children` | `text` を残して欄を足す（既存コードは壊れないが、`text` と `translate` が同時に立つ不正な状態を作れ、Option が増え続ける） |
| builder | `append` 系 + `join`/`empty`/`newline`/`space` に加えて `+` と `FromIterator` | `+` と `FromIterator` を外す（API は小さいが、利用者の要望で足すことになった） |

## 決めたこと（確認なし）

- JSON を読む `serde_json` は `lodeframe-text` に足す（M2-09 の D40 と同じ理由）
- 色は書くとき小文字、読むときは両方
- 数値・真偽値の単独 JSON は拒否（26.3 の codec に合わせる）
- 深さの上限は 32（`serde_json` の 128 段より手前に当たる値。実測で 64 は JSON では届かなかった）

## やらないこと

| 項目 | なぜ外したか | 受け皿 |
|---|---|---|
| `ClickEvent::Custom`（payload は任意の NBT） | `lodeframe-text` は NBT 型を持たない（D20）。payload に使う場面がまだ無い | 要望が出たとき（payload を `String` の SNBT で持つ案もある） |
| `ClickEvent::ShowDialog`・`OpenFile` | ダイアログのレジストリが要る（別の REQ）。`OpenFile` はクライアントだけの動作 | ダイアログを入れるとき |
| `Content::Sprite` のプレイヤーの頭（`player:{name,...}`） | プロフィール（`ResolvableProfile`）の型が要る。スキンの REQ と一緒に決める | プロフィールの REQ |
| `ShowItem` の `components` | ItemStack の型（M2-20）が要る。`id` と `count` だけ持つ | M2-20 |
| `FontDescription` のスプライト形式（`font` が `{...}` のとき） | `font` は id の文字列だけ。他の形式は `Unsupported` で読み込みを拒否する | 要望が出たとき |
| 色の大文字小文字の正規化の出力側 | クライアントは区別しない | — |
| MiniMessage との変換 | M2-12 | M2-12 |

## 実装の順

1. `lodeframe-text` のモデル（`Content`・`Style`・`ClickEvent`・`HoverEvent`）と builder・`+`・`FromIterator`
2. `to_json` を全種に広げる。`from_json`（`serde_json`）と深さ・不正の扱い
3. `lodeframe-protocol` の NBT 書き出し・読み取りと正解値のテスト
4. 本体・ボット・example・既存のテストを新しい形に直す（`text` フィールド・`Copy`）
5. 逆向きの確認（Rust → SNBT → Java の codec）
6. 文書（requirements の受入条件、minestom-parity、architecture、backlog、v0.2-plan）。PR

## 実装の結果

- 正解値: `crates/lodeframe-protocol/tests/component_golden.txt`（30 種。実 codec がネットワーク形式で書いた NBT の 16 進数）。作り方はテストファイルの冒頭に書いた
- **書いた形が実 codec と一致する**: 30 種すべてで、キーの順と色の大文字小文字を除いて同じ NBT になる
- **実 codec が読める**（逆向きの確認、2026-10-08）: 30 種の NBT（Rust が書いたバイト列）を `NbtIo.readAnyTag` と `ComponentSerialization.CODEC` で読み、30 種すべて成功。読んだ結果を書き直して読み直した Component も一致
- 実 codec との突き合わせで直したこと: `interpret` と `plain` の両方 `true` は codec が拒否するので `NbtMode` に変えた／色の名前は小文字だけ受ける／`with` の数値・真偽値は文字として受ける／`change_page` は 1 から
- 深さの上限は 32（上の「深さ」を参照）
- 実クライアントでの表示は未確認（利用者に頼む）
