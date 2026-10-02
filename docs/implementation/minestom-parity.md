# Minestom との対照表

> 調査日: 2026-10-01。Minestom は master `d089734`（2026-09-28、最新リリース `2026.09.12-26.2`、Minecraft 26.2）、
> lodeframe は main `68e44a9`（v0.1 完了時点、26.3）

**Minestom にできて lodeframe にできないことを 1 行 1 機能で並べ、どこで埋めるかを決める表**。
順序の根拠は [roadmap.md](roadmap.md)、要件の中身は [requirements.md](../requirements.md) が正。
方針は decisions.md の D30。

- Minestom のクラスではなく**できること**で対照する。Java 特有の仕組み（Tag API、Acquirable 等）は目的に置き換える（D30）。
- 既存の決定とぶつかるものも載せる。受け皿は「別枠」とし、ぶつかる D 番号を書く。決定は覆さない。
- Minestom 自身にも無いものは末尾の表にまとめ、受け皿を持たない。
- 単位に切ったら、その計画書と backlog が正になる。この表は受け皿の列だけ追従させる。

出典の略記: `D:` = https://minestom.net/docs/ 、`S:` = Minestom の `src/main/java/net/minestom/server/`。

| 記号 | 意味 |
|---|---|
| ✅ | lodeframe にある |
| 🔶 | 一部ある（足りない部分を受け皿で埋める） |
| ⬜ | 無い |
| 別枠 | 既存の決定で本体に入れない。roadmap の「後回し」に置く |
| 不要 | lodeframe の設計で要らない（理由つき） |

## 1. ネットワーク・プロトコル

| # | Minestom の機能 | lodeframe | 受け皿 |
|---|---|---|---|
| 1.1 | 最新 1 版だけを追う（旧版は ViaProxy） | ✅ 26.3（D3） | — |
| 1.2 | TCP と Unix ドメインソケットで待受 | 🔶 TCP のみ | REQ-NET-011（v0.8） |
| 1.3 | 圧縮しきい値の設定 | ✅ `Server::compression_threshold`（既定 256） | — |
| 1.4 | online mode（Mojang 認証・暗号化・認証 URL の差し替え・proxy 接続の拒否） | ⬜ | REQ-AUTH-002（v0.3） |
| 1.5 | offline mode | ✅ | — |
| 1.6 | Velocity modern forwarding | ✅ `Server::forwarding(Forwarding::Velocity { secret })`。スキンはタブリスト、接続元は `ctx.remote_addr`（実 Velocity 4.2.0 で UUID・名前・接続元・秘密違いの拒否を確認。skin 付きは未確認） | — |
| 1.7 | BungeeCord / BungeeGuard 転送 | ✅ `Forwarding::BungeeCord { trusted }`・`Forwarding::BungeeGuard { tokens }`（ボットの BungeeCord 役で確認。実 BungeeCord は未確認） | — |
| 1.8 | HAProxy PROXY protocol v1/v2（任意・必須） | ⬜ | REQ-NET-004（v0.2） |
| 1.9 | login 段階の plugin message（タイムアウト付き） | 🔶 `login::Queries::ask`（D38）。利用者向けのフックは無い | フックは M2-04（pre-login の async イベント） |
| 1.10 | サーバー一覧（装飾 MOTD・favicon・プレイヤーサンプル・偽装版・ping の種類を区別するイベント） | 🔶 平文 MOTD・在線人数と最大人数（`Server::max_players`。D37） | 残りは REQ-NET-002（v0.3） |
| 1.11 | 旧形式（1.6 以下）の ping | ⬜ | REQ-NET-011（v0.8） |
| 1.12 | ping/pong の遅延・キャンセル | ⬜ | REQ-NET-002（v0.3） |
| 1.13 | Open to LAN | ⬜ | REQ-NET-011（v0.8） |
| 1.14 | Transfer（送信・受け入れ可否・転送元の判定・イベント） | 🔶 next_state=3 を Login として受けるだけ | REQ-NET-009（v0.3） |
| 1.15 | Cookie の保存・取得 | ⬜ | REQ-NET-009（v0.3） |
| 1.16 | play 段階の plugin message の送受信 | ✅ `PluginMessageEvent`・`ctx.send_plugin_message`・`ctx.client_brand`（D38） | — |
| 1.17 | リソースパック（送信・削除・必須・状態イベント・configuration 中の送信） | ⬜ | REQ-NET-008（v0.3） |
| 1.18 | Server links | ⬜ | REQ-PROTO-005（v0.4、Minestom も直送のみ） |
| 1.19 | Custom report details | ⬜ | REQ-PROTO-005（v0.4、同上） |
| 1.20 | keep alive の間隔・切断時間の設定、遅延（ping）の取得 | ✅ `Server::keep_alive`、応答 ID の検証、`ctx.ping`（D37） | — |
| 1.21 | kick（任意の理由） | 🔶 内部の再ログイン処理だけ | REQ-PLAYER-001（v0.2） |
| 1.22 | パケット制限（tick あたりの数・キュー長・最大サイズ・認証前の上限・NBT の容量・不正パケットの拒否） | 🔶 フレーム長・NBT の深さだけ | REQ-NET-007（v0.3） |
| 1.23 | 既定のパケット処理の差し替え | ⬜ | REQ-PROTO-005（v0.4） |
| 1.24 | 送受信パケットのフック（イベント） | ⬜ | REQ-PROTO-005（v0.4） |
| 1.25 | 全状態の全パケットを型で公開し直送できる | 🔶 ID は 260 個生成済み、型は v0.1 で使う分だけ | REQ-PROTO-005（v0.4） |
| 1.26 | バイナリ codec | ✅ `Encode` / `Decode` と derive | — |
| 1.27 | 送信の最適化（1 回の encode で複数人へ・キャッシュ・まとめ書き） | 🔶 チャンクのキャッシュとまとめ書きのみ | 本体の共有（`Arc`）は REQ-PERF-001（v0.4） |
| 1.28 | ソケットの調整（バッファ・TCP_NODELAY・タイムアウト） | 🔶 `Server::nodelay` と読み取りタイムアウト（バッファの大きさは未対応） | 要望が出てから |
| 1.29 | サーバーのブランド名 | ✅ `Server::brand` | — |
| 1.30 | デバッグ描画の購読（26.x） | ⬜ | REQ-NET-012（v0.8） |

