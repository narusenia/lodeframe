# MiniMessage の実装計画（M2-12）

> **Status**: 実装済み — 2026-10-08

要件: REQ-TEXT-002。決定: D19・D20・D43・D44（[decisions.md](../decisions.md)）。
`lodeframe-text` だけの変更だが、公開 API（`mini` モジュール・`TagResolver`・2 種類の parse）の形を決めるのと、M2-13（`text!`）がこのパーサを使うので、コードの前にここで形を決める。

## 目的

設定ファイル・DB・コマンド引数のような**実行時の文字列**を Component にする（D19）。

```rust
use lodeframe_text::mini::{self, TagResolver};

let tags = TagResolver::new()
    .unparsed("player", name_typed_by_a_user)   // `<red>` と打たれてもタグにならない
    .component("item", item_name);
let line = mini::parse_with("<gold>Hi <bold><player></bold>, you got <item>!", &tags)?;
let same = mini::parse_lenient("<red>broken <unknown> input");     // 必ず Component を返す
let text = mini::serialize(&line)?;                                // Component → MiniMessage
```

## 1. 公開 API（`lodeframe_text::mini`）

| 項目 | 内容 |
|---|---|
| `parse(&str) -> Result<Component, Error>` | 厳格。壊れた入力はバイト位置つきの `Err` |
| `parse_with(&str, &TagResolver)` | 同上。利用者のタグ・placeholder を足す |
| `parse_lenient(&str) -> Component` | 壊れた部分を**平文として残す**。必ず返す |
| `parse_lenient_with(&str, &TagResolver)` | 同上 |
| `serialize(&Component) -> Result<String, SerializeError>` | Component → MiniMessage。表せない内容は `Err` |
| `TagResolver` | 名前 → 処理の表。builder で作る（下） |
| `Error { position, kind }` | `position` は入力の**バイト位置**。`kind` は `non_exhaustive` の enum |

`Component::from_mini` のような別名は足さない（入口を 1 つにする）。再 export は `lodeframe::text::mini`（facade が `lodeframe_text` ごと出している）。

### TagResolver（利用者の判断: Adventure 風。D44）

```rust
TagResolver::new()
    .unparsed("name", "text")             // 文字列をそのまま差す。タグとして解釈しない（安全）
    .component("item", component)         // Component をそのまま差す（安全）
    .parsed("greeting", "<green>hi")      // MiniMessage として解釈して差す（**入力を信用できるときだけ**）
    .insert("n", |args| Some(Component))  // 独自タグ。`<n:a:b>` の引数を受けて Component を差す
    .style("loud", |args| Some(Style))    // 独自タグ。`<loud>…</loud>` の中身に Style を掛ける
    .and(other)                           // 2 つの resolver を連結する（先のものが優先）
```

- 名前は `[a-z0-9_-]` の 1 文字以上。違えば `panic!`（名前はコードに書くリテラルで、利用者の入力ではない。`# Panics` に書く）
- タグ名は**大文字小文字を区別せず**照合する。利用者の resolver を先に引き、標準タグは後（Adventure と同じ。同名なら利用者が勝つ）
- placeholder に引数（`<name:x>`）を付けたら `BadArgument`
- `parsed` は自分自身を含んでよい（深さ制限で止まる）
- `Clone`・`Send + Sync`（独自タグは `Arc<dyn Fn + Send + Sync>`）。`Debug` は手書き（クロージャを出さない）

## 2. 構文

### 字句

