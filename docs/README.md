# lodeframe ドキュメント索引

lodeframe は Rust 製の軽量 Minecraft: Java Edition サーバー**ライブラリ**。
Minestom と同じく vanilla の挙動を持たず、利用者が自分のサーバー
（ロビー、ミニゲーム等）をコードで組み立てる。名前は仮。

## 役割で引く

| 知りたいこと | 場所 |
|---|---|
| 何を作るか（要件） | [requirements.md](requirements.md) |
| なぜそう決めたか | [decisions.md](decisions.md) |
| どう組むか（設計） | [specifications/architecture.md](specifications/architecture.md) |
| どの順で作るか、なぜその順か | [implementation/roadmap.md](implementation/roadmap.md) |
| Minestom の機能との対照（どこで埋めるか） | [implementation/minestom-parity.md](implementation/minestom-parity.md) |
| 今どの単位に着手できるか | [implementation/backlog.md](implementation/backlog.md) |
| マイルストーンごとの実装計画 | [implementation/](implementation/) の `*-plan.md` |

## 開発

- ツールとタスクは mise（`mise.toml`）。`mise run check` が検証の正で、CI も同じタスクを回す。
- `mise run` で一覧、`mise run fmt:fix` で整形。
- プロトコルのデータ（ブロック状態・パケット ID 等）は `mise run datagen [version]` で再生成する。Java 25 と `curl`・`unzip` を使い、生成物は commit する。

## 規約

- 同じ内容を 2 箇所に書かない。実装と食い違うときは**実装が正**で、気づいた文書をその変更で直す。
- 要件 ID は `REQ-<領域>-<番号>`、実装単位 ID は `M<マイルストーン>-<番号>`。
- 計画書を更新したら `backlog.md` も同じ変更で更新する。
- 計画書は着手するマイルストーンの分だけ書く。先のマイルストーンは `roadmap.md` の粒度で十分。
