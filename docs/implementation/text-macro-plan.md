# `text!` の実装計画（M2-13）

> **Status**: 実装済み — 2026-10-09

要件: REQ-MACRO-005。決定: D19・D20・D44・D45（[decisions.md](../decisions.md)）。
`lodeframe-macros`・`lodeframe-text`・`lodeframe`（facade）にまたがり、公開 API（マクロと `From` の追加）が増えるので、コードの前にここで形を決める。

## 使い方

```rust
use lodeframe::text;

// 静的な文字列。タグの誤りはコンパイル時に分かる
let motd = text!("<gradient:gold:red>Welcome</gradient> to <bold>lodeframe</bold>");

// {name} はスコープの変数（format! の暗黙のキャプチャと同じ）。タグとして解釈されない
let name = "Alice";
let count = 3;
let line = text!("<green>{name}</green> joined (<yellow>{count}</yellow> online)");

// 名前つき引数。{{ と }} は波括弧そのもの
let line = text!("<red>{who}</red>: {{hello}}", who = player.name());
```

戻り値は `Component`。`lodeframe::text` はモジュール（`lodeframe_text`）でもあるが、マクロと型・モジュールは名前空間が別なので、`use lodeframe::text;` で `text!(..)` と `text::Component` の両方が書ける（`std::vec` と `vec!` と同じ）。

## 1. 展開（利用者の判断: 実行時パーサを呼ぶ + 静的な文字列は 1 回だけ。D45）

コンパイル時に、実行時と**同じパーサ**（`lodeframe_text::mini`、D20）で入力を検証する。展開されるコードは実行時パーサの呼び出しなので、「実行時パーサと同じ Component を生成する」は構造上成り立つ。

- placeholder が無い: 展開は `static` の `LazyLock<Component>` で、初回に `mini::parse_lenient(..)`、以後は `clone`
- placeholder がある: 展開は `mini::parse_lenient_with(.., &TagResolver::new().component(..)..)`（呼ぶたびに解析する）
- 実行時は**寛容**版を呼ぶ。コンパイル時に厳格版で通っていて、placeholder の中身（`component`）はタグとして解釈されず構文の正否に影響しないので、実行時に失敗や食い違いは起きない（パニックする経路を作らない）

```rust
// text!("<green>{name}</green> {n}", n = count) の展開
{
    let tags = ::lodeframe::text::mini::TagResolver::new()
        .component("lodeframe-ph-0", ::lodeframe::text::Component::from(name))
        .component("lodeframe-ph-1", ::lodeframe::text::Component::from(count));
    ::lodeframe::text::mini::parse_lenient_with("<green><lodeframe-ph-0></green> <lodeframe-ph-1>", &tags)
}
```

選ばなかった案: Component の構築コードを生成する（実行時の解析が要らないが、出力器を書く必要があり、文字数が実行時に決まる placeholder は gradient・rainbow の中で使えなくなる）、常に実行時に解析する（静的な文字列も毎回解析する）。

## 2. 入力の構文と placeholder（利用者の判断: `Into<Component>` + `From` を足す。D45）

```
text!( "リテラル" [, 名前 = 式]* [,] )
```

- 最初の引数は**文字列リテラル**（raw 文字列も可）。それ以外はコンパイルエラー
- リテラルの中: `{{` は `{`、`}}` は `}`、`{名前}` は placeholder。名前は Rust の識別子。`{0}`・`{a.b}`・`{x:?}`・閉じていない `{` は構文エラー
- `{名前}` の値は、`名前 = 式` があればその式、無ければ**同名の変数**（暗黙のキャプチャ。`format!` と同じく、リテラルの span で解決する）
- 値は `Component::from(値)`。同じ名前は 1 回だけ評価し、使った箇所の数だけ複製する（`format!` が参照を取るのと同じで、`String` を 2 回使っても move の二重にならない）
- 値は**タグとして解釈されない**（`TagResolver::component` と同じ。利用者の入力を入れても安全。D44）。`parsed`（MiniMessage として解釈）は使えない。必要なら実行時の `parse_with`
- `名前 = 式` を使わなかったら、使っていない引数のコンパイルエラー