- `<名前[:引数[:引数…]]>` が開きタグ、`</名前[:…]>` が閉じタグ。名前は `[A-Za-z0-9_#!?-]`（`#` は `<#rrggbb>`、先頭の `!` は否定）
- 引数は `:` 区切り。`'…'` か `"…"` で囲むと `:`・`>`・空白を含められ、中では `\'`（`\"`）と `\\` だけがエスケープ。囲みは引数の先頭でだけ開く
- 本文の `\<` は `<`、`\\` は `\`。それ以外の `\x` は 2 文字そのまま
- `<` のあとが名前の文字でない（`a < b`、`<3`）は、タグではなく**平文**（厳格でもエラーにしない）。名前の文字が続いたあと `>` が無いまま終わる（`<red`）は `Unterminated`（寛容では平文）

### 標準タグ

| タグ | 内容 |
|---|---|
| 色 | `<red>`（16 色の名前。`grey`・`dark_grey` も）・`<#rrggbb>`・`<color:x>`（`c`・`colour`）。`x` は名前か `#rrggbb` |
| 装飾 | `bold`(`b`)・`italic`(`i`, `em`)・`underlined`(`u`)・`strikethrough`(`st`)・`obfuscated`(`obf`)。`<!bold>` と `<bold:false>` は明示の off、`<bold:true>` は on |
| `<reset>` | これまでに開いたタグをすべて閉じる（以後の文字は無装飾）。`reset` で閉じられたタグの閉じタグが後で来ても、エラーにしない |
| `<gradient[:色…][:phase]>` | 中の文字を 1 文字ずつ色付けする。色が無ければ黒→白。`phase` は最後の引数が数のとき（-1〜1） |
| `<rainbow[:!][:phase]>` | 虹色。`!` で逆向き |
| `<transition:色…:phase>` | `phase`（0〜1）の位置の**1 色**を、中身全体に掛ける |
| `<hover:show_text:'…'>` | 中は MiniMessage として解釈（placeholder も効く）。`show_item:id[:count]`・`show_entity:type:uuid[:name]` |
| `<click:action:値>` | `open_url`・`run_command`・`suggest_command`・`change_page`（1 以上）・`copy_to_clipboard`。値の `:` は残りの引数をつなぎ直す（`<click:open_url:https://x>` が書ける） |
| `<insertion:text>`・`<font:id>` | 値は残りの引数を `:` でつなぎ直す（`<font:minecraft:uniform>`） |
| `<shadow:色[:alpha]>` | alpha は 0〜1（既定 1）。`#aarrggbb` の 8 桁も受ける。`<!shadow>` は影を消す（透明） |
| `<key:key.jump>` | Keybind |
| `<lang:key[:引数…]>`（`tr`・`translate`） | Translatable。引数は MiniMessage として解釈。`<lang_or:key:fallback[:引数…]>`（`tr_or`・`translate_or`）は fallback（平文）つき |
| `<newline>`（`br`） | 改行 |

入れない: `selector`・`score`・`nbt`・`sprite`・`head`（サーバー側で解決・実クライアント確認が要るもの。利用者入力から出せると危ない。D44）。**入れていないタグは未知のタグとして扱う**。

### 構造と閉じ方

- 開きタグは入れ子に積み、閉じタグは**同じ名前の一番内側**を閉じる。名前は別名を正規化してから照合する（`<b>`…`</bold>` は閉じる）
- 閉じていないタグは、文字列の終わりで閉じる（**厳格でも**エラーにしない。チャットの `<red>Error` を許すため）
- 閉じタグが一番内側でない（`<red><bold>a</red>`）: 厳格は `Mismatched`、寛容は間のタグも閉じる
- 開きタグの無い閉じタグ（`</red>`）: 厳格は `UnmatchedClose`、寛容は平文
- 未知のタグ・不正な引数: 厳格は `UnknownTag`・`BadArgument`、寛容は**そのタグの原文を平文**として残す
- 入れ子の深さは **32** まで（`json.rs` の `MAX_DEPTH` と同じ。hover や parsed placeholder の入れ子も同じ予算を使う）。超えたら厳格は `TooDeep`、寛容はそのタグを平文にする。パーサの入れ子は積みで持つが、hover・parsed placeholder は再帰するので、**スタックを絞ったテスト**で確かめる（rust.md「再帰とスタック」）

## 3. 出力の形

- 開いたタグは、その Style を持つ**空の Component**で、中身が children になる（入れ子のまま）。本文は 1 つの `Component::text`
- `gradient`・`rainbow` は中の本文を **1 文字（`char`）ごと**の Component にして色を付ける。色の数は、`gradient` の範囲に入る本文（placeholder の `unparsed` を含む。Component placeholder・lang・key は数えず、色も付けない）の文字数で決める。範囲の中に明示の色タグがあれば、その中は明示の色が勝つ（文字の位置は進める）
- 色の補間は RGB の線形、`round`。虹は色相 `i / n`（HSV、S = V = 1）。phase は位置を足して `[0,1)` に折り返す（phase 0 のときは両端の色がちょうど出る）

