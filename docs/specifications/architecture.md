# アーキテクチャ

要件は [requirements.md](../requirements.md)、根拠は [decisions.md](../decisions.md)。
API の具体形は実装で確定させ、確定したらここを直す。

## crate 構成（D14）

```
lodeframe/
├── crates/
│   ├── lodeframe-protocol/   # 基本型・Encode/Decode・NBT・パケット・生成データ。本体非依存
│   │   └── src/generated/    # xtask datagen の出力（commit する）
│   ├── lodeframe-macros/     # proc-macro（Encode/Decode, Packet, command, event, UI）
│   └── lodeframe/            # 本体。protocol と macros を re-export する facade
├── xtask/                    # datagen / bot / bench
└── examples/lobby/
```

利用者は `lodeframe` だけに依存する。util crate（AI 等）は必要になった時点で足す。

## スレッドと所有（D6, D16）

```
          tokio runtime (I/O)
  ┌─────────────────────────────────┐
  │ conn task ×N                    │   decode / encode / 圧縮 / 暗号化
  │   └ inbound  mpsc ──┐           │
  │   ┌ outbound mpsc ◀─┼───┐       │
  └───┼─────────────────┼───┼───────┘
      │                 ▼   │
  ┌───┴─────────────────────┴───┐
  │ Instance thread (20 TPS)    │  Instance を &mut で単独所有
  │  1. inbound を drain        │  イベントノードを発火
  │  2. spawn 結果を drain      │  ctx.spawn の then を実行
  │  3. tick（物理・スケジューラ）│
  │  4. 差分を outbound へ       │
  └─────────────────────────────┘
        ▲  Instance 間は message のみ
        ▼
  ┌─────────────────────────────┐
  │ Instance thread …           │
  └─────────────────────────────┘
```

- conn task は状態機械（Handshake → Status / Login → Configuration → Play）を持ち、Play 以降のパケットだけを Instance に渡す。
- ログイン前の async イベント（forwarding 検証、利用者の BAN 照会等）は conn task 側で await する。Instance には確定したプレイヤーだけが入る。
- `ctx.spawn(fut).then(cb)`: fut は tokio で実行、結果は Instance の受信 channel に戻り、次 tick 冒頭で cb が `&mut` 付きで走る。
- グローバル可変シングルトンは置かない。サーバー全体の共有物（レジストリ等）は起動時に確定し不変で共有する。

## イベント（D9）

- `EventNode` は木。ルート（サーバー全体）と Instance ごとのノードがあり、利用者は子ノードを付け外しする。
- ハンドラは `fn(&mut E, &mut Ctx)`。キャンセル可能イベントは `ev.cancel()` で既定動作を止める。
- 発火は Instance スレッド上で同期実行。ハンドラ内のブロッキングは禁止（ドキュメントで明示）。

## プロトコルデータ（D3〜D5）

- 対象は最新リリース 1 本。版番号は着手時点の最新 stable で確定し、`lodeframe-protocol` の定数に置く。
- `cargo xtask datagen <version>`: server.jar を取得 → data generator（`--reports` 等）を実行 → JSON からブロック状態・レジストリ・パケット ID の Rust コードを生成。server.jar と JSON 自体は commit しない。
- configuration フェーズは Known Packs で vanilla データの送信を省き、利用者が追加したレジストリだけ本体を送る。

## 版追従の手順

1. `cargo xtask datagen <新版>` で生成物を差し替え
2. 差分のあるパケットを手書き分で修正（protocol の変更点は wiki.vg 後継資料と生成物の diff で確認）
3. ボット統合テストを通す
