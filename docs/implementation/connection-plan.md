# 接続層 実装計画（M1-06）

> **Status**: 実装済み（レビュー待ち）— 2026-09-30

要件: REQ-NET-001。決定: D6・D14・D16（[decisions.md](../decisions.md)）。
protocol と本体の 2 crate にまたがるためここに設計を置く。

## 問題

クライアントとのバイト列を「パケット 1 つ分の本体（id + payload）」に切り出し、戻す層が要る。
ボットや proxy も使うので、ソケットに依存しない部分は protocol に置く（D14）。

## 分担

| crate | 置くもの | 依存 |
|---|---|---|
| `lodeframe-protocol` | `frame`: 長さ接頭辞 + zlib の同期 codec（`FrameDecoder`・`encode_frame`）。`packets::handshake::Intention` | `flate2`（追加） |
| `lodeframe` | `net`: 非同期 `Connection`（読み書き・タイムアウト・状態）、`serve`（accept ループ） | `tokio`（追加） |

- `thiserror` は入れない。`Error` は protocol の `Error` を使い回す（タイムアウトは `Io(TimedOut)`、切断は `Io(UnexpectedEof)`）。
- flate2 は既定（`miniz_oxide`、純 Rust）。C の依存を持たない。

## フレーム形式

- 非圧縮: `VarInt 長さ | VarInt id | payload`
- 圧縮有効: `VarInt 長さ | VarInt 展開後の長さ | (zlib 圧縮済みの id + payload)`。展開後の長さが 0 なら閾値未満で**非圧縮**（長さの後ろはそのまま id + payload）
- 閾値は送信側だけが決める。閾値以上のパケットだけ圧縮する

## 安全側の挙動（受入条件）

| 条件 | 挙動 |
|---|---|
| 宣言された長さが上限超過 | 長さ接頭辞を読んだ時点で拒否。本体を待たない |
| 圧縮時、宣言された展開後の長さが上限超過 | 展開前に拒否 |
| 展開結果が宣言と違う | 展開を宣言長 + 1 で打ち切り、不一致を拒否（zip bomb 対策） |
| 読み取りが一定時間止まる | その接続だけ切断 |
| 不正パケット | 該当 `Connection` がエラーを返す。`serve` は他の接続を続ける |

上限は 1 フレーム 2^21 − 1 バイト（長さ接頭辞 3 バイトで表せる最大）。展開後は 8 MiB。
接頭辞が 4 バイト以上に続く入力は、その時点で `VarIntTooLong` として拒否する（3 バイトでは上限超過を表せないため）。

## `net` の形

- `Connection<S: AsyncRead + AsyncWrite + Unpin>`: `S` を総称にして、テストは `tokio::io::duplex` で動かす
  - `read_frame() -> Result<Vec<u8>>`（id + payload）、`write_frame(&[u8])`
  - `set_compression(Option<usize>)`（受信は展開後の長さで判別、送信は閾値）
  - `state()` / `set_state()`
  - `read_handshake()`: Handshake 状態で `Intention` を読み、intent に応じて Status / Login へ遷移（Transfer は Login 扱い）
- `serve(listener, config, handler)`: accept → 接続ごとに task。`handler` は handshake 後の `Connection` を受け取る async 関数。ハンドラの失敗はその task だけで終わる

Status と Login の中身は M1-07・M1-08。この単位は「接続を受け、フレームを読み書きし、handshake で状態を分ける」まで。

## 完了条件

- [x] 圧縮なし・あり（閾値の上下）でフレームが往復する（protocol の単体テスト）
- [x] 上限超過を本体を読む前に拒否する（テスト）
- [x] zip bomb（宣言より大きい展開結果）を拒否する（テスト）
- [x] 不正な handshake・タイムアウトで該当接続だけ切れ、`serve` は次の接続を受ける（実 TCP の統合テスト）

## 非対象

暗号化（online mode は v0.1 外）、Status / Login / Configuration のパケット（M1-07・M1-08）、conn task と Instance の channel（M1-09）。
