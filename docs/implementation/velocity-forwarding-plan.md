# Velocity modern forwarding 実装計画（M2-08）

> **Status**: 実装済み — 2026-10-03

要件: REQ-AUTH-001。決定: D6・D21・D37・D38・D39（[decisions.md](../decisions.md)）。
`lodeframe`（本体）と `lodeframe-bot` にまたがり、`Profile`（公開型）が変わるので、コードの前にここで形を決める。M2-09（BungeeCord）・M2-10（HAProxy）が同じ設定の入り口に足す。

## 流れ

Velocity の modern forwarding は、login 中の plugin message で転送情報を渡す。

1. クライアント（= Velocity）が `Hello`（名前）を送る
2. サーバーが `CustomQuery`（チャンネル `velocity:player_info`、`data` = `[1]`）を送る。`1` は要求する転送形式の版（鍵情報の無い `MODERN_DEFAULT`）。鍵付きの版（2 以上）は要求しない
3. Velocity が `CustomQueryAnswer` で返す。`data` = **32 バイトの HMAC-SHA256 署名** + 本体。署名は本体全体に対し、共有シークレットを鍵とする
4. 本体: `VarInt 版`（要求した 1 であること）・`String 接続元アドレス`・`UUID`・`String 名前`・`VarInt 件数`・件数ぶんの `{ String name, String value, bool 署名あり, [String signature] }`
5. サーバーは署名を**定数時間で**照らし、通ったら UUID・名前・properties でログインを続ける（`LoginFinished` に properties を載せる）

## 1. 設定（`server.rs`）

```rust
#[non_exhaustive]
pub enum Forwarding {
    /// proxy を使わない（既定）。名前から offline の UUID を作る
    None,
    /// Velocity modern forwarding。`secret` は Velocity の `forwarding.secret` と同じ
    Velocity { secret: Vec<u8> },
}
impl Server { pub fn forwarding(self, forwarding: Forwarding) -> Self; }
```

- 既定は `Forwarding::None`。今の挙動のまま
- `non_exhaustive`: M2-09 が `BungeeCord`・`BungeeGuard { tokens }` を足しても利用者の `match` が壊れない
- `secret` が空なら `start` が `InvalidInput` で返す（空の鍵は誰でも署名できる。server-config-plan の「黙って直さない」と同じ）
- `Forwarding` は `Debug` で `secret` を出さない（手書きの `Debug`。ログへの混入を防ぐ）
- 転送の待ち時間は `Server::forwarding_timeout(Duration)`（既定 5 秒）。Velocity が応答しない（直接つないだクライアント）ときの打ち切り

## 2. login（`login.rs`）

- `login::offline` と並べて **`login::velocity(conn, compression_threshold, secret, timeout) -> Result<Profile>`**（`Queries::ask` を内部で使う）。`Server` の接続処理が `Forwarding` で選ぶ
- 順: `Hello` を読む → 圧縮を有効にする（`offline` と同じ）→ `Queries::ask` → 検証 → `LoginFinished` → `LoginAcknowledged`
- **拒否**は login 段階の `Disconnect` で理由つき。切るのは次の場合:
  - 応答が無い（`ask` が `TimedOut`）。理由は「This server requires you to connect with Velocity.」
  - `Ok(None)`（クライアントがチャンネルを知らない = 直接つないだ vanilla）。理由は同じ
  - 署名が合わない・本体が短い・版が 1 でない・UTF-8 が壊れている。理由は「Unable to verify player details」
- 署名の照合は `hmac` の `verify_slice`（定数時間）。署名の前に本体を読まない（検証前の入力を解釈しない）
- 本体の上限: 1 件の文字列は vanilla と同じ上限（名前 16・value 32767・signature 1024）、件数は 64。確保の前に残りバイト数と照らす（rust.md の `check_remaining`）。通った後も名前は 1〜16 文字を再検査する
- 転送された名前・UUID を信用する（Hello の名前は捨てる）。Velocity はここを書き換えるため（オフライン UUID を作らない）

## 3. `Profile`（公開型の変更）

```rust
pub struct Profile {
    pub uuid: Uuid,
    pub name: String,
    /// スキンなど。転送されなければ空
    pub properties: Vec<ProfileProperty>,
    /// proxy が教えた本物の接続元。転送が無ければ `None`
    pub remote_addr: Option<IpAddr>,
}
```

- `remote_addr` は、線の上では文字列（ポートの無い IP）で届く。`IpAddr` にパースし、パースできなければ拒否する。`login::offline` はソケットを知らないので `None` を入れる（直接つないだ接続の peer アドレスは `Profile` に入れない。proxy の IP と本物の IP を取り違えないため）
- `Message::Join` は `Profile` を運ぶので配線は増えない。**`Profile` を作る自作 Instance・テストは新しい欄を足す**（v0.x の変更として docs に書く）
- `ProfileProperty` は protocol の既存型（`LoginFinished` が使うもの）

## 4. 世界側（`world.rs`）

