# BungeeCord / BungeeGuard 転送の実装計画（M2-09）

> **Status**: 計画 — 2026-10-03

要件: REQ-AUTH-003。決定: D14・D21・D27・D39・D40（[decisions.md](../decisions.md)）。
`lodeframe`（本体）・`lodeframe-bot` にまたがり、公開関数 `net::serve` のハンドラ型が変わるので、コードの前にここで形を決める。M2-08 の [velocity-forwarding-plan.md](velocity-forwarding-plan.md) が作った `Forwarding` に変種を足す。

## 流れ

legacy forwarding は handshake の `server_address` 欄に情報を詰める。署名は無い。

```
<host> \0 <接続元 IP> \0 <UUID（ハイフン無しの 32 桁 16 進）> [ \0 <properties の JSON 配列> ]
```

- 末尾の properties は省略されることがある（古い BungeeCord）。区切りは 3 つ（properties 無し）か 4 つ（あり）だけを受け、他は拒否する
- properties は `[{"name":"textures","value":"..","signature":".."}]`。`signature` は無いことがある
- 名前は handshake に無い。login の `Hello` の名前を使う（Velocity と違い、proxy は名前を書き換えない）
- BungeeGuard は同じ形式の properties に、`name = "bungeeguard-token"` の 1 件を足す。`value` が共有トークン。サーバーはトークンを照合し、**その 1 件を取り除いてから** `Profile.properties` に載せる（利用者のコードやタブリストにトークンを出さない）

legacy forwarding は誰でも書ける（署名が無い）。直接つながったクライアントが任意の UUID を名乗れるので、次のどちらかで送信元を縛る。

| 設定 | 縛り方 |
|---|---|
| `BungeeCord { trusted }` | 接続元のソケットアドレスが `trusted` にある接続だけ受ける |
| `BungeeGuard { tokens }` | トークンが `tokens` のどれかと一致する接続だけ受ける。送信元は見ない |

## 1. 設定（`server.rs`）

```rust
#[non_exhaustive]
pub enum Forwarding {
    None,
    Velocity { secret: Vec<u8> },
    /// BungeeCord legacy forwarding。`trusted` の送信元だけを受ける
    BungeeCord { trusted: Vec<IpAddr> },
    /// BungeeGuard。`tokens` のどれかを持つ接続だけを受ける
    BungeeGuard { tokens: Vec<String> },
}
```

- `trusted`・`tokens` が空なら `start` が `InvalidInput` で返す（空の許可リストは全員を拒否し、空のトークン表は何も通さないので、設定の間違いとして気づかせる。空の `secret` と同じ扱い）。空文字のトークンも `InvalidInput`
- 手書きの `Debug` は `tokens` を出さない（`trusted` は IP なので出す）
- `tokens` は複数持てる（トークンの入れ替え中に新旧を両方受けるため）

## 2. 接続元アドレスの渡し方（`net.rs`）

許可リストの照合にはソケットの peer が要る。いまのハンドラは `Fn(Connection<TcpStream>, Intention)` で、peer を受け取らない。

- `serve` のハンドラを **`Fn(Connection<TcpStream>, Intention, SocketAddr)`** にする（`accept` が返した peer を渡す）。利用者が呼ぶ公開関数なので **v0.x の破壊的変更**として docs に書く。呼び出し元は `server.rs` と `tests/{logging,connection}.rs` の 3 か所
- `Profile.remote_addr` は転送された値で、ソケットの peer ではない（M2-08 の注意）。許可リストの照合には `SocketAddr` の peer だけを使い、`Profile.remote_addr` と取り違えない
- M2-10（HAProxy）は PROXY ヘッダの後ろの接続元を peer として扱う必要がある。そのときこの引数を差し替える（HAProxy を許可リストで縛るのは同じ peer）

## 3. login（`login.rs`）

`login::offline`・`login::velocity` と並べて、関数を 2 つ足す。中身は共通の `legacy` に寄せる。

- `login::bungeecord(conn, threshold, address, peer, trusted) -> Result<Profile>`
- `login::bungeeguard(conn, threshold, address, tokens) -> Result<Profile>`

順: **許可リストの検査（BungeeCord のみ）→ `Hello` → 圧縮 → `address` の解析 → トークンの照合（BungeeGuard のみ）→ `LoginFinished` → `LoginAcknowledged`**。

- 許可リストに無い送信元は、`Hello` も読まずに login の `Disconnect`（理由「This server requires you to connect with BungeeCord.」）。解析も何もしない（未検証の入力を読まない）
- 拒否の理由は Velocity と同じ流儀。トークンが無い・合わないは同じ理由（「Unable to verify player details」）にして、トークンが「無い」のか「違う」のかを外に漏らさない
- トークンの照合は **定数時間**。`tokens` の全部と比べ、途中で打ち切らない（どのトークンに一致したかを時間で漏らさない）。長さの違いは打ち切ってよい（トークンは固定長で生成される前提。長さは秘密に含めない）。比較は 10 行ほどの関数を `login.rs` に置く（`hmac` は署名の検証用で、ここは単純な比較）
- 上限: `Intention` の `String` は protocol のデコーダが 32767 文字（UTF-16）まで受ける。legacy forwarding の `address` はその範囲に収まるので、デコーダの上限をそのまま使い、解析の側で properties は 64 件まで、各文字列は vanilla と同じ上限（name 64・value 32767・signature 1024）で拒否する
- UUID は 32 桁の 16 進だけ（ハイフン付き・不正は拒否）。名前は `Hello` で 1〜16 文字を検査（`offline` と同じ）
- 転送された IP は `IpAddr` にパースする。パースできなければ拒否。**IPv6 のスコープ ID（`%eth0` 付き）は BungeeCord が付けることがあるので、`%` 以降を落としてからパースする**