## 2. configuration 段階・registry

| # | Minestom の機能 | lodeframe | 受け皿 |
|---|---|---|---|
| 2.1 | configuration のイベント（出現先・hardcore・チャット消去・registry 送信の有無・feature flags） | ⬜ | REQ-NET-010（v0.3） |
| 2.2 | Known Packs の交渉（応答タイムアウト付き） | ✅ `Server::known_packs_timeout` | — |
| 2.3 | vanilla 分を除いた registry の送信 | ✅ | — |
| 2.4 | 実行中の registry 登録・削除 | 🔶 起動前の `Registries::set` のみ | REQ-PROTO-006（v0.3） |
| 2.5〜2.19 | 型付きの registry 値（dimension type・biome・damage type・chat type・banner・trim・enchantment と効果部品・painting・jukebox song・instrument・mob の variant・dialog・timeline / world clock・sulfur cube archetype・component predicate） | 🔶 32 種を名前で扱う。値を組み立てる型が無い | REQ-PROTO-006（v0.3） |
| 2.20 | 静的 registry（block・item・potion effect・potion type・entity type・fluid・game event・game rule・particle・sound 等） | 🔶 block と entity type のみ | 使う REQ で順に生成（item は REQ-ENT-003、potion は REQ-ENT-009、particle は REQ-WORLD-014、sound は REQ-TEXT-003、game rule は REQ-WORLD-013） |
| 2.21 | tags（型・実行中の変更と再送） | 🔶 起動時の送信のみ | REQ-PROTO-006（v0.3） |
| 2.22 | feature flags の設定 | 🔶 `minecraft:vanilla` 固定 | REQ-NET-010（v0.3） |
| 2.23 | DataPack の概念（core / 名前なし） | 🔶 core のみ | REQ-PROTO-006（v0.3） |
| 2.24 | Code of conduct | ⬜ | REQ-PROTO-005（v0.4、Minestom も直送のみ） |
| 2.25 | チャットのリセット | ⬜ | REQ-NET-010（v0.3） |
| 2.26 | play から configuration への再突入 | ⬜ | REQ-NET-010（v0.3） |
| 2.27 | NBT と JSON を同じ定義で変換する codec | ⬜ | REQ-PROTO-007（v0.3） |

## 3. インスタンス・ワールド

