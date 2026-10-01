# 要件

優先度は Must / Should / Could。マイルストーンは [roadmap](implementation/roadmap.md) を参照。
根拠は [decisions.md](decisions.md)。

## スコープ外

vanilla 挙動（mob AI、レッドストーン、ワールド生成、クラフト、戦闘計算）、
複数プロトコル版の同時対応、Anvil への保存。AI 等は将来 util crate で扱う（D13）。
サーバー側翻訳（GlobalTranslator 相当）は後回し（D19）。

## 一覧

| ID | タイトル | 優先度 | MS |
|---|---|---|---|
| REQ-PROTO-001 | プロトコル基本型と Encode / Decode | Must | v0.1 |
| REQ-PROTO-002 | vanilla 生成データからの codegen | Must | v0.1 |
| REQ-PROTO-003 | NBT | Must | v0.1 |
| REQ-PROTO-004 | 座標・ベクトル型 | Must | v0.1 |
| REQ-NET-001 | 接続層（フレーミング・圧縮・状態遷移） | Must | v0.1 |
| REQ-NET-002 | Status ping | Must | v0.1 |
| REQ-NET-003 | offline login と configuration | Must | v0.1 |
| REQ-AUTH-001 | Velocity modern forwarding | Must | v0.2 |
| REQ-AUTH-002 | online mode（暗号化・Mojang 認証） | Must | v0.3 |
| REQ-WORLD-001 | Instance と tick ループ | Must | v0.1 |
| REQ-WORLD-002 | チャンクと `ChunkLoader` | Must | v0.1 |
| REQ-WORLD-003 | Anvil 読込 | Should | v0.3 |
| REQ-WORLD-004 | ライティング計算 | Should | v0.3 |
| REQ-WORLD-005 | 複数 Instance とプレイヤー移送 | Must | v0.3 |
| REQ-ENT-001 | プレイヤーの表示・移動同期 | Must | v0.1 |
| REQ-ENT-002 | エンティティと簡易物理 | Must | v0.2 |
| REQ-ENT-003 | インベントリ | Must | v0.2 |
| REQ-ENT-004 | スコアボード | Should | v0.3 |
| REQ-TEXT-001 | Component モデル | Must | v0.1 / v0.2 |
| REQ-TEXT-002 | MiniMessage 実行時パーサ | Must | v0.2 |
| REQ-TEXT-003 | Audience | Must | v0.2 |
| REQ-API-001 | 階層イベントノード | Must | v0.1 |
| REQ-API-002 | チャット・ブロック操作イベント | Must | v0.1 |
| REQ-API-003 | 非同期処理の spawn→戻し | Must | v0.2 |
| REQ-API-004 | コマンド | Must | v0.2 |
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

## NET

### REQ-NET-001: 接続層

- tokio の TCP リスナー、パケット長フレーミング、zlib 圧縮（閾値設定可）、Handshake → Status / Login → Configuration → Play の状態機械。
- **受入条件**
  - [ ] 圧縮有無の両方でクライアントが接続できる
  - [ ] 不正パケット・タイムアウトで該当接続だけが切断され、サーバーは継続する
  - [ ] 1 パケットの最大長を超える入力を読み切る前に拒否する

### REQ-NET-002: Status ping

- **受入条件**
  - [ ] サーバー一覧に MOTD・人数・版が表示され、ping 値が出る
  - [ ] MOTD と人数を利用者コードから差し替えられる

### REQ-NET-003: offline login と configuration

- Known Packs を使い、クライアントが持つ vanilla データはレジストリ本体の送信を省く。
- **受入条件**
  - [ ] vanilla クライアントが offline mode でログインし Play に入る
  - [ ] レジストリ（dimension type / biome 等）を利用者が追加・差し替えできる

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

## ENT

### REQ-ENT-001: プレイヤーの表示・移動同期

- **受入条件**
  - [ ] 2 クライアントが互いを視認でき、移動・視線・スニークが同期する
  - [ ] タブリストに参加者が載り、退出で消える

### REQ-ENT-002: エンティティと簡易物理

- 表示・移動・メタデータ、重力とブロック衝突。AI は持たない（D13）。
- **受入条件**
  - [ ] 任意のエンティティ種をスポーンし、重力で落下して地面で止まる
  - [x] 視界外のエンティティは送信されない（プレイヤーのエンティティ。他の種は上の項目と一緒に v0.2）

### REQ-ENT-003: インベントリ

- **受入条件**
  - [ ] プレイヤーインベントリとチェスト型 GUI を開閉・操作でき、クリックがイベントになる

### REQ-ENT-004: スコアボード

- **受入条件**
  - [ ] サイドバー・チームを利用者コードから表示・更新できる

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

- Player・Instance・任意のグループが共通 trait を実装する: メッセージ、Title（時間指定）、ActionBar、Sound（位置・カテゴリ）、BossBar の表示・更新・非表示。
- **受入条件**
  - [ ] 同じコードで 1 人・Instance 全員・任意のグループに送れる
  - [ ] BossBar の進捗・色・タイトル変更が表示中のクライアントに反映される

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

### REQ-API-004: コマンド

- Brigadier 互換のコマンドツリーを送り、クライアント補完が効く。
- **受入条件**
  - [ ] 引数型（整数・文字列・プレイヤー・座標）の補完とパースが動く
  - [ ] 権限でコマンドの可視性を切り替えられる

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
