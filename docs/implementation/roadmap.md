# ロードマップ

> 最終更新: 2026-10-01

**どの順でやるか、なぜその順か**だけを持つ。単位の状態は [backlog.md](backlog.md)、
単位の中身は各計画書が正。

## 進捗

| MS | 内容 | 状態 | 計画書 | 完了日 |
|---|---|---|---|---|
| v0.1 | 接続〜スポーン + マルチプレイ基礎 | 完了 | [v0.1-plan.md](v0.1-plan.md) | 2026-10-01 |
| v0.2 | proxy 運用とゲームを作れる API | 未着手 | 未作成 | — |
| v0.3 | 単体公開と実ワールド | 未着手 | 未作成 | — |
| v0.4 | 性能目標の達成と公開 | 未着手 | 未作成 | — |
| v0.5 | エンティティの拡充 | 未着手 | 未作成 | — |
| v0.6 | ワールドの拡充 | 未着手 | 未作成 | — |
| v0.7 | プレイヤー UI の拡充 | 未着手 | 未作成 | — |
| v0.8 | 運用・監視の残り | 未着手 | 未作成 | — |

状態は `未着手` / `進行中` / `完了`。MS 内の全単位が完了条件を満たした時点で `完了`。

## 並べる基準

上にあるものが強い。

1. **プロトコル追従の土台を最初に**。codegen と derive が無い状態でパケットを書くと、後で全部書き直しになる。
2. **API の形を早く検証する**。イベント・所有モデルの誤りは後になるほど直せない。v0.1 にマルチプレイ基礎を入れるのはこのため。
3. **運用の現実に近い順**。proxy 配下運用（v0.2）が単体公開（v0.3）より多い。
4. **性能は計測してから最適化**。ベースラインは v0.1 で取り、目標の確定と達成は v0.4。
5. **Minestom と同じことができるまでを、ミニゲームで使う頻度の順に**（D30）。v0.4 までは各 MS の完了の目安に要るものと、同じサブシステムで安く乗るものだけを入れ、残りは公開後の v0.5〜v0.8 に置く。エンティティ（NPC・Display・投射物・ダメージ）→ ワールド（生成・handler・世界の状態）→ UI（専用 GUI・Dialog・実績）→ 運用の残り。1 行ずつの対照は [minestom-parity.md](minestom-parity.md)。

## マイルストーン

### v0.1 — 接続〜スポーン + マルチプレイ基礎

REQ-PROTO-001〜004, REQ-NET-001〜003, REQ-WORLD-001〜002, REQ-ENT-001, REQ-API-001〜002, REQ-TEXT-001（最小）, REQ-MACRO-001, REQ-INFRA-001〜002, REQ-INFRA-004

完了の目安: vanilla クライアント 2 台が offline mode で平坦ワールドに入り、互いが見え、チャットとブロック設置・破壊ができる。

### v0.2 — proxy 運用とゲームを作れる API

REQ-AUTH-001, REQ-AUTH-003, REQ-NET-002（在線人数）, REQ-NET-004〜006, REQ-ENT-002〜003, REQ-PLAYER-001, REQ-API-003〜007（007 は Block 以外）, REQ-API-009, REQ-TEXT-001〜003, REQ-MACRO-002〜003, REQ-MACRO-005

完了の目安: Velocity 配下で、コマンド・インベントリ GUI・MiniMessage 装飾・Title / BossBar を使う簡単なミニゲームが examples に書ける。ゲームモードの切り替え、スケジューラによるカウントダウン、アイテムに付けた値による GUI の判別を含む。

BungeeCord 転送と login 段階の plugin message は Velocity と同じ login の経路に乗り、HAProxy PROXY protocol は handshake より前の接続層で同じ「proxy 配下で動かす」設定に乗るので、一緒に入れる。プレイヤーの状態・スケジューラ・利用者データ（Tag API 相当）は、完了の目安のミニゲームに要る。

### v0.3 — 単体公開と実ワールド

REQ-AUTH-002, REQ-NET-002（装飾・favicon）, REQ-NET-007〜010, REQ-PROTO-006〜007, REQ-WORLD-003〜007, REQ-ENT-004, REQ-PLAYER-002〜003, REQ-API-007（Block）, REQ-TEXT-004, REQ-OPS-003, REQ-MACRO-004

完了の目安: proxy なしで公開でき、vanilla で建築したロビーから複数のゲーム Instance へ移送できる。リソースパックを必須にでき、不正なクライアントや panic するハンドラで他の人が止まらない。

外から直接つながるので、制限（REQ-NET-007）と panic の扱い（REQ-OPS-003）はここで要る。ブロックの利用者データは、Anvil の block entity を位置ごとに持つ仕組みと同じなので一緒に入れる。

### v0.4 — 性能目標の達成と公開

REQ-PERF-001, REQ-INFRA-003, REQ-PROTO-005

全パケットの直送（REQ-PROTO-005）を公開の前に置くのは、高レベル API が v0.5 以降になる機能を、公開直後の利用者が直送で補えるようにするため。

### v0.5 — エンティティの拡充

REQ-ENT-005〜012, REQ-API-008

完了の目安: Text Display のホログラム、スキン付きの NPC、投射物の当たり判定、ダメージと死亡を使うミニゲームが examples に書ける。

### v0.6 — ワールドの拡充

REQ-WORLD-008〜015

完了の目安: 生成 API で作ったマップを複製して部屋を並べ、ワールドボーダー・時間・天候・パーティクルを部屋ごとに変えられる。

### v0.7 — プレイヤー UI の拡充

REQ-UI-001〜009

完了の目安: 専用 GUI・Dialog・実績・クリックのコールバックを使うメニューが examples に書ける。

### v0.8 — 運用・監視の残り

REQ-NET-011〜012, REQ-OPS-001〜002

完了の目安: [minestom-parity.md](minestom-parity.md) の「別枠」と「Minestom にも無いもの」を除く全行が ✅ になる。

### 後回し（必要になったら）

Minestom にあって既存の決定で本体に入れないもの（[minestom-parity.md](minestom-parity.md) の「別枠」）もここに置く（D30）。

- util crate（AI の goal / target・経路探索・戦闘・Ease や WeightedList 等の補助）— D13
- Instance 内並列化（region 分割。Minestom の ThreadDispatcher 相当）— D6
- Anvil への保存 — D10
- 軽量独自ワールド形式（Polar 的）と Schematic
- サーバー側翻訳（GlobalTranslator 相当）— D19
- WASM プラグイン層（MoonBit 等でゲームロジックを書く）

## リスク

| リスク | 影響 | 対策 |
|---|---|---|
| Mojang のプロトコル変更頻度 | 追従作業が継続的に発生 | codegen + ボット統合テストで差分検出を機械化 |
| configuration フェーズのレジストリ要件が版ごとに変わる | ログインできなくなる | Known Packs で vanilla データ送信を最小化 |
| Rust の所有モデルと Minestom 風 API の相性 | API が窮屈になる | v0.1 で examples/lobby を書いて早期検証（基準 2） |
| proc-macro のコンパイル時間 | 開発体験の悪化 | macros を段階導入し、ビルド時間を CI で記録 |
| Minestom 同等の範囲が広く v0.5 以降が終わらない | 公開後の利用者が待たされる | v0.4 で全パケットの直送を入れて補える状態にし、v0.5 以降は対照表の行単位で単位に切る（D30） |
| 性能目標が Minestom に届かない | 「lightweight」の根拠が崩れる | v0.1 でベースライン、目標値は計測後に確定 |
