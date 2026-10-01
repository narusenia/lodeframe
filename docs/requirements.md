# 要件

優先度は Must / Should / Could。マイルストーンは [roadmap](implementation/roadmap.md) を参照。
根拠は [decisions.md](decisions.md)。

## スコープ外

vanilla 挙動（mob AI、レッドストーン、ワールド生成、クラフト、戦闘計算）、
複数プロトコル版の同時対応、Anvil への保存。AI 等は将来 util crate で扱う（D13）。
サーバー側翻訳（GlobalTranslator 相当）は後回し（D19）。
Minestom にあってこの理由で入れないもの（AI goal・経路探索・Anvil 保存・翻訳・Instance 内の並列 tick）は
[minestom-parity.md](implementation/minestom-parity.md) に「別枠」として載せる（D30）。

## 一覧

| ID | タイトル | 優先度 | MS |
|---|---|---|---|
| REQ-PROTO-001 | プロトコル基本型と Encode / Decode | Must | v0.1 |
| REQ-PROTO-002 | vanilla 生成データからの codegen | Must | v0.1 |
| REQ-PROTO-003 | NBT | Must | v0.1 |
| REQ-PROTO-004 | 座標・ベクトル型 | Must | v0.1 |
| REQ-PROTO-005 | 全パケットの型・直送・フック | Must | v0.4 |
| REQ-PROTO-006 | 型付きの registry 値と実行中の変更 | Should | v0.3 |
| REQ-PROTO-007 | NBT と JSON の共通 codec | Should | v0.3 |
| REQ-NET-001 | 接続層（フレーミング・圧縮・状態遷移） | Must | v0.1 |
| REQ-NET-002 | Status ping | Must | v0.1 / v0.2 / v0.3 |
| REQ-NET-003 | offline login と configuration | Must | v0.1 |
| REQ-NET-004 | HAProxy PROXY protocol | Should | v0.2 |
| REQ-NET-005 | plugin message（login・play） | Must | v0.2 |
| REQ-NET-006 | サーバーの設定と停止 | Must | v0.2 |
| REQ-NET-007 | 不正なクライアントへの制限 | Must | v0.3 |
| REQ-NET-008 | リソースパック | Must | v0.3 |
| REQ-NET-009 | Transfer と Cookie | Should | v0.3 |
| REQ-NET-010 | configuration の制御と再突入 | Should | v0.3 |
| REQ-NET-011 | LAN 公開・旧形式 ping・Unix ソケット | Could | v0.8 |
| REQ-NET-012 | デバッグ描画の購読 | Could | v0.8 |
| REQ-AUTH-001 | Velocity modern forwarding | Must | v0.2 |
| REQ-AUTH-002 | online mode（暗号化・Mojang 認証） | Must | v0.3 |
| REQ-AUTH-003 | BungeeCord / BungeeGuard 転送 | Should | v0.2 |
| REQ-WORLD-001 | Instance と tick ループ | Must | v0.1 |
| REQ-WORLD-002 | チャンクと `ChunkLoader` | Must | v0.1 |
| REQ-WORLD-003 | Anvil 読込 | Should | v0.3 |
| REQ-WORLD-004 | ライティング計算 | Should | v0.3 |
| REQ-WORLD-005 | 複数 Instance とプレイヤー移送 | Must | v0.3 |
| REQ-WORLD-006 | チャンクの管理（破棄・非同期・流量・イベント） | Must | v0.3 |
| REQ-WORLD-007 | ディメンションと biome | Must | v0.3 |
| REQ-WORLD-008 | 生成 API | Should | v0.6 |
| REQ-WORLD-009 | チャンクを共有する Instance と複製 | Should | v0.6 |
| REQ-WORLD-010 | ブロックの handler と block entity | Should | v0.6 |
| REQ-WORLD-011 | 設置ルール | Should | v0.6 |
| REQ-WORLD-012 | ブロックの batch | Could | v0.6 |
| REQ-WORLD-013 | 世界の状態（ボーダー・時間・天候・game rule 等） | Should | v0.6 |
| REQ-WORLD-014 | 演出（パーティクル・world event・爆発） | Should | v0.6 |
| REQ-WORLD-015 | ブロックの補助（走査・破壊時間・predicate・fluid） | Could | v0.6 |
| REQ-ENT-001 | プレイヤーの表示・移動同期 | Must | v0.1 |
| REQ-ENT-002 | エンティティと簡易物理 | Must | v0.2 |
| REQ-ENT-003 | インベントリ | Must | v0.2 |
| REQ-ENT-004 | スコアボード | Should | v0.3 |
| REQ-ENT-005 | 全種のメタデータと Display・NPC | Should | v0.5 |
| REQ-ENT-006 | 衝突・当たり判定・空間検索 | Should | v0.5 |
| REQ-ENT-007 | 乗り物とリード | Could | v0.5 |
| REQ-ENT-008 | 装備 | Should | v0.5 |
| REQ-ENT-009 | ポーション効果と属性 | Should | v0.5 |
| REQ-ENT-010 | 体力・ダメージ・死亡 | Should | v0.5 |
| REQ-ENT-011 | 投射物・アイテム・経験値オーブ | Should | v0.5 |
| REQ-ENT-012 | 視認と同期の制御 | Should | v0.5 |
| REQ-PLAYER-001 | プレイヤーの状態 | Must | v0.2 |
| REQ-PLAYER-002 | タブリスト | Should | v0.3 |
| REQ-PLAYER-003 | スキン | Should | v0.3 |
| REQ-UI-001 | 専用 GUI | Should | v0.7 |
| REQ-UI-002 | 実績とトースト | Could | v0.7 |
| REQ-UI-003 | レシピとレシピブック | Could | v0.7 |
| REQ-UI-004 | Dialog | Should | v0.7 |
| REQ-UI-005 | 本と看板 | Could | v0.7 |
| REQ-UI-006 | 地図 | Could | v0.7 |
| REQ-UI-007 | クライアント表示の細目 | Could | v0.7 |
| REQ-UI-008 | クリックのコールバック | Should | v0.7 |
| REQ-UI-009 | アイテムの全データコンポーネント | Should | v0.7 |
| REQ-TEXT-001 | Component モデル | Must | v0.1 / v0.2 |
| REQ-TEXT-002 | MiniMessage 実行時パーサ | Must | v0.2 |
| REQ-TEXT-003 | Audience | Must | v0.2 |
| REQ-TEXT-004 | 端末向けの変換とログ | Could | v0.3 |
| REQ-API-001 | 階層イベントノード | Must | v0.1 |
| REQ-API-002 | チャット・ブロック操作イベント | Must | v0.1 |
| REQ-API-003 | 非同期処理の spawn→戻し | Must | v0.2 |
| REQ-API-004 | コマンド | Must | v0.2 |
| REQ-API-005 | プレイヤーの操作イベント | Must | v0.2 |
| REQ-API-006 | スケジューラ | Must | v0.2 |
| REQ-API-007 | 利用者データの付与（Tag API 相当） | Must | v0.2 / v0.3 |
| REQ-API-008 | コマンドの全引数型とセレクタ | Should | v0.5 |
| REQ-API-009 | イベントノードの拡張 | Should | v0.2 |
| REQ-MACRO-001 | `derive(Encode, Decode)` | Must | v0.1 |
| REQ-MACRO-002 | `#[command]` | Must | v0.2 |
| REQ-MACRO-003 | `#[event]` / `derive(Event)` | Should | v0.2 |
| REQ-MACRO-004 | アイテム・GUI の宣言的マクロ | Could | v0.3 |
| REQ-MACRO-005 | `text!` | Should | v0.2 |
| REQ-PERF-001 | 性能目標 | Must | v0.4 |
| REQ-INFRA-001 | CI | Must | v0.1 |
| REQ-INFRA-002 | 自前ボットによる統合・負荷試験 | Must | v0.1 |
| REQ-INFRA-003 | crates.io 公開 | Must | v0.4 |
| REQ-INFRA-004 | 利用者向けテストハーネス | Must | v0.1〜 |
| REQ-OPS-001 | 監視 | Could | v0.8 |
| REQ-OPS-002 | スナップショット | Could | v0.8 |
| REQ-OPS-003 | ハンドラの panic で止まらない | Must | v0.3 |

