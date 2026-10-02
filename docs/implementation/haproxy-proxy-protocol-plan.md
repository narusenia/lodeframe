# HAProxy PROXY protocol 実装計画（M2-10）

> **Status**: 計画 — 2026-10-03

要件: REQ-NET-004。決定: D14・D21・D27・D40・D41（[decisions.md](../decisions.md)）。
`lodeframe`（本体）・`lodeframe-bot` にまたがり、`net::Config`（公開型）と `serve` が受け取る peer の意味が変わるので、コードの前にここで形を決める。M2-09 の [bungeecord-forwarding-plan.md](bungeecord-forwarding-plan.md) が予告した「PROXY ヘッダの後ろの接続元を peer として扱う」を実現する。

## 流れ

HAProxy などの負荷分散装置は、TCP 接続の**先頭**に実クライアントのアドレスを書いたヘッダを置く（Minecraft の handshake より前）。サーバーはこれを読み、接続の相手としてそのアドレスを使う。誰でも書けるので、ヘッダは信頼する送信元（許可リスト）から来たものだけを受ける。

- **v1**（テキスト）: `PROXY TCP4|TCP6 <src> <dst> <sport> <dport>\r\n`。全体で 107 バイトまで。`PROXY UNKNOWN ...\r\n` はアドレス無し
- **v2**（バイナリ）: 12 バイトの署名 `0D 0A 0D 0A 00 0D 0A 51 55 49 54 0A`、版と命令（上位 4 ビットが 2、下位が 0 = LOCAL・1 = PROXY）、族とプロトコル、長さ（u16）、本体。TCP over IPv4（12 バイト）と IPv6（36 バイト）だけアドレスを読み、後ろの TLV は読み飛ばす。それ以外の族（UNSPEC・UNIX）・DGRAM・LOCAL はアドレス無し
- アドレス無しのヘッダ（LOCAL・UNKNOWN・UNSPEC）は、受けた上でソケットの相手をそのまま使う（死活監視がこれを送る）

## 1. 設定（`server.rs`・`net.rs`）

```rust
#[non_exhaustive]
pub enum ProxyProtocol {
    /// ヘッダを読まない（既定）。ヘッダが来れば handshake として壊れて切れる
    Off,
    /// `trusted` の送信元のヘッダだけを受ける。ヘッダの無い接続はそのまま受ける
    Optional { trusted: Vec<IpAddr> },
    /// 全部の接続に `trusted` の送信元のヘッダを求める
    Required { trusted: Vec<IpAddr> },
}
impl Server { pub fn proxy_protocol(self, mode: ProxyProtocol) -> Self; }
```

- `Forwarding` とは**独立**。接続層（誰が相手か）と identity（誰が入ったか）が直交するので、HAProxy の背後に Velocity・BungeeCord を組み合わせられる（D41）
- `Optional`・`Required` の `trusted` が空なら `start` が `InvalidInput` で返す（空の許可リストは全員を拒否する。M2-09 と同じ扱い）
- `net::Config` に同じ型の `proxy_protocol` を足す。`serve` が接続ごとに適用する（`Server` は渡すだけ）

## 2. 接続層（`net.rs`）

`serve` が接続を受けた直後、handshake を読む前に、接続ごとのタスクの中で行う（`accept` を止めない）。

| 設定 | 送信元 | ヘッダ | 結果 |
|---|---|---|---|
| Off | — | — | 何も読まない |
| Optional | trusted | あり | 読む。アドレスがあればそれが peer |
| Optional | trusted | なし | そのまま受ける |
| Optional | trusted でない | あり | **拒否**（形式が正しくても。誰でも IP を偽れる） |
| Optional | trusted でない | なし | そのまま受ける |
| Required | trusted | あり | 読む |
| Required | trusted | なし | **拒否** |
| Required | trusted でない | どちらでも | **拒否** |

- 送信元は**ソケットの相手**（`TcpListener::accept` が返したアドレス）。`to_canonical` で比べる（dual-stack の `::ffff:a.b.c.d`）
- 拒否は理由を返さずに切る（ヘッダの前に Minecraft のパケットは無く、切る前に書くものが無い。ログには理由を出す）
- 信頼しない送信元のヘッダは**読まない**。先頭が署名と一致するかだけを見て、一致したら切る（未検証の入力を解釈しない）
- 判定は**消費しながら**行う: バイトを読み、`PROXY ` と v2 の署名のどちらかに全部一致する間は続ける。どちらとも食い違った時点で「ヘッダではない」と決め、読んだバイトは Minecraft のパケットとして `Connection` の decoder に戻す（`peek` は新しいデータを待てず busy loop になり、TCP 以外では使えない）。Minecraft の handshake は先頭が長さの VarInt、2 バイト目がパケット id `0x00` なので、`PROXY ` とも v2 署名（2 バイト目 `0x0A`）とも 2 バイト以内に食い違う
- ヘッダ全体は `Config::read_timeout` の中に収める。v1 は 107 バイト、v2 は本体の長さ 1024 バイトまで。越えたら拒否する（確保の前に長さを照らす。rust.md の `check_remaining`）
- 読んだヘッダより後ろのバイト（同じ `read` で届いた handshake の先頭）も decoder に戻す

## 3. peer の意味と接続元（D41）