| # | Minestom の機能 | lodeframe | 受け皿 |
|---|---|---|---|
| 3.1 | 自作 Instance | ✅ `Instance` trait | — |
| 3.2 | チャンクを持つ Instance | ✅ `World` | — |
| 3.3 | SharedInstance（親のチャンクを共有） | ⬜ | REQ-WORLD-009（v0.6） |
| 3.4 | Instance の複製 | ⬜ | REQ-WORLD-009（v0.6） |
| 3.5 | Instance の作成・登録・解除 | 🔶 1 つだけ | REQ-WORLD-005（v0.3） |
| 3.6 | チャンク実装の差し替え・パレット | 🔶 パレットあり、実装は 1 種 | REQ-WORLD-008（v0.6） |
| 3.7 | Generator API（区画単位の fill・相対座標・fork で境界をまたぐ） | 🔶 `ChunkLoader` と平坦生成器のみ | REQ-WORLD-008（v0.6） |
| 3.8 | ChunkLoader（読み込み・保存・アンロード・並列） | 🔶 同期の読み込みのみ | 非同期とアンロードは REQ-WORLD-006（v0.3）、保存は別枠（D10） |
| 3.9 | Anvil の読み込み | ⬜ | REQ-WORLD-003（v0.3） |
| 3.9b | Anvil への保存 | ⬜ | 別枠（D10） |
| 3.10 | チャンクのロード管理とイベント | 🔶 必要時のロードのみ、破棄しない | REQ-WORLD-006（v0.3） |
| 3.11 | チャンク送信の流量制御（クライアントの申告） | 🔶 1 tick の固定予算 | REQ-WORLD-006（v0.3） |
| 3.12 | Block API（不変、property・state id・NBT・handler） | 🔶 NBT と handler が無い | NBT は REQ-API-007（v0.3）、handler は REQ-WORLD-010（v0.6） |
| 3.13 | BlockHandler（設置・破壊・操作・接触・tick、block entity の tag） | ⬜ | REQ-WORLD-010（v0.6） |
| 3.14 | BlockPlacementRule（設置時・隣接更新時の状態決定） | ⬜ | REQ-WORLD-011（v0.6）。隣接更新の挙動そのものは書かない（D13） |
| 3.15 | block entity | ⬜ | REQ-WORLD-010（v0.6） |
| 3.16 | block predicate（adventure の can_break / can_place） | ⬜ | REQ-WORLD-015（v0.6） |
| 3.17 | Block batch（取り消し用の逆 batch つき） | ⬜ | REQ-WORLD-012（v0.6） |
| 3.18 | ライト計算 | 🔶 全 15 固定 | REQ-WORLD-004（v0.3） |
| 3.19 | 正しい heightmap | 🔶 近似 | REQ-WORLD-004（v0.3） |
| 3.20 | biome の取得・設定 | 🔶 全 plains 相当 | REQ-WORLD-007（v0.3） |
| 3.21 | ワールドボーダー | ⬜ | REQ-WORLD-013（v0.6） |
| 3.22 | 時間（速度・一時停止）・WorldClock / Timeline | ⬜ | REQ-WORLD-013（v0.6） |
| 3.23 | 天候 | ⬜ | REQ-WORLD-013（v0.6） |
| 3.24 | 爆発の口（抽象のみ）と爆発パケット | ⬜ | パケットは REQ-WORLD-014（v0.6）。爆発の計算は vanilla 挙動（D1） |
| 3.25 | エンティティの空間検索 | ⬜ | REQ-ENT-006（v0.5） |
| 3.26 | ワールド演出（block action・world event・破壊アニメーション） | ⬜ | REQ-WORLD-014（v0.6） |
| 3.27 | game rule（registry とクライアントからの取得・設定要求） | ⬜ | REQ-WORLD-013（v0.6） |
| 3.28 | fluid の registry | ⬜ | REQ-WORLD-015（v0.6）。流れる挙動は Minestom にも無い |
| 3.29 | 難易度 | ⬜ | REQ-WORLD-013（v0.6） |
| 3.30 | 月齢 | ⬜ | REQ-WORLD-013（v0.6） |
| 3.31 | environment attributes（26.x） | ⬜ | REQ-WORLD-013（v0.6） |
| 3.33 | セクション無効化イベント | ⬜ | REQ-WORLD-006（v0.3） |
| 3.34 | ブロックの補助（視線方向の走査・破壊時間の計算） | ⬜ | REQ-WORLD-015（v0.6） |
| 3.35 | 座標 API | ✅ D23 | Area・ChunkRange 相当は使う REQ で足す |
| 3.36 | Instance 単位の event / scheduler / 利用者データ | ✅ event・scheduler・`ctx.data()`（D35） | REQ-API-006・REQ-API-007（v0.2） |
| 3.37 | hashed seed | 🔶 0 固定 | REQ-WORLD-007（v0.3） |
| 3.x | 複数ディメンション（Respawn を伴う移送） | ⬜ overworld 固定 | REQ-WORLD-007（v0.3）。Minestom は DimensionType で持つ |