v0.5 以降と、Minestom の機能との 1 行ずつの対照は [minestom-parity.md](implementation/minestom-parity.md)（D30）。

---

## PROTO

### REQ-PROTO-001: プロトコル基本型と Encode / Decode

- VarInt / VarLong / String / UUID / BlockPos / BitSet / Identifier / 固定長配列 / Option / 長さ前置 Vec を `Encode` / `Decode` trait で扱う。
- `lodeframe-protocol` は本体に依存しない（ボット・proxy で単独利用可）。
- **受入条件**
  - [ ] 全基本型の round-trip テストが通る
  - [ ] 不正入力（VarInt 超過長、長さ上限超過、不正 UTF-8）でパニックせずエラーを返す

### REQ-PROTO-002: vanilla 生成データからの codegen

- server.jar の data generator 出力からブロック状態・レジストリ・パケット ID を Rust コードに生成する（D4, D5）。
- **受入条件**
  - [x] `mise run datagen [version]` 1 コマンドで再生成でき、再実行で差分が出ない
  - [x] 生成物は commit され、通常ビルドはネットワーク・Java 不要
  - [x] ブロック状態 ID ⇔ (ブロック, プロパティ) の相互変換ができる（全 35,723 状態をテスト）

### REQ-PROTO-003: NBT

- ネットワーク NBT（名前なしルート、ルートは任意のタグ）の読み書き。文字列は modified UTF-8。
- ネスト上限は 64（vanilla は 512）。再帰デコーダは 1 段に数 KB のスタックを使い、512 では 2MB のワーカースレッドで溢れるため。
- **受入条件**
  - [ ] 仕様書の例と同じバイト列を読み書きできる（実装済み。vanilla 実機の出力での検証は M1-05 / M1-08）
  - [ ] 深すぎる入力・巨大な長さ・不正な文字列でパニックやスタック溢れを起こさない
  - [ ] vanilla のレジストリデータを NBT で送ってクライアントが受理する

### REQ-PROTO-004: 座標・ベクトル型

- `Vec3`（glam の `DVec3`）、`Pos`（`DVec3` + yaw / pitch）、`BlockPos`（i32）。`BlockPos` は wire のパック形式、チャンク・セクション座標への変換、`Pos` との相互変換を持つ（D23）。
- **受入条件**
  - [ ] `BlockPos` ⇔ チャンク座標・セクション内座標の変換が負の座標でも正しい（床除算）
  - [ ] `Pos` → `BlockPos` はブロックの floor になる（負の座標でも）
  - [ ] `Vec3` は `Encode` / `Decode` できる（x, y, z の f64）

### REQ-PROTO-005: 全パケットの型・直送・フック

