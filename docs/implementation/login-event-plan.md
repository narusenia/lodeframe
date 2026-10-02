# ログイン前の async イベント実装計画（M2-04）

> **Status**: 実装済み — 2026-10-03

要件: REQ-API-003（ログイン前の項目）。決定: D16・D21・D31・D38・D39・D40・D41・D42（[decisions.md](../decisions.md)）。
`lodeframe`（本体）と `lodeframe-text` にまたがり、login の内部を組み替えるので、コードの前にここで形を決める。M2-07（D38）が「login の利用者向けフックは M2-04」と預けた分。

## 何をするか

利用者が、プレイヤーが入室する**前**に、非同期の処理（DB・BAN 照会・外部 API）を挟めるようにする。できること:

- 入室の拒否（理由つき）
- UUID・名前・properties（スキン）の差し替え

ハンドラは Instance のスレッドではなく**接続ごとのタスク（conn task）**で await する（D16 の「ログイン前検証用の async イベントだけ別枠」）。Instance には、確定して通ったプレイヤーだけが入る。

## 1. 流れ

名前と UUID はクライアントへ `LoginFinished` で伝わるので、差し替えはその**前**でなければならない。

```
login の各方式（offline / Velocity / BungeeCord / BungeeGuard）が Profile を確定する  ← ここまでが identify
        ↓
フックを登録順に 1 つずつ await する（Profile を受け、Allow(Profile) か Deny(理由)）
        ↓
Deny ならその理由で login の Disconnect を送って切る
        ↓
名前・properties を検査し、LoginFinished を送る（accept）→ 在線の枠を取る → configuration へ
```

- フックは**在線の枠を取る前**に走る。拒否した接続が枠を使わず、満員の判定にも影響しない
- ステータス要求（サーバー一覧）では走らない。login の接続だけ
- Velocity・BungeeCord・BungeeGuard の転送が確定した後に走るので、フックは転送された UUID・名前・接続元を見て、差し替えられる

## 2. 公開 API

```rust
/// フックが受ける、入ろうとしているプレイヤー。所有しているので `await` をまたいで持てる
pub struct LoginAttempt {
    /// 確定した名前・UUID・properties・転送された接続元。書き換えてから `allow` すると差し替わる
    pub profile: Profile,
    /// 接続の相手（PROXY ヘッダがあればその IP）。`Profile.remote_addr` とは別で、転送された値ではない
    pub peer: SocketAddr,
}

#[non_exhaustive]
pub enum LoginDecision {
    Allow(Profile),
    Deny(Component),
}

impl LoginAttempt {
    pub fn allow(self) -> LoginDecision;
    pub fn deny(self, reason: impl Into<Component>) -> LoginDecision;
}

impl Server {
    /// 何度でも呼べる。登録順に 1 つずつ await し、前のフックが返した Profile を次が受ける
    pub fn on_login<F, Fut>(self, hook: F) -> Self
    where F: Fn(LoginAttempt) -> Fut + Send + Sync + 'static,
          Fut: Future<Output = LoginDecision> + Send + 'static;
    /// 全部のフックに許す時間（既定 10 秒）。越えたら拒否する
    pub fn login_hook_timeout(self, timeout: Duration) -> Self;
}
```