**未検証**: gradient・rainbow の色の値と phase の向きは、Adventure のソースを見ずに式を決めた。「同じ入力で同じ見た目」（受入条件）は Adventure 本体との突き合わせが要る。jar の取得になるので、利用者に確認してから行う。

## 4. serialize（Component → MiniMessage）

- 1 つの Component の Style を開きタグにし（色 → 影 → 装飾 → insertion → font → click → hover の順）、内容・children を書いて、逆順に閉じる。色は名前か `<#rrggbb>`。装飾の `Some(false)` は `<!bold>`
- 本文は `\` → `\\`、`<` → `\<`。タグの引数は `'…'` で囲み、中の `\`・`'` をエスケープする。hover の `show_text` の中身は再帰して書く
- 書ける内容: Text・Translatable（fallback つきは `lang_or`）・Keybind。**書けない内容**（Score・Selector・Nbt・Sprite）は `SerializeError`（理由は種類の名前）。標準タグに入れていないので、書くと再び読めなくなるため
- gradient の復元はしない（1 文字ごとの色タグになる）。`parse(serialize(c))` は、**見た目が同じ**になることを保証する（Component の構造は同じとは限らない。空のコンテナの入れ子が変わる）
- 書く側の再帰深さは `MAX_DEPTH`（32）で止める。超えたら `SerializeError`

## 5. 安全

- **利用者入力は `unparsed`・`component` で渡す**とタグとして解釈されない（受入条件）。テストで `<red>`・`</bold>`・`<click:run_command:/op me>`・`\<` を `unparsed` に渡し、平文のまま Style が付かないことを確かめる
- `parsed` と `parse*` の第 1 引数は信頼できる入力のためのもの（doc に書く）。信頼できない文字列に `click` を許したくなければ、`TagResolver` を使わず**標準タグを絞る**必要があるが、v0.2 では絞る設定は持たない（やらないこと）
- パニックしない: 任意のバイト列（UTF-8）で `parse`・`parse_lenient` が返ること。決定的な乱数でタグ記号を詰めた文字列を大量に流すテストを置く
- 入力の大きさの上限は持たない（gradient は 1 文字 1 Component になるので、入力に比例して Component が増える）。REQ-NET-007（v0.3）の対象

## 6. ファイル

- `crates/lodeframe-text/src/mini.rs`: 公開 API・`Error`・`TagResolver`
- `crates/lodeframe-text/src/mini/parse.rs`: 字句と木づくり（開閉・reset・深さ）
- `crates/lodeframe-text/src/mini/tags.rs`: タグ名 → 意味（標準タグの引数の解釈）、gradient などの色
- `crates/lodeframe-text/src/mini/write.rs`: serialize
- 依存は増やさない。M2-13（`text!`）は、`parse.rs` の木と `tags.rs` の解釈をマクロから呼ぶ想定で、公開の形は M2-13 の計画で決める（ここでは `pub(crate)`）

## 確認して決めたこと（2026-10-08）

| 判断 | 選んだ案 | 選ばなかった案 |
|---|---|---|
| タグの範囲 | 標準タグ一式（要件のタグ + insertion・font・key・lang・newline・shadow・transition・否定・エスケープ） | 要件の最小限（Adventure と「同じ入力で同じ見た目」が主要タグでも崩れやすい） / selector・score・nbt・sprite・head まで（利用者入力から解決系のタグが出せてしまう。実クライアント確認も要る） |
| 不正な入力 | 厳格（位置つき `Err`）と寛容（平文に落とす）の両方の関数 | 厳格のみ（実行時の文字列が壊れていると表示できない） / 寛容のみ（位置つきエラーを返せず、受入条件を満たさない） |
| placeholder・独自タグ | Adventure 風の `TagResolver`（`unparsed`・`component`・`parsed`・独自の `insert`・`style`） | 呼び出しごとの引数だけ（独自タグが作れない） / `{name}` の文字列置換（MiniMessage の仕様に無く、Adventure と同じ入力で動かない） |

## やらないこと

