# 同じ名前で入り直したときの扱い 実装計画（M1-25）

> **Status**: 実装済み（実機の確認待ち: 同じ名前で 2 つ入れて、古いほうに「別の場所からログインされました」と出ること）— 2026-10-01

要件: REQ-NET-003 に準じた接続の後片付け。決定: D6（[decisions.md](../decisions.md)）。
protocol（`Disconnect`）と本体（`Message::Leave`、`World::join`）にまたがる。実機で見つかった不具合の修正。

## 起きたこと

offline mode の UUID は名前から決まる（`Uuid::offline`）。**同じ名前の 2 つ目のクライアント**を入れると、両方が切られた。サーバーのログ（利用者の実機）:

```
joined name=niacha players=1
joined name=niacha players=2
... player{name=niacha uuid=5a6f1471-…}: lodeframe::play: left      ← 1 つ目の接続
can't keep up, skipping ticks behind_ms=2013
... player{name=niacha uuid=5a6f1471-…}: lodeframe::play: left      ← 2 つ目の接続
```

原因は 2 つ:

1. `Sessions::join` が同じ UUID の出力チャンネルを**置き換える**。1 つ目の接続は閉じられて終わる
2. 終わった接続は `Message::Leave { player }` を送る。**UUID だけで識別される**ので、置き換えられた 2 つ目のプレイヤーを退出させてしまい、2 つ目の接続も閉じる

（`can't keep up` は debug ビルド（`cargo run`）でチャンクを作ったため。release なら出ない）

## 調べたこと

vanilla は同じ UUID の再ログインで、**古いほうを切る**（`multiplayer.disconnect.duplicate_login` = 「別の場所からログインされました」）。26.3 の codec で play の `Disconnect`（clientbound 32）を確かめた: 本体は Component 1 つだけ（例: `08 00 02 68 69`）。

## 設計

- **古い接続を切って、新しい接続を入れる**（vanilla と同じ）。`World::join` は、同じ UUID がすでに居るなら、先にその人へ `Disconnect`（理由: 別の場所からログインされた）を送り、通常の退出（`leave`: 他の人へ消滅の通知、`PlayerLeaveEvent`）をしてから、新しい人を通常の入室として扱う
- **終わった接続の `Leave` は、その接続が今の接続のときだけ効く**。`Message::Leave` に、その接続の出力チャンネルへの**弱い参照**（`mpsc::WeakSender<Packets>`）を持たせる。受け取った側は、今のチャンネルと同じものか（`same_channel`）で判断し、置き換えられた古い接続の `Leave` は無視する。強い参照を接続側が持つと、インスタンスが切ったときに接続が気づけなくなるので、弱い参照にする
- 切断されたプレイヤーへ「理由」を見せるため、`packets::play::Disconnect { reason: Component }`

## v0.1 でやらなかったこと（後で対応する）

| 項目 | 今の動き | なぜ外したか | 受け皿 |
|---|---|---|---|
| 古い接続から、切られる直前に届いたパケット | 新しいプレイヤーのものとして扱われうる（数パケット。古いクライアントは切断を受けて直ぐ閉じる） | `Message::Packet` ごとに接続の識別を付けると、毎パケットのコストが増える | 起きると分かったら（`Packet` にも識別を付ける） |
| 新しいほうを断る設定（「すでに接続中」） | しない。常に古いほうを切る | vanilla の既定と同じ。設定の面は要るものから足す | `Server` の設定（REQ-AUTH-001 の前後） |
| online mode での同一アカウントの扱い | offline のみ | online mode が未実装 | REQ-AUTH-002 |

## テスト

- protocol: `Disconnect` のバイト列が実 codec の例と一致する
- ハーネス: 同じ名前で入ると、古い接続は `Disconnect` を受けてから閉じる。新しいプレイヤーは普通に入り、他の人からは 1 人だけに見える（出現と消滅が対になる）。古い接続の `Leave` が新しいプレイヤーを消さない。`PlayerLeaveEvent` が古いほうの分だけ 1 回、`PlayerJoinEvent` は入るたびに
- ボットで実サーバー: 同じ名前の 2 体目を入れると 1 体目が切られ、2 体目はそのまま遊べる
- 既存のテストは `Leave` の形が変わる分だけ直す