- 全状態・両方向のパケットを型で持ち、利用者が直送できる。送受信のフック（イベント）と、既定のパケット処理の差し替え口を持つ。Server links・Code of conduct・Custom report など、高レベル API の無い機能もこれで使える（Minestom も直送だけ）。
- 公開前（v0.4）に置くのは、高レベル API が足りない部分を利用者が直送で補えるようにするため。
- **受入条件**
  - [ ] 生成済みの全パケット ID に型があり、全部の round-trip テストが通る
  - [ ] 送信・受信のフックで、パケットの観察・キャンセル・差し替えができる

### REQ-PROTO-006: 型付きの registry 値と実行中の変更

- dimension type・biome・damage type・chat type・banner・trim・enchantment・painting・jukebox song・instrument・mob の variant・dialog・timeline 等を、名前ではなく値で組み立てる型。tags の実行中の変更と再送、データパックの区別。
- **受入条件**
  - [ ] 独自の dimension type と biome を型で定義し、クライアントが受理する
  - [ ] 実行中に tag を変えて再送すると、クライアントに反映される

### REQ-PROTO-007: NBT と JSON の共通 codec

- 1 つの定義から NBT と JSON の両方へ変換する（Component・registry 値・アイテムが使う）。
- **受入条件**
  - [ ] 同じ値の NBT と JSON の round-trip が一致する

## NET

### REQ-NET-001: 接続層

- tokio の TCP リスナー、パケット長フレーミング、zlib 圧縮（閾値設定可）、Handshake → Status / Login → Configuration → Play の状態機械。
- **受入条件**
  - [ ] 圧縮有無の両方でクライアントが接続できる
  - [ ] 不正パケット・タイムアウトで該当接続だけが切断され、サーバーは継続する
  - [ ] 1 パケットの最大長を超える入力を読み切る前に拒否する

### REQ-NET-002: Status ping

- v0.1: 平文の MOTD・版・ping。v0.2: 在線人数。v0.3: 装飾付き MOTD・favicon・プレイヤーサンプル・偽装する版、問い合わせごとに差し替えるイベント、ping の遅延・キャンセル（単体公開でサーバー一覧が要るため）。
- **受入条件**
  - [ ] サーバー一覧に MOTD・人数・版が表示され、ping 値が出る
  - [ ] MOTD と人数を利用者コードから差し替えられる
  - [ ] v0.3: favicon と装飾付き MOTD が表示され、問い合わせごとに内容を変えられる

### REQ-NET-003: offline login と configuration

- Known Packs を使い、クライアントが持つ vanilla データはレジストリ本体の送信を省く。
- **受入条件**
  - [ ] vanilla クライアントが offline mode でログインし Play に入る
  - [ ] レジストリ（dimension type / biome 等）を利用者が追加・差し替えできる

### REQ-NET-004: HAProxy PROXY protocol

- v1 / v2 を受け、実クライアントの IP を接続に持たせる。任意か必須かを設定で選ぶ。ヘッダは信頼する proxy の送信元（許可リスト）からだけ受け付ける。誰でも IP を偽れるため。
- **受入条件**
  - [ ] PROXY ヘッダの IP が接続の相手として取れる
  - [ ] 必須の設定で、ヘッダの無い接続を拒否する
  - [ ] 許可リストに無い送信元からの PROXY ヘッダを、形式が正しくても拒否する

### REQ-NET-005: plugin message（login・play）

- login 段階の要求・応答（タイムアウト付き。Velocity の転送が使う）と、play 段階の任意チャンネルの送受信・イベント。
- **受入条件**
  - [ ] login 段階で要求を送り、応答を受けるか、タイムアウトで切断できる
  - [ ] play 段階で任意チャンネルを送受信でき、受信がイベントになる

### REQ-NET-006: サーバーの設定と停止

- `Server` の設定項目: 圧縮しきい値、keep alive の間隔と切断時間（応答 ID の検証と遅延の計測を含む）、Known Packs の応答タイムアウト、最大人数、tick レートと追いつきの上限、ソケットの調整。シグナル（Ctrl-C）での停止と、停止時のタスク。
- **受入条件**
  - [ ] 上の各項目を `Server` から設定でき、既定値は今の挙動と同じ
  - [ ] Ctrl-C で全員に理由を出して切断し、停止時のタスクを実行してから終わる
  - [ ] 各プレイヤーの遅延（ping）が取れる

### REQ-NET-007: 不正なクライアントへの制限

- 1 tick あたりのパケット数、受信キューの長さ、最大パケットサイズ（認証前は小さく）、NBT の容量を制限し、超えた接続だけを切る。不正パケットのログは抑える。単体公開（v0.3）で外から直接つながるため。
- **受入条件**
  - [ ] 制限を超えるボットが切断され、他のプレイヤーと tick に影響しない
  - [ ] 認証前の大きなパケットを読み切る前に拒否する

### REQ-NET-008: リソースパック

- 送信・削除・必須指定・状態のイベント。configuration 中にも送れる。
- **受入条件**
  - [ ] 必須のパックを拒否したプレイヤーを、イベントの処理で切断できる
  - [ ] 複数のパックを個別に削除できる

### REQ-NET-009: Transfer と Cookie

- 別サーバーへの転送、転送で来た接続の判定と受け入れ可否、Cookie の保存と取得（取得は非同期）。
- **受入条件**
  - [ ] 転送先で Cookie を読み、転送元が保存した値が取れる

### REQ-NET-010: configuration の制御と再突入