3.32（Schematic）は Minestom にも無いので末尾の表へ。

## 4. エンティティ

| # | Minestom の機能 | lodeframe | 受け皿 |
|---|---|---|---|
| 4.1 | Entity / LivingEntity / Creature / Item / Projectile / ExpOrb の階層 | ⬜ プレイヤーのみ | 汎用エンティティは REQ-ENT-002（v0.2）、種別ごとは v0.5 の各 REQ |
| 4.2 | 全エンティティ種別 | 🔶 161 種の ID と名前のみ | REQ-ENT-002（v0.2） |
| 4.3 | 全種の型付きメタデータ・一括送信 | 🔶 flags と pose のみ | 汎用の SetEntityData は REQ-ENT-002（v0.2）、全種の型は REQ-ENT-005（v0.5） |
| 4.4 | Display（Block / Item / Text） | ⬜ | REQ-ENT-005（v0.5） |
| 4.5 | Mannequin による NPC（スキン指定） | ⬜ | REQ-ENT-005（v0.5） |
| 4.6 | Interaction / Marker | ⬜ | REQ-ENT-005（v0.5） |
| 4.7 | 種別の動的変更 | ⬜ | REQ-ENT-012（v0.5） |
| 4.8 | 視認管理（自動・条件・手動追加・表示距離） | 🔶 プレイヤー間の自動のみ | REQ-ENT-012（v0.5） |
| 4.9 | 位置同期の間隔 | 🔶 毎 tick の差分のみ | REQ-ENT-012（v0.5） |
| 4.10 | 速度・重力・空気抵抗 | ⬜ | REQ-ENT-002（v0.2） |
| 4.11 | 衝突（ブロック形状・エンティティ・掃引） | ⬜ | ブロックとの衝突は REQ-ENT-002（v0.2）、形状の全種・エンティティ間・掃引は REQ-ENT-006（v0.5） |
| 4.12 | 当たり判定・視線・レイキャスト | ⬜ | REQ-ENT-006（v0.5） |
| 4.13 | ノックバック | ⬜ | REQ-ENT-006（v0.5） |
| 4.14 | 乗り物・passenger・操縦入力 | ⬜ | REQ-ENT-007（v0.5） |
| 4.15 | リード | ⬜ | REQ-ENT-007（v0.5） |
| 4.16 | 装備とイベント | ⬜ | REQ-ENT-008（v0.5） |
| 4.17 | ポーション効果とイベント | ⬜ | REQ-ENT-009（v0.5） |
| 4.18 | 属性と modifier | ⬜ | REQ-ENT-009（v0.5） |
| 4.19 | ダメージ（種類・無敵時間・イベント・演出パケット） | ⬜ | REQ-ENT-010（v0.5） |
| 4.20 | 死亡と消える演出 | ⬜ | REQ-ENT-010（v0.5） |
| 4.21 | 体力・回復 | ⬜ | プレイヤーは REQ-PLAYER-001（v0.2）、他は REQ-ENT-010（v0.5） |
| 4.22 | 炎上とイベント | ⬜ | REQ-ENT-010（v0.5） |
| 4.23 | 状態・アニメーション（腕振り等） | ⬜ | REQ-ENT-010（v0.5） |
| 4.24 | AI の goal / target | ⬜ | 別枠（D13、util crate） |
| 4.25 | 経路探索 | ⬜ | 別枠（D13、util crate） |
| 4.26 | 投射物と衝突イベント | ⬜ | REQ-ENT-011（v0.5） |
| 4.27 | アイテムエンティティ（合体・拾得・ドロップ） | ⬜ | REQ-ENT-011（v0.5） |
| 4.28 | 経験値オーブ | ⬜ | REQ-ENT-011（v0.5） |
| 4.29 | ベッド | ⬜ | REQ-ENT-005（v0.5） |
| 4.30 | 刺さった矢・エリトラ飛行 | ⬜ | REQ-ENT-005（v0.5） |
| 4.31 | 見た目のフラグ（発光・透明・名前表示・無音・姿勢） | 🔶 スニークのみ | REQ-ENT-002（v0.2） |
| 4.32 | エンティティ単位の scheduler / event / 利用者データ | ⬜ | REQ-API-006・REQ-API-007・REQ-API-009（v0.2） |
| 4.33 | 村人の職業と取引 | ⬜ | 職業はメタデータで REQ-ENT-005（v0.5）、取引は REQ-UI-001（v0.7） |
| 4.34 | ライフサイクルのイベント（spawn・despawn・tick・teleport・velocity） | ⬜ | REQ-ENT-002（v0.2） |