### `From` の追加（`lodeframe-text`）

`Into<Component>` を取る既存の API（`append`・`arg`・`hover_text` など）でも使えるようになる。

- 整数（`i8`〜`i128`・`u8`〜`u128`・`isize`・`usize`）・`f32`・`f64`・`bool`・`char`: `Display` の文字列の `Component::text`
- `&String`・`&Component`（`Component` は複製）

## 3. placeholder を置ける場所

実行時の `component` placeholder と同じ。タグの**引数の中**（`<click:run_command:/msg {name}>` の `{name}`）は展開されない（引数は文字列のまま）。`hover:show_text` の中（引用符で囲んだ MiniMessage）は置ける。

- 展開されない `{名前}` は、**黙って壊れないよう**コンパイルエラーにする。検出は、各 placeholder の値に 1 文字の目印（私用領域の 1 文字）の Component を渡して厳格に解析し、結果の Component の本文に目印が**使った回数ぶん**現れるかを数える（hover・translatable の引数・子も辿る）。数が合わなければ、その名前でエラー
- 目印は 1 文字なので、gradient の中でも 1 文字として数えられる。目印の文字はリテラルに含まれない範囲から選ぶ
- 内部のタグ名は `lodeframe-ph-<番号>`。リテラルにその文字列が含まれていたら、含まれない番号の範囲を選ぶ

## 4. エラー（REQ-MACRO-005: 位置を指す）

- `mini::Error`（種類と**バイト位置**）を、元のリテラルの位置に直す。展開前の文字列（`{名前}` をタグに置き換え、`{{` を `{` にしたもの）と元の文字列の対応表を持ち、位置を引き戻す
- **stable の Rust では、リテラルの途中だけに下線を引けない**（`Literal::subspan` は nightly の `proc_macro_span`）。そのため、エラーの span は**リテラル全体**で、文言に位置と抜粋・印を入れる:

```
error: invalid MiniMessage at byte 5: unknown tag <nope>
       <red><nope>
            ^
```

- 位置は文字列の**値**のバイト位置（Rust のエスケープを解いたあと）。抜粋は位置のある行を出す。印は文字数で置く
- 受入条件「タグの誤りがリテラル内の位置を指すコンパイルエラー」は、この形で満たす（下線ではなく文言）。nightly が安定したら span だけ差し替えられる

## 5. ファイル

- `crates/lodeframe-macros/src/text.rs`: 入力の解析・placeholder の切り出し・検証・展開（`proc_macro2` と `syn` だけで書き、単体テストを置く）
- `crates/lodeframe-macros/src/lib.rs`: `#[proc_macro] pub fn text`
- `crates/lodeframe/src/lib.rs`: `pub use lodeframe_macros::text;`（`macros::text` からも使える）
- `crates/lodeframe-text/src/lib.rs`: `From` の追加
- 生成コードの crate の道は `::lodeframe::text`（`derive` の既定と同じ。`lodeframe` に依存すれば全部使える）。他の crate の中で使うための上書きは持たない（`text!` を使う crate は `lodeframe` に依存する）

## 利用者に見える変更（v0.x）

- `text!` が増えた（`lodeframe::text!`・`lodeframe::macros::text!`）
- `Component` に `From<数値・bool・char・&String・&Component>` が増えた。既存の呼び出しは変わらない
- 新しい依存は無い（`lodeframe-macros` は `lodeframe-text` に依存済み）

## 確認して決めたこと（2026-10-09）

| 判断 | 選んだ案 | 選ばなかった案 |
|---|---|---|
| 展開 | 実行時パーサを呼ぶ。placeholder の無いリテラルは `LazyLock` で 1 回だけ | Component の構築コードを生成する（出力器が要り、gradient の中の placeholder に制限が付く） / 常に実行時に解析する |
| placeholder の値 | `Into<Component>` に整数・小数・bool・char・`&String`・`&Component` の `From` を足す。暗黙のキャプチャと `名前 = 式` | `Into<Component>` のみ（数値は毎回 `to_string()`） / `Display`（装飾済みの Component を入れられない） |