- configuration 段階のイベント（出現先の Instance・hardcore・feature flags・チャットの消去・registry を送るか）と、play から configuration へ戻す操作。
- **受入条件**
  - [ ] play 中のプレイヤーを configuration に戻し、registry を差し替えて play に戻せる

### REQ-NET-011: LAN 公開・旧形式 ping・Unix ソケット

- **受入条件**
  - [ ] 同じ LAN の vanilla クライアントの一覧に出る
  - [ ] 1.6 以前の形式の ping に応答する
  - [ ] Unix ドメインソケットで待ち受けられる

### REQ-NET-012: デバッグ描画の購読

- 26.x のデバッグ表示の購読要求を受け、値を送る。
- **受入条件**
  - [ ] 購読の要求がイベントになり、値を送るとクライアントのデバッグ表示に出る

## AUTH

### REQ-AUTH-001: Velocity modern forwarding

- **受入条件**
  - [ ] Velocity 配下で正しい UUID・スキンでログインできる
  - [ ] 共有シークレット不一致の接続を拒否する

### REQ-AUTH-002: online mode

- RSA 鍵交換、AES-CFB8 ストリーム暗号、Mojang session server 照会。
- **受入条件**
  - [ ] 正規アカウントでのみログインでき、スキンが表示される
  - [ ] session server 照会が tick スレッドをブロックしない
  - [ ] 認証 URL を差し替えられ、proxy 経由の接続を拒否する設定がある

### REQ-AUTH-003: BungeeCord / BungeeGuard 転送

- legacy forwarding（handshake のアドレス欄）と、BungeeGuard のトークン検証。Velocity と同じ設定の入り口から選ぶ。legacy forwarding は署名が無く、直接つながったクライアントが任意の UUID を名乗れるため、BungeeGuard を使わないときは信頼する proxy の送信元（許可リスト）からの接続だけを受け付ける。
- **受入条件**
  - [ ] BungeeCord 配下で正しい UUID・スキンでログインできる
  - [ ] BungeeGuard のトークンが無いか合わない接続を拒否する
  - [ ] BungeeGuard を使わない設定では、許可リストに無い送信元からの接続を拒否する

## WORLD

### REQ-WORLD-001: Instance と tick ループ

- Instance を 1 スレッドが所有し 20 TPS で tick する（D6）。接続とは channel でやり取りする。
- **受入条件**
  - [ ] tick が 50ms 周期で回り、超過時は追いつき処理とログを出す
  - [ ] Instance の状態にロックなしでアクセスできる
  - [ ] Instance 間の相互作用はメッセージ経由でしか行えない（型で保証）

### REQ-WORLD-002: チャンクと `ChunkLoader`

- パレットコンテナ、ハイトマップ、チャンクデータパケット。チャンク供給は `ChunkLoader` trait（生成器も同じ trait）。v0.1 はライトを全 15 で送る。
- **受入条件**
  - [ ] 平坦生成器でスポーン周辺が描画される
  - [ ] view distance に応じてチャンクの送信・破棄が行われる
  - [ ] 利用者が独自の `ChunkLoader` を差し込める

### REQ-WORLD-003: Anvil 読込

- **受入条件**
  - [ ] 最新版 vanilla で保存した `.mca` ワールドを読み込み、ブロックが一致する

### REQ-WORLD-004: ライティング計算

- **受入条件**
  - [ ] ブロック設置・破壊で空・ブロック光が vanilla と同等に更新される

### REQ-WORLD-005: 複数 Instance とプレイヤー移送

- **受入条件**
  - [ ] 複数 Instance がそれぞれのスレッドで並行に tick する
  - [ ] プレイヤーを Instance 間で移送できる（再ログインなし）

### REQ-WORLD-006: チャンクの管理

- 誰も見ていないチャンクの破棄、非同期の `ChunkLoader`、ロード・破棄・セクション無効化のイベント、クライアントの申告（`ChunkBatchReceived`）による送信量の調整、プレイヤーごとの view distance の実行中の変更。
- **受入条件**
  - [ ] 全員が去ったチャンクがメモリから消え、ロード・破棄がイベントになる
  - [ ] 遅い `ChunkLoader` が tick を止めない

### REQ-WORLD-007: ディメンションと biome

- overworld 以外のディメンション（Respawn を伴う移送）、biome の取得・設定、hashed seed。
- **受入条件**
  - [ ] nether 相当の Instance へ移送すると、空と高さが変わる
  - [ ] 設定した biome がクライアントに表示される

### REQ-WORLD-008: 生成 API

- 区画単位の fill・高さまでの fill・相対座標の setBlock / setBiome と、区画の境界をまたぐ構造物（fork）。チャンク実装の差し替え。`ChunkLoader` と同じ trait の上に置く（D10）。
- **受入条件**
  - [ ] 境界をまたぐ構造物が、隣の区画の生成時に欠けずに置かれる

### REQ-WORLD-009: チャンクを共有する Instance と複製

- 親のチャンクを共有する Instance（変更は相互に見える）と、Instance の複製。ゲームの部屋を同じマップで並べるため。
- **受入条件**
  - [ ] 1 つのマップから複数の部屋を作り、ブロックの変更が部屋の間で漏れない（複製の場合）

### REQ-WORLD-010: ブロックの handler と block entity

- ブロックに NBT と handler（設置・破壊・操作・接触・tick）を付ける。block entity の送信。
- **受入条件**
  - [ ] 看板やチェストの block entity がクライアントに表示される
  - [ ] handler の tick が、登録したブロックにだけ届く