- **所有した値を受けて判定を返す**形にする。`&mut LoginAttempt` を await をまたいで渡すと、フックの Future が借用を持つので `Send` と寿命の扱いが重く、ボックス化の型が利用者に漏れる
- `Deny` は**拒否した時点で止まる**（後ろのフックは走らない）
- `LoginDecision` は `non_exhaustive`（後で「保留して再試行」などを足せる）
- 拒否の理由は **`Component`**（D42）。login の Disconnect は JSON 文字列なので、`lodeframe-text` に `Component::to_json` を最小で足す（text・color・5 つの装飾。エスケープは `"`・`\`・制御文字）。M2-11 が完全版に広げる
- フックが返さない・時間を越えたときは、理由「Login timed out」で拒否する。panic は接続のタスクが落ちて切れる（ログに出る。他の接続と Instance には影響しない）

## 3. 差し替えの検査

フックが返した Profile は、`LoginFinished` を送る前に検査する（フックは利用者のコードで、信用しない）。

- 名前は 1〜16 文字。UUID は何でもよい（利用者が決める）
- properties は 64 件まで、各文字列は vanilla と同じ上限（name 64・value 32767・signature 1024）
- 満たさなければ理由「Unable to verify player details」で拒否し、ログに原因を出す（フックの間違いに気づけるように、黙って直さない）

## 4. login の組み替え（`login.rs`）

いまの 4 つの公開関数（`offline`・`velocity`・`bungeecord`・`bungeeguard`）は、Profile の確定から `LoginFinished` までを 1 つの関数で行う。フックはその間に入るので、内部を 2 つに分ける。

- crate 内部の `identify`（方式の列挙 `Identity` を受け、`Hello` の読み取り・圧縮・転送の検証をして `Profile` を返す。まだ `LoginFinished` は送らない）と、`accept`（`LoginFinished` を送り、`LoginAcknowledged` を待つ）
- 公開の 4 関数は**今の形のまま**（`identify` + `accept` の薄い包み）。フック無しで使う自作サーバー・既存のテストは変わらない
- `Server` は `identify` → フック → `accept` と自分で繋ぐ。`Server` の判定表（M2-09 の `Forwarding` の match）は `Identity` を作るだけになる

## 5. ハーネスとテスト（D15・D21）

フックは `Server`（接続層）の設定で、Instance を持たないので、`TestEnv` では動かせない。**実ソケットの統合テスト（ボット）で検証する**（rust.md の「公開 API はハーネスから」の例外として、ここに理由を書く）。

- 統合（`tests/login_event.rs`）:
  - 何も登録しなければ今までどおり入れる
  - 拒否した接続は Instance に入らず（`PlayerJoinEvent` が来ない）、在線人数に数えられず、満員にも影響しない
  - 拒否の理由が login の Disconnect の JSON で届く（`Component` の色つきも）
  - 名前・UUID・properties を差し替えると、ワールド側（`PlayerInfo`・`ctx`）と `LoginFinished` が差し替え後になる
  - 複数のフックが登録順に走り、前の差し替えを後ろが見る。後ろが拒否すれば、そこで止まる
  - await するフックが他の接続と tick を止めない
  - フックが時間を越えると拒否される
  - 差し替え後の名前が空・17 文字、properties が 65 件のときに拒否される
  - Velocity・BungeeCord・PROXY ヘッダの後ろでも、フックが転送後の Profile と peer を見る
  - ステータス要求では走らない
- 単体（`lodeframe-text`）: `to_json` の text・color（名前と `#rrggbb`）・5 つの装飾・エスケープ
- `mise run check`・`mise run msrv`

## 利用者に見える変更（v0.x）

- `LoginAttempt`・`LoginDecision`・`Server::on_login`・`Server::login_hook_timeout`、`Component::to_json` が増えた
- 公開の `login::*` は変わらない
- 依存は増えない

## 確認して決めたこと（2026-10-03）

| 判断 | 選んだ案 | 選ばなかった案 |
|---|---|---|
| フックの数 | 複数・登録順に実行（前の Profile を次が受ける。拒否で止まる） | 単一のフック（順序の問題は無いが、複数の関心事を利用者が 1 つの関数にまとめる必要がある） |
| 拒否の理由の型 | `Component`（今の欄だけの `to_json` を足し、M2-11 で広げる） | `String` の平文（text crate に触れずに済むが、M2-11 の後に型を変える破壊的変更が要り、色つきの理由が出せない） |

## 決めたこと（確認なし）

- 受け渡しは所有した `LoginAttempt` → `LoginDecision`（借用を await に持ち込まない）
- フックは在線の枠を取る前に走り、ステータス要求では走らない
- 時間の上限は全体で 1 つ（既定 10 秒）
- 公開の `login::*` は変えず、内部を `identify` と `accept` に分ける

## やらないこと

| 項目 | なぜ外したか | 受け皿 |
|---|---|---|
| 「同じ人が既に在線」の判定 | フックで利用者が書ける。`Server` は在線の一覧を持たない（Instance の持ち物） | 要望が出たとき |
| online mode の認証 | REQ-AUTH-002 | v0.3 |
| configuration に入った後の拒否・再突入 | REQ-NET-010 | v0.3 |
| 認証前の接続数・同時に走るフックの数の制限 | REQ-NET-007 の対象 | v0.3 |
| `Component` の完全な JSON | M2-11 | M2-11 |

## 実装の順

1. `Component::to_json` と単体テスト
2. `login.rs` を `identify` と `accept` に分ける（公開の関数と既存のテストは変えない）
3. `LoginAttempt`・`LoginDecision`・`Server::on_login`・`login_hook_timeout` と、`Server` の接続処理への配線
4. 統合テスト
5. 文書（requirements の受入条件、minestom-parity、architecture、backlog、v0.2-plan）。PR
