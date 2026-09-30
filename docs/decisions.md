# 決定事項

設計セッション（2026-09-30）で決めたこと。覆すときは行を消さず、
ステータスを `Superseded` にして新しい行を足す。

| # | 決定 | 根拠 | ステータス |
|---|---|---|---|
| D1 | **ライブラリ型**（Minestom 型）。単体サーバーは作らず、`examples/` に動くロビーを 1 つ置く | vanilla 互換（mob AI、レッドストーン、ワールド生成）は実装量が桁違いで minimal と矛盾し、Pumpkin / FerrumC と正面衝突する。Rust のライブラリ型は Valence が ECS で重く停滞気味で、素直なイベント API の隙間がある | Accepted |
| D2 | 言語は **Rust** | tokio / flate2 / aes / rsa / serde が揃う。ライブラリ型なので利用者層が要る。MoonBit は AES-CFB8・RSA・zlib・HTTPS が未成熟で C FFI 自前になり、利用者もほぼいない | Accepted |
| D3 | 対応プロトコルは**最新リリース 1 本のみ**。古いクライアントは ViaProxy 等を前段に置いて吸収 | 実装量最小。特定版固定は Valence（1.20.1 で停止）と同じ失速パターン | Accepted |
| D4 | パケットは**必要な分だけ手書き**、ブロック状態・レジストリ・パケット ID は **vanilla の data generator 出力から codegen** | 数万のブロック状態を手で持つのは非現実的。既存 crate（azalea-protocol 等）依存はクライアント寄りの型と追従タイミングをコアごと他人に握られる | Accepted |
| D5 | codegen は **xtask で実行し、生成済み Rust コードを commit** する。`build.rs` で server.jar を取得しない | crates.io からのビルドをオフラインで完結させる。Minestom / Pumpkin も生成データを repo に持つ | Accepted |
| D6 | **Instance 単位の単一スレッド所有**。I/O は tokio、Instance 間は message passing のみ、グローバル可変シングルトン禁止 | ロック不要でハンドラに `&mut` を渡せる。複数 Instance で自然にスケールし、将来の Instance 内並列化（region 分割）の余地も残る | Accepted |
| D7 | v0.1 = **接続〜スポーン + マルチプレイ基礎** | イベント API の形を検証できる最小セット | Accepted |
| D8 | 認証は **Velocity modern forwarding 先行（v0.2）→ 単体 online mode（v0.3）** | forwarding は HMAC 検証だけで軽い。Minestom 利用者の実態も proxy 配下運用が主流 | Accepted |
| D9 | イベントは**型付きハンドラ登録 + 階層ノード**（Minestom の EventNode 相当） | 複数ミニゲームの同居・付け外しに階層が要る | Accepted |
| D10 | ワールドは **`ChunkLoader` trait + Anvil 読込のみ**。保存は利用者実装 | ミニゲームは毎回初期化が基本。バニラで建築したロビー配布はカバーする | Accepted |
| D11 | proc-macro は **4 系統**: `derive(Encode, Decode)`（v0.1）、`#[command]`・`#[event]` / `derive(Event)`（v0.2）、宣言的 UI（`text!` / `item!` / GUI、v0.3） | 開発体験の向上。コンパイル時間とエラーの分かりにくさはコストとして受け入れ、段階導入する | Superseded in part by D19（`text!` を v0.2 へ） |
| D12 | 性能は**実測で比較可能な目標**を置き、Minestom と同条件で比べるベンチを持つ | 「lightweight」を主張する根拠 | Accepted |
| D13 | 標準機能は **Minestom 相当**。AI・pathfinding・戦闘は持たず、必要なら後で util crate として配る | minimal の境界線 | Accepted |
| D14 | crate は**最小分割 + facade**（protocol / macros / 本体）。境界が固まったら分割を再検討 | 初期は境界が動くので細分割は摩擦になる。protocol はボット・proxy 用途で単独利用できるよう分ける | Accepted（D20 で text を追加分割） |
| D15 | テストは **単体 + 自前ボット**。ボットは lodeframe-protocol で書き、統合テストと負荷試験に兼用 | azalea はボット 1 体が重く負荷試験側がボトルネックになり、版追従がずれるとテストが止まる | Accepted |
| D16 | ハンドラは**同期 fn 固定**。非同期処理は `ctx.spawn(async {..}).then(|res, ctx| ..)` で tokio に投げ、結果は同じ Instance スレッドで受ける。ログイン前検証用の async イベントだけ別枠 | async ハンドラは await 中に `&mut World` を保持できず所有モデルと衝突し、tick 遅延の温床になる | Accepted |
| D17 | OSS、**MIT OR Apache-2.0**、crates.io 公開 | Rust エコシステム標準 | Accepted |
| D18 | **stable 最新追従、edition 2024**。MSRV は明記するが積極的に上げる | 初期利用者は最新 stable 前提で問題ない | Accepted |
| D19 | **Adventure 相当のテキスト層**を自前で持つ: Component モデル完全版（v0.1 は最小、v0.2 で完全）、Audience 抽象、MiniMessage 実行時パーサ（v0.2）。`text!` は同じパーサをコンパイル時に使い v0.2 に前倒し。翻訳は後回し | ミニゲームで最も書くコード。設定・DB 由来の文字列には実行時パースが要る。Rust に Adventure 相当の定番 crate は無い | Accepted |
| D20 | Component と MiniMessage は **`lodeframe-text` crate** に置き、protocol と macros の両方が依存する。Audience は本体 | macros が protocol（生成データ込み）に依存するとマクロのビルドが重い。ボット・proxy からも単独で使える明確な境界 | Accepted |
| D21 | 利用者向けに**ヘッドレステストハーネス**を提供する（`lodeframe` の `test-util` feature）。`TestEnv` でネットワークなしに Instance を作り、`tick(n)` で時間を手動で進め、`FakePlayer` でパケットを注入し、送信パケットと発火イベントを assert する。v0.1 から用意し、以後の API は追加と同時にハーネス対応する | 単一スレッド所有（D6）なので決定的に tick を進められる。通常の `#[test]` で高速に動く。別 crate にせず feature にすることで内部状態に触れられる | Accepted |
| D22 | derive が生成するコードは既定で `::lodeframe::protocol` を指し、`#[lodeframe(crate = <path>)]` で上書きする。`Encode` / `Decode` / `Packet` の trait と derive は `lodeframe-protocol` が同名で再エクスポートする。enum の tag は Rust の判別子規則（整数リテラル、省略時は前の値 + 1）で、重複は rustc に検出させる | 利用者は facade の `lodeframe` だけに依存する（D14）。proc-macro-crate による自動検出は依存が重いので、属性による上書きで足りる間はそれで済ませる | Accepted |
| D23 | 座標・ベクトルは **`Vec3 = glam::DVec3`**（再エクスポート）と自前の **`Pos`**（`DVec3` + yaw / pitch）・**`BlockPos`**（i32 の newtype）。いまの wire 型 `Position` は `BlockPos` に改名する。型は protocol に置く | Minestom の `Vec` / `Pos` / `BlockVec` 相当。ベクトル演算（dot / length / normalize）を自作すると glam とほぼ同じものを書くことになる。glam は Rust のゲーム開発での事実上の標準で、利用者が自分のコードと直接つなげられる。`Pos` と `BlockPos` はワイヤー形式・範囲・チャンク座標変換の意味を持つので自前 | Accepted |
| D24 | ブロック状態は**表 + 計算**で持つ。ブロックごとに名前・先頭状態 ID・既定状態・プロパティ定義だけを生成し、状態 ID ⇔ プロパティ値は混合基数（プロパティ名のアルファベット順、先頭が最上位）で計算する。生成時に全状態で検証し、崩れたら失敗させる | 35,723 状態を全展開すると生成物が数 MB になる。26.3 で全ブロックがこの規則に従うことを確認済み（JSON 上のプロパティ順とは 12 ブロックで違う） | Accepted |
| D25 | `#[packet(id = ..)]` は整数リテラルに限らず**定数式**を受け付け、生成された `ids::<state>::<side>::<NAME>` を参照する | 版を上げて再生成しても手書きパケットが無修正で追従し、消えたパケットはコンパイルエラーで分かる | Accepted |
| D26 | datagen に要る Java は **`mise.toml` の `java` で揃える**。通常のビルド・CI（check / msrv）は Java を要求しない | datagen は開発者だけが使うタスクで、生成物は commit される（D5） | Accepted |