## やらないこと

| 項目 | なぜ外したか | 受け入れ先 |
|---|---|---|
| `{名前:parsed}`（MiniMessage として解釈する placeholder） | 利用者入力が通る危険な経路をマクロに持ち込まない。実行時の `TagResolver::parsed` で足りる | 要望が出たとき |
| 式をそのまま `{expr}` に書く | proc-macro は識別子の暗黙キャプチャまで（`format!` と同じ）。式は `名前 = 式` | — |
| リテラルの途中に下線を引く | stable に無い（`Literal::subspan`） | nightly の安定後 |
| 独自タグ（`TagResolver::insert`・`style`）を `text!` から渡す | 実行時の `parse_with` を使う | 要望が出たとき |
| 他の crate 内で使うための crate 道の上書き | `lodeframe` に依存すれば足りる | 要望が出たとき |

## テスト

- 単体（`text.rs`）: `{{`・`}}`・`{名前}` の切り出しと構文エラー（`{0}`・`{a.b}`・閉じていない `{`・空の `{}`）／位置の引き戻し（`{名前}` と `{{` を挟んだあとの位置）／目印の文字の選び方／内部のタグ名がリテラルと衝突しない
- 統合（`crates/lodeframe/tests/text_macro.rs`）: 静的な文字列が `mini::parse` と等しい／`{name}` の値が `parse_with(.., component)` と等しい（`<name>` と書いた実行時の結果と同じ）／同じ名前を 2 回使う（`String` でも）／名前つき引数・暗黙のキャプチャ・`{{ }}`／数値・bool・char・`&String`・`&Component`／利用者入力の `<red>` がタグにならない／gradient の中の placeholder が実行時と同じ色になる／hover の中に置ける／静的な文字列が 2 回目以降も同じ値（`LazyLock`）／raw 文字列
- コンパイルエラー（trybuild、`tests/ui/text_*`）: 未知のタグ・閉じていない引用符・タグの引数の中の placeholder・使っていない名前つき引数・`{0}`・文字列リテラルでない引数。メッセージが位置と抜粋・印を含む
- `lodeframe-text`: 追加した `From` の単体テスト
- `mise run check`・`mise run msrv`・`mise run lint:license`

## 実装の順

1. `lodeframe-text` に `From` を足す
2. `text.rs`（入力の解析・切り出し・検証・展開）と単体テスト、`lib.rs` に登録、facade に再 export
3. 統合テストと trybuild のケース
4. 文書（requirements・minestom-parity・architecture・backlog・decisions D45）。PR

## 実装の結果

- 計画どおり。`lodeframe::text!`（`macros::text!` も）・`Component::from` の追加
- 計画から足したこと
  - `{名前}` の名前は、Rust の識別子（キーワードと `r#` を除く）だけを受ける。`{0}`・`{a.b}`・`{x:?}`・空・空白つきは構文エラー
  - 使っていない名前つき引数・同じ名前の二重指定はエラー（引数の span で指す）
  - 展開されない placeholder の検出は、1 文字の目印を値にして厳格に解析し、結果の本文に目印が**使った回数ぶん**あるかを数える。数が合わなければ「タグの引数の中には置けない」というエラー（引数の途中の 1 つだけ展開されない場合も検出する）
  - 展開は `static` の `LazyLock<Component>`（placeholder が無いとき）と `parse_lenient_with`。`Clone::clone(&*TEXT)`（`LazyLock` 自体は `Clone` でない）
- 検証: マクロの単体テスト 8 件、統合テスト 11 件（`mini::parse`・`parse_with` と等しい・利用者入力がタグにならない・gradient と hover の中の placeholder）、trybuild 7 件、`From` の単体テスト、`mise run check`・`msrv`・`lint:license`
- 制約: エラーの下線はリテラル全体（上の「エラー」）。値は `Component::from(値)` で 1 回 move されるので、後で使うなら `名前 = &値`（`&String`・`&Component` は `From` がある）