### REQ-WORLD-011: 設置ルール

- 設置時と隣接の変化時に状態を決める口（向き・つながり）と置き換え可否。隣接更新の vanilla 挙動そのものは書かない（D13）。
- **受入条件**
  - [ ] 階段の向きを決めるルールを利用者が書け、設置で使われる

### REQ-WORLD-012: ブロックの batch

- まとめて適用・取り消し用の逆 batch。
- **受入条件**
  - [ ] 1 万ブロックの batch を 1 tick で適用し、逆 batch で元に戻る

### REQ-WORLD-013: 世界の状態

- ワールドボーダー（変化アニメーション・警告）、時間（速度・一時停止・world clock / timeline）、天候、難易度、月齢、game rule（クライアントからの取得・設定要求を含む）、environment attributes（26.x）。
- **受入条件**
  - [ ] 各項目を Instance 単位で変えると、中のプレイヤーに反映される

### REQ-WORLD-014: 演出

- パーティクル、world event、block action、破壊アニメーション、爆発のパケット。爆発の計算は vanilla 挙動で持たない（D1）。
- **受入条件**
  - [ ] 指定した位置にパーティクルを出し、近くのプレイヤーだけに届く

### REQ-WORLD-015: ブロックの補助

- 視線方向のブロック走査、破壊時間の計算（survival の掘りが使う）、adventure モード用の block predicate、fluid の registry。
- **受入条件**
  - [ ] 視線の先の最初の固体ブロックが取れる
  - [ ] 破壊時間がツールと効果を考慮して vanilla と一致する

## ENT

### REQ-ENT-001: プレイヤーの表示・移動同期

- **受入条件**
  - [ ] 2 クライアントが互いを視認でき、移動・視線・スニークが同期する
  - [ ] タブリストに参加者が載り、退出で消える

### REQ-ENT-002: エンティティと簡易物理

- 表示・移動・汎用のメタデータ（任意の index と値）、見た目のフラグ（発光・透明・名前表示・無音・姿勢）、速度・重力・空気抵抗とブロック衝突、ライフサイクルのイベント（spawn・despawn・tick・teleport）。AI は持たない（D13）。種別ごとの型は REQ-ENT-005。
- **受入条件**
  - [ ] 任意のエンティティ種をスポーンし、重力で落下して地面で止まる
  - [x] 視界外のエンティティは送信されない（プレイヤーのエンティティ。他の種は上の項目と一緒に v0.2）

### REQ-ENT-003: インベントリ

- プレイヤーとコンテナ（25 種）、vanilla と同じクリック処理、開く・閉じる・クリック前・クリックのイベント、creative の操作。ItemStack は GUI に要る分（種類・数・名前・説明）。全データコンポーネントは REQ-UI-009。
- **受入条件**
  - [ ] プレイヤーインベントリとチェスト型 GUI を開閉・操作でき、クリックがイベントになる

### REQ-ENT-004: スコアボード

- サイドバー、名前の下、タブリストの objective、チーム。Scoreboard と Team は Audience になる（REQ-TEXT-003）。
- **受入条件**
  - [ ] サイドバー・チームを利用者コードから表示・更新できる
  - [ ] 名前の下とタブリストにスコアを出せる

### REQ-ENT-005: 全種のメタデータと Display・NPC

- 全エンティティ種の型付きメタデータ（一括送信）、Block / Item / Text Display、Interaction、Marker、Mannequin による NPC（スキン指定）、ベッド・エリトラ・刺さった矢などの状態、村人の職業。
- **受入条件**
  - [ ] Text Display でホログラムを出し、文字を更新できる
  - [ ] Mannequin に任意のスキンを付けて立たせられる

### REQ-ENT-006: 衝突・当たり判定・空間検索

- 全ブロック形状との衝突、エンティティ間の衝突、掃引判定、バウンディングボックス、視線判定、レイキャスト、ノックバック、範囲内のエンティティの検索。
- **受入条件**
  - [ ] 階段・ハーフブロックの上をエンティティが正しく歩く
  - [ ] レイキャストで最初に当たるブロックかエンティティが取れる

### REQ-ENT-007: 乗り物とリード

- **受入条件**
  - [ ] プレイヤーをエンティティに乗せ、操縦の入力がイベントになる
  - [ ] リードでつないだ表示が出る

### REQ-ENT-008: 装備

- **受入条件**
  - [ ] エンティティとプレイヤーの装備が他のプレイヤーに見え、変更がイベントになる

### REQ-ENT-009: ポーション効果と属性

- 効果の付与・削除とイベント、属性と modifier（速度・最大体力等）。
- **受入条件**
  - [ ] 移動速度の modifier でクライアントの移動が速くなる

### REQ-ENT-010: 体力・ダメージ・死亡

- 体力と回復、種類つきのダメージと無敵時間、ダメージ・死亡のイベント、消える演出、炎上、状態のアニメーション（腕振り・被ダメージ）。戦闘の計算は持たない（D13）。
- **受入条件**
  - [ ] ダメージで体力が減り、演出が見え、0 で死亡イベントが出る

### REQ-ENT-011: 投射物・アイテム・経験値オーブ

- 投射物の発射とブロック・エンティティへの衝突イベント、アイテムエンティティ（合体・拾得・ドロップ）、経験値オーブ（拾得）。
- **受入条件**
  - [ ] 雪玉が当たったエンティティがイベントで取れる
  - [ ] 落ちたアイテムを拾うと、インベントリに入りイベントになる