## 4. properties の JSON（依存）

properties は JSON なので、認証前の入力を読むパーサが要る。

- 本体に **`serde_json`** を足す（workspace に宣言済みで xtask が使っており、Cargo.lock にもある。新しく取得するものは無い）。`serde` の derive は足さず、`serde_json::Value` から取り出す（属性の derive を使うと `serde` と `serde_derive` が本体の依存に増える。取り出す欄は 3 つだけ）。`serde_json::from_str` は既定で深さ 128 に制限があり、再帰でスタックが溢れない（rust.md の「再帰とスタック」）
- 理由: 標準ライブラリに JSON パーサは無い。認証前の入力を自前のパーサで読むと、エスケープや入れ子の取りこぼしが認証の突破になりうる。実績のあるパーサに任せる
- 本体だけに足す。protocol には入れない（D14）。MSRV（1.90）は `mise run msrv` で確かめる

## 5. `Profile` と世界側

変更なし。`Profile.properties`・`remote_addr` は M2-08 で入っており、legacy forwarding の値をそのまま載せる。`ctx.remote_addr`・`ctx.profile_properties`・タブリストの `PlayerInfo` も同じ道を通る。

## 6. ハーネスとボット（D21・D27）

- ハーネスは `Profile` を直接渡す（`connect_as`）ので変更なし。login の関数は実ソケットのボットで確かめる
- ボットに **BungeeCord 役**を足す。`Bot::connect_behind` が受ける proxy 役を `Velocity` の 1 型から、役を表す型にする（`Proxy` の enum か、`Velocity` と並ぶ `Bungee`）。実装時に既存の呼び出し（M2-08 のテスト）を壊さない形を選び、選んだ形を実装の PR に書く
  - handshake の `server_address` に `host\0ip\0uuid\0properties` を詰める。BungeeGuard のトークンも載せられる
  - 誤ったトークン・トークン無し・不正な形式（区切りの数・UUID・JSON）を作れるオプションを持つ
- ボットは protocol にだけ依存する（D27）ので、JSON の組み立ては文字列の連結で足りる。`serde_json` はボットに足さない
- 許可リストの送信元の検査は、ボットが 127.0.0.1 からつなぐので、`trusted` に 127.0.0.1 を入れた／入れていない設定の 2 通りで確かめる

## 利用者に見える変更（v0.x）

- `Forwarding::BungeeCord { trusted }`・`Forwarding::BungeeGuard { tokens }` が増えた
- **`net::serve` のハンドラの第 3 引数に peer（`SocketAddr`）が増えた**。自作のサーバーループがあれば引数を足す
- 新しい依存: `serde_json`（本体）。理由は上

## 確認して決めたこと（2026-10-03）

| 判断 | 選んだ案 | 選ばなかった案 |
|---|---|---|
| peer の渡し方 | `serve` のハンドラ引数に `SocketAddr` を足す | `Connection` に `peer_addr` を持たせる（`Connection<S>` の汎用性に `Option` の欄が増える。`serve` の型は保てる） |
| 許可リスト | `IpAddr` の一覧 | CIDR（マスク計算を自前で書くか依存を足す。境界のバグが認証の突破に直結する。M2-10 と共通化する価値はあるが、必要になったときに型を足せる） |
| JSON の読み取り | `serde_json` を本体に足す | 手書きの最小パーサ（認証前の入力を自前で読む。取りこぼしが脆弱性になる） |

## やらないこと

| 項目 | なぜ外したか | 受け皿 |
|---|---|---|
| CIDR の許可リスト | `IpAddr` の一覧で足りる。必要になれば型を足せる（`non_exhaustive` の `Forwarding` に影響しない） | M2-10 で共通化を検討 |
| BungeeGuard と許可リストの併用 | REQ-AUTH-003 は「BungeeGuard を使わないとき」だけ許可リストを要求する。併用は設定を増やす | 要望が出たとき |
| login の利用者向けフック（スキンの書き換え） | D38。M2-04 で決める | M2-04 |
| 認証前の入力の数・大きさの制限 | REQ-NET-007 の対象 | v0.3 |
| 実 BungeeCord での確認 | Java と BungeeCord の jar が要る。取得の前に利用者に確認する | 実装後に利用者へ確認 |

## テスト

- 単体（login）: 正しい address で `Profile`（UUID・名前・properties・アドレス）を得る／properties 無しの 3 区切りも通る／区切りの数・UUID・IP・JSON が不正なら拒否／BungeeGuard のトークンが無い・違う・長さが違うを拒否／トークンの 1 件が `Profile.properties` に残らない／IPv6 のスコープ ID を落とす／許可リスト外は `Hello` を読まずに理由つき `Disconnect`／圧縮を有効にした接続でも通る
- 統合（`tests/server.rs`）: ボットの BungeeCord 役でログインでき、転送した UUID・名前・properties になる／`trusted` に無い送信元・BungeeGuard のトークン違いが拒否される／空の `trusted`・`tokens`・空文字のトークンは `start` が `InvalidInput`／`Forwarding::None` と Velocity は今と変わらない
- `tests/connection.rs`・`logging.rs`: ハンドラの peer が接続元のアドレスと一致する
- `mise run check`・`mise run msrv`

## 実装の順

1. `serve` のハンドラに peer を足し、呼び出し元を直す
2. `Forwarding` の変種・`start` の検証・`Debug`（`serde_json` の依存と MSRV の確認）
3. `login::bungeecord`・`login::bungeeguard` と単体テスト
4. `Server` の接続処理への配線
5. ボットの BungeeCord 役と統合テスト
6. 文書（requirements の受入条件、minestom-parity 10.7、architecture、backlog、v0.2-plan）。PR