- `Player` が `properties` と `remote_addr` を持つ。`Player::info` が `PlayerInfo.properties` に載せる（いまは `Vec::new()`）。**タブリストと、他のプレイヤーに見えるスキンはこれだけ**で足りる（スキンは PlayerInfo の properties で表示される。エンティティ側に別の欄は無い）
- `ctx.remote_addr(PlayerId) -> Option<IpAddr>`、`ctx.profile_properties(PlayerId) -> &[ProfileProperty]`。退出済みの `PlayerId` には `None`・空（D31）
- 後から入ったプレイヤーの `PlayerInfo`（既存プレイヤーの分を送る道）も同じ `Player::info` を通るので、既存の送り先の修正は要らない

## 5. ハーネスとボット（D21）

- `TestEnv::connect_with` の `Profile` 相当が properties と `remote_addr` を指定できる形（`connect_as(Profile)`）を足し、ハーネスで `ctx.remote_addr`・`ctx.profile_properties`・`PlayerInfo` の properties を検証する
- `Bot` に **Velocity 役**を足す: `Bot::velocity(secret, player: ForwardedPlayer)` で、login 中の `CustomQuery` に正しい署名つきで答える。誤った署名・版・応答なしを作れるオプションも持ち、拒否のテストを書ける
- ボットは protocol にだけ依存する（D27）ので、**HMAC は bot にも要る**。ボットに `hmac`・`sha2` を足す（dev 用途のみ）。本体に依存させないため、署名の実装を共有する場所は作らない（二重だが、ボットは「本体と独立に protocol を実装している」ことが検証になる）

## 利用者に見える変更（v0.x）

- `Forwarding`・`Server::forwarding`・`Server::forwarding_timeout` が増えた
- `login::Profile` に `properties` と `remote_addr` が増えた。`Profile` を直接組み立てる自作 Instance・テストは欄を足す
- `ctx.remote_addr`・`ctx.profile_properties` が増えた
- 新しい依存: `hmac`・`sha2`（本体）。理由は下

## 確認して決めたこと（2026-10-03）

| 判断 | 選んだ案 | 選ばなかった案 |
|---|---|---|
| 設定の入り口 | `enum Forwarding`（`non_exhaustive`）+ `Server::forwarding` | 方式ごとの平らなメソッド（Velocity と BungeeCord の同時指定を実行時に弾く必要がある）／設定用 struct（server-config-plan の方針と食い違い、Superseded が要る） |
| HMAC | `hmac` + `sha2`（本体のみ） | `sha2` だけで HMAC を自前（定数時間比較を含め暗号の部品を持つことになる）／`ring`（C・アセンブリを含み、軽量の方針に合わない） |
| 公開範囲 | `Profile` に載せ、タブリストと `ctx` の読み出しまで | タブリストだけ（本物の IP が取れない）／転送情報のイベント（login の利用者向けフックは M2-04 と決めた D38 と食い違う） |

## 依存を足す理由

- 標準ライブラリと既存依存に HMAC・SHA-256 は無い。署名の検証は認証の突破に直結するので、自前で書かず、定数時間比較（`verify_slice`）つきの RustCrypto を使う
- 本体だけに足す。protocol には入れない（D14。ボットや proxy が単独で使う crate に暗号を持ち込まない）。M2-09（BungeeGuard のトークン照合は単純な比較なので追加は要らない）、REQ-AUTH-002（online mode）でも同じ系統を使える
- MSRV（1.90）で通ることを `mise run msrv` で確かめる

## やらないこと

| 項目 | なぜ外したか | 受け皿 |
|---|---|---|
| 鍵つきの転送（版 2 以上。チャットの署名鍵） | secure chat（REQ-CHAT 系）が無い間は使わない。版 1 を要求すれば Velocity はそれで返す | secure chat を入れるとき |
| `Forwarding::BungeeCord` / `BungeeGuard` | M2-09 | M2-09 |
| login の利用者向けフック（スキンを書き換える） | D38。M2-04 で決める | M2-04 |
| 認証前の入力の数・大きさの制限 | REQ-NET-007 の対象 | v0.3 |
| 実 Velocity での確認 | 手元に無い。ボットの Velocity 役で統合テストし、実機は利用者に頼む | 利用者 |

## テスト

- 単体（login）: 正しい署名で `Profile`（UUID・名前・properties・アドレス）を得る／署名 1 ビット違いを拒否／本体が短い・版が違う・件数が上限超え・UTF-8 不正を拒否／`Ok(None)` と応答なし（`TimedOut`）で理由つき `Disconnect`／圧縮を有効にした接続でも通る
- ハーネス（`tests/world.rs`）: properties がタブリストの `PlayerInfo` に載る／後から入ったプレイヤーにも既存の properties が届く／`ctx.remote_addr`・`ctx.profile_properties`／退出済みは `None`・空
- 統合（`tests/server.rs`）: ボットの Velocity 役でログインでき、転送した UUID・名前になる／シークレット違いと応答なしの接続が拒否される／`Forwarding::None` の既定は今と変わらない／空の `secret` は `start` が `InvalidInput`
- `mise run check`・`mise run msrv`

## 実装の順

1. 依存（`hmac`・`sha2`）と MSRV の確認、`Forwarding`・`Server::forwarding`
2. `Profile` の欄と、offline・world の追従
3. `login::velocity` と単体テスト
4. `Server` の接続処理への配線と、ハーネスの `ctx` 読み出し
5. ボットの Velocity 役と統合テスト
6. 文書（requirements の受入条件、minestom-parity 10.7、architecture、backlog、v0.2-plan）。PR