### REQ-ENT-012: 視認と同期の制御

- viewer の条件と手動の追加・削除、エンティティごとの表示距離、位置同期の間隔、種別の動的な変更。
- **受入条件**
  - [ ] 特定のプレイヤーにだけ見えるエンティティを作れる

## PLAYER

### REQ-PLAYER-001: プレイヤーの状態

- ゲームモード（変更イベント・F3+F4 の要求）、能力（飛行・速度・視野・即時破壊・無敵）、権限レベル（op 0〜4）、体力・空腹・経験値、リスポーン（地点・画面・死亡地点）、テレポート（相対指定・確認 ID の管理）、任意の理由での kick。
- **受入条件**
  - [ ] ゲームモードを変えると、クライアントの操作（creative の飛行・survival の体力表示）が変わる
  - [ ] テレポートの確認前に届いた古い位置を無視する
  - [ ] 落下したプレイヤーをスポーンへ戻せる（backlog の lobby の持ち越し）

### REQ-PLAYER-002: タブリスト

- 一覧への表示可否、並び順、表示名、遅延（ping）の表示。ヘッダー・フッターは REQ-TEXT-003。
- **受入条件**
  - [ ] 表示名と並び順を変えると、全員のタブリストに反映される

### REQ-PLAYER-003: スキン

- Mojang からのプロフィール取得と署名の検証、接続時のスキン設定イベント、実行中の変更。online mode（REQ-AUTH-002）と proxy 転送（REQ-AUTH-001）で受けたスキンを使う。
- **受入条件**
  - [ ] offline mode でも、指定した名前のスキンを付けられる
  - [ ] 実行中にスキンを変えると、他のプレイヤーに反映される

## UI

クライアントの画面に出る機能のうち、v0.2 の Audience とインベントリより先のもの。

### REQ-UI-001: 専用 GUI

- 金床（名前入力のイベント）、ビーコン、醸造台、エンチャント台、かまど、村人の取引、馬、GUI の数値表示、ボタン、バンドルの選択。
- **受入条件**
  - [ ] 金床の名前入力と村人の取引がイベントとして取れる

### REQ-UI-002: 実績とトースト

- **受入条件**
  - [ ] 実績のタブを作って進捗を付けられ、任意のトーストを出せる

### REQ-UI-003: レシピとレシピブック

- レシピの追加・削除・表示、レシピブックの設定、ゴーストレシピ。クラフト結果の計算は持たない（D1。Minestom にも無い）。
- **受入条件**
  - [ ] 追加したレシピがレシピブックに出る

### REQ-UI-004: Dialog

- 5 種（Notice・Confirmation・MultiAction・DialogList・ServerLinks）の表示・閉じる・カスタムクリックのイベント。
- **受入条件**
  - [ ] 確認の Dialog を出し、押したボタンがイベントで取れる

### REQ-UI-005: 本と看板

- 本を開く・編集のイベント、看板の編集画面を開く・編集のイベント。
- **受入条件**
  - [ ] 看板の編集画面を開き、入力された文字がイベントで取れる

### REQ-UI-006: 地図

- 地図データの送信（まずはパケットの直送。描画の補助は計測と需要を見て）。
- **受入条件**
  - [ ] 任意の画像を地図に表示できる

### REQ-UI-007: クライアント表示の細目

- 観戦・カメラ、locator bar の waypoint、統計、デバッグ画面の情報制限、ゲーム状態の変更（雨・エンドロール・デモ）、アイテムのクールダウン表示、クライアントの tick 速度制御。
- **受入条件**
  - [ ] 各項目をプレイヤー単位で送ると、クライアントの表示が変わる

### REQ-UI-008: クリックのコールバック

- Component の click にサーバー側の処理を結び付ける（使用回数・期限つき）。
- **受入条件**
  - [ ] チャットの文字を押すと、結び付けた処理が 1 回だけ動く

### REQ-UI-009: アイテムの全データコンポーネント

- 食料・道具・消費・装着・武器・地図・本・花火・旗・トリム等の約 40 種、NBT / JSON の codec（REQ-PROTO-007）、hover でのアイテム表示。
- **受入条件**
  - [ ] 全種のコンポーネントを付けたアイテムが vanilla クライアントで正しく表示される

## TEXT

Adventure 相当の層（D19）。Component と MiniMessage は `lodeframe-text`、Audience は本体（D20）。

### REQ-TEXT-001: Component モデル

- v0.1: text / color / decoration のみ。v0.2: style（font・shadow 含む）、hover / click イベント、translatable / score / selector / keybind、子要素。builder API。NBT と JSON の両方でシリアライズ。
- **受入条件**
  - [x] v0.1: 色・装飾付きテキストをチャットに表示できる
  - [ ] v0.2: 全種の Component が vanilla クライアントで正しく表示される
  - [ ] NBT / JSON の round-trip が一致する

### REQ-TEXT-002: MiniMessage 実行時パーサ

- `<red>`、`<bold>`、`<gradient>`、`<rainbow>`、`<hover>`、`<click>`、`<reset>`、placeholder（名前付き引数）。Component → MiniMessage 文字列の逆変換。
- **受入条件**
  - [ ] Adventure の MiniMessage と同じ入力で同じ見た目になる（主要タグ）
  - [ ] 不正な入力はパニックせず、エラー位置付きで返すか平文として扱う（選択可能）
  - [ ] placeholder に利用者入力を渡してもタグとして解釈されない

