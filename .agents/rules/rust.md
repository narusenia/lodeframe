---
paths:
  - "Cargo.toml"
  - "crates/**/Cargo.toml"
  - "crates/**/*.rs"
  - "xtask/**"
  - "examples/**"
---

# Rust・crate 境界・並行モデル

各ルールの括弧内は根拠となる `docs/decisions.md` の ID。

## crate 境界

- `lodeframe-protocol` は `lodeframe`（本体）に依存しない。ボットや proxy が単独で使えるため（D14）
- `lodeframe-text` は protocol・macros の両方から使われる。これらのどちらにも依存しない（D20）
- 利用者は `lodeframe` だけに依存すれば全部触れる。新しい公開型は facade から re-export する（D14）
- proc-macro は `lodeframe-macros` にだけ置く。proc-macro crate は他の型を export できないため
- derive の生成コードは既定で `::lodeframe::protocol` を指す。protocol 内部では型に `#[lodeframe(crate = crate)]` を付ける。facade だけに依存する利用者のコードを動かすため（D22）
- derive の誤用はコンパイルエラーにし、`crates/lodeframe/tests/ui/` に trybuild のケースを足す。rustc 自身が出すエラーは snapshot に混ぜない（rustc の版で壊れるため）

## 再帰とスタック

- 入力を再帰でたどるデコーダには深さ制限を置き、**実測で**決める。NBT は 1 段あたり数 KB 使い、vanilla の上限 512 では 2MB のスレッド（tokio ワーカー）で溢れた。上限を変えるときはスタックを絞ったテストで確かめる（例: `RUST_MIN_STACK=524288`）
- 入力に含まれる長さで `Vec` を確保する前に、残りバイト数と照らして拒否する（`check_remaining`）

## ライセンス表記

- 新しい `.rs` ファイルは 1 行目に `// SPDX-License-Identifier: Apache-2.0 OR MIT` を置く。crate 単位で配布されてもライセンスが分かるように（D17）。`mise run lint:license` が検査する
- `crates/lodeframe/tests/ui/` の trybuild 用ファイルは対象外。snapshot が行番号を含み、1 行増えると全部書き直しになるため

## 並行モデル

- Instance の状態は 1 スレッドが `&mut` で所有する。`Arc<Mutex<_>>` で Instance を共有しない（D6）
- Instance 間の相互作用は message passing のみ。グローバル可変シングルトンを置かない（D6）
- イベントハンドラは同期 `fn`。ハンドラ内でブロッキング処理・`.await` をしない（D16）
- 非同期処理は `ctx.spawn(..).then(..)` で tokio に投げ、結果は同じ Instance スレッドで受ける（D16）
- tick ループは実時間を直接読まず、差し替え可能な時計を通す。テストハーネスが tick を手動で進めるため（D21）

## データと依存

- `src/generated/` は手で編集しない。`mise run datagen` で再生成する（D4, D5）。生成器は全ブロック状態で混合基数の規則を検証するので、その検証を緩めて通さない（D24）
- Mojang の生成元データ（server.jar、生成 JSON）を commit しない（D17）
- 新しい依存を足すときは、標準ライブラリと既存依存で足りないことを PR に書く
- `unsafe` は禁止（workspace lint で `forbid`）。必要になったら理由付きでこのルールを更新する

## テスト

- 利用者向けの挙動は `test-util` のヘッドレスハーネスで書く。実ソケットを開くテストは統合テスト（ボット）だけ（D15, D21）
- 公開 API を足したら、ハーネスから操作・検証できる形にする（D21）