4.35（ホログラム専用クラス）は Minestom にも無いので末尾の表へ。

## 5. プレイヤー

| # | Minestom の機能 | lodeframe | 受け皿 |
|---|---|---|---|
| 5.1 | 独自の Player と UUID の差し替え | 🔶 利用者データは `ctx.player_data(id)`（D35） | ログイン前の UUID・名前の差し替えは REQ-API-003（v0.2）。クラスの継承は不要（Rust では持たせるデータで足りる） |
| 5.2 | 接続の流れのイベント（pre-login・configuration・spawn・loaded・disconnect） | 🔶 join / leave のみ | pre-login は REQ-API-003（v0.2）、configuration は REQ-NET-010（v0.3）、loaded は REQ-API-005（v0.2） |
| 5.3 | ゲームモードと変更イベント（F3+F4 を含む） | 🔶 creative 固定 | REQ-PLAYER-001（v0.2） |
| 5.4 | 能力（飛行・速度・視野・即時破壊・無敵）と飛行のイベント | ⬜ | REQ-PLAYER-001（v0.2） |
| 5.5 | 権限レベル（op 0〜4） | ⬜ | REQ-PLAYER-001（v0.2）。コマンドの可視性は REQ-API-004 |
| 5.6 | 体力・空腹・経験値 | ⬜ | REQ-PLAYER-001（v0.2） |
| 5.7 | リスポーン（地点・画面・死亡地点・イベント） | ⬜ | REQ-PLAYER-001（v0.2） |
| 5.8 | テレポート（相対指定・確認 ID の管理） | ⬜ 位置はクライアントの申告を信じる | REQ-PLAYER-001（v0.2） |
| 5.9 | 移動・入力・ダッシュ・tick のイベント | ⬜ | REQ-API-005（v0.2） |
| 5.10 | 観戦・カメラ | ⬜ | REQ-UI-007（v0.7） |
| 5.11 | プレイヤーごとの view distance（実行中の変更） | 🔶 World 全体の固定値 | REQ-WORLD-006（v0.3） |
| 5.12 | クライアント設定・言語のイベント | ⬜ 捨てている | REQ-API-005（v0.2） |
| 5.13 | チャットイベントの書式変更・チャット表示設定の尊重 | 🔶 本文の差し替えのみ | REQ-API-005（v0.2、D29 の (4)） |
| 5.14 | chat type と disguised chat | 🔶 disguised chat のみ | REQ-API-005（v0.2、D29 の (4)） |
| 5.15 | クリックのコールバック（回数・期限つき） | ⬜ | REQ-UI-008（v0.7） |
| 5.16 | インベントリ（プレイヤー・コンテナ 25 種・vanilla のクリック処理・イベント・creative 操作） | ⬜ | REQ-ENT-003（v0.2） |
| 5.17 | 専用 GUI（金床・ビーコン・醸造台・エンチャント台・かまど・村人取引・数値表示） | ⬜ | REQ-UI-001（v0.7） |
| 5.18 | GUI の補助操作（ボタン・バンドル・馬） | ⬜ | REQ-UI-001（v0.7） |
| 5.19 | ItemStack（不変・builder・NBT/JSON） | ⬜ | REQ-ENT-003（v0.2、名前・説明・数など GUI に要る分） |
| 5.20 | アイテムの全データコンポーネント（約 40 種） | ⬜ | REQ-UI-009（v0.7） |
| 5.21 | アイテムの使用（開始・終了・中断・クールダウン） | ⬜ | イベントは REQ-API-005（v0.2）、クールダウンの表示は REQ-UI-007（v0.7） |
| 5.22 | ブロック操作のイベント（採掘の開始・中断・完了、ピック） | 🔶 破壊と設置のみ | REQ-API-005（v0.2） |
| 5.23 | エンティティ操作のイベント（操作・攻撃・槍・腕振り） | ⬜ | REQ-API-005（v0.2） |
| 5.24 | 持ち替え・ドロップ・スロット選択 | ⬜ | REQ-API-005（v0.2） |
| 5.25 | ボスバー | ⬜ | REQ-TEXT-003（v0.2） |
| 5.26 | スコアボード（サイドバー・名前の下・タブリストの objective・チーム） | ⬜ | REQ-ENT-004（v0.3） |
| 5.27 | タイトル・actionbar | ⬜ | REQ-TEXT-003（v0.2） |
| 5.28 | タブリスト（ヘッダー/フッター・表示可否・並び順・表示名・遅延） | 🔶 全員を載せるのみ、遅延 0 | ヘッダー/フッターは REQ-TEXT-003（v0.2）、残りは REQ-PLAYER-002（v0.3） |
| 5.29 | スキン（Mojang から取得・接続時のイベント・`setSkin`） | ⬜ properties が空 | REQ-PLAYER-003（v0.3） |
| 5.30 | サウンド（再生・停止・独自音・自分以外へ） | ⬜ | REQ-TEXT-003（v0.2） |
| 5.31 | パーティクル | ⬜ | REQ-WORLD-014（v0.6） |
| 5.32 | 実績 | ⬜ | REQ-UI-002（v0.7） |
| 5.33 | 通知トースト | ⬜ | REQ-UI-002（v0.7） |
| 5.34 | レシピとレシピブック | ⬜ | REQ-UI-003（v0.7）。結果の計算は Minestom にも無い |
| 5.35 | 地図 | ⬜ | REQ-UI-006（v0.7） |
| 5.36 | 本を開く・編集イベント | ⬜ | REQ-UI-005（v0.7） |
| 5.37 | 看板の編集 | ⬜ | REQ-UI-005（v0.7） |
| 5.38 | Dialog（5 種・カスタムクリック） | ⬜ | REQ-UI-004（v0.7） |
| 5.39 | locator bar の waypoint | ⬜ | REQ-UI-007（v0.7） |
| 5.40 | 統計 | ⬜ | REQ-UI-007（v0.7） |
| 5.41 | デバッグ画面の情報制限 | ⬜ | REQ-UI-007（v0.7） |
| 5.42 | ゲーム状態の変更（雨・エンドロール・デモ等） | 🔶 チャンク読み込み開始のみ | REQ-UI-007（v0.7） |
| 5.43 | 名前の下のスコア | ⬜ | REQ-ENT-004（v0.3） |