### REQ-TEXT-003: Audience

- Player・Instance・任意のグループが共通 trait を実装する: メッセージ、Title（時間指定）、ActionBar、Sound（位置・カテゴリ・停止・自分以外へ）、BossBar の表示・更新・非表示、タブリストのヘッダー・フッター。key で登録する独自の audience と、1 回の encode で全員へ送るまとめ送り。
- **受入条件**
  - [ ] 同じコードで 1 人・Instance 全員・任意のグループに送れる
  - [ ] BossBar の進捗・色・タイトル変更が表示中のクライアントに反映される
  - [ ] 1 人を除いて送れる（D29 の (5)）

### REQ-TEXT-004: 端末向けの変換とログ

- Component から ANSI・legacy（`§`）への変換、Component をそのまま書けるログ出力。
- **受入条件**
  - [ ] 色付きの Component が端末のログに色付きで出る

## API

### REQ-API-001: 階層イベントノード

- `node.on::<E>(|ev, ctx| ..)` で型付きハンドラを登録。ノードは木構造でグローバル / Instance 単位に付け外しでき、キャンセル可能イベントを持つ（D9）。
- **受入条件**
  - [x] 子ノードの付け外しでハンドラ群がまとめて有効・無効になる
  - [x] キャンセルされたイベントは既定動作（ブロック設置等）が行われない
  - [x] ハンドラから `&mut` で Instance を操作できる

### REQ-API-002: チャット・ブロック操作イベント

- **受入条件**
  - [x] チャット送信・ブロック設置 / 破壊がイベントとして届き、キャンセル・改変できる
  - [x] 既定動作で他プレイヤーにチャット・ブロック変更が反映される

### REQ-API-003: 非同期処理の spawn→戻し

- ハンドラは同期 fn。`ctx.spawn(async {..}).then(|res, ctx| ..)` で結果を同じ Instance スレッドで受ける。ログイン前検証用に async イベントを別枠で持つ（D16）。
- **受入条件**
  - [ ] ハンドラから DB 相当の非同期処理を投げても tick が遅延しない
  - [ ] 結果コールバックでプレイヤーが退出済みの場合を扱える
  - [ ] ログイン前の async イベントで、入室の拒否と UUID・名前の差し替えができる

### REQ-API-004: コマンド

- Brigadier 互換のコマンドツリーを送り、クライアント補完が効く。
- **受入条件**
  - [ ] 引数型（整数・文字列・プレイヤー・座標）の補完とパースが動く
  - [ ] 権限でコマンドの可視性を切り替えられる
  - [ ] サブコマンド・別名・実行条件・未知のコマンドの処理・コマンドの横取りイベント・コンソールからの実行がある

### REQ-API-005: プレイヤーの操作イベント

- 移動・入力・ダッシュ・tick、クライアント設定と言語、`PlayerLoaded`、採掘の開始・中断・完了、ピック、アイテムの使用（開始・終了・中断）、エンティティの操作・攻撃、持ち替え・ドロップ・スロット選択、チャットの書式と chat type（D29 の (4)）。
- **受入条件**
  - [ ] 各イベントがキャンセル可能なものはキャンセルで既定動作が止まる（移動のキャンセルは位置を戻す）

### REQ-API-006: スケジューラ

- 遅延・繰り返し・次回を自分で決めるタスク、tick の開始時か終了時か、停止。全体・Instance・エンティティ・プレイヤー単位で持ち、持ち主が消えたら止まる。`ctx.spawn`（REQ-API-003）と同じ文脈の型で受ける。Cooldown 等の補助。
- **受入条件**
  - [ ] 1 秒ごとのカウントダウンを書け、プレイヤーの退出でそのプレイヤーのタスクが止まる
  - [ ] テストハーネスの `env.tick(n)` で決定的に進む

### REQ-API-007: 利用者データの付与

- Minestom の Tag API の目的（D30）。型付きの key で値を付ける。
- v0.2: Player・Entity・Instance（実行中の値、保存しない）と ItemStack（アイテムの `custom_data` に入り、クライアントとの往復とインベントリの操作で残る）。
- v0.3: Block（位置ごとの NBT。REQ-WORLD-003 の block entity の読込と同じ仕組み）。
- **受入条件**
  - [ ] 型の合わない key での読み出しがコンパイルエラーか `None` になる（実行時に panic しない）
  - [ ] GUI のアイテムに付けた値を、クリックのイベントで読める
  - [ ] v0.3: ブロックに付けた値を、そのブロックの破壊・操作のイベントで読める

### REQ-API-008: コマンドの全引数型とセレクタ

- Minestom の全引数型（ItemStack・BlockState・NBT・Component・相対座標・範囲・時間・リソース等）とエンティティセレクタ（`@e[type=..]`）。エンティティの揃う v0.5。
- **受入条件**
  - [ ] `@e[type=zombie,distance=..10]` が vanilla と同じ集合を返す

### REQ-API-009: イベントノードの拡張

- 優先度、条件・利用者データで絞るノード、リスナーの失効（回数・条件）、キャンセル済みを無視する指定、継承イベント（親のイベントのリスナーが子も受ける）、複数イベントの束ね、エンティティ・Instance ごとのノード。発火中に追加したハンドラと内側のイベントを失わない（D29 の (2)）。
- **受入条件**
  - [ ] 優先度の高いハンドラが先に呼ばれる
  - [ ] ハンドラの中で起こしたイベントが、同じ tick のうちに届く

## MACRO

