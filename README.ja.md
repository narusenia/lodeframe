<p align="center">
  <img src="assets/logotype.png" alt="lodeframe" width="500">
</p>

[English](README.md) | 日本語

[Minestom](https://github.com/Minestom/Minestom) の考え方を Rust で実装する、
軽量な Minecraft: Java Edition サーバー**ライブラリ**。

> **ステータス: pre-alpha。** まだ動作しない。名前は仮。

lodeframe は vanilla のゲームプレイを持たない。mob AI、レッドストーン、
ワールド生成、クラフトは含まない。利用者はプロトコル・ワールド・
エンティティ・イベントの API を使い、自分のサーバー（ロビー、ミニゲーム）を
Rust で組み立てる。

## 目標

- **ライブラリが本体**: 利用者は `lodeframe` crate に依存し、挙動を自分で書く
- **最新プロトコルのみ**: 最新の Minecraft リリースに追従する。古いクライアントは ViaProxy を前段に置く
- **ロック不要のゲーム状態**: 各 `Instance`（ワールド）を 1 スレッドが所有し、20 TPS で tick する。通信は tokio
- **素直なイベント API**: 階層イベントツリーへの型付きハンドラ登録（例: `node.on::<PlayerChat>(|ev, ctx| ..)`）
- **Adventure 相当のテキスト層**: リッチな Component、MiniMessage パーサ、メッセージ・Title・ActionBar・Sound・BossBar を送る Audience
- **マクロによる開発体験**: `derive(Encode, Decode)`、`#[command]`、`derive(Event)`、テキスト・アイテム・GUI の宣言的マクロ
- **テストしやすい設計**: `test-util` feature のヘッドレス `TestEnv` で tick を手動で進め、fake player を操作できる。ポートを開かずに通常の `#[test]` でゲームロジックを検証できる
- **軽さを計測で示す**: メモリ・起動時間・ボット負荷のベンチマークを Minestom と比較する

## 目標外

vanilla との完全互換、コアでの複数バージョン対応、Anvil 形式での保存。
AI・pathfinding・戦闘は、必要なら後から別の util crate で提供する。

## ロードマップ

| マイルストーン | 範囲 |
|---|---|
| v0.1 | ログイン、平坦ワールド、移動、プレイヤー表示、チャット、ブロック設置・破壊 |
| v0.2 | Velocity modern forwarding、コマンド、エンティティと簡易物理、インベントリ、非同期タスク、Component / MiniMessage / Audience |
| v0.3 | online mode、Anvil 読込、ライティング、複数 Instance、スコアボード、アイテム・GUI マクロ |
| v0.4 | 性能目標の達成、crates.io 公開 |

詳細は [`docs/`](docs/README.md)。

## ライセンス

次のいずれかを選択して利用できる。

- Apache License, Version 2.0（[LICENSE-APACHE](LICENSE-APACHE)）
- MIT license（[LICENSE-MIT](LICENSE-MIT)）

明示的に別段の意思表示をしない限り、Apache-2.0 ライセンスの定義に従って
本プロジェクトへの取り込みを意図して提出されたコントリビューションは、
追加の条項や条件なしに上記のデュアルライセンスで提供されるものとする。

lodeframe は Mojang Studios および Microsoft とは無関係である。
「Minecraft」は Mojang Synergies AB の商標である。