| 項目 | なぜ外したか | 受け皿 |
|---|---|---|
| selector・score・nbt・sprite・head タグ | サーバー側で解決するもの。利用者入力から出せると危なく、クライアント確認も要る | 要望が出たとき |
| 標準タグを絞る設定（click だけ禁止など） | 信頼できない入力は `unparsed` で渡せば足りる。絞る設定は API を増やす | 要望が出たとき |
| Adventure 本体との色の突き合わせ | jar の取得は利用者に確認してから | 実装後に利用者へ確認 |
| gradient の復元・`<lang>` 引数の型の保持 | 見た目が同じならよい | — |
| 入力の大きさ・タグ数の上限 | REQ-NET-007 の対象 | v0.3 |
| ANSI・legacy（`§`）への変換 | REQ-TEXT-004（v0.3） | v0.3 |

## テスト

- 標準タグ 1 つずつ: 色（名前・hex・`color:`・別名・大文字）・装飾（別名・`!`・`:false`）・hover 3 種・click 5 種（`:` つきの値）・insertion・font・shadow・key・lang・lang_or・newline・reset・transition
- gradient・rainbow の色の値（3 文字・phase・色が 3 つ以上・1 文字・明示の色が中にある・入れ子）
- 構造: 入れ子・閉じ忘れ・別名の閉じ・交差・開きの無い閉じ・reset のあとの閉じ・エスケープ（`\<`・`\\`・引数の `\'`）
- 厳格と寛容: 同じ壊れた入力の `Err`（種類と位置）と平文の結果
- placeholder: unparsed・component・parsed・独自 insert・独自 style・同名で利用者が勝つ・引数つきは BadArgument・安全（上の 4 入力）・parsed の自己参照が止まる
- serialize: 書いた文字列を読むと見た目が同じ（Style・hover・click・translatable・エスケープ・入れ子）／書けない内容は `Err`／深さ
- 堅牢性: 決定的な乱数の入力でパニックしない・深い入れ子（10000 段）が溢れない（512KB のスレッド）
- `mise run check`・`mise run msrv`・`mise run lint:license`

## 実装の順

1. `mini.rs`（`Error`・`TagResolver`）と `parse.rs`（字句・木・厳格／寛容）
2. `tags.rs`（標準タグの解釈・色）と木から Component への変換
3. `write.rs`（serialize）
4. テスト、docs（requirements・minestom-parity・architecture・backlog・decisions D44）。PR

## 実装の結果

- 計画どおり。公開は `lodeframe_text::mini`（`parse`・`parse_with`・`parse_lenient`・`parse_lenient_with`・`serialize`・`TagResolver`・`Error`・`ErrorKind`・`SerializeError`）。`Color::rgb()` を足した（gradient が名前の色の RGB を使う）
- 計画から足した・決めたこと
  - `parsed` placeholder が自分を含むと、深さ 32 では二股に分かれて指数的に増えるので、**1 回の parse で入れられる `parsed` は 256 まで**（`TooManyExpansions`）
  - 引数の囲み（`'…'`）の直後が `:` か `>` でないものは `Malformed`
  - `<hover:show_item:…>` の id は、`minecraft:stone:2` のように名前空間が `:` で割れるので、**末尾が整数ならその数を count、そうでなければ残りを id につなぐ**。囲む（`'minecraft:stone':2`）のが確実で、`serialize` は囲んで書く
  - `<hover:show_entity:type:uuid[:name]>` の type も同様に、最初に UUID として読める引数の手前までを type につなぐ
  - hover のネスト: 各段で内側の引用符をエスケープするので、文字列は段ごとに倍になる。32 段に届くには非現実的な長さが要り、スタックの限界より先に入力の大きさが制約になる。深さ 32 の打ち切りは、`parsed` placeholder の連鎖と、閉じないタグ 1 万個で、512KB のスレッドで確かめた
- 検証: 単体テスト 35 件（タグごと・色の値・厳格と寛容・placeholder の安全・独自タグ・serialize と往復・深さ・乱数 2 万件で panic しないことと「読んだものは書けて、書いたものは同じ見た目に読める」）、doctest、`mise run check`・`msrv`・`lint:license`
- 未検証: Adventure 本体との見た目の突き合わせ（gradient・rainbow・transition の式と phase の向き、gradient の中の明示の色の扱い）。jar の取得になるので、利用者に確認してから行う