### REQ-MACRO-001: `derive(Encode, Decode)`

- `#[derive(Packet)] #[packet(id = .., state = Play, side = Clientbound)]` を含む。パケット ID は状態と向きごとにしか一意でないため `side` が要る。利用者の独自パケット・プラグインメッセージにも使える。
- derive は既定で `::lodeframe::protocol` を指す。protocol 内部や protocol を直接使う crate は `#[lodeframe(crate = crate)]` で指定する（D22）。
- **受入条件**
  - [ ] v0.1 の全パケットがマクロで定義されている
  - [ ] 属性の誤りはフィールドを指すコンパイルエラーになる

### REQ-MACRO-002: `#[command]`

- 関数シグネチャからコマンドツリー・補完・パースを生成。`Option<T>` は省略可能引数。
- **受入条件**
  - [ ] `#[command("give")] fn give(ctx, target: Player, count: Option<u8>)` が補完付きで動く

### REQ-MACRO-003: `#[event]` / `derive(Event)`

- **受入条件**
  - [ ] 独自イベントを derive 1 行で定義し、イベントノードに流せる

### REQ-MACRO-004: アイテム・GUI の宣言的マクロ

- アイテム定義、インベントリ GUI 定義。
- **受入条件**
  - [ ] 書式の誤りがコンパイルエラーになる

### REQ-MACRO-005: `text!`

- `text!("<red>Hello {name}")`。REQ-TEXT-002 と同じパーサをコンパイル時に使う。`{name}` はスコープ内の変数を placeholder として埋め込む。
- **受入条件**
  - [ ] タグの誤りがリテラル内の位置を指すコンパイルエラーになる
  - [ ] 実行時パーサと同じ Component を生成する

## PERF

### REQ-PERF-001: 性能目標

- 数値は暫定。M1-18 のベースライン計測（[bench-plan.md](implementation/bench-plan.md) の結果）と Minestom の同条件計測で v0.4 までに確定する。
- ベースライン（2026-10-01、Apple M4 Pro）: 起動 3〜4 ms、アイドル 3.1 MB、プレイヤー 1 人で約 120 MB。20 TPS を保つのは、視界の外へ散らばれば 500 体（[view-culling-plan.md](implementation/view-culling-plan.md)）、全員が同じ場所でも 500 体（1 tick の平均 28 ms、[move-batch-plan.md](implementation/move-batch-plan.md)）。
- **受入条件**
  - [ ] アイドル時 RSS < 10MB（examples/lobby、プレイヤー 0）
  - [ ] 起動からリッスン開始まで < 50ms
  - [x] ボット 500 体の接続・移動で 20 TPS を維持（散らばった場合も全員が同じ場所の場合も達成。Apple M4 Pro、ボットは同じマシン。余裕は大きくない: 同じ場所で 1 tick の平均 28 ms）
  - [ ] 同条件の Minestom との比較結果を CI で出力

## INFRA

### REQ-INFRA-001: CI

- **受入条件**
  - [ ] fmt / clippy（警告エラー）/ test が PR で走る
  - [ ] MSRV を明記しビルドを CI で検査する

### REQ-INFRA-002: 自前ボットによる統合・負荷試験

- `lodeframe-protocol` で書いた軽量ボット（D15）。
- **受入条件**
  - [x] ボットがログイン・移動・チャットを行う統合テストが CI で走る
  - [x] 同じボットで N 体の負荷試験を起動できる

### REQ-INFRA-003: crates.io 公開

- **受入条件**
  - [ ] MIT OR Apache-2.0 で 3 crate を公開し、docs.rs でドキュメントが読める

### REQ-INFRA-004: 利用者向けテストハーネス

- `lodeframe` の `test-util` feature（D21）。`TestEnv`（ネットワーク・実時間なしの Instance）、`env.tick(n)`、`FakePlayer`（パケット注入）、送信パケットと発火イベントの記録。`ctx.spawn` の future は `env.run_until_idle()` で完了まで進める。
- v0.1 は接続・移動・チャット・ブロック操作まで。以後の API（コマンド、インベントリ、Audience 等）は追加と同時にハーネスから操作・検証できるようにする。
- **受入条件**
  - [x] 利用者のイベントハンドラを通常の `#[test]` で、ポートを開かずに検証できる
  - [x] 同じテストを何度実行しても結果が同じ（実時間・乱数・スレッドのタイミングに依存しない）
  - [ ] lodeframe 自身の v0.1 以降の機能テストもこのハーネスで書かれている

## OPS

### REQ-OPS-001: 監視

- tick の監視イベント（時間・遅れ）と、主なイベント（参加・退出・チャット・コマンド・チャンクのロード）の計測を tracing で出す。Minestom は JFR に出す。
- **受入条件**
  - [ ] tick の時間を毎 tick イベントで受け取れる

### REQ-OPS-002: スナップショット

- サーバー・Instance・チャンク・エンティティ・プレイヤーの不変の写しを、別スレッドへ渡せる形で取る。
- **受入条件**
  - [ ] 取った写しが、その後の tick の変更に影響されない

### REQ-OPS-003: ハンドラの panic で止まらない

- 今はハンドラの panic で Instance のスレッドが止まり、中の全員が固まる。panic を捕まえて利用者の例外ハンドラに渡し、Instance を続ける。単体公開（v0.3）で 1 人の入力が全員を止めないため。
- **受入条件**
  - [ ] panic するハンドラがあっても、他のプレイヤーの tick が続き、例外ハンドラが呼ばれる
