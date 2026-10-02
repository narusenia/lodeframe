# lodeframe リポジトリガイド

## 概要

lodeframe は Rust 製の軽量 Minecraft: Java Edition サーバー**ライブラリ**
（Minestom 型）。vanilla の挙動は持たず、利用者が自分のサーバーを組む。
名前は仮。

実装と文書が食い違うときは**実装が正**。気づいた文書はその変更で直す。

## リポジトリの地図

1 crate 1 行。各 crate が守る不変条件は `.agents/rules/` にあり、ここには書かない。

- `crates/lodeframe-text`: Component と MiniMessage パーサ
- `crates/lodeframe-protocol`: 基本型・Encode/Decode・NBT・パケット・生成データ。本体に依存しない
- `crates/lodeframe-macros`: proc-macro
- `crates/lodeframe-bot`: protocol だけで書いた軽量ボット（統合テストと負荷試験）
- `crates/lodeframe`: 本体。上の 3 つを re-export する facade
- `xtask`: 開発タスク（datagen / bot / bench）
- `examples/lobby`: イベント API だけで書いたロビー
- `docs/`: 索引は `docs/README.md`

主要な参照:

- `docs/requirements.md`（何を作るか）
- `docs/decisions.md`（なぜそう決めたか。覆すときは行を消さず Superseded にする）
- `docs/specifications/architecture.md`（どう組むか）
- `docs/implementation/backlog.md`（着手できる単位の一覧。作業はここから探す）
- `docs/implementation/roadmap.md`（順序と根拠）

## パス別ルール

共有ルールは `.agents/rules/`。ファイルを編集する前に、対象パスに一致する
`paths` frontmatter を持つルールをすべて読む。Claude Code は `.claude/rules`
経由で同じルールを自動で読む。

- `.agents/rules/rust.md`: Rust・Cargo・crate 境界・並行モデル

**このファイルは索引であり、マニュアルではない。** ルールは実装しながら育てる:

- ルールには**必ず理由**を添える（決定事項なら `docs/decisions.md` の ID）。理由のないルールは次の人に消される
- 「同じ指摘を 2 回した」「同じバグを 2 回踏んだ」時点でルール候補にする
- 機械的に検出できるものは lint / テストにし、ルール文書には「なぜそれが要るか」だけ残す
- ルールを動かすときは理由も一緒に動かす

## 設計ゲート

複数 crate にまたがる変更や、サブシステム（所有モデル、イベント、プロトコル codegen）
の作り直しは、コードを書く前に `docs/implementation/` へ計画書を置く。
単一 crate 内の小さな変更は不要。

## 設計判断の確認

利用者に設計判断を頼むときは、**選択肢ごとにメリットとデメリットを示し**、推奨があれば先頭に置いて理由を添える。
結論だけ聞くと、利用者は根拠を知らないまま選ぶことになり、後で覆す手間が増える（M2-02 では選択肢を示した後に、
選ばなかった案も作れると分かって設計が広がった）。判断の結果は計画書か `docs/decisions.md` に理由つきで残す。

## 検証

- `mise run check`（fmt・clippy・test）が検証の正。CI も同じタスクを回す
- MSRV は `mise run msrv`（`rust-version` は Cargo.toml が正）
- 新しい clone / worktree では最初に `mise trust`

## Git

- 利用者が頼まない限り commit / push / PR はしない
- commit は論理単位 1 つ、英語 1 行の Conventional Commit（`feat:` `fix:` `refactor:` `docs:` `test:` `chore:` `perf:` `ci:`）。prefix の後は小文字
- ブランチ名は同じ prefix + 具体的な kebab-case（例: `feat/varint-codec`）
- commit メッセージにタスク ID・issue 番号・エージェント名を入れない
- PR は merge commit でマージする

## 完了の定義

- 頼まれた動作が、無関係な変更なしで実装されている
- 動作を確かめるテストがある。無い場合はその旨を書く
- `mise run check` が通る
- 影響する文書（requirements・backlog・計画書）が更新されている
- 報告に、変更ファイル・実施した検証・残る制約が入っている