## 6. イベント

| # | Minestom の機能 | lodeframe | 受け皿 |
|---|---|---|---|
| 6.1 | ノードの木（親から子へ、優先度順） | 🔶 木と深さ優先あり、優先度なし | REQ-API-009（v0.2） |
| 6.2 | 条件・tag で絞るノード | ⬜ | REQ-API-009（v0.2） |
| 6.3 | 対象の種類によるフィルタ | 🔶 イベントの型で受けるのみ | REQ-API-009（v0.2） |
| 6.4 | 優先度 | ⬜ | REQ-API-009（v0.2） |
| 6.5 | キャンセル | ✅ | — |
| 6.6 | リスナーの失効（回数・条件）・キャンセル済みを無視 | ⬜ | REQ-API-009（v0.2） |
| 6.7 | ノードの型で受けられるイベントが決まる | ✅ 型付きハンドラ | — |
| 6.8 | 子ノードの管理・複数の親 | 🔶 追加・削除・取得のみ | REQ-API-009（v0.2） |
| 6.9 | 継承イベント（親のリスナーが子イベントも受ける） | ⬜ | REQ-API-009（v0.2） |
| 6.10 | ListenerHandle（検索を省いた直接呼び出し） | ⬜ | REQ-PERF-001（v0.4、計測次第） |
| 6.11 | 複数イベントの束ね | ⬜ | REQ-API-009（v0.2） |
| 6.12 | エンティティ・Instance ごとのノード | 🔶 World に 1 つ | REQ-API-009（v0.2） |
| 6.13 | 独自イベントの発火 | 🔶 `impl Event` を手書き | REQ-MACRO-003（v0.2） |
| 6.14 | 非同期イベント | ⬜ | REQ-API-003（v0.2） |
| 6.15 | イベントの種類（約 110） | 🔶 5 種 | 各機能の REQ で足す。主なものは REQ-API-005（v0.2） |
| 6.16 | 監視（JFR へのイベント出力） | ⬜ | REQ-OPS-001（v0.8、tracing で） |
| 6.x | 発火中に追加したハンドラ・内側のイベント | ⬜ 失われる | REQ-API-009（v0.2、D29 の (2)）。Minestom は失わない |

## 7. コマンド