- `serve` がハンドラに渡す peer（M2-09 で足した第 3 引数）は、**ヘッダにアドレスがあればそのアドレス（IP とポート）、なければソケットの相手**にする。BungeeCord の許可リストは、HAProxy の背後でも「proxy の本当の接続元」を見る。ヘッダは trusted の送信元から来たものだけなので偽れない
  - 注意: HAProxy の `trusted` の設定ミスは、BungeeCord の許可リストの突破に連鎖する。docs に書く
- `Connection::proxied_addr() -> Option<SocketAddr>` を足す（`serve` が入れる。ヘッダにアドレスがあったときだけ `Some`）。ハンドラの型は変えない
- `Profile.remote_addr` が `None` のとき（転送が無い login）、`server.rs` が `conn.proxied_addr()` の IP を入れる。`ctx.remote_addr` が HAProxy 越しの本当の IP を返す。Velocity・BungeeCord が転送した値があればそちらが勝つ（identity を決めるのはそちら。proxy が教える接続元の方が新しい情報）
- 直接つないだ接続（ヘッダ無し）の `Profile.remote_addr` は `None` のまま（M2-08 と同じ。ソケットの相手は入れない）

## 4. ボット（D27）

- `HaProxy`（ヘッダの役）を足す: `V1(SocketAddr)`・`V2(SocketAddr)`・`V2Local`・`Raw(Vec<u8>)`（壊れたヘッダを作る）。アドレスは IPv4・IPv6 の両方
- `Bot::connect_haproxy(addr, name, header)`・`login_haproxy(stream, name, header)`。ヘッダを handshake の前に書く。BungeeCord 役との組み合わせ用に `connect_haproxy_bungee(addr, name, header, bungee)`
- ボットは protocol にだけ依存する（D27）。ヘッダの組み立ては本体のコードを使わずに書く

## 利用者に見える変更（v0.x）

- `ProxyProtocol`・`Server::proxy_protocol`・`Config::proxy_protocol` が増えた
- `net::Config` に欄が増えた。構造体リテラルで作っていれば `..Config::default()` が要る
- `serve` が渡す peer は、PROXY ヘッダがあればそのアドレスになる
- `Connection::proxied_addr` が増えた
- 依存は増えない

## 確認して決めたこと（2026-10-03）

| 判断 | 選んだ案 | 選ばなかった案 |
|---|---|---|
| 設定の入り口 | 独立した `Server::proxy_protocol` | `Forwarding` の変種（入り口は 1 つだが、HAProxy と Velocity・BungeeCord を併用できず、`Forwarding` が接続層と identity の 2 つの意味を持つ） |
| peer の意味 | ヘッダの送信元（なければソケット） | 常にソケットの相手（意味は単純だが、HAProxy 越しの BungeeCord は許可リストに HAProxy の IP を入れることになり、HAProxy を通る誰でも通ってしまう） |

## やらないこと

| 項目 | なぜ外したか | 受け皿 |
|---|---|---|
| CIDR の許可リスト | `IpAddr` の一覧で足りる（M2-09 と同じ）。HAProxy は台数が少ない | 要望が出たとき |
| TLV の解釈（SSL 情報・authority など） | 使い道が無い。読み飛ばす | 要望が出たとき |
| UNIX ソケットの族 | TCP だけ待ち受ける | REQ-NET-011 |
| 認証前の接続数・大きさの制限 | REQ-NET-007 の対象 | v0.3 |
| 実 HAProxy での確認 | 取得の前に利用者へ確認する | 実装後に利用者へ確認 |

## テスト

- 単体（`net.rs` の `serve` を実ソケットで）: v1 の TCP4・TCP6・UNKNOWN／v2 の PROXY（IPv4・IPv6）・LOCAL・TLV つき／ヘッダの直後に handshake が同じ書き込みで届く／ヘッダが数回に分かれて届く
- 拒否: 許可リスト外のヘッダ（Optional）／許可リスト外・ヘッダ無し・trusted のヘッダ無し（Required）／壊れたヘッダ（v1 の長さ超過・族の食い違い・IP 不正・v2 の版・命令・長さ超過・本体が短い）／署名の途中で切れる
- ヘッダ無しの通常の接続が Optional で通る（handshake の先頭が `PROXY ` や v2 署名と食い違う）／Off ではヘッダが来ると切れる
- 統合（`tests/server.rs`）: HAProxy 越しのボットが `ctx.remote_addr` に本当の IP を得る／Optional の許可リスト外のヘッダを拒否／Required でヘッダ無しを拒否／BungeeCord と併用して許可リストがヘッダの送信元を見る／空の `trusted` は `start` が `InvalidInput`
- `mise run check`・`mise run msrv`

## 実装の順

1. `ProxyProtocol`・`Config`・`Server::proxy_protocol`・`start` の検証
2. ヘッダの解析（`net.rs` の中の `proxy` モジュール）と単体テスト
3. `serve` への配線（判定表・peer・`Connection::proxied_addr`・decoder への書き戻し）
4. `server.rs` で `Profile.remote_addr` を埋める
5. ボットの `HaProxy` と統合テスト
6. 文書（requirements の受入条件、minestom-parity 1.8・10.7、architecture、backlog、v0.2-plan）。PR