| # | Minestom の機能 | lodeframe | 受け皿 |
|---|---|---|---|
| 7.1 | 名前・別名・構文・既定の実行 | ⬜ | REQ-API-004（v0.2） |
| 7.2 | サブコマンド・条件付き構文 | ⬜ | REQ-API-004（v0.2） |
| 7.3 | 実行条件（プレイヤーのみ等） | ⬜ | REQ-API-004（v0.2） |
| 7.4 | 引数ごとのエラー処理 | ⬜ | REQ-API-004（v0.2） |
| 7.5 | 全引数型（セレクタ・ItemStack・BlockState・NBT・Component・相対座標 等） | ⬜ | 基本型（整数・文字列・プレイヤー・座標）は REQ-API-004（v0.2）、残りは REQ-API-008（v0.5） |
| 7.6 | サジェスト | ⬜ | REQ-API-004（v0.2） |
| 7.7 | 省略可能な引数・既定値・変換 | ⬜ | REQ-API-004（v0.2）、REQ-MACRO-002 |
| 7.8 | 戻り値とサーバーからの実行 | ⬜ | REQ-API-004（v0.2） |
| 7.9 | コンソールの送信者 | ⬜ | REQ-API-004（v0.2）。標準入力を読むのは Minestom にも無い |
| 7.10 | 未知のコマンドの処理 | ⬜ | REQ-API-004（v0.2） |
| 7.11 | クライアント補完用の構文木と再送 | ⬜ | REQ-API-004（v0.2） |
| 7.12 | コマンドの横取りイベント | ⬜ | REQ-API-004（v0.2） |
| 7.13 | エンティティセレクタ | ⬜ | REQ-API-008（v0.5） |
| 7.14 | 署名付きコマンドの受信 | ⬜ | REQ-API-004（v0.2、署名は検証せず本文を読む） |

## 8. スケジューラ・tick・スレッド

| # | Minestom の機能 | lodeframe | 受け皿 |
|---|---|---|---|
| 8.1 | 全体のスケジューラ | ✅ `ctx.after(..)`（D34） | — |
| 8.2 | Instance・エンティティ・プレイヤー単位のスケジューラ | 🔶 Instance・プレイヤー（`for_player`） | エンティティは M2-17 |
| 8.3 | 実行時期（次 tick・tick 数・時間・future 完了時・停止） | 🔶 次 tick・tick 数・時間（`Delay`）・停止（`cancel`）。future 完了時は `ctx.spawn(..).then(..)`（D33） | — |
| 8.4 | tick の開始時か終了時か | ✅ `at_end` | — |
| 8.5 | 次回を自分で決めるタスク | ✅ `Next::After` | — |
| 8.6 | Executor として使う | 不要 | Instance スレッドへの投入は `ctx.spawn` の戻し（D16、実装済み: D33）で足りる |
| 8.7 | シャットダウン時のタスク | ✅ `ShutdownEvent`（D37） | — |
| 8.8 | tick レート・追いつきの上限の設定 | ✅ `Server::tick_rate`・`max_catch_up`（D37） | — |
| 8.9 | ThreadDispatcher（Instance 内の並列 tick） | ⬜ | 別枠（D6、region 分割） |
| 8.10 | ThreadProvider | ⬜ | 別枠（D6、同上） |
| 8.11 | Acquirable（別スレッドの物へ安全に触る） | 不要 | Instance 間は message passing のみ（D6）。所有は型が保証する |
| 8.12 | クライアントの tick 速度制御 | ⬜ | REQ-UI-007（v0.7、Minestom も直送のみ） |
| 8.13 | Cooldown 等の時間の補助 | ✅ `Cooldown`（`Data` の key に入れる。サーバー側の判定のみ。D36） | vanilla のアイテムのクールダウン表示は M2-20 以降 |

## 9. Adventure 統合

| # | Minestom の機能 | lodeframe | 受け皿 |
|---|---|---|---|
| 9.1 | Audience（Player・送信者・Instance・Scoreboard・Team） | ⬜ | REQ-TEXT-003（v0.2）。Scoreboard・Team は REQ-ENT-004（v0.3） |
| 9.2 | 登録できる独自の audience | ⬜ | REQ-TEXT-003（v0.2） |
| 9.3 | まとめ送り（viewer 全員を 1 audience として） | ⬜ | REQ-TEXT-003（v0.2） |
| 9.4 | Component と色（RGB・alpha・染料） | 🔶 text と style のみ | REQ-TEXT-001（v0.2） |
| 9.5 | 変換（plain・legacy・JSON・NBT・ANSI）と MiniMessage | 🔶 NBT の encode のみ | REQ-TEXT-001・002（v0.2）。ANSI と legacy は REQ-TEXT-004（v0.3） |
| 9.6 | 翻訳（GlobalTranslator・受信者の言語へ自動翻訳） | ⬜ | 別枠（D19） |
| 9.7 | Component のログ出力 | ⬜ | REQ-TEXT-004（v0.3、ANSI と一緒に） |
| 9.8 | hover でのアイテム表示 | ⬜ | REQ-UI-009（v0.7） |
| 9.9 | タイトル・actionbar・ボスバー・サウンド・本・リソースパック・タブリスト | ⬜ | REQ-TEXT-003（v0.2）、本は REQ-UI-005、リソースパックは REQ-NET-008、タブリストは REQ-PLAYER-002 |

## 10. その他

| # | Minestom の機能 | lodeframe | 受け皿 |
|---|---|---|---|
| 10.2 | Tag API（Entity・Item・Block・Instance に型付きの値を付け、NBT に保存） | 🔶 Player・Instance（`Data`・`Key`、保存しない。D35） | Entity は M2-17、ItemStack（NBT）は M2-20、Block は v0.3（Anvil の block entity と同じ仕組み） |
| 10.3 | Snapshot（不変の状態の写し） | ⬜ | REQ-OPS-002（v0.8） |
| 10.4 | tick の監視イベント | 🔶 `TickStats` を取れるのみ | REQ-OPS-001（v0.8） |
| 10.5 | 例外ハンドラ | ⬜ ハンドラの panic で Instance のスレッドが止まる | REQ-OPS-003（v0.3） |
| 10.6 | シャットダウン（シグナルで止める） | ✅ `Server::run` が Ctrl-C で `shutdown`（D37） | — |
| 10.7 | 設定項目（約 60） | 🔶 motd・brand・REQ-NET-006 の 7 項目・`forwarding`（Velocity・BungeeCord・BungeeGuard）・`forwarding_timeout` | 項目ごとの REQ（proxy 系の HAProxy は M2-10 が同じ入り口に足す） |
| 10.8 | Mojang プロフィールの取得・署名の検証 | ⬜ | REQ-PLAYER-003（v0.3） |
| 10.9 | プロセスの作り直し | 不要 | `Server` を作り直せば足りる（グローバルな状態を持たない、D6） |
| 10.10 | テスト支援 | ✅ `test-util`（D21） | — |
| 10.11 | ベンチマーク | 🔶 `xtask bench`（全体の負荷） | 部品のベンチは REQ-PERF-001（v0.4） |
| 10.12 | 並行性テスト | 不要 | Instance は単一スレッド所有（D6）。境界は channel のみ |
| 10.13 | コード生成 | ✅ D4・D5 | — |
| 10.14 | 汎用の補助（Ease・WeightedList・Range 等） | ⬜ | 別枠（util crate と一緒に、D13） |
| 10.15 | デモサーバー | 🔶 examples/lobby | 各 MS の完了の目安の example で足す |
| 10.16 | 外部ライブラリ（Polar・schem）の案内 | ⬜ | 別枠（軽量独自ワールド形式と一緒に） |

10.1（プラグイン機構）は Minestom にも無いので末尾の表へ。

## Minestom にも無いもの（受け皿なし）

比べるときに「足りない」に数えない。

| 機能 | Minestom の状況 | lodeframe |
|---|---|---|
| Schematic | 外部ライブラリ（schem）を案内 | 別枠の独自ワールド形式と一緒に考える |
| ホログラム専用クラス | TextDisplay や名前表示で代用 | Display（REQ-ENT-005）で代用 |
| プラグイン機構 | Extensions は削除済み | WASM プラグイン層は後回し（roadmap） |
| 爆発の既定実装 | 抽象と口だけ | vanilla 挙動（D1） |
| 流体の挙動 | registry のみ | vanilla 挙動（D1） |
| クラフト結果の計算 | 自前で書く | vanilla 挙動（D1） |
| 権限文字列 | op レベルのみ | 利用者データで書ける |
| 署名付きチャット | 署名を無視 | offline で署名できない（chat-plan.md） |
| コンソールの標準入力 | 自前で書く | 同じ |
| データパックファイルの読み込み | 見当たらない（推測） | 同じ |
| 地図の Framebuffer | 文書にあるがソースから消えている | REQ-UI-006 はパケットの直送から |

## 集計

対照した 226 行（1〜10 章）の受け皿ごとの数。1 行に受け皿が複数あるものは、それぞれに数えた。

| 受け皿 | 行数 |
|---|---|
| v0.2 | 86 |
| v0.3 | 38 |
| v0.4 | 9 |
| v0.5 | 30 |
| v0.6 | 22 |
| v0.7 | 20 |
| v0.8 | 7 |
| 別枠 | 9 |
| ✅ / 不要 | 16 |
| 複数の MS に分かれる（2.20 静的 registry、10.15 デモ） | 2 |

ほかに、Minestom にも無いものが 11 行ある。
